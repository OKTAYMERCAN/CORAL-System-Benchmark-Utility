//! Disk benchmark: a block-size sweep plus real-world scenarios.
//!
//! * Sweep: sequential read / write throughput for block sizes 0.5 KB ... 8 MB at one or more
//!   queue depths (the block-size chart).
//! * Scenarios: 4K random QD1 (with latency percentiles) and QD32, mixed 8K 70/30, 1 MB
//!   sequential, and 8 concurrent 256K streams (how many video streams can the disk feed).
//! * Small-file test (create / read / delete), optional sustained-write test.
//!
//! I/O is unbuffered (O_DIRECT on Linux/Android, F_NOCACHE on macOS) so the page cache does not
//! inflate the numbers. Queue depth is emulated with N threads that each keep one request in
//! flight. If direct I/O is not available (tmpfs, some network file systems) the test falls back
//! to buffered I/O and says so (`direct == false`).

use crate::bench::Profile;
use crate::hw::DiskInfo;
use crate::ui::Ui;
use crate::util::*;
use serde::{Deserialize, Serialize};
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::fs::{File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Barrier;
use std::time::{Duration, Instant};

// ───────────────────────── result types ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SweepPoint {
    pub bs: u32,
    /// Requested queue depth.
    pub qd: u32,
    /// Queue depth actually used (limited so that bs * qd stays within the memory cap).
    pub qd_eff: u32,
    pub read_mbs: Option<f64>,
    pub write_mbs: Option<f64>,
    pub read_iops: Option<f64>,
    pub write_iops: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Scenario {
    pub key: String,
    pub bs: u32,
    pub qd: u32,
    pub mbs: f64,
    pub iops: f64,
    pub p50_us: Option<f64>,
    pub p99_us: Option<f64>,
    pub p999_us: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StreamResult {
    pub streams: u32,
    pub total_mbs: f64,
    /// Slowest single stream, MB/s.
    pub min_stream_mbs: f64,
    /// total / 10 MB/s (a 4K video stream is about 80 Mbit/s).
    pub uhd_capacity: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SmallFiles {
    pub files: u32,
    pub create_per_s: f64,
    pub read_per_s: f64,
    pub delete_per_s: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Sustained {
    pub file_mb: u32,
    /// MB/s for each second of the run.
    pub samples_mbs: Vec<f64>,
    pub start_mbs: f64,
    pub end_mbs: f64,
    pub min_mbs: f64,
    pub avg_mbs: f64,
    /// How much slower the end is than the start, percent (0 when it did not drop).
    pub drop_pct: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DiskResults {
    /// Set when the test file could not be created (read-only, no space, ...).
    pub error: Option<String>,
    /// true = unbuffered I/O; false = fell back to buffered I/O (numbers may include cache).
    pub direct: bool,
    pub sync_writes: bool,
    pub file_mb: u32,
    pub logical_block: u32,
    /// Access pattern of the sweep ("sequential").
    pub sweep_pattern: String,
    pub sweep: Vec<SweepPoint>,
    pub scenarios: Vec<Scenario>,
    pub streams: Option<StreamResult>,
    pub small_files: Option<SmallFiles>,
    pub sustained: Option<Sustained>,
}

impl DiskResults {
    pub fn scenario(&self, key: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.key == key)
    }
    pub fn seq_read_mbs(&self) -> Option<f64> {
        self.scenario("seq1m_read_qd4").map(|s| s.mbs)
    }
    pub fn seq_write_mbs(&self) -> Option<f64> {
        self.scenario("seq1m_write_qd4").map(|s| s.mbs)
    }
    pub fn rand_read_qd1_iops(&self) -> Option<f64> {
        self.scenario("rand4k_read_qd1").map(|s| s.iops)
    }
    pub fn rand_read_qd32_iops(&self) -> Option<f64> {
        self.scenario("rand4k_read_qd32").map(|s| s.iops)
    }
    pub fn rand_write_qd32_iops(&self) -> Option<f64> {
        self.scenario("rand4k_write_qd32").map(|s| s.iops)
    }
}

// ───────────────────────── aligned buffer ─────────────────────────

/// Heap buffer aligned to 4096 bytes (required by O_DIRECT).
pub struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: Layout,
}

// SAFETY: the buffer owns its allocation and is only accessed through &self / &mut self.
unsafe impl Send for AlignedBuf {}

impl AlignedBuf {
    pub fn new(len: usize) -> Self {
        let layout = Layout::from_size_align(len.max(1), 4096).expect("valid layout");
        // SAFETY: layout has non-zero size.
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "out of memory allocating I/O buffer");
        AlignedBuf { ptr, len, layout }
    }
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr is valid for len bytes for the lifetime of self.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and we hold a unique reference.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        // SAFETY: allocated with the same layout in `new`.
        unsafe { dealloc(self.ptr, self.layout) }
    }
}

// ───────────────────────── test file handling ─────────────────────────

/// Test file that is removed when dropped.
struct TestFile {
    path: PathBuf,
    file: File,
    direct: bool,
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Directory that is removed (with contents) when dropped.
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Positional read / write that does not depend on a shared file offset (pread / pwrite on
/// unix; on Windows the offset is passed per call, so threads can share one handle).
trait PosIo {
    fn pread(&self, buf: &mut [u8], off: u64) -> io::Result<usize>;
    fn pwrite(&self, buf: &[u8], off: u64) -> io::Result<usize>;
}

#[cfg(unix)]
impl PosIo for File {
    fn pread(&self, buf: &mut [u8], off: u64) -> io::Result<usize> {
        std::os::unix::fs::FileExt::read_at(self, buf, off)
    }
    fn pwrite(&self, buf: &[u8], off: u64) -> io::Result<usize> {
        std::os::unix::fs::FileExt::write_at(self, buf, off)
    }
}

#[cfg(windows)]
impl PosIo for File {
    fn pread(&self, buf: &mut [u8], off: u64) -> io::Result<usize> {
        std::os::windows::fs::FileExt::seek_read(self, buf, off)
    }
    fn pwrite(&self, buf: &[u8], off: u64) -> io::Result<usize> {
        std::os::windows::fs::FileExt::seek_write(self, buf, off)
    }
}

#[cfg(windows)]
fn open_with(path: &Path, direct: bool, sync: bool) -> io::Result<File> {
    // FILE_FLAG_NO_BUFFERING = 0x2000_0000 (needs sector-aligned buffers, offsets, sizes: the
    // test already uses 4 KiB aligned buffers), FILE_FLAG_WRITE_THROUGH = 0x8000_0000.
    let mut flags: u32 = 0;
    if direct {
        flags |= 0x2000_0000;
    }
    if sync {
        flags |= 0x8000_0000;
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(true).custom_flags(flags).open(path)
}

#[cfg(unix)]
fn open_with(path: &Path, direct: bool, sync: bool) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true).write(true).create(true).truncate(true).mode(0o600);
    let mut flags: i32 = 0;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if direct {
        flags |= libc::O_DIRECT;
    }
    if sync {
        flags |= libc::O_DSYNC;
    }
    o.custom_flags(flags);
    let f = o.open(path)?;
    #[cfg(target_os = "macos")]
    if direct {
        // SAFETY: plain fcntl on a valid descriptor.
        unsafe { libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1) };
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    let _ = direct;
    Ok(f)
}

/// Fill the file with incompressible random data (so reads hit real blocks, never holes).
fn prepare(f: &File, size: u64) -> io::Result<()> {
    const CH: usize = 1 << 20;
    let mut buf = AlignedBuf::new(CH);
    let mut rng = Rng::new(0xFEED_FACE);
    let mut off = 0u64;
    while off < size {
        rng.fill(buf.as_mut_slice());
        let n = f.pwrite(buf.as_slice(), off)?;
        if n != CH {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short write while preparing test file"));
        }
        off += CH as u64;
    }
    f.sync_all()
}

fn create_test_file(path: &Path, size: u64, want_direct: bool, sync: bool) -> io::Result<TestFile> {
    if want_direct {
        if let Ok(f) = open_with(path, true, sync) {
            if prepare(&f, size).is_ok() {
                return Ok(TestFile { path: path.to_path_buf(), file: f, direct: true });
            }
        }
    }
    let r = open_with(path, false, sync).and_then(|f| prepare(&f, size).map(|_| f));
    match r {
        Ok(f) => Ok(TestFile { path: path.to_path_buf(), file: f, direct: false }),
        Err(e) => {
            let _ = std::fs::remove_file(path);
            Err(e)
        }
    }
}

/// Best-effort: make the next read come from the device in buffered mode.
fn drop_cache(f: &File) {
    let _ = f.sync_all();
    #[cfg(any(target_os = "linux", target_os = "android"))]
    // SAFETY: advisory call on a valid descriptor.
    unsafe {
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

// ───────────────────────── I/O engine ─────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pat {
    Seq,
    Rand,
}

#[derive(Clone, Copy, Debug)]
struct Spec {
    bs: usize,
    threads: usize,
    pat: Pat,
    /// 100 = only reads, 0 = only writes.
    read_pct: u32,
    secs: f64,
    /// record per-operation latency
    lat: bool,
}

#[derive(Default)]
struct ThreadOut {
    ops_r: u64,
    ops_w: u64,
    bytes: u64,
    elapsed: f64,
    lat_ns: Vec<u32>,
}

#[derive(Default)]
struct Outcome {
    ops: u64,
    bytes: u64,
    elapsed: f64,
    lat_ns: Vec<u32>,
    thread_bytes: Vec<u64>,
}

impl Outcome {
    fn mbs(&self) -> f64 {
        if self.elapsed > 0.0 {
            self.bytes as f64 / self.elapsed / 1e6
        } else {
            0.0
        }
    }
    fn iops(&self) -> f64 {
        if self.elapsed > 0.0 {
            self.ops as f64 / self.elapsed
        } else {
            0.0
        }
    }
}

const LAT_CAP: usize = 200_000;

fn worker(f: &File, region: u64, spec: Spec, seed: u64, t: usize, barrier: &Barrier) -> io::Result<ThreadOut> {
    let bs = spec.bs;
    let mut buf = AlignedBuf::new(bs);
    let mut rng = Rng::new(seed);
    rng.fill(buf.as_mut_slice());
    let nblocks = (region / bs as u64).max(1);
    let slice = (nblocks / spec.threads as u64).max(1);
    let mut cursor = (t as u64 * slice) % nblocks;
    let mut counter = 0u64;
    let mut out = ThreadOut::default();
    let mut err: Option<io::Error> = None;
    let deadline = Duration::from_secs_f64(spec.secs);
    let check_mask: u64 = if spec.lat || bs > 4096 { 0 } else { 15 };

    barrier.wait();
    let start = Instant::now();
    let mut n_ops = 0u64;
    loop {
        if n_ops & check_mask == 0 && start.elapsed() >= deadline {
            break;
        }
        let blk = match spec.pat {
            Pat::Seq => {
                let b = cursor;
                cursor += 1;
                if cursor >= nblocks {
                    cursor = 0;
                }
                b
            }
            Pat::Rand => rng.below(nblocks),
        };
        let off = blk * bs as u64;
        let is_read = spec.read_pct >= 100 || (spec.read_pct > 0 && rng.below(100) < spec.read_pct as u64);
        let t0 = if spec.lat { Some(Instant::now()) } else { None };
        let r = if is_read {
            f.pread(buf.as_mut_slice(), off)
        } else {
            counter += 1;
            buf.as_mut_slice()[..8].copy_from_slice(&counter.to_le_bytes());
            f.pwrite(buf.as_slice(), off)
        };
        match r {
            Ok(n) if n == bs => {
                if is_read {
                    out.ops_r += 1;
                } else {
                    out.ops_w += 1;
                }
                out.bytes += bs as u64;
                if let Some(t0) = t0 {
                    if out.lat_ns.len() < LAT_CAP {
                        out.lat_ns.push(t0.elapsed().as_nanos().min(u32::MAX as u128) as u32);
                    }
                }
            }
            Ok(_) => {
                err = Some(io::Error::new(io::ErrorKind::UnexpectedEof, "short I/O"));
                break;
            }
            Err(e) => {
                err = Some(e);
                break;
            }
        }
        n_ops += 1;
    }
    out.elapsed = start.elapsed().as_secs_f64();
    match err {
        Some(e) if out.ops_r + out.ops_w == 0 => Err(e),
        _ => Ok(out),
    }
}

fn run_spec(f: &File, region: u64, spec: Spec, seed: u64) -> io::Result<Outcome> {
    let barrier = Barrier::new(spec.threads);
    let results: Vec<io::Result<ThreadOut>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..spec.threads)
            .map(|t| {
                let barrier = &barrier;
                sc.spawn(move || worker(f, region, spec, seed.wrapping_add(t as u64 * 7919), t, barrier))
            })
            .collect();
        hs.into_iter()
            .map(|h| match h.join() {
                Ok(r) => r,
                Err(_) => Err(io::Error::new(io::ErrorKind::Other, "I/O worker panicked")),
            })
            .collect()
    });
    let mut o = Outcome::default();
    for r in results {
        let r = r?;
        o.ops += r.ops_r + r.ops_w;
        o.bytes += r.bytes;
        o.elapsed = o.elapsed.max(r.elapsed);
        o.thread_bytes.push(r.bytes);
        o.lat_ns.extend(r.lat_ns);
    }
    Ok(o)
}

// ───────────────────────── scenarios ─────────────────────────

struct ScenDef {
    key: &'static str,
    bs: usize,
    qd: usize,
    pat: Pat,
    read_pct: u32,
    lat: bool,
}

const SCENARIOS: [ScenDef; 6] = [
    ScenDef { key: "rand4k_read_qd1", bs: 4096, qd: 1, pat: Pat::Rand, read_pct: 100, lat: true },
    ScenDef { key: "rand4k_read_qd32", bs: 4096, qd: 32, pat: Pat::Rand, read_pct: 100, lat: false },
    ScenDef { key: "rand4k_write_qd32", bs: 4096, qd: 32, pat: Pat::Rand, read_pct: 0, lat: false },
    ScenDef { key: "mixed8k_70r30w_qd32", bs: 8192, qd: 32, pat: Pat::Rand, read_pct: 70, lat: false },
    ScenDef { key: "seq1m_read_qd4", bs: 1 << 20, qd: 4, pat: Pat::Seq, read_pct: 100, lat: false },
    ScenDef { key: "seq1m_write_qd4", bs: 1 << 20, qd: 4, pat: Pat::Seq, read_pct: 0, lat: false },
];

/// Memory cap for I/O buffers of one test point.
const BUF_CAP: usize = 64 << 20;

fn small_file_test(dir: &Path, n: usize) -> io::Result<SmallFiles> {
    use std::io::Write;
    let tmp = TempDir(dir.join(format!(".syscheck_small_{}", std::process::id())));
    std::fs::create_dir_all(&tmp.0)?;
    let mut data = vec![0u8; 4096];
    Rng::new(77).fill(&mut data);
    let name = |i: usize| tmp.0.join(format!("f{i:06}"));

    let t = Instant::now();
    for i in 0..n {
        let mut f = File::create(name(i))?;
        f.write_all(&data)?;
    }
    let create = n as f64 / t.elapsed().as_secs_f64();

    let t = Instant::now();
    let mut sum = 0usize;
    for i in 0..n {
        sum += std::fs::read(name(i))?.len();
    }
    std::hint::black_box(sum);
    let read = n as f64 / t.elapsed().as_secs_f64();

    let t = Instant::now();
    for i in 0..n {
        std::fs::remove_file(name(i))?;
    }
    let del = n as f64 / t.elapsed().as_secs_f64();
    Ok(SmallFiles { files: n as u32, create_per_s: create, read_per_s: read, delete_per_s: del })
}

fn sustained_test(ui: &Ui, dir: &Path, secs: u32, size_mb: u64, sync: bool) -> io::Result<Sustained> {
    const CH: usize = 1 << 20;
    const WRITERS: usize = 4;
    let path = dir.join(format!(".syscheck_sus_{}.tmp", std::process::id()));
    let tf = create_test_file(&path, size_mb << 20, true, sync)?;
    let nblocks = size_mb.max(1);
    let bytes = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let mut samples: Vec<f64> = Vec::new();
    let file = &tf.file;
    std::thread::scope(|sc| {
        for t in 0..WRITERS {
            let (bytes, stop) = (&bytes, &stop);
            sc.spawn(move || {
                let mut buf = AlignedBuf::new(CH);
                let mut rng = Rng::new(900 + t as u64);
                rng.fill(buf.as_mut_slice());
                let mut k = t as u64 * (nblocks / WRITERS as u64).max(1);
                let mut ctr = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    ctr += 1;
                    buf.as_mut_slice()[..8].copy_from_slice(&ctr.to_le_bytes());
                    if file.pwrite(buf.as_slice(), (k % nblocks) * CH as u64).is_err() {
                        break;
                    }
                    bytes.fetch_add(CH as u64, Ordering::Relaxed);
                    k += 1;
                }
            });
        }
        let mut last = 0u64;
        let mut t_last = Instant::now();
        for s in 0..secs {
            std::thread::sleep(Duration::from_secs(1));
            let now = bytes.load(Ordering::Relaxed);
            let dt = t_last.elapsed().as_secs_f64();
            t_last = Instant::now();
            samples.push((now - last) as f64 / dt / 1e6);
            last = now;
            ui.progress(&format!("{} {}/{} s  {:.0} MB/s", ui.t("prog_disk_sustained"), s + 1, secs, samples[samples.len() - 1]));
        }
        stop.store(true, Ordering::Relaxed);
    });
    drop(tf);
    let k = (samples.len() / 5).clamp(1, 5);
    let start = mean(&samples[..k.min(samples.len())]);
    let end = mean(&samples[samples.len().saturating_sub(k)..]);
    Ok(Sustained {
        file_mb: size_mb as u32,
        start_mbs: start,
        end_mbs: end,
        min_mbs: samples.iter().cloned().fold(f64::MAX, f64::min),
        avg_mbs: mean(&samples),
        drop_pct: if start > 0.0 { ((1.0 - end / start) * 100.0).max(0.0) } else { 0.0 },
        samples_mbs: samples,
    })
}

// ───────────────────────── main entry ─────────────────────────

pub fn run(ui: &Ui, dir: &Path, info: &DiskInfo, prof: &Profile) -> DiskResults {
    let mut res = DiskResults { sweep_pattern: "sequential".into(), sync_writes: prof.disk_sync, ..Default::default() };
    let logical = info.logical_block.unwrap_or(512).max(512) as usize;
    res.logical_block = logical as u32;

    // file size: as configured, but never more than 40 % of the free space; multiple of 8 MiB
    let mut mb = prof.disk_file_mb.max(16);
    if info.free_gb > 0.0 {
        while mb > 16 && mb as f64 > info.free_gb * 1000.0 * 0.4 {
            mb /= 2;
        }
    }
    mb = (mb / 8).max(2) * 8;
    res.file_mb = mb as u32;
    let region = mb << 20;

    ui.progress(&ui.t("prog_disk_prepare"));
    let path = dir.join(format!(".syscheck_{}.tmp", std::process::id()));
    let tf = match create_test_file(&path, region, true, prof.disk_sync) {
        Ok(t) => t,
        Err(e) => {
            ui.clear_progress();
            res.error = Some(e.to_string());
            return res;
        }
    };
    res.direct = tf.direct;
    let f = &tf.file;

    // ---- block-size sweep ---------------------------------------------------------------------
    let sizes: Vec<u32> = prof.disk_sizes.iter().copied().filter(|&b| !tf.direct || b as usize >= logical).collect();
    let total = sizes.len() * prof.disk_qds.len();
    let mut done = 0;
    for &qd in &prof.disk_qds {
        for &bs in &sizes {
            done += 1;
            ui.progress(&format!("{}: {} QD{} [{}/{}]", ui.t("prog_disk_sweep"), bs_label(bs), qd, done, total));
            let qd_eff = qd.min((BUF_CAP / bs as usize).max(1));
            let mk = |read_pct| Spec { bs: bs as usize, threads: qd_eff, pat: Pat::Seq, read_pct, secs: prof.disk_point_secs, lat: false };
            if !tf.direct {
                drop_cache(f);
            }
            let r = run_spec(f, region, mk(100), 11).ok();
            let w = run_spec(f, region, mk(0), 12).ok();
            res.sweep.push(SweepPoint {
                bs,
                qd: qd as u32,
                qd_eff: qd_eff as u32,
                read_mbs: r.as_ref().map(|o| o.mbs()),
                read_iops: r.as_ref().map(|o| o.iops()),
                write_mbs: w.as_ref().map(|o| o.mbs()),
                write_iops: w.as_ref().map(|o| o.iops()),
            });
        }
    }

    // ---- scenarios ------------------------------------------------------------------------------
    for (i, sc) in SCENARIOS.iter().enumerate() {
        ui.progress(&format!("{}: {} [{}/{}]", ui.t("prog_disk_scen"), sc.key, i + 1, SCENARIOS.len() + 1));
        let bs = sc.bs.max(logical);
        let qd = sc.qd.min((BUF_CAP / bs).max(1));
        if !tf.direct && sc.read_pct > 0 {
            drop_cache(f);
        }
        let spec = Spec { bs, threads: qd, pat: sc.pat, read_pct: sc.read_pct, secs: prof.disk_scen_secs, lat: sc.lat };
        if let Ok(mut o) = run_spec(f, region, spec, 100 + i as u64) {
            let (p50, p99, p999) = if sc.lat && !o.lat_ns.is_empty() {
                o.lat_ns.sort_unstable();
                (
                    Some(percentile(&o.lat_ns, 50.0) / 1000.0),
                    Some(percentile(&o.lat_ns, 99.0) / 1000.0),
                    Some(percentile(&o.lat_ns, 99.9) / 1000.0),
                )
            } else {
                (None, None, None)
            };
            res.scenarios.push(Scenario { key: sc.key.into(), bs: bs as u32, qd: qd as u32, mbs: o.mbs(), iops: o.iops(), p50_us: p50, p99_us: p99, p999_us: p999 });
        }
    }

    // 8 concurrent sequential 256K streams
    ui.progress(&format!("{}: streams [{}/{}]", ui.t("prog_disk_scen"), SCENARIOS.len() + 1, SCENARIOS.len() + 1));
    if !tf.direct {
        drop_cache(f);
    }
    let sbs = (256 << 10usize).max(logical);
    let spec = Spec { bs: sbs, threads: 8, pat: Pat::Seq, read_pct: 100, secs: prof.disk_scen_secs, lat: false };
    if let Ok(o) = run_spec(f, region, spec, 500) {
        let per: Vec<f64> = o.thread_bytes.iter().map(|b| *b as f64 / o.elapsed / 1e6).collect();
        let total = o.mbs();
        res.streams = Some(StreamResult {
            streams: 8,
            total_mbs: total,
            min_stream_mbs: per.iter().cloned().fold(f64::MAX, f64::min),
            uhd_capacity: (total / 10.0).floor() as u32,
        });
    }
    drop(tf);

    // ---- small files ------------------------------------------------------------------------------
    ui.progress(&ui.t("prog_disk_small"));
    res.small_files = small_file_test(dir, prof.small_files).ok();

    // ---- sustained write (optional) ----------------------------------------------------------------
    if prof.sustained_secs > 0 {
        let free_mb = if info.free_gb > 0.0 { (info.free_gb * 1000.0 * 0.25) as u64 } else { 1024 };
        let size_mb = free_mb.clamp(mb, 8192).max(16);
        res.sustained = sustained_test(ui, dir, prof.sustained_secs, size_mb, prof.disk_sync).ok();
    }
    ui.clear_progress();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("syscheck_test_{}_{}", name, std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn buffers_are_aligned_and_sized() {
        for len in [512usize, 4096, 1 << 20] {
            let mut b = AlignedBuf::new(len);
            assert_eq!(b.as_slice().as_ptr() as usize % 4096, 0);
            assert_eq!(b.as_mut_slice().len(), len);
        }
    }

    #[test]
    fn test_file_is_created_filled_and_removed() {
        let d = scratch("file");
        let p = d.join("t.tmp");
        {
            let tf = create_test_file(&p, 8 << 20, true, false).unwrap();
            assert_eq!(std::fs::metadata(&p).unwrap().len(), 8 << 20);
            // no holes: the first block must not be all zeros
            let mut b = AlignedBuf::new(4096);
            tf.file.pread(b.as_mut_slice(), 0).unwrap();
            assert!(b.as_slice().iter().any(|x| *x != 0));
        }
        assert!(!p.exists(), "guard must remove the file");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn engine_runs_all_patterns() {
        let d = scratch("engine");
        let p = d.join("t.tmp");
        let tf = create_test_file(&p, 16 << 20, true, false).unwrap();
        for (pat, read_pct, threads) in [(Pat::Seq, 100, 2), (Pat::Seq, 0, 2), (Pat::Rand, 100, 3), (Pat::Rand, 70, 4)] {
            let spec = Spec { bs: 4096, threads, pat, read_pct, secs: 0.1, lat: true };
            let o = run_spec(&tf.file, 16 << 20, spec, 1).unwrap();
            assert!(o.ops > 0 && o.bytes == o.ops * 4096, "{pat:?} {read_pct}");
            assert!(o.mbs() > 0.0 && o.iops() > 0.0);
            assert_eq!(o.thread_bytes.len(), threads);
            assert!(!o.lat_ns.is_empty());
        }
        drop(tf);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn large_blocks_work() {
        let d = scratch("large");
        let p = d.join("t.tmp");
        let tf = create_test_file(&p, 16 << 20, true, false).unwrap();
        let spec = Spec { bs: 8 << 20, threads: 2, pat: Pat::Seq, read_pct: 100, secs: 0.1, lat: false };
        let o = run_spec(&tf.file, 16 << 20, spec, 1).unwrap();
        assert!(o.ops > 0);
        drop(tf);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn small_files_clean_up() {
        let d = scratch("small");
        let r = small_file_test(&d, 30).unwrap();
        assert_eq!(r.files, 30);
        assert!(r.create_per_s > 0.0 && r.read_per_s > 0.0 && r.delete_per_s > 0.0);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "temp dir must be removed");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn full_run_on_tiny_profile() {
        let d = scratch("full");
        let ui = Ui::new(crate::i18n::Lang::En, true);
        let mut prof = Profile::new(true);
        prof.disk_file_mb = 16;
        prof.disk_point_secs = 0.03;
        prof.disk_scen_secs = 0.1;
        prof.small_files = 20;
        prof.disk_sizes = vec![512, 4096, 1 << 20];
        let info = crate::hw::disk_info(&d);
        let r = run(&ui, &d, &info, &prof);
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(!r.sweep.is_empty());
        assert_eq!(r.scenarios.len(), SCENARIOS.len());
        assert!(r.streams.is_some() && r.small_files.is_some());
        assert!(r.seq_read_mbs().unwrap() > 0.0);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "no temp files left behind");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn unwritable_directory_reports_error() {
        let ui = Ui::new(crate::i18n::Lang::En, true);
        let prof = Profile::new(true);
        let r = run(&ui, Path::new("/proc/definitely/not/here"), &DiskInfo::default(), &prof);
        assert!(r.error.is_some());
        assert!(r.sweep.is_empty());
    }
}
