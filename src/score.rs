//! Scores and findings.
//!
//! Every score is derived from raw measurements with a log-scale mapping ("a 2x faster machine
//! is worth the same number of points everywhere"). The anchors below are PROVISIONAL: they are
//! meant to be recalibrated once results from many devices have been collected. Raw values are
//! always stored in the record, so scores can be recomputed later.

use crate::hw::SystemInfo;
use crate::record::{Finding, Results, Scores};
use crate::util::*;

fn fnd(cat: &str, level: &str, key: &str, args: &[String]) -> Finding {
    Finding { cat: cat.into(), level: level.into(), key: key.into(), args: args.to_vec() }
}

fn s1(x: f64) -> String {
    format!("{x:.1}")
}

pub fn verdict_key(score: f64) -> &'static str {
    if score >= 75.0 {
        "ver_excellent"
    } else if score >= 55.0 {
        "ver_good"
    } else if score >= 35.0 {
        "ver_limited"
    } else {
        "ver_poor"
    }
}

pub fn compute(sys: &SystemInfo, r: &Results) -> (Scores, Vec<Finding>) {
    let mut f: Vec<Finding> = Vec::new();
    let mut comp: Vec<(&str, Option<f64>)> = Vec::new();

    // ── components ─────────────────────────────────────────────────────────────────────────
    let cpu = r.cpu.as_ref();
    let cpu_single = cpu.map(|c| lscore(c.single_geo, 100.0, 2000.0));
    let cpu_multi = cpu.map(|c| lscore(c.multi_geo, 200.0, 40000.0));
    let cpu_fp = cpu.and_then(|c| c.kernel("matmul_f32")).map(|k| lscore(k.multi, 500.0, 400_000.0));
    let crypto = cpu.map(|c| lscore(c.sha.hw_mbs.unwrap_or(c.sha.soft_mbs), 50.0, 4000.0));
    let ram = Some(lscore(sys.mem.total_mb as f64 / 1024.0, 1.0, 128.0)).filter(|_| sys.mem.total_mb > 0);
    let mem = r.mem.as_ref();
    let mem_bw = mem.map(|m| lscore(m.multi.triad.max(m.single.triad), 3.0, 150.0));
    let mem_lat = mem.filter(|m| m.dram_ns > 0.0).map(|m| lscore(1000.0 / m.dram_ns, 1000.0 / 300.0, 1000.0 / 60.0));
    let disk = r.disk.as_ref().filter(|d| d.error.is_none() && !d.scenarios.is_empty());
    let disk_seq = disk.and_then(|d| match (d.seq_read_mbs(), d.seq_write_mbs()) {
        (Some(a), Some(b)) => Some(lscore(geomean(&[a, b]), 50.0, 7000.0)),
        _ => None,
    });
    let disk_iops = disk.and_then(|d| match (d.rand_read_qd32_iops(), d.rand_write_qd32_iops()) {
        (Some(a), Some(b)) => Some((lscore(a, 1000.0, 800_000.0) + lscore(b, 500.0, 500_000.0)) / 2.0),
        _ => None,
    });
    let disk_qd1 = disk.and_then(|d| d.rand_read_qd1_iops()).map(|x| lscore(x, 100.0, 20_000.0));
    let streams = disk.and_then(|d| d.streams.as_ref()).map(|s| lscore(s.uhd_capacity as f64, 1.0, 200.0));
    let net = r.net.as_ref().filter(|n| n.mode == "remote" && n.error.is_none()).and_then(|n| n.best_mbps()).map(|x| lscore(x, 50.0, 25_000.0));

    // video encoding (ffmpeg), CPU path tracer, GPU
    let video_r = r.video.as_ref();
    let cpu_encode = video_r.and_then(|v| v.cpu_h264()).and_then(|e| e.fps).map(|x| lscore(x, 5.0, 300.0));
    let hw_encode = video_r.and_then(|v| v.best_hw()).and_then(|e| e.realtime_streams).map(|x| lscore(x as f64, 1.0, 40.0));
    let rt_cpu = r.rt.as_ref().map(|x| lscore(x.multi_mrays, 2.0, 1500.0));
    let gpu_best = r.gpu.as_ref().and_then(|g| g.best());
    let gpu_fp32 = gpu_best.and_then(|g| g.fp32_gflops).map(|x| lscore(x, 200.0, 80_000.0));
    let gpu_mem = gpu_best.and_then(|g| g.mem_gbs).map(|x| lscore(x, 10.0, 1500.0));
    let gpu_matmul = gpu_best.and_then(|g| g.matmul_gflops).filter(|_| gpu_best.and_then(|g| g.matmul_ok) != Some(false)).map(|x| lscore(x, 100.0, 40_000.0));
    let gpu_rt = gpu_best.and_then(|g| g.rt.as_ref()).filter(|x| x.luma_matches_cpu).map(|x| lscore(x.mrays, 20.0, 20_000.0));
    let vram_mb = sys.gpus.iter().filter_map(|g| g.vram_mb).max();
    let gpu_vram = vram_mb.map(|m| lscore(m as f64 / 1024.0, 2.0, 48.0));

    comp.extend([
        ("cpu_encode", cpu_encode),
        ("hw_encode", hw_encode),
        ("rt_cpu", rt_cpu),
        ("gpu_fp32", gpu_fp32),
        ("gpu_mem", gpu_mem),
        ("gpu_matmul", gpu_matmul),
        ("gpu_rt", gpu_rt),
        ("gpu_vram", gpu_vram),
        ("cpu_single", cpu_single),
        ("cpu_multi", cpu_multi),
        ("cpu_fp", cpu_fp),
        ("crypto", crypto),
        ("ram", ram),
        ("mem_bw", mem_bw),
        ("mem_lat", mem_lat),
        ("disk_seq", disk_seq),
        ("disk_iops", disk_iops),
        ("disk_qd1", disk_qd1),
        ("streams", streams),
        ("net", net),
    ]);

    // ── categories ─────────────────────────────────────────────────────────────────────────
    let c = &sys.cpu;
    let virt_ok = c.virt_ext == Some(true) || c.kvm;
    let virt_blocked = c.virt_ext == Some(false) && !c.kvm;
    let mut virt_score = wavg(&[(cpu_multi, 0.25), (ram, 0.25), (disk_iops, 0.20), (cpu_single, 0.10), (mem_bw, 0.10), (disk_qd1, 0.10)]);
    if let Some(p) = virt_score.as_mut() {
        if sys.mem.ecc == Some(true) {
            *p = (*p + 3.0).min(100.0);
        }
        if virt_blocked {
            *p = p.min(20.0);
        }
    }
    let nas = wavg(&[(disk_seq, 0.25), (net, 0.25), (ram, 0.15), (crypto, 0.15), (cpu_multi, 0.10), (cpu_single, 0.10)]);
    let video = wavg(&[(cpu_multi, 0.15), (cpu_encode, 0.25), (hw_encode, 0.25), (streams, 0.10), (cpu_single, 0.10), (mem_bw, 0.05), (ram, 0.05), (disk_seq, 0.05)]);
    let game = wavg(&[(cpu_single, 0.40), (mem_lat, 0.15), (ram, 0.15), (disk_qd1, 0.15), (cpu_multi, 0.15)]);
    let ai = if gpu_matmul.is_some() {
        wavg(&[(gpu_matmul, 0.35), (gpu_mem, 0.20), (gpu_vram, 0.15), (ram, 0.10), (cpu_fp, 0.10), (mem_bw, 0.10)])
    } else {
        wavg(&[(mem_bw, 0.40), (cpu_fp, 0.30), (ram, 0.30)])
    };
    let gfx = wavg(&[(gpu_fp32, 0.40), (gpu_mem, 0.30), (gpu_matmul, 0.30)]);
    let rt_score = gpu_rt;
    let general = wavg(&[(cpu_single, 0.30), (cpu_multi, 0.30), (ram, 0.15), (mem_bw, 0.10), (disk_seq, 0.075), (disk_qd1, 0.075)]);

    let mut scores = Scores { general, ..Default::default() };
    for (k, v) in comp {
        if let Some(v) = v {
            scores.components.insert(k.into(), v);
        }
    }
    for (k, v) in [("virt", virt_score), ("nas", nas), ("video", video), ("game", game), ("ai", ai), ("gfx", gfx), ("rt", rt_score)] {
        if let Some(v) = v {
            scores.categories.insert(k.into(), v);
        }
    }

    // ── findings ───────────────────────────────────────────────────────────────────────────
    // virtualization
    if virt_ok {
        f.push(fnd("virt", "ok", "fnd_virt_ok", &[]));
    } else if c.virt_ext == Some(false) && c.hypervisor {
        f.push(fnd("virt", "warn", "fnd_virt_guest_nested", &[]));
    } else if virt_blocked {
        f.push(fnd("virt", "warn", "fnd_virt_none", &[]));
    } else {
        f.push(fnd("virt", "info", "fnd_virt_unknown", &[]));
    }
    match c.iommu_groups {
        Some(n) if n > 0 => f.push(fnd("virt", "ok", "fnd_iommu_yes", &[n.to_string()])),
        _ => f.push(fnd("virt", "info", "fnd_iommu_no", &[])),
    }
    // memory
    let gb = sys.mem.total_mb as f64 / 1024.0;
    if sys.mem.total_mb > 0 && gb < 8.0 {
        f.push(fnd("virt", "warn", "fnd_ram_low_vm", &[s1(gb)]));
    }
    match sys.mem.ecc {
        Some(true) => f.push(fnd("nas", "ok", "fnd_ecc_yes", &[])),
        Some(false) => f.push(fnd("nas", "info", "fnd_no_ecc", &[])),
        None => {}
    }
    // cpu
    if let Some(cr) = cpu {
        if cr.max_noise_pct > 5.0 {
            f.push(fnd("general", "warn", "fnd_noise_high", &[s1(cr.max_noise_pct)]));
        }
        if !cr.pinned {
            f.push(fnd("general", "info", "fnd_not_pinned", &[]));
        }
        if let Some(hw) = &cr.sha.hw_feature {
            f.push(fnd("nas", "ok", "fnd_sha_hw", &[hw.clone(), s1(cr.sha.speedup.unwrap_or(0.0))]));
        }
        if let Some(s) = &cr.simd {
            f.push(fnd("ai", "ok", "fnd_simd", &[s.feature.clone(), s1(s.speedup)]));
        }
    }
    if c.groups.len() > 1 {
        f.push(fnd("general", "info", "fnd_hybrid_cpu", &[c.groups.len().to_string()]));
    }
    // disk
    if sys.disk.ram_backed {
        f.push(fnd("disk", "warn", "fnd_disk_ram", &[]));
    }
    if sys.disk.kind == "hdd" {
        f.push(fnd("virt", "warn", "fnd_disk_hdd", &[]));
    }
    if let Some(d) = r.disk.as_ref() {
        if let Some(e) = &d.error {
            f.push(fnd("disk", "warn", "fnd_disk_error", &[e.clone()]));
        } else {
            if !d.direct {
                f.push(fnd("disk", "warn", "fnd_disk_buffered", &[]));
            }
            if let Some(s) = &d.streams {
                f.push(fnd("video", if s.uhd_capacity >= 4 { "ok" } else { "warn" }, "fnd_disk_streams", &[s.uhd_capacity.to_string()]));
            }
            if let Some(s) = &d.sustained {
                if s.drop_pct > 40.0 {
                    f.push(fnd("disk", "warn", "fnd_sustained_drop", &[format!("{:.0}", s.drop_pct)]));
                } else {
                    f.push(fnd("disk", "ok", "fnd_sustained_ok", &[format!("{:.0}", s.drop_pct)]));
                }
            }
        }
    }
    // network
    match r.net.as_ref() {
        Some(n) if n.error.is_some() => f.push(fnd("net", "warn", "fnd_net_error", &[n.error.clone().unwrap_or_default()])),
        Some(n) if n.mode == "loopback" => f.push(fnd("net", "info", "fnd_net_loopback", &[])),
        Some(n) => {
            if let Some(l) = n.link_mbps {
                if l < 1000 {
                    f.push(fnd("nas", "warn", "fnd_net_link_slow", &[l.to_string()]));
                }
            }
            if let Some(rt) = &n.rtt {
                if rt.p99_us > 5000.0 {
                    f.push(fnd("net", "warn", "fnd_net_rtt_high", &[s1(rt.p99_us / 1000.0)]));
                }
            }
            match (n.efficiency_pct, n.best_mbps()) {
                (Some(e), _) if e < 70.0 => f.push(fnd("net", "warn", "fnd_net_eff_low", &[format!("{e:.0}")])),
                (Some(e), Some(b)) => f.push(fnd("net", "ok", "fnd_net_ok", &[format!("{b:.0}"), format!("{e:.0}")])),
                _ => {}
            }
        }
        None if r.cpu.is_some() && r.mem.is_some() && r.disk.is_some() => f.push(fnd("net", "info", "fnd_net_not_tested", &[])),
        None => {}
    }
    // video encoders
    match r.video.as_ref() {
        Some(v) if v.ffmpeg_version.is_none() => f.push(fnd("video", "info", "fnd_ffmpeg_missing", &[])),
        Some(v) => {
            if let Some(hw) = v.best_hw() {
                f.push(fnd("video", "ok", "fnd_video_hw_ok", &[hw.name.clone(), format!("{:.0}", hw.fps.unwrap_or(0.0)), hw.realtime_streams.unwrap_or(0).to_string()]));
                if hw.source_limited {
                    f.push(fnd("video", "info", "fnd_video_source_limited", &[]));
                }
            } else {
                f.push(fnd("video", "info", "fnd_video_hw_none_working", &[]));
            }
            if let Some(x) = v.cpu_h264() {
                f.push(fnd("video", "info", "fnd_video_cpu", &[format!("{:.0}", x.fps.unwrap_or(0.0)), x.realtime_streams.unwrap_or(0).to_string()]));
            }
        }
        None if sys.hw_video.is_empty() => f.push(fnd("video", "info", "fnd_hw_video_none", &[])),
        None => f.push(fnd("video", "info", "fnd_hw_video", &[sys.hw_video.join(", ")])),
    }
    // path tracing reference
    if let Some(x) = r.rt.as_ref() {
        if !x.checksum_ok {
            f.push(fnd("gpu", "warn", "fnd_rt_checksum", &[]));
        }
    }
    // gpu
    match r.gpu.as_ref() {
        Some(g) if !g.supported => f.push(fnd("gpu", "info", "fnd_gpu_not_built", &[])),
        Some(g) => {
            if g.adapters.is_empty() {
                f.push(fnd("gpu", "info", "fnd_gpu_none", &[]));
            }
            for a in &g.adapters {
                if let Some(e) = &a.error {
                    f.push(fnd("gpu", "warn", "fnd_gpu_error", &[a.info.name.clone(), e.clone()]));
                    continue;
                }
                if a.is_software() {
                    f.push(fnd("gpu", "warn", "fnd_gpu_software", &[a.info.name.clone()]));
                    continue;
                }
                f.push(fnd("gpu", "ok", "fnd_gpu_tested", &[a.info.name.clone(), a.info.backend.clone(), a.fp32_gflops.map(|x| format!("{x:.0}")).unwrap_or_else(|| "-".into())]));
                if a.matmul_ok == Some(false) {
                    f.push(fnd("gpu", "warn", "fnd_gpu_matmul_bad", &[a.info.name.clone()]));
                }
                if let Some(x) = &a.rt {
                    if !x.luma_matches_cpu {
                        f.push(fnd("gpu", "warn", "fnd_rt_gpu_mismatch", &[a.info.name.clone()]));
                    }
                    if x.hw_ray_query {
                        f.push(fnd("gpu", "info", "fnd_hw_rt_detected", &[a.info.name.clone()]));
                    }
                }
            }
            if vram_mb.is_none() && g.best().is_some() {
                f.push(fnd("ai", "info", "fnd_vram_unknown", &[]));
            }
        }
        None if sys.gpus.is_empty() => f.push(fnd("gpu", "info", "fnd_gpu_none", &[])),
        None => {
            let names = sys.gpus.iter().map(|g| g.name.clone()).collect::<Vec<_>>().join("; ");
            f.push(fnd("gpu", "info", "fnd_gpu_found", &[names]));
        }
    }
    (scores, f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::cpu::CpuResults;

    fn base() -> (SystemInfo, Results) {
        let mut s = SystemInfo::default();
        s.mem.total_mb = 32768;
        s.cpu.virt_ext = Some(true);
        let mut r = Results::default();
        r.cpu = Some(CpuResults { single_geo: 700.0, multi_geo: 5000.0, pinned: true, ..Default::default() });
        (s, r)
    }

    #[test]
    fn verdict_thresholds() {
        assert_eq!(verdict_key(80.0), "ver_excellent");
        assert_eq!(verdict_key(60.0), "ver_good");
        assert_eq!(verdict_key(40.0), "ver_limited");
        assert_eq!(verdict_key(10.0), "ver_poor");
    }

    #[test]
    fn missing_measurements_are_not_scored() {
        let (s, r) = base();
        let (sc, _) = compute(&s, &r);
        assert!(sc.categories.contains_key("virt"));
        assert!(!sc.components.contains_key("disk_seq") && !sc.components.contains_key("net"));
        assert!(!sc.categories.contains_key("gfx") && !sc.categories.contains_key("rt"));
    }

    #[test]
    fn no_virtualization_caps_virt() {
        let (mut s, r) = base();
        let (ok, _) = compute(&s, &r);
        s.cpu.virt_ext = Some(false);
        let (bad, f) = compute(&s, &r);
        assert!(bad.categories["virt"] <= 20.0);
        assert!(ok.categories["virt"] > bad.categories["virt"]);
        assert!(f.iter().any(|x| x.key == "fnd_virt_none" && x.level == "warn"));
        s.cpu.hypervisor = true;
        let (_, f) = compute(&s, &r);
        assert!(f.iter().any(|x| x.key == "fnd_virt_guest_nested"));
    }

    #[test]
    fn ecc_adds_points_and_scores_stay_in_range() {
        let (mut s, r) = base();
        let (a, _) = compute(&s, &r);
        s.mem.ecc = Some(true);
        let (b, _) = compute(&s, &r);
        assert!(b.categories["virt"] > a.categories["virt"]);
        for v in b.categories.values().chain(b.components.values()) {
            assert!((0.0..=100.0).contains(v));
        }
    }

    #[test]
    fn faster_cpu_never_scores_lower() {
        let (s, mut r) = base();
        let (a, _) = compute(&s, &r);
        r.cpu.as_mut().unwrap().single_geo = 1400.0;
        r.cpu.as_mut().unwrap().multi_geo = 10000.0;
        let (b, _) = compute(&s, &r);
        for k in ["virt", "game", "video"] {
            assert!(b.categories[k] >= a.categories[k], "{k}");
        }
    }

    #[test]
    fn loopback_network_is_not_scored() {
        let (s, mut r) = base();
        r.net = Some(crate::bench::net::NetResults { mode: "loopback".into(), up4_mbps: Some(40000.0), down4_mbps: Some(40000.0), ..Default::default() });
        let (sc, f) = compute(&s, &r);
        assert!(!sc.components.contains_key("net"));
        assert!(f.iter().any(|x| x.key == "fnd_net_loopback"));
    }
}
