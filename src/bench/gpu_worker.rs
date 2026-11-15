//! GPU worker: runs inside the child process (`syscheck __gpu list|run ...`) and prints one JSON
//! document on stdout. All GPU code of the program is in this file.

use crate::bench::gpu::{GpuAdapterInfo, GpuAdapterResult};
use crate::bench::rt::{self, RtGpu};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ───────────────────────── shaders (WGSL) ─────────────────────────

const FMA_WGSL: &str = r#"
struct P { iters: u32, m: f32, k: f32, s: f32 };
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  var a = f32(gid.x) * 1e-6 + p.s;
  var b = a + 0.5; var c = a + 1.0; var d = a + 1.5;
  var e = a + 2.0; var f = a + 2.5; var g = a + 3.0; var h = a + 3.5;
  for (var i = 0u; i < p.iters; i++) {
    a = fma(a, p.m, p.k); b = fma(b, p.m, p.k); c = fma(c, p.m, p.k); d = fma(d, p.m, p.k);
    e = fma(e, p.m, p.k); f = fma(f, p.m, p.k); g = fma(g, p.m, p.k); h = fma(h, p.m, p.k);
  }
  out[gid.x] = a + b + c + d + e + f + g + h;
}
"#;

const INT_WGSL: &str = r#"
struct P { iters: u32, m: u32, k: u32, s: u32 };
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  var a = gid.x + p.s; var b = a ^ 0x9E3779B9u; var c = a + 12345u; var d = a * 3u;
  var e = a ^ 0x85EBCA6Bu; var f = a + 777u; var g = a * 5u; var h = a ^ 0xC2B2AE35u;
  for (var i = 0u; i < p.iters; i++) {
    a = (a * p.m + p.k) ^ (a >> 7u); b = (b * p.m + p.k) ^ (b >> 7u);
    c = (c * p.m + p.k) ^ (c >> 7u); d = (d * p.m + p.k) ^ (d >> 7u);
    e = (e * p.m + p.k) ^ (e >> 7u); f = (f * p.m + p.k) ^ (f >> 7u);
    g = (g * p.m + p.k) ^ (g >> 7u); h = (h * p.m + p.k) ^ (h >> 7u);
  }
  out[gid.x] = a ^ b ^ c ^ d ^ e ^ f ^ g ^ h;
}
"#;

const COPY_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
  let i = gid.y * nwg.x * 256u + gid.x;
  if (i < arrayLength(&src)) {
    dst[i] = src[i] * 1.0001 + vec4<f32>(1.0);
  }
}
"#;

const MATMUL_WGSL: &str = r#"
struct M { n: u32, a: u32, b: u32, c: u32 };
@group(0) @binding(0) var<uniform> m: M;
@group(0) @binding(1) var<storage, read> A: array<f32>;
@group(0) @binding(2) var<storage, read> B: array<f32>;
@group(0) @binding(3) var<storage, read_write> C: array<f32>;
var<workgroup> ta: array<f32, 256>;
var<workgroup> tb: array<f32, 256>;
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) g: vec3<u32>, @builtin(local_invocation_id) l: vec3<u32>) {
  let n = m.n;
  var acc = 0.0;
  for (var t = 0u; t < n / 16u; t++) {
    ta[l.y * 16u + l.x] = A[g.y * n + t * 16u + l.x];
    tb[l.y * 16u + l.x] = B[(t * 16u + l.y) * n + g.x];
    workgroupBarrier();
    for (var k = 0u; k < 16u; k++) {
      acc = fma(ta[l.y * 16u + k], tb[k * 16u + l.x], acc);
    }
    workgroupBarrier();
  }
  C[g.y * n + g.x] = acc;
}
"#;

const NOOP_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) { out[0] = gid.x; }
"#;

/// Same scene and algorithm as `rt.rs`. One thread = one pixel of one pass (z = pass slice).
const RT_WGSL: &str = r#"
struct P { pass_base: u32, seed: u32, w: u32, h: u32, nsph: u32, npl: u32, p0: u32, p1: u32 };
@group(0) @binding(0) var<uniform> P_: P;
@group(0) @binding(1) var<storage, read> prims: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> accum: array<f32>;
var<private> st: u32;
fn hash(x: u32) -> u32 {
  let s = x * 747796405u + 2891336453u;
  let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
  return (w >> 22u) ^ w;
}
fn rnd() -> f32 { st = hash(st); return f32(st >> 8u) * (1.0 / 16777216.0); }
fn nrm(v: vec3<f32>) -> vec3<f32> { return v * (1.0 / sqrt(dot(v, v))); }
fn random_unit() -> vec3<f32> {
  for (var i = 0; i < 8; i++) {
    let x = rnd() * 2.0 - 1.0;
    let y = rnd() * 2.0 - 1.0;
    let z = rnd() * 2.0 - 1.0;
    let v = vec3<f32>(x, y, z);
    let l = dot(v, v);
    if (l <= 1.0 && l > 1e-6) { return v * (1.0 / sqrt(l)); }
  }
  return vec3<f32>(0.0, 1.0, 0.0);
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
  if (g.x >= P_.w || g.y >= P_.h) { return; }
  let idx = g.y * P_.w + g.x;
  let pidx = P_.pass_base + g.z;
  st = hash(idx * 9781u + pidx * 6271u + P_.seed);
  let rx = rnd();
  let ry = rnd();
  let tan_half = 0.62;
  let aspect = f32(P_.w) / f32(P_.h);
  let u = ((f32(g.x) + rx) / f32(P_.w) * 2.0 - 1.0) * tan_half * aspect;
  let v = (1.0 - (f32(g.y) + ry) / f32(P_.h) * 2.0) * tan_half;
  var d = nrm(vec3<f32>(u, v, -1.0));
  var o = vec3<f32>(0.0, 1.4, 3.6);
  var thr = vec3<f32>(1.0, 1.0, 1.0);
  var rad = vec3<f32>(0.0, 0.0, 0.0);
  for (var depth = 0u; depth < 5u; depth++) {
    var best = 3.4e38;
    var hn = vec3<f32>(0.0, 1.0, 0.0);
    var hc = vec4<f32>(0.0);
    var found = false;
    for (var i = 0u; i < P_.nsph; i++) {
      let a = prims[i * 2u];
      let col = prims[i * 2u + 1u];
      let c = a.xyz;
      let oc = o - c;
      let b = dot(oc, d);
      let cc = dot(oc, oc) - a.w * a.w;
      let disc = b * b - cc;
      if (disc < 0.0) { continue; }
      let sq = sqrt(disc);
      var t = -b - sq;
      if (t < 0.001) { t = -b + sq; }
      if (t > 0.001 && t < best) {
        best = t;
        let p = o + d * t;
        hn = (p - c) * (1.0 / a.w);
        hc = col;
        found = true;
      }
    }
    for (var i = 0u; i < P_.npl; i++) {
      let a = prims[(P_.nsph + i) * 2u];
      let col = prims[(P_.nsph + i) * 2u + 1u];
      let denom = dot(a.xyz, d);
      if (abs(denom) < 1e-6) { continue; }
      let t = (a.w - dot(a.xyz, o)) / denom;
      if (t > 0.001 && t < best) {
        best = t;
        hn = a.xyz;
        hc = col;
        found = true;
      }
    }
    if (!found) { break; }
    if (hc.w == 2.0) { rad = rad + thr * hc.xyz; break; }
    var n = hn;
    if (dot(n, d) > 0.0) { n = -n; }
    let p = o + d * best;
    thr = thr * hc.xyz;
    if (hc.w == 1.0) {
      d = d - n * (2.0 * dot(d, n));
    } else {
      let ru = random_unit();
      d = nrm(n + ru);
    }
    o = p + n * 0.001;
  }
  let base = (g.z * P_.w * P_.h + idx) * 3u;
  accum[base] = rad.x;
  accum[base + 1u] = rad.y;
  accum[base + 2u] = rad.z;
}
"#;

// ───────────────────────── helpers ─────────────────────────

struct Ctx {
    device: wgpu::Device,
    queue: wgpu::Queue,
    errors: Arc<Mutex<Vec<String>>>,
}

fn wait_idle(ctx: &Ctx) {
    let _ = ctx.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(60)) });
}

fn gpu_buffer(ctx: &Ctx, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    ctx.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false })
}

fn pipeline(ctx: &Ctx, wgsl: &str) -> wgpu::ComputePipeline {
    let module = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)) });
    ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None })
}

fn bind(ctx: &Ctx, pl: &wgpu::ComputePipeline, bufs: &[&wgpu::Buffer]) -> wgpu::BindGroup {
    let layout = pl.get_bind_group_layout(0);
    let entries: Vec<wgpu::BindGroupEntry> = bufs.iter().enumerate().map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() }).collect();
    ctx.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &entries })
}

/// Record `n` dispatches, submit, wait; returns the wall time in seconds.
fn dispatch_n(ctx: &Ctx, pl: &wgpu::ComputePipeline, bg: &wgpu::BindGroup, groups: (u32, u32, u32), n: u32) -> f64 {
    let mut enc = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
        pass.set_pipeline(pl);
        pass.set_bind_group(0, bg, &[]);
        for _ in 0..n {
            pass.dispatch_workgroups(groups.0, groups.1, groups.2);
        }
    }
    let t = Instant::now();
    ctx.queue.submit(std::iter::once(enc.finish()));
    wait_idle(ctx);
    t.elapsed().as_secs_f64()
}

/// Run dispatches in batches of about 100-150 ms (short enough for driver watchdogs) until
/// `target` seconds were measured. Returns (dispatch count, seconds). The first (calibration)
/// batch is not counted.
fn timed(ctx: &Ctx, pl: &wgpu::ComputePipeline, bg: &wgpu::BindGroup, groups: (u32, u32, u32), target: f64) -> (u64, f64) {
    let t1 = dispatch_n(ctx, pl, bg, groups, 1).max(1e-6);
    let mut n = ((0.12 / t1) as u32).clamp(1, 2000);
    let (mut units, mut secs) = (0u64, 0.0);
    while secs < target {
        let e = dispatch_n(ctx, pl, bg, groups, n);
        units += n as u64;
        secs += e;
        let per = (e / n as f64).max(1e-7);
        n = ((0.12 / per) as u32).clamp(1, 2000);
        if secs > target * 20.0 {
            break; // pathological (very slow adapter)
        }
    }
    (units, secs)
}

fn read_back(ctx: &Ctx, src: &wgpu::Buffer, size: u64) -> Option<Vec<u8>> {
    let staging = gpu_buffer(ctx, size, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
    let mut enc = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_buffer_to_buffer(src, 0, &staging, 0, size);
    ctx.queue.submit(std::iter::once(enc.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r.is_ok());
    });
    wait_idle(ctx);
    if !rx.recv_timeout(Duration::from_secs(30)).ok()? {
        return None;
    }
    let data = staging.slice(..).get_mapped_range().ok()?.to_vec();
    staging.unmap();
    Some(data)
}

fn errors_clear(ctx: &Ctx) -> bool {
    let mut e = ctx.errors.lock().unwrap_or_else(|p| p.into_inner());
    let had = !e.is_empty();
    if had && std::env::var_os("SYSCHECK_GPU_DEBUG").is_some() {
        for m in e.iter() {
            eprintln!("gpu error: {m}");
        }
    }
    e.clear();
    had
}

// ───────────────────────── benchmarks ─────────────────────────

const THREADS: u32 = 1 << 20; // 1M invocations per ALU dispatch
const ALU_ITERS: u32 = 128;

fn alu(ctx: &Ctx, wgsl: &str, float: bool, secs: f64) -> Option<f64> {
    let pl = pipeline(ctx, wgsl);
    let ub = gpu_buffer(ctx, 16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
    let out = gpu_buffer(ctx, THREADS as u64 * 4, wgpu::BufferUsages::STORAGE);
    let params: [u32; 4] = if float { [ALU_ITERS, 0.9999f32.to_bits(), 0.0001f32.to_bits(), 0.25f32.to_bits()] } else { [ALU_ITERS, 1664525, 1013904223, 17] };
    ctx.queue.write_buffer(&ub, 0, bytemuck::cast_slice(&params));
    let bg = bind(ctx, &pl, &[&ub, &out]);
    let (n, s) = timed(ctx, &pl, &bg, (THREADS / 256, 1, 1), secs);
    if errors_clear(ctx) || n == 0 || s <= 0.0 {
        return None;
    }
    let per_iter = if float { 16.0 } else { 32.0 }; // 8 chains x (2 flops | 4 int ops)
    Some(THREADS as f64 * ALU_ITERS as f64 * per_iter * n as f64 / s / 1e9)
}

fn mem_bw(ctx: &Ctx, mb: u64, secs: f64, max_buf: u64) -> Option<(f64, u64)> {
    let size = (mb << 20).min(max_buf).min(128 << 20) / 16 * 16;
    let pl = pipeline(ctx, COPY_WGSL);
    let src = gpu_buffer(ctx, size, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
    let dst = gpu_buffer(ctx, size, wgpu::BufferUsages::STORAGE);
    ctx.queue.write_buffer(&src, 0, &vec![0x3Fu8; 1 << 20]);
    let bg = bind(ctx, &pl, &[&src, &dst]);
    let groups = ((size / 16 / 256) as u32).max(1);
    let (x, y) = if groups > 65535 { (65535, groups.div_ceil(65535)) } else { (groups, 1) };
    let (n, s) = timed(ctx, &pl, &bg, (x, y, 1), secs);
    if errors_clear(ctx) || n == 0 || s <= 0.0 {
        return None;
    }
    Some((2.0 * size as f64 * n as f64 / s / 1e9, size >> 20))
}

fn matmul_inputs(n: usize) -> (Vec<f32>, Vec<f32>) {
    let a = (0..n * n).map(|x| ((x / n * 7 + x % n * 3) % 13) as f32 / 13.0).collect();
    let b = (0..n * n).map(|x| ((x / n * 5 + x % n * 11) % 17) as f32 / 17.0).collect();
    (a, b)
}

fn matmul(ctx: &Ctx, n: u32, secs: f64) -> Option<(f64, bool)> {
    let nn = n as usize;
    let (a, b) = matmul_inputs(nn);
    let pl = pipeline(ctx, MATMUL_WGSL);
    let ub = gpu_buffer(ctx, 16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
    let bytes = (nn * nn * 4) as u64;
    let (ba, bb) = (gpu_buffer(ctx, bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST), gpu_buffer(ctx, bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST));
    let bc = gpu_buffer(ctx, bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    ctx.queue.write_buffer(&ub, 0, bytemuck::cast_slice(&[n, 0u32, 0, 0]));
    ctx.queue.write_buffer(&ba, 0, bytemuck::cast_slice(&a));
    ctx.queue.write_buffer(&bb, 0, bytemuck::cast_slice(&b));
    let bg = bind(ctx, &pl, &[&ub, &ba, &bb, &bc]);
    let groups = (n / 16, n / 16, 1);
    dispatch_n(ctx, &pl, &bg, groups, 1);
    // correctness against a CPU reference at a few positions
    let c: Vec<f32> = bytemuck::cast_slice(&read_back(ctx, &bc, bytes)?).to_vec();
    let mut ok = c.len() == nn * nn;
    for &(i, j) in &[(0usize, 0usize), (nn - 1, nn - 1), (nn / 2, nn / 3), (3, nn - 5), (nn - 2, 7)] {
        if !ok {
            break;
        }
        let want: f64 = (0..nn).map(|k| a[i * nn + k] as f64 * b[k * nn + j] as f64).sum();
        ok &= (c[i * nn + j] as f64 - want).abs() <= 1e-3 * want.abs().max(1.0);
    }
    let (cnt, s) = timed(ctx, &pl, &bg, groups, secs);
    if errors_clear(ctx) || cnt == 0 || s <= 0.0 {
        return None;
    }
    Some((2.0 * (nn as f64).powi(3) * cnt as f64 / s / 1e9, ok))
}

fn transfer(ctx: &Ctx, quick: bool) -> Option<(f64, f64)> {
    let size: u64 = if quick { 32 << 20 } else { 64 << 20 };
    let buf = gpu_buffer(ctx, size, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC);
    let data = vec![0x5Au8; size as usize];
    let (mut h2d, mut d2h) = (0f64, 0f64);
    for _ in 0..3 {
        let t = Instant::now();
        ctx.queue.write_buffer(&buf, 0, &data);
        ctx.queue.submit(std::iter::empty());
        wait_idle(ctx);
        h2d = h2d.max(size as f64 / t.elapsed().as_secs_f64() / 1e9);
        let t = Instant::now();
        read_back(ctx, &buf, size)?;
        d2h = d2h.max(size as f64 / t.elapsed().as_secs_f64() / 1e9);
    }
    (!errors_clear(ctx)).then_some((h2d, d2h))
}

fn latency(ctx: &Ctx) -> Option<(f64, f64)> {
    let pl = pipeline(ctx, NOOP_WGSL);
    let out = gpu_buffer(ctx, 16, wgpu::BufferUsages::STORAGE);
    let bg = bind(ctx, &pl, &[&out]);
    for _ in 0..10 {
        dispatch_n(ctx, &pl, &bg, (1, 1, 1), 1);
    }
    let mut v: Vec<u32> = (0..300).map(|_| (dispatch_n(ctx, &pl, &bg, (1, 1, 1), 1) * 1e9).min(u32::MAX as f64) as u32).collect();
    if errors_clear(ctx) {
        return None;
    }
    v.sort_unstable();
    Some((crate::util::percentile(&v, 50.0) / 1000.0, crate::util::percentile(&v, 99.0) / 1000.0))
}

const RT_SLICES: u32 = 32;

fn rt_gpu(ctx: &Ctx, info: &GpuAdapterInfo, hw_ray_query: bool, secs: f64) -> Option<RtGpu> {
    let sc = rt::scene();
    let mut flat: Vec<f32> = Vec::new();
    for p in sc.spheres.iter().chain(sc.planes.iter()) {
        flat.extend_from_slice(&p.a);
        flat.extend_from_slice(&p.col);
    }
    let (w, h) = (rt::W as u32, rt::H as u32);
    let pl = pipeline(ctx, RT_WGSL);
    let ub = gpu_buffer(ctx, 32, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
    let prims = gpu_buffer(ctx, (flat.len() * 4) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
    let acc_bytes = (RT_SLICES * w * h * 3 * 4) as u64;
    let acc = gpu_buffer(ctx, acc_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    ctx.queue.write_buffer(&prims, 0, bytemuck::cast_slice(&flat));
    let params = [0u32, 0, w, h, sc.spheres.len() as u32, sc.planes.len() as u32, 0, 0];
    ctx.queue.write_buffer(&ub, 0, bytemuck::cast_slice(&params));
    let bg = bind(ctx, &pl, &[&ub, &prims, &acc]);
    let groups = (w.div_ceil(8), h.div_ceil(8), 4); // validation: passes 0..3 like the CPU reference
    dispatch_n(ctx, &pl, &bg, groups, 1);
    let img: Vec<f32> = bytemuck::cast_slice(&read_back(ctx, &acc, acc_bytes)?).to_vec();
    let px = (w * h * 3) as usize;
    let luma = img[..4 * px].iter().map(|v| *v as f64).sum::<f64>() / (4 * px) as f64;
    let finite = img[..4 * px].iter().all(|v| v.is_finite());
    let (_, rays_per_pass, cpu_luma, _) = rt::reference();
    let groups = (w.div_ceil(8), h.div_ceil(8), RT_SLICES);
    let (n, s) = timed(ctx, &pl, &bg, groups, secs);
    if errors_clear(ctx) || n == 0 || s <= 0.0 || !finite {
        return None;
    }
    let passes = n * RT_SLICES as u64;
    Some(RtGpu {
        adapter: info.name.clone(),
        backend: info.backend.clone(),
        mrays: rays_per_pass as f64 * passes as f64 / s / 1e6,
        passes: passes.min(u32::MAX as u64) as u32,
        mean_luma: luma,
        luma_matches_cpu: (luma - cpu_luma).abs() / cpu_luma.max(1e-9) < 0.03,
        hw_ray_query,
    })
}

// ───────────────────────── entry points ─────────────────────────

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle())
}

fn adapter_info(a: &wgpu::Adapter) -> GpuAdapterInfo {
    let i = a.get_info();
    GpuAdapterInfo {
        name: i.name.clone(),
        vendor_id: i.vendor,
        device_id: i.device,
        device_type: match i.device_type {
            wgpu::DeviceType::DiscreteGpu => "discrete",
            wgpu::DeviceType::IntegratedGpu => "integrated",
            wgpu::DeviceType::VirtualGpu => "virtual",
            wgpu::DeviceType::Cpu => "cpu",
            _ => "other",
        }
        .into(),
        backend: format!("{:?}", i.backend).to_lowercase(),
        driver: i.driver.clone(),
        driver_info: i.driver_info.clone(),
    }
}

pub fn list() -> Vec<GpuAdapterInfo> {
    pollster::block_on(instance().enumerate_adapters(wgpu::Backends::all())).iter().map(adapter_info).collect()
}

fn run_adapter(index: usize, name: &str, compute: bool, rt_stage: bool, quick: bool, secs: f64, bw_mb: u64, mm_n: u32) -> GpuAdapterResult {
    let adapters = pollster::block_on(instance().enumerate_adapters(wgpu::Backends::all()));
    let Some(adapter) = adapters.get(index).filter(|a| a.get_info().name == name).or_else(|| adapters.iter().find(|a| a.get_info().name == name)) else {
        return GpuAdapterResult { error: Some("adapter not found".into()), ..Default::default() };
    };
    let info = adapter_info(adapter);
    let mut res = GpuAdapterResult { info: info.clone(), ..Default::default() };
    let limits = adapter.limits();
    res.max_buffer_mb = Some(limits.max_buffer_size >> 20);
    let hw_rq = adapter.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY);
    let desc = wgpu::DeviceDescriptor { required_limits: limits.clone(), ..Default::default() };
    let (device, queue) = match pollster::block_on(adapter.request_device(&desc)) {
        Ok(x) => x,
        Err(e) => {
            res.error = Some(format!("device: {e}"));
            return res;
        }
    };
    let errors = Arc::new(Mutex::new(Vec::new()));
    let e2 = errors.clone();
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        if let Ok(mut v) = e2.lock() {
            v.push(e.to_string());
        }
    }));
    let ctx = Ctx { device, queue, errors };

    if compute {
        res.fp32_gflops = alu(&ctx, FMA_WGSL, true, secs);
        res.int_gops = alu(&ctx, INT_WGSL, false, secs);
        if let Some((g, mb)) = mem_bw(&ctx, bw_mb, secs, limits.max_buffer_size.min(limits.max_storage_buffer_binding_size as u64)) {
            res.mem_gbs = Some(g);
            res.mem_buffer_mb = Some(mb);
        }
        if let Some((g, ok)) = matmul(&ctx, mm_n, secs) {
            res.matmul_gflops = Some(g);
            res.matmul_n = Some(mm_n);
            res.matmul_ok = Some(ok);
        }
        if let Some((a, b)) = transfer(&ctx, quick) {
            res.h2d_gbs = Some(a);
            res.d2h_gbs = Some(b);
        }
        if let Some((a, b)) = latency(&ctx) {
            res.latency_p50_us = Some(a);
            res.latency_p99_us = Some(b);
        }
    }
    if rt_stage {
        res.rt = rt_gpu(&ctx, &info, hw_rq, secs * 2.0);
    }
    let all_failed = res.fp32_gflops.is_none() && res.mem_gbs.is_none() && res.matmul_gflops.is_none() && res.rt.is_none();
    if all_failed {
        res.error = Some("no GPU test could run".into());
    }
    res
}

/// `syscheck __gpu list` / `syscheck __gpu run --index N --name NAME --stages compute,rt [--quick]`
pub fn worker_main(args: &[String]) {
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("list") => println!("{}", serde_json::to_string(&list()).unwrap_or_else(|_| "[]".into())),
        Some("run") => {
            let quick = args.iter().any(|a| a == "--quick");
            let stages = get("--stages").unwrap_or_default();
            let (secs, bw, mm) = if quick { (0.15, 64, 512) } else { (0.4, 128, 1024) };
            let r = run_adapter(get("--index").and_then(|s| s.parse().ok()).unwrap_or(0), &get("--name").unwrap_or_default(), stages.contains("compute"), stages.contains("rt"), quick, secs, bw, mm);
            println!("{}", serde_json::to_string(&r).unwrap_or_default());
        }
        _ => std::process::exit(2),
    }
}
