//! Terminal / text report. The same function produces the colored terminal output (color = true)
//! and the plain `.txt` report (color = false). All text goes through the i18n table, so a saved
//! JSON record can be rendered again in any language.

use crate::bench::disk::DiskResults;
use crate::i18n::{self, Lang};
use crate::record::{Finding, Record};
use crate::score::verdict_key;
use crate::util::*;

const W: usize = 76;

struct Cx {
    lang: Lang,
    color: bool,
    out: String,
}

impl Cx {
    fn t(&self, k: &str) -> String {
        i18n::t(self.lang, k).to_string()
    }
    fn tf(&self, k: &str, a: &[String]) -> String {
        i18n::tf(self.lang, k, a)
    }
    fn p(&self, code: &str, s: &str) -> String {
        paint(self.color, code, s)
    }
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }
    fn head(&mut self, key: &str) {
        let title = self.t(key);
        self.line("");
        let l = self.p(ansi::CYAN, &"─".repeat(W));
        self.line(&l);
        let t = self.p(&format!("{}{}", ansi::BOLD, ansi::CYAN), &format!("  {title}"));
        self.line(&t);
        self.line(&l);
    }
    fn sub(&mut self, title: &str) {
        let t = self.p(&format!("{}{}", ansi::BOLD, ansi::BLUE), &format!("\n  {title}"));
        self.line(&t);
    }
    fn kv(&mut self, key: &str, v: &str) {
        let k = pad_right(&self.t(key), 34);
        let k = self.p(ansi::GRAY, &k);
        let mut first = true;
        for part in wrap(v, W - 38) {
            if first {
                self.line(&format!("  {k} {part}"));
                first = false;
            } else {
                self.line(&format!("  {} {part}", " ".repeat(34)));
            }
        }
    }
    fn note(&mut self, s: &str) {
        for part in wrap(s, W - 4) {
            let t = self.p(ansi::GRAY, &format!("  {part}"));
            self.line(&t);
        }
    }
}

/// Word wrap by characters (not bytes), so Turkish text is measured correctly.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

fn yes_no(cx: &Cx, v: Option<bool>) -> String {
    match v {
        Some(true) => cx.t("yes"),
        Some(false) => cx.t("no"),
        None => cx.t("unknown"),
    }
}

fn level_color(score: f64) -> &'static str {
    if score >= 75.0 {
        ansi::GREEN
    } else if score >= 55.0 {
        ansi::CYAN
    } else if score >= 35.0 {
        ansi::YELLOW
    } else {
        ansi::RED
    }
}

fn opt(x: Option<f64>) -> String {
    x.map(num).unwrap_or_else(|| "-".into())
}

pub fn finding_text(lang: Lang, f: &Finding) -> String {
    i18n::tf(lang, &f.key, &f.args)
}

pub fn text(rec: &Record, lang: Lang, color: bool) -> String {
    let mut cx = Cx { lang, color, out: String::new() };
    banner(&mut cx, rec);
    system(&mut cx, rec);
    if let Some(c) = &rec.results.cpu {
        cpu(&mut cx, c);
    }
    if let Some(m) = &rec.results.mem {
        mem(&mut cx, m);
    }
    if let Some(d) = &rec.results.disk {
        disk(&mut cx, d);
    }
    if let Some(n) = &rec.results.net {
        net(&mut cx, n);
    } else if rec.results.cpu.is_some() && rec.results.mem.is_some() && rec.results.disk.is_some() {
        cx.head("h_net");
        let t = cx.t("msg_net_howto");
        cx.note(&t);
    }
    if let Some(v) = &rec.results.video {
        video(&mut cx, v);
    }
    if rec.results.rt.is_some() || rec.results.gpu.as_ref().map(|g| g.adapters.iter().any(|a| a.rt.is_some())).unwrap_or(false) {
        rt(&mut cx, rec);
    }
    if let Some(g) = &rec.results.gpu {
        gpu(&mut cx, g);
    }
    scores(&mut cx, rec);
    findings(&mut cx, rec);
    let foot = cx.t("msg_footer");
    cx.line("");
    cx.note(&foot);
    cx.out
}

fn banner(cx: &mut Cx, rec: &Record) {
    let l = cx.p(ansi::MAGENTA, &"═".repeat(W));
    cx.line(&l);
    let t = cx.p(&format!("{}{}", ansi::BOLD, ansi::MAGENTA), &format!("  SysCheck {}  ·  {}", rec.tool.version, cx.t("msg_subtitle")));
    cx.line(&t);
    let info = format!("  {}  ·  {}  ·  {}  ·  {}", rec.created_utc, rec.tool.spec, rec.mode, rec.run_id);
    let g = cx.p(ansi::GRAY, &info);
    cx.line(&g);
    cx.line(&l);
}

fn system(cx: &mut Cx, rec: &Record) {
    cx.head("h_system");
    let s = &rec.system;
    if let Some(l) = &s.label {
        cx.kv("l_label", l);
    }
    if let Some(m) = &s.model {
        cx.kv("l_model", m);
    }
    cx.kv("l_os", &format!("{}  ({} / {})", s.os.name, s.os.kernel, s.os.arch));
    if s.os.android {
        cx.kv("l_android", &cx.t("yes"));
    }
    if s.os.container {
        cx.kv("l_container", &cx.t("yes"));
    }
    let c = &s.cpu;
    cx.kv("l_cpu", &c.model);
    cx.kv("l_cores", &format!("{} {} / {} {}  ({})", c.usable, cx.t("u_threads"), c.physical, cx.t("u_cores"), c.arch));
    for g in &c.groups {
        if c.groups.len() > 1 {
            let mhz = g.max_mhz.map(|m| format!(" @ {m} MHz")).unwrap_or_default();
            let part = g.part.clone().unwrap_or_default();
            let kind = g.kind.clone().map(|k| format!("{k}-core ")).unwrap_or_default();
            cx.kv("l_cluster", &format!("{}× {}{}{}", g.count, kind, part, mhz));
        }
    }
    if let Some(m) = c.max_mhz {
        cx.kv("l_freq", &format!("{m} MHz"));
    }
    if !c.caches.is_empty() {
        let mut parts = Vec::new();
        for lvl in 1..=4u8 {
            let sz: u64 = c.caches.iter().filter(|x| x.level == lvl && x.kind != "Instruction").map(|x| x.size_kb).max().unwrap_or(0);
            if sz > 0 {
                parts.push(format!("L{lvl} {}", if sz >= 1024 { format!("{} MB", sz / 1024) } else { format!("{sz} KB") }));
            }
        }
        cx.kv("l_cache", &parts.join("  "));
    }
    if !c.features.is_empty() {
        let mut f = c.features.join(" ");
        if f.len() > 56 {
            f.truncate(56);
            f.push('…');
        }
        cx.kv("l_features", &f);
    }
    cx.kv("l_virt", &format!("{}   KVM: {}   IOMMU: {}", yes_no(cx, c.virt_ext), yes_no(cx, Some(c.kvm)), c.iommu_groups.map(|n| n.to_string()).unwrap_or_else(|| "-".into())));
    if c.hypervisor {
        cx.kv("l_hypervisor", &cx.t("yes"));
    }
    let m = &s.mem;
    cx.kv("l_ram", &format!("{:.1} GB  ({:.1} GB {})   swap {:.1} GB   ECC: {}", m.total_mb as f64 / 1024.0, m.available_mb as f64 / 1024.0, cx.t("u_free"), m.swap_mb as f64 / 1024.0, yes_no(cx, m.ecc)));
    let d = &s.disk;
    cx.kv(
        "l_disk",
        &format!("{} {}  {}  {:.0} GB ({:.0} GB {})", d.kind, d.fs.clone().unwrap_or_default(), d.model.clone().unwrap_or_default(), d.total_gb, d.free_gb, cx.t("u_free")).replace("  ", " "),
    );
    for n in &s.nics {
        let sp = n.speed_mbps.map(|x| format!("{x} Mbit/s")).unwrap_or_else(|| "?".into());
        let def = if n.default_route { " *" } else { "" };
        cx.kv("l_nic", &format!("{} ({}) {}{}", n.name, n.kind, sp, def));
    }
    if s.gpus.is_empty() {
        cx.kv("l_gpu", &cx.t("none"));
    }
    for g in &s.gpus {
        let v = g.vram_mb.map(|v| format!("  {} MB", v)).unwrap_or_default();
        cx.kv("l_gpu", &format!("{}{}", g.name, v));
    }
    if !s.hw_video.is_empty() {
        cx.kv("l_hwvideo", &s.hw_video.join(", "));
    }
}

fn cpu(cx: &mut Cx, c: &crate::bench::cpu::CpuResults) {
    cx.head("h_cpu");
    let hdr = format!("  {} {} {}", pad_right(&cx.t("l_kernel"), 24), pad_left(&cx.t("l_single"), 12), pad_left(&cx.t("l_multi"), 12));
    let h = cx.p(ansi::BOLD, &hdr);
    cx.line(&h);
    for k in &c.kernels {
        let name = cx.t(&format!("k_{}", k.name));
        cx.line(&format!("  {} {} {}  {}", pad_right(&name, 24), pad_left(&num(k.single), 12), pad_left(&num(k.multi), 12), k.unit));
    }
    cx.line("");
    cx.kv("l_geo", &format!("{}  /  {}", num(c.single_geo), num(c.multi_geo)));
    cx.kv("l_scaling", &format!("{:.2}×  ({} {}, {:.0} %)", c.scaling, c.threads, cx.t("u_threads"), c.efficiency_pct));
    cx.kv("l_noise", &if c.reps >= 2 { format!("{:.1} %", c.max_noise_pct) } else { "-".to_string() });
    cx.kv("l_pinned", &yes_no(cx, Some(c.pinned)));
    let sha = match (c.sha.hw_mbs, &c.sha.hw_feature, c.sha.speedup) {
        (Some(h), Some(f), Some(s)) => format!("{} MB/s → {} MB/s  ({}, {:.1}×)", num(c.sha.soft_mbs), num(h), f, s),
        _ => format!("{} MB/s  ({})", num(c.sha.soft_mbs), cx.t("msg_no_hw")),
    };
    cx.kv("l_sha", &sha);
    match &c.simd {
        Some(s) => cx.kv("l_simd", &format!("{} MFLOPS → {} MFLOPS  ({}, {:.1}×)", num(s.base_mflops), num(s.accel_mflops), s.feature, s.speedup)),
        None => cx.kv("l_simd", &cx.t("msg_no_simd")),
    }
    if !c.groups.is_empty() {
        cx.sub(&cx.t("l_clusters"));
        let max = c.groups.iter().map(|g| g.int_mops).fold(0.0, f64::max).max(1e-9);
        for g in &c.groups {
            let b = bar(cx.color, ansi::GREEN, g.int_mops / max, 24, true);
            cx.line(&format!("  {} {} {} Mops/s  {} MFLOPS", pad_right(&g.label, 22), b, pad_left(&num(g.int_mops), 7), num(g.matmul_mflops)));
        }
    }
}

fn mem(cx: &mut Cx, m: &crate::bench::mem::MemResults) {
    cx.head("h_mem");
    let hdr = format!("  {} {} {}", pad_right(&cx.t("l_op"), 14), pad_left(&format!("1 {}", cx.t("u_thread")), 14), pad_left(&format!("{} {}", m.threads, cx.t("u_threads")), 14));
    let h = cx.p(ansi::BOLD, &hdr);
    cx.line(&h);
    for (k, a, b) in [("l_read", m.single.read, m.multi.read), ("l_write", m.single.write, m.multi.write), ("l_copy", m.single.copy, m.multi.copy), ("l_triad", m.single.triad, m.multi.triad)] {
        cx.line(&format!("  {} {} {}  GB/s", pad_right(&cx.t(k), 14), pad_left(&num(a), 14), pad_left(&num(b), 14)));
    }
    cx.note(&cx.tf("msg_array", &[m.array_mb.to_string()]));
    cx.sub(&cx.t("l_latency"));
    let max = m.latency.iter().map(|p| p.ns).fold(0.0, f64::max).max(1e-9);
    let min = m.latency.iter().map(|p| p.ns).fold(f64::MAX, f64::min).max(1e-9);
    for p in &m.latency {
        let frac = if max > min { ((p.ns / min).ln() / (max / min).ln()).max(0.02) } else { 0.5 };
        let b = bar(cx.color, ansi::YELLOW, frac, 36, true);
        cx.line(&format!("  {} {} {} ns", pad_left(&crate::bench::mem::bs_human(p.bytes), 8), b, pad_left(&num(p.ns), 7)));
    }
    cx.note(&cx.tf("msg_lat_summary", &[num(m.l1_ns), num(m.dram_ns)]));
}

fn sweep_chart(cx: &mut Cx, d: &DiskResults, qd: u32) {
    let pts: Vec<_> = d.sweep.iter().filter(|p| p.qd == qd).collect();
    if pts.is_empty() {
        return;
    }
    let max = pts.iter().flat_map(|p| [p.read_mbs.unwrap_or(0.0), p.write_mbs.unwrap_or(0.0)]).fold(0.0, f64::max).max(1e-9);
    let eff = pts.iter().map(|p| p.qd_eff).min().unwrap_or(qd);
    let title = if eff != qd { format!("{} QD{}  ({} QD{})", cx.t("l_sweep"), qd, cx.t("msg_qd_capped"), eff) } else { format!("{} QD{}", cx.t("l_sweep"), qd) };
    cx.sub(&title);
    let leg = format!("  {}  {}   {}  {}   (MB/s)", cx.p(ansi::RED, "██"), cx.t("l_write"), cx.p(ansi::GREEN, "██"), cx.t("l_read"));
    cx.line(&leg);
    for p in pts {
        let (w, r) = (p.write_mbs.unwrap_or(0.0), p.read_mbs.unwrap_or(0.0));
        let wb = bar(cx.color, ansi::RED, w / max, 20, true);
        let rb = bar(cx.color, ansi::GREEN, r / max, 20, true);
        cx.line(&format!("  {} {} {}  {} {}", pad_left(&bs_label(p.bs), 6), wb, pad_left(&opt(p.write_mbs), 6), rb, pad_left(&opt(p.read_mbs), 6)));
    }
}

fn disk(cx: &mut Cx, d: &DiskResults) {
    cx.head("h_disk");
    if let Some(e) = &d.error {
        let m = cx.p(ansi::RED, &format!("  {}: {}", cx.t("msg_disk_error"), e));
        cx.line(&m);
        return;
    }
    cx.kv("l_io_mode", &if d.direct { cx.t("msg_direct") } else { cx.t("msg_buffered") });
    cx.kv("l_testfile", &format!("{} MB   {}: {} B", d.file_mb, cx.t("l_block"), d.logical_block));
    let mut qds: Vec<u32> = d.sweep.iter().map(|p| p.qd).collect();
    qds.dedup();
    for qd in qds {
        sweep_chart(cx, d, qd);
    }
    cx.sub(&cx.t("l_scenarios"));
    let hdr = format!("  {} {} {} {}", pad_right(&cx.t("l_scenario"), 28), pad_left("MB/s", 9), pad_left("IOPS", 10), pad_left("p50 / p99 / p99.9 µs", 24));
    let h = cx.p(ansi::BOLD, &hdr);
    cx.line(&h);
    for s in &d.scenarios {
        let lat = match (s.p50_us, s.p99_us, s.p999_us) {
            (Some(a), Some(b), Some(c)) => format!("{} / {} / {}", num(a), num(b), num(c)),
            _ => String::new(),
        };
        cx.line(&format!("  {} {} {} {}", pad_right(&cx.t(&format!("s_{}", s.key)), 28), pad_left(&num(s.mbs), 9), pad_left(&num(s.iops), 10), pad_left(&lat, 24)));
    }
    if let Some(s) = &d.streams {
        cx.kv("l_streams", &cx.tf("msg_streams", &[s.streams.to_string(), num(s.total_mbs), num(s.min_stream_mbs), s.uhd_capacity.to_string()]));
    }
    if let Some(s) = &d.small_files {
        cx.kv("l_smallfiles", &cx.tf("msg_smallfiles", &[s.files.to_string(), num(s.create_per_s), num(s.read_per_s), num(s.delete_per_s)]));
    }
    if let Some(s) = &d.sustained {
        cx.sub(&cx.t("l_sustained"));
        let sp = spark(&s.samples_mbs);
        cx.line(&format!("  {}", sp));
        cx.note(&cx.tf("msg_sustained", &[num(s.start_mbs), num(s.end_mbs), num(s.min_mbs), format!("{:.0}", s.drop_pct)]));
    }
}

fn net(cx: &mut Cx, n: &crate::bench::net::NetResults) {
    cx.head("h_net");
    if let Some(e) = &n.error {
        let m = cx.p(ansi::RED, &format!("  {}: {}", cx.t("msg_net_error"), e));
        cx.line(&m);
    }
    let mode = if n.mode == "remote" { cx.t("msg_net_remote") } else { cx.t("msg_net_loopback") };
    cx.kv("l_net_mode", &mode);
    let mb = |x: Option<f64>| x.map(|v| format!("{} Mbit/s", num(v))).unwrap_or_else(|| "-".into());
    for (k, v) in [("l_up1", n.up1_mbps), ("l_up4", n.up4_mbps), ("l_down1", n.down1_mbps), ("l_down4", n.down4_mbps), ("l_bidir", n.bidir_mbps)] {
        cx.kv(k, &mb(v));
    }
    if let (Some(l), Some(c), Some(e)) = (n.link_mbps, n.ceiling_mbps, n.efficiency_pct) {
        cx.kv("l_link", &format!("{l} Mbit/s  →  {} {:.0} Mbit/s  ({:.0} %)", cx.t("l_ceiling"), c, e));
    }
    if let Some(r) = &n.rtt {
        cx.kv("l_rtt", &format!("p50 {} µs   p99 {} µs   {} {} µs", num(r.p50_us), num(r.p99_us), cx.t("l_jitter"), num(r.jitter_us)));
    }
    if let Some(r) = &n.gateway_rtt {
        cx.kv("l_gateway", &format!("p50 {} µs   p99 {} µs   ({} {})", num(r.p50_us), num(r.p99_us), r.samples, cx.t("u_samples")));
    }
    if !n.sweep.is_empty() {
        cx.sub(&cx.t("l_msgsweep"));
        let max = n.sweep.iter().map(|p| p.mbs).fold(0.0, f64::max).max(1e-9);
        for p in &n.sweep {
            let b = bar(cx.color, ansi::BLUE, p.mbs / max, 28, true);
            cx.line(&format!("  {} {} {} MB/s  {} msg/s", pad_left(&bs_label(p.size), 6), b, pad_left(&num(p.mbs), 8), num(p.msgs_per_s)));
        }
    }
    if n.mode == "loopback" {
        cx.note(&cx.t("msg_loopback_note"));
    }
}

fn video(cx: &mut Cx, v: &crate::bench::video::VideoResults) {
    cx.head("h_video");
    let Some(ver) = &v.ffmpeg_version else {
        let t = cx.t("msg_ffmpeg_missing");
        cx.note(&t);
        return;
    };
    cx.kv("l_ffmpeg", ver);
    cx.kv("l_source_fps", &format!("{} fps", opt(v.source_fps)));
    let hdr = format!("  {} {} {} {} {}", pad_right(&cx.t("l_encoder"), 22), pad_right(&cx.t("l_codec"), 6), pad_left("fps", 9), pad_left(&format!("4× fps"), 9), pad_left(&cx.t("l_rt_streams"), 14));
    let h = cx.p(ansi::BOLD, &hdr);
    cx.line(&h);
    for e in &v.encoders {
        let kind = if e.kind == "hw" { "HW" } else { "CPU" };
        let name = format!("{} [{}]", e.name, kind);
        if e.ok {
            let star = if e.source_limited { " *" } else { "" };
            cx.line(&format!("  {} {} {} {} {}{}", pad_right(&name, 22), pad_right(&e.codec, 6), pad_left(&opt(e.fps), 9), pad_left(&opt(e.parallel_fps), 9), pad_left(&e.realtime_streams.map(|x| x.to_string()).unwrap_or_else(|| "-".into()), 14), star));
        } else {
            let g = cx.p(ansi::GRAY, &format!("  {} {} {}", pad_right(&name, 22), pad_right(&e.codec, 6), cx.t("msg_enc_unavailable")));
            cx.line(&g);
        }
    }
    let n = cx.t("msg_video_note");
    cx.note(&n);
}

fn rt(cx: &mut Cx, rec: &Record) {
    cx.head("h_rt");
    let cpu = rec.results.rt.as_ref();
    if let Some(c) = cpu {
        cx.kv("l_pt_scene", &format!("{}×{}, {} {}, {} {}", c.width, c.height, c.primitives, cx.t("u_prims"), c.max_depth, cx.t("u_bounces")));
        cx.kv("l_cpu_pt1", &format!("{} Mrays/s", num(c.single_mrays)));
        cx.kv("l_cpu_ptall", &format!("{} Mrays/s  ({} {})", num(c.multi_mrays), c.threads, cx.t("u_threads")));
        let st = if c.checksum_ok { cx.tf("msg_checksum_ok", &[c.checksum.clone()]) } else { cx.tf("msg_checksum_bad", &[c.checksum.clone()]) };
        cx.kv("l_checksum", &st);
    }
    let gpus: Vec<_> = rec.results.gpu.iter().flat_map(|g| g.adapters.iter()).filter(|a| a.rt.is_some()).collect();
    if !gpus.is_empty() {
        cx.sub(&cx.t("l_gpu_pt"));
        let max = gpus.iter().filter_map(|a| a.rt.as_ref().map(|r| r.mrays)).chain(cpu.map(|c| c.multi_mrays)).fold(0.0, f64::max).max(1e-9);
        for a in gpus {
            let r = a.rt.as_ref().unwrap();
            let b = bar(cx.color, ansi::MAGENTA, r.mrays / max, 24, true);
            let sw = if a.is_software() { format!(" ({})", cx.t("msg_software_short")) } else { String::new() };
            let vs = cpu.filter(|c| c.multi_mrays > 0.0).map(|c| format!("  {:.1}× {}", r.mrays / c.multi_mrays, cx.t("msg_vs_cpu"))).unwrap_or_default();
            cx.line(&format!("  {} {} {} Mrays/s{}", pad_right(&format!("{}{}", a.info.name.chars().take(34 - sw.chars().count()).collect::<String>(), sw), 34), b, pad_left(&num(r.mrays), 8), vs));
        }
    }
    let n = cx.t("msg_rt_note");
    cx.note(&n);
}

fn gpu(cx: &mut Cx, g: &crate::bench::gpu::GpuResults) {
    cx.head("h_gpu");
    if !g.supported {
        let t = cx.t("msg_gpu_not_built");
        cx.note(&t);
        return;
    }
    if g.adapters.is_empty() {
        let t = cx.t("msg_gpu_none");
        cx.note(&t);
        return;
    }
    for a in &g.adapters {
        cx.sub(&format!("{}  ({}, {})", a.info.name, a.info.backend, a.info.device_type));
        if let Some(e) = &a.error {
            let m = cx.p(ansi::RED, &format!("  {}: {}", cx.t("msg_gpu_failed"), e));
            cx.line(&m);
            continue;
        }
        if a.is_software() {
            let t = cx.t("msg_gpu_software");
            cx.note(&t);
        }
        let g1 = |x: Option<f64>, unit: &str| x.map(|v| format!("{} {}", num(v), unit)).unwrap_or_else(|| "-".into());
        if a.fp32_gflops.is_some() || a.mem_gbs.is_some() || a.matmul_gflops.is_some() {
            cx.kv("l_fp32", &g1(a.fp32_gflops, "GFLOPS"));
            cx.kv("l_int", &g1(a.int_gops, "Gops/s"));
            cx.kv("l_membw", &match (a.mem_gbs, a.mem_buffer_mb) {
                (Some(v), Some(mb)) => format!("{} GB/s  ({} MB)", num(v), mb),
                _ => "-".into(),
            });
            let ok = match a.matmul_ok {
                Some(true) => " ✔",
                Some(false) => " ✘",
                None => "",
            };
            let n = a.matmul_n.map(|n| n.to_string()).unwrap_or_default();
            cx.kv("l_matmul", &format!("{}{}  ({n}×{n})", g1(a.matmul_gflops, "GFLOPS"), ok));
            cx.kv("l_transfer", &format!("↑ {} GB/s   ↓ {} GB/s", opt(a.h2d_gbs), opt(a.d2h_gbs)));
            cx.kv("l_dispatch", &format!("p50 {} µs   p99 {} µs", opt(a.latency_p50_us), opt(a.latency_p99_us)));
        }
        if let Some(r) = &a.rt {
            if r.hw_ray_query {
                let t = cx.t("msg_hw_rt_detected");
                cx.note(&t);
            }
        }
    }
}

fn scores(cx: &mut Cx, rec: &Record) {
    cx.head("h_scores");
    for key in ["virt", "nas", "video", "game", "ai", "gfx", "rt"] {
        let name = pad_right(&cx.t(&format!("cat_{key}")), 30);
        match rec.scores.categories.get(key) {
            Some(&s) => {
                let col = level_color(s);
                let b = bar(cx.color, col, s / 100.0, 24, true);
                let v = cx.p(col, &cx.t(verdict_key(s)));
                cx.line(&format!("  {} {} {} {}", name, b, pad_left(&format!("{s:.0}"), 3), v));
            }
            None => {
                let m = cx.p(ansi::GRAY, &format!("  {} {}", name, cx.t("msg_not_measured")));
                cx.line(&m);
            }
        }
    }
    if let Some(g) = rec.scores.general {
        cx.line("");
        let b = bar(cx.color, level_color(g), g / 100.0, 24, true);
        cx.line(&format!("  {} {} {}", pad_right(&cx.t("cat_general"), 30), b, pad_left(&format!("{g:.0}"), 3)));
    }
    if !rec.scores.components.is_empty() {
        cx.sub(&cx.t("l_components"));
        for (k, v) in &rec.scores.components {
            let b = bar(cx.color, ansi::GRAY, v / 100.0, 20, true);
            cx.line(&format!("  {} {} {}", pad_right(&cx.t(&format!("cmp_{k}")), 30), b, pad_left(&format!("{v:.0}"), 3)));
        }
    }
}

fn findings(cx: &mut Cx, rec: &Record) {
    cx.head("h_findings");
    for (lvl, icon, col) in [("warn", "⚠", ansi::YELLOW), ("ok", "✔", ansi::GREEN), ("info", "ℹ", ansi::BLUE)] {
        for f in rec.findings.iter().filter(|f| f.level == lvl) {
            let text = format!("[{}] {}", cx.t(&format!("fc_{}", f.cat)), finding_text(cx.lang, f));
            for (i, part) in wrap(&text, W - 6).iter().enumerate() {
                if i == 0 {
                    let tag = cx.p(col, icon);
                    cx.line(&format!("  {tag} {part}"));
                } else {
                    cx.line(&format!("    {part}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_has_no_escape_codes() {
        let rec = crate::record::tests_support::sample_full();
        let s = text(&rec, Lang::En, false);
        assert!(!s.contains('\x1b'));
        assert!(s.contains("SysCheck"));
    }

    #[test]
    fn both_languages_render_without_leaking_keys() {
        let rec = crate::record::tests_support::sample_full();
        let en = text(&rec, Lang::En, false);
        let tr = text(&rec, Lang::Tr, false);
        assert_ne!(en, tr);
        for out in [&en, &tr] {
            for tok in out.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                let looks_like_key = ["h_", "l_", "k_", "s_", "cat_", "cmp_", "fc_", "fnd_", "ver_", "msg_", "u_"].iter().any(|p| tok.starts_with(p)) && tok.contains('_');
                assert!(!looks_like_key, "untranslated key in output: {tok}");
            }
        }
    }

    #[test]
    fn lines_fit_in_terminal_width() {
        let rec = crate::record::tests_support::sample_full();
        let s = text(&rec, Lang::En, false);
        for l in s.lines() {
            assert!(l.chars().count() <= 100, "too wide ({}): {l}", l.chars().count());
        }
    }
}
