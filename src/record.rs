//! The result record: one JSON document per run, plus a flat CSV row for spreadsheets / databases.
//!
//! Privacy: no host name, user name, IP/MAC address or file path is stored. The only free text is
//! the optional `--label` / `--notes` the user types.

use crate::bench::{cpu::CpuResults, disk::DiskResults, gpu::GpuResults, mem::MemResults, net::NetResults, rt::RtCpu, video::VideoResults};
use crate::hw::SystemInfo;
use crate::util::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;
/// Benchmark specification version. Results are comparable only within the same spec: bump it when
/// a workload changes. (Scoring constants may change without a bump; raw values are always stored.)
pub const SPEC: &str = "syscheck-spec-1";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
    pub spec: String,
    pub build_profile: String,
    pub rustc: String,
    pub target: String,
}

impl ToolInfo {
    pub fn current() -> Self {
        ToolInfo {
            name: "syscheck".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            spec: SPEC.into(),
            build_profile: env!("SYSCHECK_PROFILE").into(),
            rustc: env!("SYSCHECK_RUSTC").into(),
            target: env!("SYSCHECK_TARGET").into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Results {
    pub cpu: Option<CpuResults>,
    pub mem: Option<MemResults>,
    pub disk: Option<DiskResults>,
    pub net: Option<NetResults>,
    pub video: Option<VideoResults>,
    /// CPU path tracer (the GPU path tracer result is inside `gpu`).
    pub rt: Option<RtCpu>,
    pub gpu: Option<GpuResults>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Scores {
    /// virt, nas, video, game, ai (only categories that could be computed).
    pub categories: BTreeMap<String, f64>,
    /// Normalised 0-100 building blocks (cpu_single, ram, disk_iops, ...).
    pub components: BTreeMap<String, f64>,
    pub general: Option<f64>,
}

/// Language independent finding; `key` (and `args`) are translated when the report is rendered.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Finding {
    /// category: general | virt | nas | video | game | ai | gpu | disk | net
    pub cat: String,
    /// ok | warn | info
    pub level: String,
    pub key: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Record {
    pub schema: u32,
    pub tool: ToolInfo,
    pub created_utc: String,
    pub run_id: String,
    /// "full" or "quick"
    pub mode: String,
    pub notes: Option<String>,
    pub system: SystemInfo,
    pub results: Results,
    pub scores: Scores,
    pub findings: Vec<Finding>,
}

impl Record {
    pub fn new(quick: bool, system: SystemInfo, results: Results, notes: Option<String>) -> Self {
        let (scores, findings) = crate::score::compute(&system, &results);
        let now = unix_secs();
        let id = (unix_nanos() as u64 ^ ((std::process::id() as u64) << 20)) as u32;
        let mut rec = Record {
            schema: SCHEMA,
            tool: ToolInfo::current(),
            created_utc: rfc3339_utc(now),
            run_id: format!("{id:08x}"),
            mode: if quick { "quick" } else { "full" }.into(),
            notes,
            system,
            results,
            scores,
            findings,
        };
        if rec.tool.build_profile == "debug" {
            rec.findings.insert(0, Finding { cat: "general".into(), level: "warn".into(), key: "fnd_debug_build".into(), args: vec![] });
        }
        rec
    }

    pub fn file_stem(&self) -> String {
        let secs = unix_secs();
        format!("syscheck-{}-{}", stamp_compact(secs), self.run_id)
    }

    pub fn save_json(&self, dir: &Path) -> io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let p = dir.join(format!("{}.json", self.file_stem()));
        let s = serde_json::to_string_pretty(self).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        std::fs::write(&p, s)?;
        Ok(p)
    }

    pub fn load(path: &Path) -> io::Result<Record> {
        let s = std::fs::read_to_string(path)?;
        let r: Record = serde_json::from_str(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))?;
        Ok(r)
    }
}

// ───────────────────────── CSV ─────────────────────────

pub const CSV_COLUMNS: [&str; 67] = [
    "schema", "created_utc", "run_id", "mode", "tool_version", "spec", "rustc", "target", "build_profile", "label", "model", "os", "kernel",
    "arch", "cpu_model", "cpu_usable", "cpu_physical", "ram_mb", "cpu_single", "cpu_multi", "cpu_scaling", "sha_soft_mbs", "sha_hw_mbs",
    "simd_speedup", "mem_single_triad_gbs", "mem_multi_triad_gbs", "mem_multi_read_gbs", "mem_l1_ns", "mem_dram_ns", "disk_kind",
    "disk_direct", "disk_seq_read_mbs", "disk_seq_write_mbs", "disk_rand4k_read_qd1_iops", "disk_rand4k_read_qd32_iops",
    "disk_rand4k_write_qd32_iops", "disk_rand4k_read_qd1_p99_us", "disk_uhd_streams", "disk_small_create_per_s", "net_mode", "net_link_mbps",
    "net_up4_mbps", "net_down4_mbps", "net_rtt_p50_us", "score_virt", "score_nas", "score_video", "score_game", "score_ai", "score_general",
    "cpu_pt_single_mrays", "cpu_pt_multi_mrays", "video_x264_fps", "video_hw_encoder", "video_hw_fps", "video_hw_streams", "gpu_name", "gpu_backend",
    "gpu_type", "gpu_fp32_gflops", "gpu_mem_gbs", "gpu_matmul_gflops", "gpu_rt_mrays", "score_gfx", "score_rt",
    "notes", "findings_warn",
];

fn f2(x: f64) -> String {
    if x.abs() >= 100.0 {
        format!("{x:.1}")
    } else {
        format!("{x:.3}")
    }
}
fn o2(x: Option<f64>) -> String {
    x.map(f2).unwrap_or_default()
}

pub fn csv_escape(s: &str) -> String {
    if s.contains(|c| c == ',' || c == '"' || c == '\n' || c == '\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn csv_header() -> String {
    CSV_COLUMNS.join(",")
}

pub fn csv_row(r: &Record) -> String {
    let s = &r.system;
    let cpu = r.results.cpu.as_ref();
    let mem = r.results.mem.as_ref();
    let disk = r.results.disk.as_ref();
    let net = r.results.net.as_ref();
    let video = r.results.video.as_ref();
    // the GPU row describes the strongest real GPU, or the software renderer when that is all there is
    let gpu = r.results.gpu.as_ref().and_then(|g| g.best().or_else(|| g.adapters.first()));
    let sc = |k: &str| r.scores.categories.get(k).copied();
    let warns = r.findings.iter().filter(|f| f.level == "warn").map(|f| f.key.trim_start_matches("fnd_").to_string()).collect::<Vec<_>>().join(";");
    let cells: Vec<String> = vec![
        r.schema.to_string(),
        r.created_utc.clone(),
        r.run_id.clone(),
        r.mode.clone(),
        r.tool.version.clone(),
        r.tool.spec.clone(),
        r.tool.rustc.clone(),
        r.tool.target.clone(),
        r.tool.build_profile.clone(),
        s.label.clone().unwrap_or_default(),
        s.model.clone().unwrap_or_default(),
        s.os.name.clone(),
        s.os.kernel.clone(),
        s.os.arch.clone(),
        s.cpu.model.clone(),
        s.cpu.usable.to_string(),
        s.cpu.physical.to_string(),
        s.mem.total_mb.to_string(),
        o2(cpu.map(|c| c.single_geo)),
        o2(cpu.map(|c| c.multi_geo)),
        o2(cpu.map(|c| c.scaling)),
        o2(cpu.map(|c| c.sha.soft_mbs)),
        o2(cpu.and_then(|c| c.sha.hw_mbs)),
        o2(cpu.and_then(|c| c.simd.as_ref().map(|x| x.speedup))),
        o2(mem.map(|m| m.single.triad)),
        o2(mem.map(|m| m.multi.triad)),
        o2(mem.map(|m| m.multi.read)),
        o2(mem.map(|m| m.l1_ns)),
        o2(mem.map(|m| m.dram_ns)),
        s.disk.kind.clone(),
        disk.filter(|d| d.error.is_none()).map(|d| d.direct.to_string()).unwrap_or_default(),
        o2(disk.and_then(|d| d.seq_read_mbs())),
        o2(disk.and_then(|d| d.seq_write_mbs())),
        o2(disk.and_then(|d| d.rand_read_qd1_iops())),
        o2(disk.and_then(|d| d.rand_read_qd32_iops())),
        o2(disk.and_then(|d| d.rand_write_qd32_iops())),
        o2(disk.and_then(|d| d.scenario("rand4k_read_qd1")).and_then(|x| x.p99_us)),
        disk.and_then(|d| d.streams.as_ref()).map(|x| x.uhd_capacity.to_string()).unwrap_or_default(),
        o2(disk.and_then(|d| d.small_files.as_ref()).map(|x| x.create_per_s)),
        net.map(|n| n.mode.clone()).unwrap_or_default(),
        net.and_then(|n| n.link_mbps).map(|x| x.to_string()).unwrap_or_default(),
        o2(net.and_then(|n| n.up4_mbps)),
        o2(net.and_then(|n| n.down4_mbps)),
        o2(net.and_then(|n| n.rtt.as_ref()).map(|x| x.p50_us)),
        o2(sc("virt")),
        o2(sc("nas")),
        o2(sc("video")),
        o2(sc("game")),
        o2(sc("ai")),
        o2(r.scores.general),
        o2(r.results.rt.as_ref().map(|x| x.single_mrays)),
        o2(r.results.rt.as_ref().map(|x| x.multi_mrays)),
        o2(video.and_then(|v| v.cpu_h264()).and_then(|e| e.fps)),
        video.and_then(|v| v.best_hw()).map(|e| e.name.clone()).unwrap_or_default(),
        o2(video.and_then(|v| v.best_hw()).and_then(|e| e.fps)),
        video.and_then(|v| v.best_hw()).and_then(|e| e.realtime_streams).map(|x| x.to_string()).unwrap_or_default(),
        gpu.map(|g| g.info.name.clone()).unwrap_or_default(),
        gpu.map(|g| g.info.backend.clone()).unwrap_or_default(),
        gpu.map(|g| g.info.device_type.clone()).unwrap_or_default(),
        o2(gpu.and_then(|g| g.fp32_gflops)),
        o2(gpu.and_then(|g| g.mem_gbs)),
        o2(gpu.and_then(|g| g.matmul_gflops)),
        o2(gpu.and_then(|g| g.rt.as_ref()).map(|x| x.mrays)),
        o2(sc("gfx")),
        o2(sc("rt")),
        r.notes.clone().unwrap_or_default(),
        warns,
    ];
    debug_assert_eq!(cells.len(), CSV_COLUMNS.len());
    cells.iter().map(|c| csv_escape(c)).collect::<Vec<_>>().join(",")
}

/// Append one row (writing the header first when the file is new). Refuses to mix schemas.
pub fn append_csv(path: &Path, r: &Record) -> io::Result<()> {
    use std::io::Write;
    let header = csv_header();
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    if existing.trim().is_empty() {
        out.push_str(&header);
        out.push('\n');
    } else if existing.lines().next().map(str::trim) != Some(header.as_str()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} has a different column layout; use another file", path.display())));
    } else if !existing.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&csv_row(r));
    out.push('\n');
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(out.as_bytes())
}

/// Merge every `*.json` record in `dir` into one CSV (oldest first). Returns the number of rows.
pub fn export_dir(dir: &Path, out: &Path) -> io::Result<usize> {
    let mut recs: Vec<Record> = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().map(|x| x == "json").unwrap_or(false) {
            if let Ok(r) = Record::load(&p) {
                recs.push(r);
            }
        }
    }
    recs.sort_by(|a, b| a.created_utc.cmp(&b.created_utc));
    let mut s = csv_header();
    s.push('\n');
    for r in &recs {
        s.push_str(&csv_row(r));
        s.push('\n');
    }
    std::fs::write(out, s)?;
    Ok(recs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Record {
        let mut sys = SystemInfo::default();
        sys.label = Some("Test, \"box\"".into());
        sys.cpu.model = "Test CPU".into();
        sys.cpu.usable = 4;
        sys.mem.total_mb = 16384;
        let mut res = Results::default();
        res.cpu = Some(CpuResults { single_geo: 700.0, multi_geo: 2500.0, scaling: 3.5, ..Default::default() });
        Record::new(true, sys, res, Some("line1\nline2".into()))
    }

    #[test]
    fn column_count_matches_row() {
        let r = sample();
        // parse the row respecting quotes
        let row = csv_row(&r);
        let mut n = 1;
        let mut q = false;
        for c in row.chars() {
            match c {
                '"' => q = !q,
                ',' if !q => n += 1,
                _ => {}
            }
        }
        assert_eq!(n, CSV_COLUMNS.len());
        assert_eq!(csv_header().split(',').count(), CSV_COLUMNS.len());
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"x\""), "\"say \"\"x\"\"\"");
        assert_eq!(csv_escape("plain"), "plain");
    }

    #[test]
    fn json_roundtrip_and_save() {
        let r = sample();
        let d = std::env::temp_dir().join(format!("syscheck_rec_{}", std::process::id()));
        let p = r.save_json(&d).unwrap();
        let back = Record::load(&p).unwrap();
        assert_eq!(back.run_id, r.run_id);
        assert_eq!(back.system.label, r.system.label);
        assert_eq!(back.results.cpu.as_ref().unwrap().single_geo, 700.0);
        // csv append + export
        let csv = d.join("all.csv");
        append_csv(&csv, &r).unwrap();
        append_csv(&csv, &r).unwrap();
        assert_eq!(std::fs::read_to_string(&csv).unwrap().lines().count(), 1 + 2 * 2); // header + 2 rows of 2 physical lines (multi-line note)
        let out = d.join("export.csv");
        assert_eq!(export_dir(&d, &out).unwrap(), 1);
        std::fs::write(d.join("bad.csv"), "a,b\n").unwrap();
        assert!(append_csv(&d.join("bad.csv"), &r).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn no_identifying_fields() {
        let json = serde_json::to_string(&sample()).unwrap();
        let host = std::fs::read_to_string("/etc/hostname").unwrap_or_default();
        let host = host.trim();
        if host.len() > 3 {
            assert!(!json.contains(host), "hostname must not be stored");
        }
        assert!(!json.contains("/home/"));
    }
}

/// A fully populated record for renderer tests.
#[cfg(test)]
pub mod tests_support {
    use super::*;
    use crate::bench::{cpu::*, disk::*, gpu::*, mem::*, net::*, rt::*, video::*};
    use crate::hw::*;

    pub fn sample_full() -> Record {
        let mut sys = SystemInfo::default();
        sys.label = Some("Test box".into());
        sys.model = Some("Test Model".into());
        sys.os = OsInfo { name: "Linux Test".into(), kernel: "6.1".into(), arch: "x86_64".into(), android: false, container: false };
        sys.cpu = CpuInfo { model: "Test CPU @ 3 GHz".into(), arch: "x86_64".into(), logical: 8, usable: 8, physical: 4, packages: 1, max_mhz: Some(3000), virt_ext: Some(true), kvm: true, iommu_groups: Some(12), features: vec!["avx2".into(), "sha_ni".into()], ..Default::default() };
        sys.cpu.groups = vec![
            CoreGroup { count: 4, max_mhz: Some(3000), part: Some("Big".into()), kind: Some("P".into()), ..Default::default() },
            CoreGroup { count: 4, max_mhz: Some(2000), part: Some("Little".into()), kind: Some("E".into()), ..Default::default() },
        ];
        sys.cpu.caches = vec![CacheInfo { level: 1, kind: "Data".into(), size_kb: 32 }, CacheInfo { level: 3, kind: "Unified".into(), size_kb: 16384 }];
        sys.mem = MemInfo { total_mb: 32768, available_mb: 20000, swap_mb: 2048, ecc: Some(false) };
        sys.disk = DiskInfo { kind: "nvme".into(), fs: Some("ext4".into()), model: Some("Test NVMe".into()), total_gb: 1000.0, free_gb: 500.0, ..Default::default() };
        sys.nics = vec![NicInfo { name: "eth0".into(), kind: "ethernet".into(), speed_mbps: Some(2500), up: true, default_route: true }];
        sys.gpus = vec![GpuInfo { name: "Test GPU".into(), vendor: "Test".into(), vram_mb: Some(8192), ..Default::default() }];
        sys.hw_video = vec!["vaapi".into()];

        let names = [("int_alu", "Mops/s"), ("fp_f64", "MFLOPS"), ("matmul_f32", "MFLOPS"), ("sort_u32", "Melem/s"), ("sha256_soft", "MB/s"), ("deflate", "MB/s")];
        let kernels = names.iter().enumerate().map(|(i, (n, u))| KernelResult { name: n.to_string(), unit: u.to_string(), single: 1000.0 * (i + 1) as f64, multi: 5000.0 * (i + 1) as f64, noise_pct: 1.0 }).collect();
        let cpu = CpuResults {
            threads: 8,
            pinned: true,
            kernels,
            single_geo: 1500.0,
            multi_geo: 9000.0,
            scaling: 6.0,
            efficiency_pct: 75.0,
            groups: vec![GroupResult { label: "P-core Big".into(), cpu: 0, int_mops: 9000.0, matmul_mflops: 30000.0 }, GroupResult { label: "E-core Little".into(), cpu: 4, int_mops: 4000.0, matmul_mflops: 9000.0 }],
            sha: ShaResult { soft_mbs: 300.0, hw_mbs: Some(2000.0), hw_feature: Some("sha-ni".into()), speedup: Some(6.7) },
            simd: Some(SimdResult { feature: "avx2+fma".into(), base_mflops: 15000.0, accel_mflops: 28000.0, speedup: 1.9 }),
            max_noise_pct: 1.2,
            reps: 3,
        };
        let lat = [(4096u64, 1.2), (32768, 1.3), (262144, 4.0), (2097152, 12.0), (16777216, 40.0), (134217728, 95.0)];
        let mem = MemResults {
            array_mb: 256,
            threads: 8,
            single: Bw { read: 20.0, write: 15.0, copy: 25.0, triad: 22.0 },
            multi: Bw { read: 60.0, write: 40.0, copy: 70.0, triad: 65.0 },
            latency: lat.iter().map(|(b, n)| LatPoint { bytes: *b, ns: *n }).collect(),
            l1_ns: 1.2,
            dram_ns: 95.0,
        };
        let mut sweep = Vec::new();
        for qd in [1u32, 4] {
            for (i, bs) in [512u32, 4096, 65536, 1 << 20, 8 << 20].iter().enumerate() {
                let v = 50.0 * (i + 1) as f64 * qd as f64;
                sweep.push(SweepPoint { bs: *bs, qd, qd_eff: qd, read_mbs: Some(v * 2.0), write_mbs: Some(v), read_iops: Some(1000.0), write_iops: Some(900.0) });
            }
        }
        let keys = ["rand4k_read_qd1", "rand4k_read_qd32", "rand4k_write_qd32", "mixed8k_70r30w_qd32", "seq1m_read_qd4", "seq1m_write_qd4"];
        let scenarios = keys.iter().enumerate().map(|(i, k)| Scenario { key: k.to_string(), bs: 4096, qd: 1, mbs: 100.0 * (i + 1) as f64, iops: 10000.0 * (i + 1) as f64, p50_us: if i == 0 { Some(80.0) } else { None }, p99_us: if i == 0 { Some(150.0) } else { None }, p999_us: if i == 0 { Some(400.0) } else { None } }).collect();
        let disk = DiskResults {
            direct: true,
            file_mb: 512,
            logical_block: 512,
            sweep_pattern: "sequential".into(),
            sweep,
            scenarios,
            streams: Some(StreamResult { streams: 8, total_mbs: 2500.0, min_stream_mbs: 250.0, uhd_capacity: 250 }),
            small_files: Some(SmallFiles { files: 500, create_per_s: 30000.0, read_per_s: 90000.0, delete_per_s: 60000.0 }),
            sustained: Some(Sustained { file_mb: 1024, samples_mbs: vec![2000.0, 1900.0, 800.0, 700.0], start_mbs: 1950.0, end_mbs: 750.0, min_mbs: 700.0, avg_mbs: 1350.0, drop_pct: 61.0 }),
            ..Default::default()
        };
        let net = NetResults {
            mode: "remote".into(),
            up1_mbps: Some(900.0),
            up4_mbps: Some(2300.0),
            down1_mbps: Some(910.0),
            down4_mbps: Some(2350.0),
            bidir_mbps: Some(3000.0),
            rtt: Some(Rtt { samples: 1000, min_us: 90.0, p50_us: 120.0, p99_us: 300.0, jitter_us: 20.0 }),
            sweep: vec![MsgPoint { size: 64, msgs_per_s: 8000.0, mbs: 1.0 }, MsgPoint { size: 1 << 20, msgs_per_s: 600.0, mbs: 1200.0 }],
            gateway_rtt: Some(Rtt { samples: 10, min_us: 200.0, p50_us: 400.0, p99_us: 900.0, jitter_us: 50.0 }),
            link_mbps: Some(2500),
            ceiling_mbps: Some(2350.0),
            efficiency_pct: Some(100.0),
            ..Default::default()
        };
        let video = VideoResults {
            ffmpeg_version: Some("7.0".into()),
            source_fps: Some(900.0),
            frames: 240,
            encoders: vec![
                EncoderResult { name: "libx264".into(), codec: "h264".into(), kind: "cpu".into(), ok: true, fps: Some(85.0), parallel_fps: Some(90.0), realtime_streams: Some(3), source_limited: false },
                EncoderResult { name: "h264_nvenc".into(), codec: "h264".into(), kind: "hw".into(), ok: true, fps: Some(640.0), parallel_fps: Some(700.0), realtime_streams: Some(23), source_limited: true },
                EncoderResult { name: "h264_qsv".into(), codec: "h264".into(), kind: "hw".into(), ok: false, ..Default::default() },
            ],
        };
        let rtc = RtCpu { width: 256, height: 144, max_depth: 5, primitives: 55, threads: 8, single_mrays: 9.5, multi_mrays: 60.0, rays_per_pass: 130_000, checksum: "1191796675e28a08".into(), checksum_ok: true, mean_luma: 0.52 };
        let gpu = GpuResults {
            supported: true,
            listed: 1,
            adapters: vec![GpuAdapterResult {
                info: GpuAdapterInfo { name: "Test GPU 9000".into(), vendor_id: 1, device_id: 2, device_type: "discrete".into(), backend: "vulkan".into(), driver: "drv".into(), driver_info: "1.0".into() },
                max_buffer_mb: Some(4096),
                fp32_gflops: Some(20000.0),
                int_gops: Some(9000.0),
                mem_gbs: Some(400.0),
                mem_buffer_mb: Some(128),
                matmul_gflops: Some(9000.0),
                matmul_n: Some(1024),
                matmul_ok: Some(true),
                h2d_gbs: Some(12.0),
                d2h_gbs: Some(11.0),
                latency_p50_us: Some(60.0),
                latency_p99_us: Some(120.0),
                rt: Some(RtGpu { adapter: "Test GPU 9000".into(), backend: "vulkan".into(), mrays: 2500.0, passes: 4000, mean_luma: 0.52, luma_matches_cpu: true, hw_ray_query: true }),
                error: None,
            }],
        };
        let res = Results { cpu: Some(cpu), mem: Some(mem), disk: Some(disk), net: Some(net), video: Some(video), rt: Some(rtc), gpu: Some(gpu) };
        Record::new(false, sys, res, None)
    }
}
