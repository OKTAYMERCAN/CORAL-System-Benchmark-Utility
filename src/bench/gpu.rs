//! GPU benchmark (parent side). The actual GPU work happens in a child process of this same
//! program (`syscheck __gpu ...`, see `gpu_worker.rs`): graphics drivers sometimes crash or hang,
//! and this way a broken driver can only fail the GPU test, never the whole run.
//!
//! What is measured (portable WGSL compute shaders through wgpu, so one test covers NVIDIA, AMD,
//! Intel, Apple and mobile GPUs on Vulkan / Metal / DirectX 12):
//! FP32 throughput (GFLOPS), integer throughput, memory bandwidth, 16x16-tiled matrix multiply,
//! host<->device copy speed, submit round-trip latency, and the same path-traced scene as the CPU
//! ray-tracing test (`rt.rs`) running on the shader cores.
//! Hardware ray-tracing units (RT cores) are only detected, not benchmarked yet.

use crate::bench::{rt::RtGpu, Profile};
use crate::ui::Ui;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GpuAdapterInfo {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// discrete | integrated | virtual | cpu | other
    pub device_type: String,
    /// vulkan | metal | dx12 | gl
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GpuAdapterResult {
    pub info: GpuAdapterInfo,
    /// Set when the adapter could not be used (driver crash, timeout, no compute support, ...).
    pub error: Option<String>,
    pub max_buffer_mb: Option<u64>,
    pub fp32_gflops: Option<f64>,
    pub int_gops: Option<f64>,
    /// copy kernel, read + write bytes
    pub mem_gbs: Option<f64>,
    pub mem_buffer_mb: Option<u64>,
    pub matmul_gflops: Option<f64>,
    pub matmul_n: Option<u32>,
    /// The matrix result matched a CPU reference.
    pub matmul_ok: Option<bool>,
    pub h2d_gbs: Option<f64>,
    pub d2h_gbs: Option<f64>,
    pub latency_p50_us: Option<f64>,
    pub latency_p99_us: Option<f64>,
    pub rt: Option<RtGpu>,
}

impl GpuAdapterResult {
    pub fn is_software(&self) -> bool {
        self.info.device_type == "cpu"
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GpuResults {
    /// Compiled with GPU support?
    pub supported: bool,
    /// Number of adapters the system reports (before duplicates are merged).
    pub listed: u32,
    pub adapters: Vec<GpuAdapterResult>,
}

impl GpuResults {
    /// The strongest real (non-software) GPU result.
    pub fn best(&self) -> Option<&GpuAdapterResult> {
        self.adapters
            .iter()
            .filter(|a| !a.is_software() && a.error.is_none())
            .max_by(|a, b| a.fp32_gflops.unwrap_or(0.0).partial_cmp(&b.fp32_gflops.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Stages {
    pub compute: bool,
    pub rt: bool,
}

/// Lower is better. Real GPU APIs first; OpenGL last because it hides features.
fn backend_rank(b: &str) -> u8 {
    match b {
        "vulkan" | "metal" => 0,
        "dx12" => 1,
        "gl" => 2,
        _ => 3,
    }
}

fn type_rank(t: &str) -> u8 {
    match t {
        "discrete" => 0,
        "integrated" => 1,
        "virtual" => 2,
        "other" => 3,
        _ => 4,
    }
}

/// Pick which adapters to test: one entry per physical GPU (best API), real GPUs before software
/// renderers; at most `max` real GPUs. Returns indices into `list`.
pub fn select_adapters(list: &[GpuAdapterInfo], max: usize) -> Vec<usize> {
    let mut best: Vec<usize> = Vec::new();
    for (i, a) in list.iter().enumerate() {
        let same = |b: &GpuAdapterInfo| if a.device_id != 0 || b.device_id != 0 { a.vendor_id == b.vendor_id && a.device_id == b.device_id } else { a.name == b.name };
        match best.iter().position(|&j| same(&list[j])) {
            Some(p) => {
                if backend_rank(&a.backend) < backend_rank(&list[best[p]].backend) {
                    best[p] = i;
                }
            }
            None => best.push(i),
        }
    }
    let real: Vec<usize> = best.iter().copied().filter(|&i| list[i].device_type != "cpu").collect();
    let mut chosen = if real.is_empty() { best.into_iter().take(1).collect::<Vec<_>>() } else { real };
    chosen.sort_by_key(|&i| (type_rank(&list[i].device_type), backend_rank(&list[i].backend)));
    chosen.truncate(max.max(1));
    chosen
}

/// Run this program again with `args` and capture its output; kill it after `timeout`.
fn child(args: &[String], timeout: Duration) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut c = Command::new(exe).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let mut out = c.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let t = Instant::now();
    loop {
        match c.try_wait() {
            Ok(Some(st)) => {
                let s = reader.join().unwrap_or_default();
                return if st.success() { Ok(s) } else { Err("crashed".into()) };
            }
            Ok(None) if t.elapsed() > timeout => {
                let _ = c.kill();
                let _ = c.wait();
                return Err("timeout".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn list_adapters() -> Result<Vec<GpuAdapterInfo>, String> {
    let out = child(&["__gpu".into(), "list".into()], Duration::from_secs(30))?;
    serde_json::from_str(out.trim()).map_err(|e| e.to_string())
}

pub fn run(ui: &Ui, prof: &Profile, stages: Stages) -> GpuResults {
    let mut res = GpuResults { supported: cfg!(feature = "gpu"), ..Default::default() };
    if !res.supported {
        return res;
    }
    ui.progress(&format!("{}: {}", ui.t("prog_gpu"), ui.t("l_gpu_detect")));
    let list = list_adapters().unwrap_or_default();
    res.listed = list.len() as u32;
    let chosen = select_adapters(&list, 2);
    let mut st = Vec::new();
    if stages.compute {
        st.push("compute");
    }
    if stages.rt {
        st.push("rt");
    }
    for (n, &i) in chosen.iter().enumerate() {
        let a = &list[i];
        ui.progress(&format!("{}: {} [{}/{}]", ui.t("prog_gpu"), a.name, n + 1, chosen.len()));
        let mut args: Vec<String> = vec!["__gpu".into(), "run".into(), "--index".into(), i.to_string(), "--name".into(), a.name.clone(), "--stages".into(), st.join(",")];
        if prof.quick {
            args.push("--quick".into());
        }
        let timeout = Duration::from_secs(if prof.quick { 90 } else { 240 });
        let parsed = child(&args, timeout).and_then(|o| serde_json::from_str::<GpuAdapterResult>(o.trim()).map_err(|e| e.to_string()));
        res.adapters.push(match parsed {
            Ok(r) => r,
            Err(e) => GpuAdapterResult { info: a.clone(), error: Some(e), ..Default::default() },
        });
    }
    ui.clear_progress();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(name: &str, vendor: u32, device: u32, ty: &str, backend: &str) -> GpuAdapterInfo {
        GpuAdapterInfo { name: name.into(), vendor_id: vendor, device_id: device, device_type: ty.into(), backend: backend.into(), ..Default::default() }
    }

    #[test]
    fn same_gpu_on_two_apis_is_tested_once_with_the_best_api() {
        let l = vec![ad("RTX 4090", 0x10de, 0x2684, "discrete", "gl"), ad("RTX 4090", 0x10de, 0x2684, "discrete", "vulkan"), ad("llvmpipe", 0x10005, 0, "cpu", "gl")];
        assert_eq!(select_adapters(&l, 2), vec![1]);
    }

    #[test]
    fn discrete_is_preferred_over_integrated_and_software_is_last_resort() {
        let l = vec![ad("Intel UHD", 0x8086, 0x9a49, "integrated", "vulkan"), ad("RTX 3060", 0x10de, 0x2503, "discrete", "vulkan"), ad("Basic Render", 0x1414, 0x8c, "cpu", "dx12")];
        assert_eq!(select_adapters(&l, 2), vec![1, 0]);
        assert_eq!(select_adapters(&l, 1), vec![1]);
        let only_sw = vec![ad("llvmpipe", 1, 0, "cpu", "gl")];
        assert_eq!(select_adapters(&only_sw, 2), vec![0]);
        assert!(select_adapters(&[], 2).is_empty());
    }

    #[test]
    fn best_ignores_software_and_failed_adapters() {
        let mk = |ty: &str, g: f64, err: bool| GpuAdapterResult { info: GpuAdapterInfo { device_type: ty.into(), ..Default::default() }, fp32_gflops: Some(g), error: err.then(|| "x".to_string()), ..Default::default() };
        let r = GpuResults { supported: true, listed: 3, adapters: vec![mk("cpu", 9999.0, false), mk("discrete", 5000.0, true), mk("integrated", 800.0, false)] };
        assert_eq!(r.best().unwrap().fp32_gflops, Some(800.0));
    }
}
