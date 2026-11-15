//! Benchmarks. Every test is a fixed, deterministic workload implemented natively in Rust,
//! so the result does not depend on an interpreter version.

pub mod cpu;
pub mod disk;
pub mod gpu;
#[cfg(feature = "gpu")]
pub mod gpu_worker;
pub mod mem;
pub mod net;
pub mod rt;
pub mod video;

/// All knobs that control how long / how hard the tests run.
/// `quick` trades precision for speed (about 25 s instead of about 2 min).
#[derive(Clone, Debug)]
pub struct Profile {
    pub quick: bool,
    pub cpu_secs: f64,
    pub cpu_reps: usize,
    pub mem_reps: usize,
    pub lat_steps: usize,
    pub lat_max_bytes: u64,
    pub disk_file_mb: u64,
    pub disk_point_secs: f64,
    pub disk_scen_secs: f64,
    pub disk_qds: Vec<usize>,
    pub disk_sizes: Vec<u32>,
    pub small_files: usize,
    pub sustained_secs: u32,
    pub disk_sync: bool,
    pub net_secs: f64,
    pub net_rtt_secs: f64,
    pub gw_samples: usize,
    /// frames per video encode run, whether to also test HEVC, whether to run the parallel-stream test
    pub rt_secs: f64,
    /// Target seconds per GPU measurement and size of the GPU memory / matrix tests.
    pub video_frames: u32,
    pub video_hevc: bool,
    pub video_parallel: bool,
}

impl Profile {
    pub fn new(quick: bool) -> Self {
        // 0.5 KB ... 8 MB, the usual range of block-size sweeps
        let sizes: Vec<u32> = (0..15).map(|i| 512u32 << i).collect();
        if quick {
            Profile {
                quick,
                cpu_secs: 0.35,
                cpu_reps: 1,
                mem_reps: 2,
                lat_steps: 400_000,
                lat_max_bytes: 64 << 20,
                disk_file_mb: 128,
                disk_point_secs: 0.15,
                disk_scen_secs: 0.6,
                disk_qds: vec![4],
                disk_sizes: sizes,
                small_files: 500,
                sustained_secs: 0,
                disk_sync: false,
                net_secs: 2.0,
                net_rtt_secs: 0.25,
                gw_samples: 10,
                rt_secs: 0.6,
                video_frames: 90,
                video_hevc: false,
                video_parallel: false,
            }
        } else {
            Profile {
                quick,
                cpu_secs: 0.6,
                cpu_reps: 3,
                mem_reps: 5,
                lat_steps: 2_000_000,
                lat_max_bytes: 256 << 20,
                disk_file_mb: 512,
                disk_point_secs: 0.5,
                disk_scen_secs: 2.0,
                disk_qds: vec![1, 4, 32],
                disk_sizes: sizes,
                small_files: 2000,
                sustained_secs: 0,
                disk_sync: false,
                net_secs: 5.0,
                net_rtt_secs: 0.5,
                gw_samples: 30,
                rt_secs: 1.5,
                video_frames: 240,
                video_hevc: true,
                video_parallel: true,
            }
        }
    }
}
