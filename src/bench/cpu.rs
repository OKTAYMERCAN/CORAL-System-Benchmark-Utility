//! CPU benchmark: six fixed native kernels run single-threaded (pinned to the fastest core)
//! and multi-threaded (one pinned thread per usable CPU), plus hardware-acceleration probes
//! (SIMD, SHA) and a per-cluster check for big.LITTLE / hybrid CPUs.
//!
//! The kernels are deterministic: same inputs, same instruction mix on every machine. Rates
//! are "work units per second", so slow devices (RISC-V boards) finish as quickly as fast ones.

use crate::bench::Profile;
use crate::hw::{self, CpuInfo};
use crate::ui::Ui;
use crate::util::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::hint::black_box;
use std::sync::Barrier;
use std::time::Instant;

// ───────────────────────── result types ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct KernelResult {
    pub name: String,
    pub unit: String,
    pub single: f64,
    pub multi: f64,
    /// Run-to-run variation of the single-core measurement (percent).
    pub noise_pct: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GroupResult {
    pub label: String,
    pub cpu: u32,
    pub int_mops: f64,
    pub matmul_mflops: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ShaResult {
    pub soft_mbs: f64,
    pub hw_mbs: Option<f64>,
    pub hw_feature: Option<String>,
    pub speedup: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SimdResult {
    pub feature: String,
    pub base_mflops: f64,
    pub accel_mflops: f64,
    pub speedup: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CpuResults {
    pub threads: u32,
    pub pinned: bool,
    pub kernels: Vec<KernelResult>,
    /// Geometric mean of the six single-core kernel rates.
    pub single_geo: f64,
    pub multi_geo: f64,
    /// multi / single (ideal = number of threads on identical cores).
    pub scaling: f64,
    pub efficiency_pct: f64,
    pub groups: Vec<GroupResult>,
    pub sha: ShaResult,
    pub simd: Option<SimdResult>,
    pub max_noise_pct: f64,
    /// Repetitions per kernel (noise is only meaningful with 2 or more).
    #[serde(default)]
    pub reps: u32,
}

impl CpuResults {
    pub fn kernel(&self, name: &str) -> Option<&KernelResult> {
        self.kernels.iter().find(|k| k.name == name)
    }
}

// ───────────────────────── kernels ─────────────────────────

pub struct Scratch {
    rng: Rng,
    sort_buf: Vec<u32>,
    mat_a: Vec<f32>,
    mat_b: Vec<f32>,
    mat_c: Vec<f32>,
    sha_buf: Vec<u8>,
    text: Vec<u8>,
    sink: u64,
}

const MAT_N: usize = 64;
const SORT_N: usize = 16_384;
const SHA_BYTES: usize = 64 * 1024;
const TEXT_BYTES: usize = 256 * 1024;

const WORDS: [&str; 24] = [
    "server", "virt", "storage", "network", "video", "stream", "kernel", "memory", "thread", "buffer", "cluster", "backup", "volume",
    "packet", "socket", "daemon", "cache", "disk", "virtual", "machine", "container", "journal", "mirror", "snapshot",
];

impl Scratch {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mat = |n: usize, r: &mut Rng| -> Vec<f32> { (0..n).map(|_| ((r.next_u64() >> 40) as f32) / 16_777_216.0).collect() };
        let mat_a = mat(MAT_N * MAT_N, &mut rng);
        let mat_b = mat(MAT_N * MAT_N, &mut rng);
        let mut sha_buf = vec![0u8; SHA_BYTES];
        rng.fill(&mut sha_buf);
        let mut text = Vec::with_capacity(TEXT_BYTES + 16);
        while text.len() < TEXT_BYTES {
            text.extend_from_slice(WORDS[rng.below(WORDS.len() as u64) as usize].as_bytes());
            text.push(b' ');
        }
        text.truncate(TEXT_BYTES);
        Scratch { rng, sort_buf: vec![0; SORT_N], mat_a, mat_b, mat_c: vec![0.0; MAT_N * MAT_N], sha_buf, text, sink: 1 }
    }
}

/// Integer ALU: four independent dependency chains of multiply / shift / xor / rotate / add.
fn k_int(s: &mut Scratch) -> f64 {
    const K: u64 = 0x2545_F491_4F6C_DD1D;
    let (mut a, mut b, mut c, mut d) = (s.sink | 1, 0x9E37_79B9_7F4A_7C15u64, 0xD1B5_4A32_D192_ED03u64, 0x1234_5678_9ABC_DEF1u64);
    for _ in 0..65_536u32 {
        a = (a.wrapping_mul(K) ^ (a >> 29)).rotate_left(17).wrapping_add(0x9E37);
        b = (b.wrapping_mul(K) ^ (b >> 29)).rotate_left(17).wrapping_add(0x79B9);
        c = (c.wrapping_mul(K) ^ (c >> 29)).rotate_left(17).wrapping_add(0x7F4A);
        d = (d.wrapping_mul(K) ^ (d >> 29)).rotate_left(17).wrapping_add(0x7C15);
    }
    s.sink = black_box(a ^ b ^ c ^ d);
    65_536.0 * 4.0 * 5.0 // ops
}

/// Scalar double-precision FP: eight independent multiply-add chains.
fn k_fp(s: &mut Scratch) -> f64 {
    let mut acc = [1.0f64, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7];
    for _ in 0..32_768u32 {
        for x in acc.iter_mut() {
            *x = *x * 0.999_999_1 + 0.000_000_9;
        }
    }
    s.sink ^= black_box(acc.iter().sum::<f64>()).to_bits();
    32_768.0 * 8.0 * 2.0 // flops
}

/// Single-precision 64x64 matrix multiply (auto-vectorised with the baseline ISA: SSE2 / NEON).
fn k_matmul(s: &mut Scratch) -> f64 {
    let Scratch { mat_a, mat_b, mat_c, .. } = s;
    mat_c.fill(0.0);
    for i in 0..MAT_N {
        for k in 0..MAT_N {
            let aik = mat_a[i * MAT_N + k];
            let brow = &mat_b[k * MAT_N..(k + 1) * MAT_N];
            let crow = &mut mat_c[i * MAT_N..(i + 1) * MAT_N];
            for j in 0..MAT_N {
                crow[j] += aik * brow[j];
            }
        }
    }
    black_box(&mat_c);
    (2 * MAT_N * MAT_N * MAT_N) as f64
}

/// Same multiply, compiled for AVX2 + FMA (only called after runtime detection).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn matmul_avx2(a: &[f32], b: &[f32], c: &mut [f32]) {
    c.fill(0.0);
    for i in 0..MAT_N {
        for k in 0..MAT_N {
            let aik = a[i * MAT_N + k];
            let brow = &b[k * MAT_N..(k + 1) * MAT_N];
            let crow = &mut c[i * MAT_N..(i + 1) * MAT_N];
            for j in 0..MAT_N {
                crow[j] = aik.mul_add(brow[j], crow[j]);
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn k_matmul_avx2(s: &mut Scratch) -> f64 {
    let Scratch { mat_a, mat_b, mat_c, .. } = s;
    // SAFETY: the caller only selects this kernel after is_x86_feature_detected!("avx2" && "fma").
    unsafe { matmul_avx2(mat_a, mat_b, mat_c) };
    black_box(&mat_c);
    (2 * MAT_N * MAT_N * MAT_N) as f64
}

/// Sort 16 384 pseudo-random u32 (generation included, identical on every machine).
fn k_sort(s: &mut Scratch) -> f64 {
    let Scratch { rng, sort_buf, .. } = s;
    for x in sort_buf.iter_mut() {
        *x = rng.next_u64() as u32;
    }
    sort_buf.sort_unstable();
    s.sink ^= black_box(s.sort_buf[SORT_N / 2]) as u64;
    SORT_N as f64
}

/// Portable SHA-256 (no CPU instructions) over 64 KiB.
fn k_sha_soft(s: &mut Scratch) -> f64 {
    let d = sha256_soft(&s.sha_buf);
    s.sink ^= black_box(u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]));
    SHA_BYTES as f64
}

/// SHA-256 through the `sha2` crate, which uses SHA-NI / ARMv8 SHA2 when the CPU has them.
fn k_sha_hw(s: &mut Scratch) -> f64 {
    let d = Sha256::digest(&s.sha_buf);
    s.sink ^= black_box(u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]));
    SHA_BYTES as f64
}

/// DEFLATE level 6 of 256 KiB of text-like data (pure-Rust miniz_oxide backend: same code everywhere).
fn k_deflate(s: &mut Scratch) -> f64 {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut enc = ZlibEncoder::new(Vec::with_capacity(TEXT_BYTES / 2), Compression::new(6));
    let _ = enc.write_all(&s.text);
    let out = enc.finish().unwrap_or_default();
    s.sink ^= black_box(out.len()) as u64;
    TEXT_BYTES as f64
}

pub struct Kernel {
    pub name: &'static str,
    pub unit: &'static str,
    /// raw units per second -> display units
    pub div: f64,
    pub run: fn(&mut Scratch) -> f64,
}

pub const KERNELS: [Kernel; 6] = [
    Kernel { name: "int_alu", unit: "Mops/s", div: 1e6, run: k_int },
    Kernel { name: "fp_f64", unit: "MFLOPS", div: 1e6, run: k_fp },
    Kernel { name: "matmul_f32", unit: "MFLOPS", div: 1e6, run: k_matmul },
    Kernel { name: "sort_u32", unit: "Melem/s", div: 1e6, run: k_sort },
    Kernel { name: "sha256_soft", unit: "MB/s", div: 1e6, run: k_sha_soft },
    Kernel { name: "deflate", unit: "MB/s", div: 1e6, run: k_deflate },
];

// ───────────────────────── portable SHA-256 ─────────────────────────

const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha_compress(h: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA_K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *x = x.wrapping_add(y);
    }
}

pub fn sha256_soft(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut chunks = data.chunks_exact(64);
    for c in &mut chunks {
        sha_compress(&mut h, c);
    }
    let rem = chunks.remainder();
    let mut last = [0u8; 128];
    last[..rem.len()].copy_from_slice(rem);
    last[rem.len()] = 0x80;
    let total = if rem.len() < 56 { 64 } else { 128 };
    last[total - 8..total].copy_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for c in last[..total].chunks_exact(64) {
        sha_compress(&mut h, c);
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// Name of the hardware SHA-256 feature the CPU offers (what the `sha2` crate will use), if any.
pub fn sha_hw_feature() -> Option<&'static str> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sha") && is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("ssse3") {
            return Some("sha-ni");
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("sha2") {
            return Some("armv8-sha2");
        }
    }
    None
}

#[cfg(target_arch = "x86_64")]
fn simd_feature() -> Option<&'static str> {
    if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
        Some("avx2+fma")
    } else {
        None
    }
}

// ───────────────────────── measurement ─────────────────────────

/// Raw work units per second for one kernel over `secs` (after a short warm-up).
fn measure(run: fn(&mut Scratch) -> f64, s: &mut Scratch, secs: f64) -> f64 {
    let w = Instant::now();
    while w.elapsed().as_secs_f64() < 0.05 {
        black_box(run(s));
    }
    let mut total = 0.0;
    let t0 = Instant::now();
    loop {
        total += run(s);
        let e = t0.elapsed().as_secs_f64();
        if e >= secs {
            return total / e;
        }
    }
}

fn reps_of(run: fn(&mut Scratch) -> f64, s: &mut Scratch, secs: f64, reps: usize) -> Vec<f64> {
    (0..reps.max(1)).map(|_| measure(run, s, secs)).collect()
}

fn group_label(i: usize, g: &hw::CoreGroup) -> String {
    match (&g.kind, &g.part) {
        (Some(k), Some(p)) => format!("{k}-core {p}"),
        (Some(k), None) => format!("{k}-core"),
        (None, Some(p)) => p.clone(),
        _ => format!("cluster {}", i + 1),
    }
}

pub fn run(ui: &Ui, cpu: &CpuInfo, prof: &Profile) -> CpuResults {
    let threads = cpu.usable.max(1) as usize;
    let secs = prof.cpu_secs;
    let reps = prof.cpu_reps;

    // ---- single thread (pinned to the fastest core) -------------------------------------
    struct Single {
        pinned: bool,
        rates: Vec<Vec<f64>>,
        sha_hw: Option<(f64, &'static str)>,
        simd: Option<(f64, f64, &'static str)>,
    }
    let single = std::thread::scope(|sc| {
        sc.spawn(|| {
            let pinned = hw::pin_to_cpu(cpu.best_cpu);
            let mut s = Scratch::new(1);
            let mut rates = Vec::new();
            for (i, k) in KERNELS.iter().enumerate() {
                ui.progress(&format!("{}: {} [{}/{}]", ui.t("prog_cpu_single"), k.name, i + 1, KERNELS.len()));
                rates.push(reps_of(k.run, &mut s, secs, reps));
            }
            ui.progress(&ui.t("prog_cpu_accel"));
            let sha_hw = sha_hw_feature().map(|f| (median(&reps_of(k_sha_hw, &mut s, secs, reps)), f));
            #[cfg(target_arch = "x86_64")]
            let simd = simd_feature().map(|f| {
                let base = median(&reps_of(k_matmul, &mut s, secs, reps));
                let accel = median(&reps_of(k_matmul_avx2, &mut s, secs, reps));
                (base, accel, f)
            });
            #[cfg(not(target_arch = "x86_64"))]
            let simd: Option<(f64, f64, &'static str)> = None;
            Single { pinned, rates, sha_hw, simd }
        })
        .join()
        .expect("single-thread benchmark panicked")
    });

    // ---- all threads ----------------------------------------------------------------------
    let mut multi_rates: Vec<f64> = Vec::new();
    for (i, k) in KERNELS.iter().enumerate() {
        ui.progress(&format!("{}: {} [{}/{}] ({} threads)", ui.t("prog_cpu_multi"), k.name, i + 1, KERNELS.len(), threads));
        let barrier = Barrier::new(threads);
        let per_thread: Vec<Vec<f64>> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..threads)
                .map(|t| {
                    let barrier = &barrier;
                    let cpu_id = cpu.cpu_list.get(t).copied();
                    sc.spawn(move || {
                        if let Some(c) = cpu_id {
                            hw::pin_to_cpu(c);
                        }
                        let mut s = Scratch::new(100 + t as u64);
                        (0..reps.max(1))
                            .map(|_| {
                                barrier.wait();
                                measure(k.run, &mut s, secs)
                            })
                            .collect::<Vec<f64>>()
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("benchmark thread panicked")).collect()
        });
        let per_rep: Vec<f64> = (0..reps.max(1)).map(|r| per_thread.iter().map(|t| t[r]).sum()).collect();
        multi_rates.push(median(&per_rep));
    }

    // ---- per cluster (only for heterogeneous CPUs) ---------------------------------------
    let mut groups = Vec::new();
    if cpu.groups.len() > 1 {
        for (i, g) in cpu.groups.iter().enumerate() {
            let Some(&c) = g.cpus.iter().find(|c| cpu.cpu_list.contains(c)) else { continue };
            ui.progress(&format!("{}: {} ({}/{})", ui.t("prog_cpu_groups"), group_label(i, g), i + 1, cpu.groups.len()));
            let r = std::thread::scope(|sc| {
                sc.spawn(|| {
                    hw::pin_to_cpu(c);
                    let mut s = Scratch::new(7);
                    (measure(k_int, &mut s, 0.3) / 1e6, measure(k_matmul, &mut s, 0.3) / 1e6)
                })
                .join()
                .unwrap_or((0.0, 0.0))
            });
            groups.push(GroupResult { label: group_label(i, g), cpu: c, int_mops: r.0, matmul_mflops: r.1 });
        }
    }
    ui.clear_progress();

    // ---- assemble ------------------------------------------------------------------------
    let mut kernels = Vec::new();
    let mut max_noise: f64 = 0.0;
    for (i, k) in KERNELS.iter().enumerate() {
        let noise = cv_percent(&single.rates[i]);
        max_noise = max_noise.max(noise);
        kernels.push(KernelResult {
            name: k.name.to_string(),
            unit: k.unit.to_string(),
            single: median(&single.rates[i]) / k.div,
            multi: multi_rates[i] / k.div,
            noise_pct: noise,
        });
    }
    let single_geo = geomean(&kernels.iter().map(|k| k.single).collect::<Vec<_>>());
    let multi_geo = geomean(&kernels.iter().map(|k| k.multi).collect::<Vec<_>>());
    let scaling = if single_geo > 0.0 { multi_geo / single_geo } else { 0.0 };

    let soft = kernels.iter().find(|k| k.name == "sha256_soft").map(|k| k.single).unwrap_or(0.0);
    let sha = match single.sha_hw {
        Some((r, f)) => {
            let hwm = r / 1e6;
            ShaResult { soft_mbs: soft, hw_mbs: Some(hwm), hw_feature: Some(f.to_string()), speedup: if soft > 0.0 { Some(hwm / soft) } else { None } }
        }
        None => ShaResult { soft_mbs: soft, ..Default::default() },
    };
    let simd = single.simd.map(|(b, a, f)| SimdResult { feature: f.to_string(), base_mflops: b / 1e6, accel_mflops: a / 1e6, speedup: if b > 0.0 { a / b } else { 0.0 } });

    CpuResults {
        threads: threads as u32,
        pinned: single.pinned,
        kernels,
        single_geo,
        multi_geo,
        scaling,
        efficiency_pct: scaling / threads as f64 * 100.0,
        groups,
        sha,
        simd,
        max_noise_pct: max_noise,
        reps: reps.max(1) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_sha_matches_reference_implementation() {
        for len in [0usize, 1, 3, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 1000, 65_536] {
            let mut data = vec![0u8; len];
            Rng::new(len as u64 + 1).fill(&mut data);
            let want = Sha256::digest(&data);
            assert_eq!(sha256_soft(&data).as_slice(), want.as_slice(), "length {len}");
        }
        // NIST vector
        let abc = sha256_soft(b"abc");
        assert_eq!(abc[0], 0xba);
        assert_eq!(abc[31], 0xad);
    }

    #[test]
    fn kernels_report_positive_work_and_are_deterministic() {
        let mut a = Scratch::new(1);
        let mut b = Scratch::new(1);
        for k in KERNELS.iter() {
            let wa = (k.run)(&mut a);
            let wb = (k.run)(&mut b);
            assert!(wa > 0.0, "{}", k.name);
            assert_eq!(wa, wb, "{}", k.name);
        }
        assert_eq!(a.sink, b.sink, "kernels must produce identical state");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_matmul_matches_baseline() {
        if simd_feature().is_none() {
            return;
        }
        let mut a = Scratch::new(5);
        let mut b = Scratch::new(5);
        k_matmul(&mut a);
        k_matmul_avx2(&mut b);
        for (x, y) in a.mat_c.iter().zip(&b.mat_c) {
            assert!((x - y).abs() < 1e-3, "{x} vs {y}");
        }
    }

    #[test]
    fn measure_returns_a_rate() {
        let mut s = Scratch::new(3);
        let r = measure(k_int, &mut s, 0.1);
        assert!(r > 1e6, "rate {r}");
    }
}
