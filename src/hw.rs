//! Hardware detection for x86-64, ARM64 (incl. Android/Termux), RISC-V and (partially) macOS.
//! Everything is best effort: every probe can fail and returns `None`/empty instead of aborting.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ───────────────────────── data model (stored in the result file) ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CoreGroup {
    pub count: u32,
    pub max_mhz: Option<u32>,
    pub capacity: Option<u32>,
    pub part: Option<String>,
    /// "P" / "E" on hybrid CPUs when known.
    pub kind: Option<String>,
    pub cpus: Vec<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CacheInfo {
    pub level: u8,
    pub kind: String,
    pub size_kb: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CpuInfo {
    pub model: String,
    pub arch: String,
    /// Hardware logical CPUs (online).
    pub logical: u32,
    /// CPUs this process may actually use (affinity / cgroup quota).
    pub usable: u32,
    pub physical: u32,
    pub packages: u32,
    pub groups: Vec<CoreGroup>,
    pub max_mhz: Option<u32>,
    pub governor: Option<String>,
    pub caches: Vec<CacheInfo>,
    pub features: Vec<String>,
    /// VT-x / AMD-V (or KVM on ARM/RISC-V). `None` = could not be determined.
    pub virt_ext: Option<bool>,
    pub hypervisor: bool,
    pub kvm: bool,
    pub iommu_groups: Option<u32>,
    /// CPU used for single-thread tests (fastest core).
    pub best_cpu: u32,
    /// CPUs available to benchmark threads.
    pub cpu_list: Vec<u32>,
}

impl CpuInfo {
    /// Largest cache found (bytes) - used to size memory tests. Falls back to 8 MiB.
    pub fn llc_bytes(&self) -> u64 {
        self.caches.iter().map(|c| c.size_kb * 1024).max().filter(|b| *b > 0).unwrap_or(8 * 1024 * 1024)
    }
    #[allow(dead_code)]
    pub fn has_feature(&self, f: &str) -> bool {
        self.features.iter().any(|x| x == f)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemInfo {
    pub total_mb: u64,
    pub available_mb: u64,
    pub swap_mb: u64,
    pub ecc: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DiskInfo {
    pub fs: Option<String>,
    /// nvme | ssd | hdd | emmc | sd | virtio | virtual | ramdisk | unknown
    pub kind: String,
    pub model: Option<String>,
    pub total_gb: f64,
    pub free_gb: f64,
    pub logical_block: Option<u32>,
    pub physical_block: Option<u32>,
    pub rotational: Option<bool>,
    pub ram_backed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NicInfo {
    pub name: String,
    /// ethernet | wifi | other
    pub kind: String,
    pub speed_mbps: Option<u32>,
    pub up: bool,
    pub default_route: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub pci_id: Option<String>,
    pub driver: Option<String>,
    pub vram_mb: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct OsInfo {
    pub name: String,
    pub kernel: String,
    pub arch: String,
    pub android: bool,
    pub container: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SystemInfo {
    /// Free text given by the user (`--label "Pixel 8"`), the only identifying field we store.
    pub label: Option<String>,
    pub model: Option<String>,
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub mem: MemInfo,
    pub disk: DiskInfo,
    pub nics: Vec<NicInfo>,
    pub gpus: Vec<GpuInfo>,
    /// Hardware video encoder paths that look present (detected only, not benchmarked).
    pub hw_video: Vec<String>,
}

// ───────────────────────── tiny helpers ─────────────────────────

fn rd(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().trim_matches('\0').trim().to_string()).filter(|s| !s.is_empty())
}

fn rd_num<T: std::str::FromStr>(p: impl AsRef<Path>) -> Option<T> {
    rd(p).and_then(|s| s.parse().ok())
}

/// "0-3,5,7-8" -> [0,1,2,3,5,7,8]
pub fn parse_cpu_list(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                out.extend(a..=b.max(a));
            }
        } else if let Ok(n) = part.parse::<u32>() {
            out.push(n);
        }
    }
    out
}

/// "32K" / "1024K" / "8M" -> bytes
pub fn parse_size_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last()? {
        'K' | 'k' => (&s[..s.len() - 1], 1024u64),
        'M' | 'm' => (&s[..s.len() - 1], 1024 * 1024),
        'G' | 'g' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file() || (cfg!(windows) && d.join(format!("{cmd}.exe")).is_file())))
        .unwrap_or(false)
}

fn fallback_n() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
}

// ───────────────────────── affinity ─────────────────────────

/// CPUs this process is allowed to run on.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn allowed_cpus() -> Vec<u32> {
    // SAFETY: plain libc call writing into a zeroed cpu_set_t of the right size.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        if libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) == 0 {
            let v: Vec<u32> = (0..1024u32).filter(|i| libc::CPU_ISSET(*i as usize, &set)).collect();
            if !v.is_empty() {
                return v;
            }
        }
    }
    (0..fallback_n()).collect()
}

#[cfg(windows)]
pub fn allowed_cpus() -> Vec<u32> {
    let v = crate::winsys::allowed_cpus();
    if v.is_empty() {
        (0..fallback_n()).collect()
    } else {
        v
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
pub fn allowed_cpus() -> Vec<u32> {
    (0..fallback_n()).collect()
}

/// Pin the calling thread to one CPU. Returns false when unsupported / refused.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn pin_to_cpu(cpu: u32) -> bool {
    // SAFETY: zeroed cpu_set_t is a valid empty set; pid 0 = calling thread.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(cpu as usize, &mut set);
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

#[cfg(windows)]
pub fn pin_to_cpu(cpu: u32) -> bool {
    crate::winsys::pin_to_cpu(cpu)
}

#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
pub fn pin_to_cpu(_cpu: u32) -> bool {
    false
}

// ───────────────────────── CPU ─────────────────────────

fn parse_cpuinfo(txt: &str) -> Vec<BTreeMap<String, String>> {
    let mut out = Vec::new();
    let mut cur = BTreeMap::new();
    for line in txt.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            cur.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn arm_part_name(imp: &str, part: &str) -> String {
    let i = imp.trim().trim_start_matches("0x").to_lowercase();
    let p = part.trim().trim_start_matches("0x").to_lowercase();
    let known = match (i.as_str(), p.as_str()) {
        ("41", "d03") => "Cortex-A53",
        ("41", "d04") => "Cortex-A35",
        ("41", "d05") => "Cortex-A55",
        ("41", "d07") => "Cortex-A57",
        ("41", "d08") => "Cortex-A72",
        ("41", "d09") => "Cortex-A73",
        ("41", "d0a") => "Cortex-A75",
        ("41", "d0b") => "Cortex-A76",
        ("41", "d0c") => "Neoverse-N1",
        ("41", "d0d") => "Cortex-A77",
        ("41", "d40") => "Neoverse-V1",
        ("41", "d41") => "Cortex-A78",
        ("41", "d44") => "Cortex-X1",
        ("41", "d46") => "Cortex-A510",
        ("41", "d47") => "Cortex-A710",
        ("41", "d48") => "Cortex-X2",
        ("41", "d49") => "Neoverse-N2",
        ("41", "d4d") => "Cortex-A715",
        ("41", "d4e") => "Cortex-X3",
        ("41", "d4f") => "Neoverse-V2",
        ("41", "d80") => "Cortex-A520",
        ("41", "d81") => "Cortex-A720",
        ("41", "d82") => "Cortex-X4",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let vendor = match i.as_str() {
        "41" => "ARM",
        "42" => "Broadcom",
        "43" => "Cavium",
        "46" => "Fujitsu",
        "48" => "HiSilicon",
        "4e" => "NVIDIA",
        "50" => "APM",
        "51" => "Qualcomm",
        "53" => "Samsung",
        "61" => "Apple",
        "69" => "Intel",
        "c0" => "Ampere",
        _ => "ARM-impl",
    };
    format!("{vendor} {i}:{p}")
}

fn curated_features(arch: &str, flags: &str, isa: &str) -> Vec<String> {
    let set: BTreeSet<&str> = flags.split_whitespace().collect();
    let pick = |wanted: &[&str]| -> Vec<String> {
        wanted.iter().filter(|w| set.contains(*w)).map(|w| w.to_string()).collect()
    };
    match arch {
        "x86_64" | "x86" | "i686" => pick(&[
            "sse4_2", "avx", "avx2", "fma", "bmi2", "avx512f", "avx512_vnni", "avx_vnni", "amx_tile", "aes", "sha_ni", "vmx", "svm",
        ]),
        "aarch64" | "arm" | "armv7l" | "armv8l" => {
            pick(&["asimd", "aes", "sha2", "sha3", "crc32", "atomics", "fphp", "asimddp", "i8mm", "bf16", "sve", "sve2", "sme"])
        }
        a if a.starts_with("riscv") => {
            let mut v: Vec<String> = Vec::new();
            let low = isa.to_lowercase();
            let base = low.split('_').next().unwrap_or("");
            // single-letter extensions after rv64/rv32 prefix
            let letters = base.trim_start_matches("rv64").trim_start_matches("rv32");
            if letters.contains('v') {
                v.push("rvv".into());
            }
            for tok in low.split('_').skip(1) {
                if ["zba", "zbb", "zbc", "zbs", "zicboz", "zknd", "zkne", "zknh", "zvl128b", "zvl256b"].contains(&tok) {
                    v.push(tok.to_string());
                }
            }
            v
        }
        _ => Vec::new(),
    }
}

fn read_caches(cpu: u32) -> Vec<CacheInfo> {
    let base = format!("/sys/devices/system/cpu/cpu{cpu}/cache");
    let mut out: Vec<CacheInfo> = Vec::new();
    let Ok(rd_dir) = fs::read_dir(&base) else { return out };
    for e in rd_dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("index") {
            continue;
        }
        let p = e.path();
        let level: Option<u8> = rd_num(p.join("level"));
        let kind = rd(p.join("type"));
        let size = rd(p.join("size")).and_then(|s| parse_size_bytes(&s));
        if let (Some(level), Some(kind), Some(size)) = (level, kind, size) {
            out.push(CacheInfo { level, kind, size_kb: size / 1024 });
        }
    }
    out.sort_by(|a, b| (a.level, &a.kind).cmp(&(b.level, &b.kind)));
    out.dedup_by(|a, b| a.level == b.level && a.kind == b.kind);
    out
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn detect_cpu() -> CpuInfo {
    let arch = std::env::consts::ARCH.to_string();
    let txt = rd("/proc/cpuinfo").unwrap_or_default();
    let blocks = parse_cpuinfo(&txt);

    let online = rd("/sys/devices/system/cpu/online").map(|s| parse_cpu_list(&s)).unwrap_or_default();
    let online: Vec<u32> = if online.is_empty() { (0..fallback_n()).collect() } else { online };
    let allowed = allowed_cpus();
    let usable = (allowed.len() as u32).min(fallback_n()).max(1);

    // processor index -> (implementer, part) for ARM
    let mut part_of: BTreeMap<u32, String> = BTreeMap::new();
    for b in &blocks {
        if let (Some(p), Some(imp), Some(part)) = (b.get("processor").and_then(|s| s.parse::<u32>().ok()), b.get("CPU implementer"), b.get("CPU part")) {
            part_of.insert(p, arm_part_name(imp, part));
        }
    }

    let hybrid_p: Vec<u32> = rd("/sys/devices/cpu_core/cpus").map(|s| parse_cpu_list(&s)).unwrap_or_default();
    let hybrid_e: Vec<u32> = rd("/sys/devices/cpu_atom/cpus").map(|s| parse_cpu_list(&s)).unwrap_or_default();

    // group CPUs into clusters with identical (capacity, max freq, part)
    type Key = (u32, u32, String);
    let mut groups: BTreeMap<Key, Vec<u32>> = BTreeMap::new();
    for &c in &online {
        let d = format!("/sys/devices/system/cpu/cpu{c}");
        let cap: u32 = rd_num(format!("{d}/cpu_capacity")).unwrap_or(0);
        let fmax: u32 = rd_num(format!("{d}/cpufreq/cpuinfo_max_freq")).unwrap_or(0);
        let part = part_of.get(&c).cloned().unwrap_or_default();
        groups.entry((cap, fmax, part)).or_default().push(c);
    }
    let mut core_groups: Vec<CoreGroup> = groups
        .into_iter()
        .map(|((cap, fmax, part), cpus)| {
            let first = cpus[0];
            let kind = if hybrid_e.contains(&first) {
                Some("E".to_string())
            } else if hybrid_p.contains(&first) && !hybrid_e.is_empty() {
                Some("P".to_string())
            } else {
                None
            };
            CoreGroup {
                count: cpus.len() as u32,
                max_mhz: if fmax > 0 { Some(fmax / 1000) } else { None },
                capacity: if cap > 0 { Some(cap) } else { None },
                part: if part.is_empty() { None } else { Some(part) },
                kind,
                cpus,
            }
        })
        .collect();
    core_groups.sort_by(|a, b| {
        (b.capacity.unwrap_or(0), b.max_mhz.unwrap_or(0)).cmp(&(a.capacity.unwrap_or(0), a.max_mhz.unwrap_or(0)))
    });

    // fastest core that we are actually allowed to use
    let best_cpu = core_groups
        .iter()
        .flat_map(|g| g.cpus.iter())
        .find(|c| allowed.contains(c))
        .copied()
        .or_else(|| allowed.first().copied())
        .unwrap_or(0);

    // physical cores / packages from topology
    let mut cores: BTreeSet<(i64, i64)> = BTreeSet::new();
    let mut pkgs: BTreeSet<i64> = BTreeSet::new();
    for &c in &online {
        let t = format!("/sys/devices/system/cpu/cpu{c}/topology");
        let p: i64 = rd_num(format!("{t}/physical_package_id")).unwrap_or(0);
        if let Some(core) = rd_num::<i64>(format!("{t}/core_id")) {
            cores.insert((p, core));
        }
        pkgs.insert(p);
    }
    let physical = if cores.is_empty() { online.len() as u32 } else { cores.len() as u32 };

    // model name
    let first = blocks.first();
    let mut model = String::new();
    if let Some(b) = first {
        if let Some(m) = b.get("model name") {
            model = m.clone();
        } else if arch.starts_with("riscv") {
            let u = b.get("uarch").or_else(|| b.get("isa")).cloned().unwrap_or_default();
            model = format!("RISC-V {u}").trim().to_string();
        }
    }
    if model.is_empty() {
        let names: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for g in &core_groups {
                if let Some(p) = &g.part {
                    if !v.contains(p) {
                        v.push(p.clone());
                    }
                }
            }
            v
        };
        if !names.is_empty() {
            model = names.join(" + ");
        }
    }
    if model.is_empty() {
        model = format!("unknown ({arch})");
    }

    // features
    let flags = first.and_then(|b| b.get("flags").or_else(|| b.get("Features"))).cloned().unwrap_or_default();
    let isa = first.and_then(|b| b.get("isa")).cloned().unwrap_or_default();
    let features = curated_features(&arch, &flags, &isa);

    let kvm = Path::new("/dev/kvm").exists();
    let is_x86 = matches!(arch.as_str(), "x86_64" | "x86");
    let virt_ext = if is_x86 {
        Some(flags.split_whitespace().any(|f| f == "vmx" || f == "svm"))
    } else if kvm {
        Some(true)
    } else {
        None
    };
    let hypervisor = flags.split_whitespace().any(|f| f == "hypervisor")
        || Path::new("/sys/hypervisor/type").exists()
        || rd("/sys/devices/virtual/dmi/id/product_name")
            .map(|p| {
                let p = p.to_lowercase();
                ["kvm", "vmware", "virtualbox", "virtual machine", "qemu", "xen", "amazon ec2", "google compute", "bochs"]
                    .iter()
                    .any(|k| p.contains(k))
            })
            .unwrap_or(false);

    let iommu_groups = fs::read_dir("/sys/kernel/iommu_groups").ok().map(|d| d.count() as u32);

    let max_mhz = rd_num::<u32>(format!("/sys/devices/system/cpu/cpu{best_cpu}/cpufreq/cpuinfo_max_freq"))
        .map(|k| k / 1000)
        .or_else(|| {
            blocks
                .iter()
                .filter_map(|b| b.get("cpu MHz").and_then(|s| s.parse::<f64>().ok()))
                .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
                .map(|m| m as u32)
        });
    let governor = rd(format!("/sys/devices/system/cpu/cpu{best_cpu}/cpufreq/scaling_governor"));

    CpuInfo {
        model,
        arch,
        logical: online.len() as u32,
        usable,
        physical,
        packages: pkgs.len().max(1) as u32,
        groups: core_groups,
        max_mhz,
        governor,
        caches: read_caches(best_cpu),
        features,
        virt_ext,
        hypervisor,
        kvm,
        iommu_groups,
        best_cpu,
        cpu_list: allowed,
    }
}

#[cfg(target_os = "macos")]
fn sysctl(name: &str) -> Option<String> {
    run_cmd("sysctl", &["-n", name])
}

#[cfg(target_os = "macos")]
fn detect_cpu() -> CpuInfo {
    let arch = std::env::consts::ARCH.to_string();
    let logical: u32 = sysctl("hw.logicalcpu").and_then(|s| s.parse().ok()).unwrap_or_else(fallback_n);
    let physical: u32 = sysctl("hw.physicalcpu").and_then(|s| s.parse().ok()).unwrap_or(logical);
    let mut groups = Vec::new();
    for (lvl, kind) in [(0, "P"), (1, "E")] {
        if let Some(n) = sysctl(&format!("hw.perflevel{lvl}.logicalcpu")).and_then(|s| s.parse::<u32>().ok()) {
            if n > 0 {
                groups.push(CoreGroup { count: n, kind: Some(kind.to_string()), part: sysctl(&format!("hw.perflevel{lvl}.name")), ..Default::default() });
            }
        }
    }
    let mut caches = Vec::new();
    for (name, level, kind) in [("hw.l1dcachesize", 1u8, "Data"), ("hw.l2cachesize", 2, "Unified"), ("hw.l3cachesize", 3, "Unified")] {
        if let Some(b) = sysctl(name).and_then(|s| s.parse::<u64>().ok()) {
            if b > 0 {
                caches.push(CacheInfo { level, kind: kind.into(), size_kb: b / 1024 });
            }
        }
    }
    let mut features = Vec::new();
    for (name, f) in [("hw.optional.arm.FEAT_AES", "aes"), ("hw.optional.arm.FEAT_SHA256", "sha2"), ("hw.optional.arm.FEAT_SHA3", "sha3"), ("hw.optional.arm.FEAT_BF16", "bf16"), ("hw.optional.arm.FEAT_I8MM", "i8mm")] {
        if sysctl(name).as_deref() == Some("1") {
            features.push(f.to_string());
        }
    }
    CpuInfo {
        model: sysctl("machdep.cpu.brand_string").unwrap_or_else(|| format!("unknown ({arch})")),
        arch,
        logical,
        usable: fallback_n().min(logical).max(1),
        physical,
        packages: 1,
        groups,
        max_mhz: sysctl("hw.cpufrequency_max").and_then(|s| s.parse::<u64>().ok()).map(|h| (h / 1_000_000) as u32),
        governor: None,
        caches,
        features,
        virt_ext: sysctl("kern.hv_support").map(|s| s == "1"),
        hypervisor: sysctl("kern.hv_vmm_present").as_deref() == Some("1"),
        kvm: false,
        iommu_groups: None,
        best_cpu: 0,
        cpu_list: (0..logical).collect(),
    }
}

/// CPU model name from the CPUID brand string (x86-64 only).
#[cfg(all(windows, target_arch = "x86_64"))]
fn cpuid_brand() -> Option<String> {
    use std::arch::x86_64::__cpuid;
    // SAFETY: CPUID exists on every x86-64 CPU.
    #[allow(unused_unsafe)]
    unsafe {
        if __cpuid(0x8000_0000).eax < 0x8000_0004 {
            return None;
        }
        let mut b: Vec<u8> = Vec::new();
        for leaf in 0x8000_0002u32..=0x8000_0004 {
            let r = __cpuid(leaf);
            for x in [r.eax, r.ebx, r.ecx, r.edx] {
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
        let s = String::from_utf8_lossy(&b).replace('\0', " ");
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        (!s.is_empty()).then_some(s)
    }
}

/// Feature flags using the Linux flag names, plus (hypervisor present, hypervisor vendor).
#[cfg(all(windows, target_arch = "x86_64"))]
fn win_cpu_flags() -> (String, bool, String) {
    use std::arch::x86_64::__cpuid;
    let mut f: Vec<&str> = Vec::new();
    macro_rules! chk {
        ($($feat:tt => $name:expr),* $(,)?) => { $( if std::arch::is_x86_feature_detected!($feat) { f.push($name); } )* };
    }
    chk!("sse4.2" => "sse4_2", "avx" => "avx", "avx2" => "avx2", "fma" => "fma", "bmi2" => "bmi2", "avx512f" => "avx512f", "avx512vnni" => "avx512_vnni", "aes" => "aes", "sha" => "sha_ni");
    // SAFETY: CPUID exists on every x86-64 CPU.
    #[allow(unused_unsafe)]
    let (vmx, svm, hv, vendor) = unsafe {
        let l1 = __cpuid(1);
        let ext = __cpuid(0x8000_0001);
        let hv = (l1.ecx >> 31) & 1 == 1;
        let mut vendor = String::new();
        if hv {
            let r = __cpuid(0x4000_0000);
            let mut b: Vec<u8> = Vec::new();
            for x in [r.ebx, r.ecx, r.edx] {
                b.extend_from_slice(&x.to_le_bytes());
            }
            vendor = String::from_utf8_lossy(&b).trim_matches('\0').trim().to_string();
        }
        ((l1.ecx >> 5) & 1 == 1, (ext.ecx >> 2) & 1 == 1, hv, vendor)
    };
    if vmx {
        f.push("vmx");
    }
    if svm {
        f.push("svm");
    }
    (f.join(" "), hv, vendor)
}

#[cfg(all(windows, target_arch = "aarch64"))]
fn win_cpu_flags_arm() -> String {
    let mut f: Vec<&str> = Vec::new();
    macro_rules! chk {
        ($($feat:tt => $name:expr),* $(,)?) => { $( if std::arch::is_aarch64_feature_detected!($feat) { f.push($name); } )* };
    }
    chk!("neon" => "asimd", "aes" => "aes", "sha2" => "sha2", "sha3" => "sha3", "crc" => "crc32", "lse" => "atomics", "dotprod" => "asimddp");
    f.join(" ")
}

#[cfg(windows)]
fn detect_cpu() -> CpuInfo {
    let arch = std::env::consts::ARCH.to_string();
    let logical = fallback_n();
    let allowed = allowed_cpus();
    let usable = (allowed.len() as u32).min(logical).max(1);
    let topo = crate::winsys::topology();

    #[cfg(target_arch = "x86_64")]
    let (flags, hv, hv_vendor) = win_cpu_flags();
    #[cfg(target_arch = "x86_64")]
    let model = cpuid_brand();
    #[cfg(not(target_arch = "x86_64"))]
    let (flags, hv, hv_vendor) = {
        #[cfg(target_arch = "aarch64")]
        let f = win_cpu_flags_arm();
        #[cfg(not(target_arch = "aarch64"))]
        let f = String::new();
        (f, false, String::new())
    };
    #[cfg(not(target_arch = "x86_64"))]
    let model: Option<String> = None;
    let model = model.or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok()).unwrap_or_else(|| format!("unknown ({arch})"));

    // VT-x / AMD-V: visible directly, or - when Hyper-V is active and hides it - ask the firmware flag.
    let is_x86 = arch == "x86_64";
    let has_ext = flags.split_whitespace().any(|f| f == "vmx" || f == "svm");
    let virt_ext = if !is_x86 {
        None
    } else if has_ext {
        Some(true)
    } else if hv && hv_vendor.starts_with("Microsoft") {
        // Hyper-V (VBS, WSL2, Windows Sandbox...) hides the extension from the host; trust the firmware flag
        crate::winsys::virt_firmware_enabled().then_some(true)
    } else {
        Some(false)
    };
    // The Windows host running Hyper-V itself also shows the hypervisor bit; do not call it a guest unless the vendor says otherwise.
    let hypervisor = hv && !(hv_vendor.starts_with("Microsoft") && virt_ext == Some(true));

    let physical = topo.as_ref().map(|t| t.physical).filter(|p| *p > 0).unwrap_or(logical).min(logical);
    let caches = topo
        .as_ref()
        .map(|t| t.caches.iter().map(|(l, k, b)| CacheInfo { level: *l, kind: (*k).to_string(), size_kb: *b / 1024 }).collect())
        .unwrap_or_default();
    CpuInfo {
        model,
        arch: arch.clone(),
        logical,
        usable,
        physical,
        packages: topo.as_ref().map(|t| t.packages).filter(|p| *p > 0).unwrap_or(1),
        caches,
        features: curated_features(&arch, &flags, ""),
        virt_ext,
        hypervisor,
        kvm: false,
        iommu_groups: None,
        best_cpu: allowed.first().copied().unwrap_or(0),
        cpu_list: allowed,
        ..Default::default()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", windows)))]
fn detect_cpu() -> CpuInfo {
    let n = fallback_n();
    CpuInfo {
        model: format!("unknown ({})", std::env::consts::ARCH),
        arch: std::env::consts::ARCH.into(),
        logical: n,
        usable: n,
        physical: n,
        packages: 1,
        cpu_list: (0..n).collect(),
        ..Default::default()
    }
}

// ───────────────────────── memory ─────────────────────────

#[cfg(any(target_os = "linux", target_os = "android"))]
fn detect_mem() -> MemInfo {
    let mut m = MemInfo::default();
    for line in rd("/proc/meminfo").unwrap_or_default().lines() {
        let mut it = line.split_whitespace();
        let (Some(k), Some(v)) = (it.next(), it.next()) else { continue };
        let Ok(kb) = v.parse::<u64>() else { continue };
        match k {
            "MemTotal:" => m.total_mb = kb / 1024,
            "MemAvailable:" => m.available_mb = kb / 1024,
            "SwapTotal:" => m.swap_mb = kb / 1024,
            _ => {}
        }
    }
    if let Ok(d) = fs::read_dir("/sys/devices/system/edac/mc") {
        if d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("mc")) {
            m.ecc = Some(true);
        }
    }
    m
}

#[cfg(target_os = "macos")]
fn detect_mem() -> MemInfo {
    let total = sysctl("hw.memsize").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) / (1024 * 1024);
    // rough: macOS keeps a lot of "inactive" memory reclaimable; use 60 % as the usable estimate
    MemInfo { total_mb: total, available_mb: total * 6 / 10, swap_mb: 0, ecc: None }
}

#[cfg(windows)]
fn detect_mem() -> MemInfo {
    match crate::winsys::memory() {
        Some((total, avail, swap)) => MemInfo { total_mb: total / (1024 * 1024), available_mb: avail / (1024 * 1024), swap_mb: swap / (1024 * 1024), ecc: None },
        None => MemInfo::default(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", windows)))]
fn detect_mem() -> MemInfo {
    MemInfo::default()
}

// ───────────────────────── disk ─────────────────────────

#[cfg(windows)]
fn statvfs(path: &Path) -> Option<(u64, u64)> {
    crate::winsys::disk_space(path)
}

#[cfg(unix)]
fn statvfs(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: valid NUL-terminated path, zeroed output struct.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) == 0 {
            let fr = s.f_frsize as u64;
            Some((s.f_blocks as u64 * fr, s.f_bavail as u64 * fr))
        } else {
            None
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fs_type_of(path: &Path) -> Option<String> {
    let p = fs::canonicalize(path).ok()?;
    let mounts = rd("/proc/mounts")?;
    let mut best: (usize, String) = (0, String::new());
    for line in mounts.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 {
            continue;
        }
        let mp = f[1].replace("\\040", " ");
        let hit = mp == "/" || p == Path::new(&mp) || p.starts_with(&mp);
        if hit && mp.len() >= best.0 {
            best = (mp.len(), f[2].to_string());
        }
    }
    if best.1.is_empty() {
        None
    } else {
        Some(best.1)
    }
}

#[cfg(windows)]
fn fs_type_of(path: &Path) -> Option<String> {
    crate::winsys::fs_name(path)
}

#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
fn fs_type_of(_path: &Path) -> Option<String> {
    None
}

/// Resolve a path to the sysfs directory of its underlying (whole) block device.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn sysfs_disk_dir(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let dev = fs::metadata(path).ok()?.dev();
    // Linux dev_t encoding
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    if major == 0 {
        return None; // anonymous device (btrfs, overlayfs, tmpfs, ...)
    }
    let mut dir = fs::canonicalize(format!("/sys/dev/block/{major}:{minor}")).ok()?;
    for _ in 0..2 {
        if dir.join("partition").exists() {
            dir = dir.parent()?.to_path_buf();
        }
        if dir.to_string_lossy().contains("/virtual/") {
            // device-mapper / md: follow the first underlying device
            let slave = fs::read_dir(dir.join("slaves")).ok()?.flatten().next()?;
            dir = fs::canonicalize(format!("/sys/class/block/{}", slave.file_name().to_string_lossy())).ok()?;
        }
    }
    if dir.join("partition").exists() {
        dir = dir.parent()?.to_path_buf();
    }
    Some(dir)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fill_block_info(path: &Path, d: &mut DiskInfo) {
    let Some(dir) = sysfs_disk_dir(path) else { return };
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    d.rotational = rd_num::<u8>(dir.join("queue/rotational")).map(|v| v == 1);
    d.logical_block = rd_num(dir.join("queue/logical_block_size"));
    d.physical_block = rd_num(dir.join("queue/physical_block_size"));
    d.model = rd(dir.join("device/model")).or_else(|| rd(dir.join("device/name")));
    d.kind = if name.starts_with("nvme") {
        "nvme".into()
    } else if name.starts_with("mmcblk") {
        match rd(dir.join("device/type")).as_deref() {
            Some("SD") => "sd".into(),
            _ => "emmc".into(),
        }
    } else if name.starts_with("vd") || name.starts_with("xvd") {
        "virtio".into()
    } else if name.starts_with("loop") || name.starts_with("zram") || name.starts_with("ram") {
        "virtual".into()
    } else if d.rotational == Some(true) {
        "hdd".into()
    } else if d.rotational == Some(false) {
        "ssd".into()
    } else {
        "unknown".into()
    };
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn fill_block_info(_path: &Path, _d: &mut DiskInfo) {}

pub fn disk_info(path: &Path) -> DiskInfo {
    let mut d = DiskInfo { kind: "unknown".into(), ..Default::default() };
    if let Some((total, free)) = statvfs(path) {
        d.total_gb = total as f64 / 1e9;
        d.free_gb = free as f64 / 1e9;
    }
    d.fs = fs_type_of(path);
    d.ram_backed = matches!(d.fs.as_deref(), Some("tmpfs") | Some("ramfs") | Some("devtmpfs"));
    if d.ram_backed {
        d.kind = "ramdisk".into();
    } else {
        fill_block_info(path, &mut d);
    }
    d
}

// ───────────────────────── network interfaces ─────────────────────────

/// (interface, gateway IPv4) of the default route, from /proc/net/route.
pub fn default_gateway() -> Option<(String, std::net::Ipv4Addr)> {
    let txt = rd("/proc/net/route")?;
    for line in txt.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 3 && f[1] == "00000000" {
            let gw = u32::from_str_radix(f[2], 16).ok()?;
            let b = gw.to_le_bytes();
            return Some((f[0].to_string(), std::net::Ipv4Addr::new(b[0], b[1], b[2], b[3])));
        }
    }
    None
}

fn detect_nics() -> Vec<NicInfo> {
    let mut out = Vec::new();
    let def = default_gateway().map(|g| g.0);
    let Ok(dir) = fs::read_dir("/sys/class/net") else { return out };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }
        let p = e.path();
        // skip virtual devices (bridges, veth, docker, tun, ...)
        if fs::canonicalize(&p).map(|c| c.to_string_lossy().contains("/virtual/")).unwrap_or(false) {
            continue;
        }
        let up = rd(p.join("operstate")).map(|s| s == "up").unwrap_or(false);
        let speed = rd(p.join("speed")).and_then(|s| s.parse::<i64>().ok()).filter(|s| *s > 0).map(|s| s as u32);
        let kind = if p.join("wireless").exists() || p.join("phy80211").exists() { "wifi" } else { "ethernet" };
        out.push(NicInfo { default_route: def.as_deref() == Some(name.as_str()), name, kind: kind.into(), speed_mbps: speed, up });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ───────────────────────── GPU (detection only) ─────────────────────────

fn pci_ids_lookup(txt: &str, vendor: &str, device: &str) -> (Option<String>, Option<String>) {
    let mut vname = None;
    let mut in_vendor = false;
    for line in txt.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                break; // reached next vendor without finding the device
            }
            if let Some(rest) = line.strip_prefix(vendor) {
                if rest.starts_with("  ") {
                    vname = Some(rest.trim().to_string());
                    in_vendor = true;
                }
            }
        } else if in_vendor && !line.starts_with("\t\t") {
            if let Some(rest) = line.trim_start_matches('\t').strip_prefix(device) {
                if rest.starts_with("  ") {
                    return (vname, Some(rest.trim().to_string()));
                }
            }
        }
    }
    (vname, None)
}

fn detect_gpus() -> Vec<GpuInfo> {
    let mut out: Vec<GpuInfo> = Vec::new();
    let pci_ids = ["/usr/share/misc/pci.ids", "/usr/share/hwdata/pci.ids", "/usr/share/pci.ids"].iter().find_map(|p| fs::read_to_string(p).ok());
    let Ok(dir) = fs::read_dir("/sys/class/drm") else { return out };
    let mut seen = BTreeSet::new();
    let mut cards: Vec<_> = dir.flatten().filter(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit())
    }).collect();
    cards.sort_by_key(|e| e.file_name());
    for e in cards {
        let dev = e.path().join("device");
        let driver = fs::read_link(dev.join("driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
        let vid = rd(dev.join("vendor")).map(|s| s.trim_start_matches("0x").to_lowercase());
        let did = rd(dev.join("device")).map(|s| s.trim_start_matches("0x").to_lowercase());
        let pci_id = match (&vid, &did) {
            (Some(v), Some(d)) => Some(format!("{v}:{d}")),
            _ => None,
        };
        let key = pci_id.clone().or_else(|| driver.clone()).unwrap_or_default();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        let vendor = match vid.as_deref() {
            Some("10de") => "NVIDIA",
            Some("1002") => "AMD",
            Some("8086") => "Intel",
            Some("13b5") => "ARM",
            Some("5143") => "Qualcomm",
            Some(_) => "Other",
            None => match driver.as_deref() {
                Some("msm") => "Qualcomm",
                Some("panfrost") | Some("panthor") | Some("lima") => "ARM",
                Some("v3d") | Some("vc4") => "Broadcom",
                Some("etnaviv") => "Vivante",
                Some("asahi") => "Apple",
                _ => "Unknown",
            },
        };
        let mut name = String::new();
        if let (Some(txt), Some(v), Some(d)) = (&pci_ids, &vid, &did) {
            let (_, dn) = pci_ids_lookup(txt, v, d);
            if let Some(dn) = dn {
                name = dn;
            }
        }
        if name.is_empty() {
            name = match driver.as_deref() {
                Some("msm") => "Qualcomm Adreno".to_string(),
                Some("panfrost") | Some("panthor") | Some("lima") => "ARM Mali".to_string(),
                Some("v3d") => "Broadcom V3D".to_string(),
                Some("vc4") => "Broadcom VideoCore".to_string(),
                Some("etnaviv") => "Vivante GPU".to_string(),
                Some("asahi") => "Apple GPU".to_string(),
                _ => match &pci_id {
                    Some(id) => format!("{vendor} GPU [{id}]"),
                    None => format!("{vendor} GPU"),
                },
            };
        }
        let vram_mb = rd_num::<u64>(dev.join("mem_info_vram_total")).map(|b| b / (1024 * 1024));
        out.push(GpuInfo { name, vendor: vendor.to_string(), pci_id, driver, vram_mb });
    }

    // NVIDIA proprietary driver: nvidia-smi knows the real name and VRAM
    if let Some(s) = run_cmd("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]) {
        let smi: Vec<(String, Option<u64>)> = s
            .lines()
            .filter_map(|l| {
                let (n, m) = l.split_once(',')?;
                Some((n.trim().to_string(), m.trim().parse::<u64>().ok()))
            })
            .collect();
        if !smi.is_empty() {
            out.retain(|g| g.vendor != "NVIDIA");
            for (n, m) in smi {
                out.push(GpuInfo { name: n, vendor: "NVIDIA".into(), pci_id: None, driver: Some("nvidia".into()), vram_mb: m });
            }
        }
    }
    out
}

fn detect_hw_video(gpus: &[GpuInfo]) -> Vec<String> {
    let mut v = Vec::new();
    if fs::read_dir("/dev/dri").map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("renderD"))).unwrap_or(false) {
        v.push("vaapi".to_string());
    }
    if gpus.iter().any(|g| g.vendor == "NVIDIA") && which("nvidia-smi") {
        v.push("nvenc".to_string());
    }
    if cfg!(target_os = "android") {
        v.push("mediacodec".to_string());
    }
    if cfg!(target_os = "macos") {
        v.push("videotoolbox".to_string());
    }
    v
}

// ───────────────────────── OS / model ─────────────────────────

fn getprop(key: &str) -> Option<String> {
    if cfg!(target_os = "android") {
        run_cmd("getprop", &[key])
    } else {
        None
    }
}

fn detect_model() -> Option<String> {
    if cfg!(target_os = "android") {
        let m = [getprop("ro.product.manufacturer"), getprop("ro.product.model")].into_iter().flatten().collect::<Vec<_>>().join(" ");
        if !m.is_empty() {
            return Some(match getprop("ro.soc.model") {
                Some(soc) => format!("{m} [{soc}]"),
                None => m,
            });
        }
    }
    let junk = |s: &str| {
        let l = s.to_lowercase();
        l.contains("to be filled") || l.contains("system product") || l.contains("default string") || l == "none" || l.contains("not specified")
    };
    let vendor = rd("/sys/devices/virtual/dmi/id/sys_vendor").filter(|s| !junk(s));
    let prod = rd("/sys/devices/virtual/dmi/id/product_name").filter(|s| !junk(s));
    let dmi = [vendor, prod].into_iter().flatten().collect::<Vec<_>>().join(" ");
    if !dmi.is_empty() {
        return Some(dmi);
    }
    rd("/proc/device-tree/model").or_else(|| rd("/sys/firmware/devicetree/base/model")).or_else(|| {
        parse_cpuinfo(&rd("/proc/cpuinfo").unwrap_or_default())
            .iter()
            .find_map(|b| b.get("Model").or_else(|| b.get("Hardware")).cloned())
    })
    .or_else(|| {
        #[cfg(target_os = "macos")]
        {
            return sysctl("hw.model");
        }
        #[allow(unreachable_code)]
        None
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn kernel_release() -> String {
    // SAFETY: uname fills a zeroed utsname; release is NUL-terminated.
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut u) == 0 {
            let bytes: Vec<u8> = u.release.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
            return String::from_utf8_lossy(&bytes).to_string();
        }
    }
    String::new()
}

/// "Microsoft Windows [Version 10.0.22631.3447]" (or a localized variant) -> ("10.0.22631.3447", 22631)
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_win_ver(s: &str) -> Option<(String, u32)> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let ver: String = s[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let build = ver.split('.').nth(2)?.parse::<u32>().ok()?;
    Some((ver, build))
}

#[cfg(windows)]
fn windows_ver() -> Option<(String, u32)> {
    parse_win_ver(&run_cmd("cmd", &["/C", "ver"])?)
}

#[cfg(windows)]
fn kernel_release() -> String {
    windows_ver().map(|v| v.0).unwrap_or_default()
}

#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
fn kernel_release() -> String {
    run_cmd("uname", &["-r"]).unwrap_or_default()
}

fn detect_os() -> OsInfo {
    let android = cfg!(target_os = "android");
    let mut name = String::new();
    if let Some(txt) = rd("/etc/os-release") {
        for l in txt.lines() {
            if let Some(v) = l.strip_prefix("PRETTY_NAME=") {
                name = v.trim_matches('"').to_string();
            }
        }
    }
    if android {
        name = format!("Android {}", getprop("ro.build.version.release").unwrap_or_default()).trim().to_string();
    } else if cfg!(target_os = "macos") {
        name = format!("macOS {}", run_cmd("sw_vers", &["-productVersion"]).unwrap_or_default()).trim().to_string();
    }
    #[cfg(windows)]
    if let Some((ver, build)) = windows_ver() {
        name = format!("{} (build {})", if build >= 22000 { "Windows 11" } else { "Windows 10" }, ver.split('.').nth(2).unwrap_or(""));
    } else {
        name = "Windows".into();
    }
    if name.is_empty() {
        name = std::env::consts::OS.to_string();
    }
    let container = Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || rd("/proc/1/cgroup").map(|c| ["docker", "kubepods", "containerd", "lxc"].iter().any(|k| c.contains(k))).unwrap_or(false);
    OsInfo { name, kernel: kernel_release(), arch: std::env::consts::ARCH.to_string(), android, container }
}

// ───────────────────────── entry point ─────────────────────────

pub fn detect(label: Option<String>, disk_path: &Path) -> SystemInfo {
    let gpus = detect_gpus();
    SystemInfo {
        label,
        model: detect_model(),
        os: detect_os(),
        cpu: detect_cpu(),
        mem: detect_mem(),
        disk: disk_info(disk_path),
        nics: detect_nics(),
        hw_video: detect_hw_video(&gpus),
        gpus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists() {
        assert_eq!(parse_cpu_list("0-3,5,7-8"), vec![0, 1, 2, 3, 5, 7, 8]);
        assert_eq!(parse_cpu_list("0"), vec![0]);
        assert!(parse_cpu_list("").is_empty());
        assert!(parse_cpu_list("garbage").is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size_bytes("32K"), Some(32 * 1024));
        assert_eq!(parse_size_bytes("8M"), Some(8 * 1024 * 1024));
        assert_eq!(parse_size_bytes("512"), Some(512));
        assert_eq!(parse_size_bytes("x"), None);
    }

    #[test]
    fn arm_parts() {
        assert_eq!(arm_part_name("0x41", "0xd0b"), "Cortex-A76");
        assert_eq!(arm_part_name("0x41", "0xd05"), "Cortex-A55");
        assert_eq!(arm_part_name("0x51", "0x805"), "Qualcomm 51:805");
        assert_eq!(arm_part_name("0x61", "0x022"), "Apple 61:022");
    }

    #[test]
    fn cpuinfo_blocks() {
        let t = "processor\t: 0\nmodel name\t: Foo CPU\n\nprocessor\t: 1\nmodel name\t: Foo CPU\n";
        let b = parse_cpuinfo(t);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1].get("processor").unwrap(), "1");
    }

    #[test]
    fn windows_version_parsing() {
        assert_eq!(parse_win_ver("Microsoft Windows [Version 10.0.22631.3447]"), Some(("10.0.22631.3447".to_string(), 22631)));
        assert_eq!(parse_win_ver("Microsoft Windows [Sürüm 10.0.19045.4291]"), Some(("10.0.19045.4291".to_string(), 19045)));
        assert_eq!(parse_win_ver("nonsense"), None);
    }

    #[test]
    fn riscv_features() {
        let f = curated_features("riscv64", "", "rv64imafdcv_zba_zbb_zicboz");
        assert!(f.contains(&"rvv".to_string()));
        assert!(f.contains(&"zba".to_string()));
        assert!(!curated_features("riscv64", "", "rv64imafdc").contains(&"rvv".to_string()));
    }

    #[test]
    fn pci_lookup() {
        let ids = "# comment\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t744c  Navi 31 [Radeon RX 7900 XTX]\n\t\t1002 0e3a  Sub\n10de  NVIDIA Corporation\n\t2684  AD102 [GeForce RTX 4090]\n";
        assert_eq!(pci_ids_lookup(ids, "10de", "2684").1.as_deref(), Some("AD102 [GeForce RTX 4090]"));
        assert_eq!(pci_ids_lookup(ids, "1002", "744c").1.as_deref(), Some("Navi 31 [Radeon RX 7900 XTX]"));
        assert_eq!(pci_ids_lookup(ids, "1002", "ffff").1, None);
    }

    #[test]
    fn detect_smoke() {
        let s = detect(Some("test".into()), Path::new("."));
        assert!(s.cpu.logical >= 1);
        assert!(s.cpu.usable >= 1);
        assert!(!s.cpu.cpu_list.is_empty());
        assert!(!s.cpu.model.is_empty());
        assert!(s.mem.total_mb > 0);
        assert_eq!(s.label.as_deref(), Some("test"));
    }

    #[test]
    fn json_roundtrip() {
        let s = detect(None, Path::new("."));
        let j = serde_json::to_string(&s).unwrap();
        let back: SystemInfo = serde_json::from_str(&j).unwrap();
        assert_eq!(back.cpu.model, s.cpu.model);
    }
}
