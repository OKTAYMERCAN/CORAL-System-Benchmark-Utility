//! Self-contained HTML report with SVG charts (block-size sweep bars, latency curve, score bars).
//! No scripts and no external resources: the file can be opened or attached anywhere.

use crate::bench::disk::DiskResults;
use crate::i18n::{self, Lang};
use crate::record::Record;
use crate::render::finding_text;
use crate::score::verdict_key;
use crate::util::*;
use std::fmt::Write;

const CSS: &str = r#"
:root{--bg:#f6f7f9;--card:#fff;--fg:#1c2330;--mute:#667085;--line:#e3e6ec;--w:#d6455d;--r:#2f9e63;--a:#3b6fd4;--y:#d89614;--ok:#2f9e63;--warn:#d89614;--info:#3b6fd4;--bad:#d6455d}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--bg:#10141b;--card:#181e28;--fg:#e6e9ef;--mute:#98a2b3;--line:#2a3140;--w:#ef6b82;--r:#4cc58a;--a:#6d9bf0;--y:#f0b73c}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:920px;margin:0 auto;padding:24px 16px 48px}h1{font-size:24px;margin:0 0 4px}h2{font-size:17px;margin:0 0 12px}
.sub{color:var(--mute);font-size:13px}.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;margin:16px 0;overflow-x:auto}
table{border-collapse:collapse;width:100%}td,th{padding:4px 8px;text-align:left;border-bottom:1px solid var(--line);font-size:14px}th{color:var(--mute);font-weight:600}
td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}.k{color:var(--mute);width:36%}
.bar{height:10px;background:var(--line);border-radius:5px;overflow:hidden;min-width:120px}.bar i{display:block;height:100%}
.row{display:grid;grid-template-columns:minmax(120px,200px) 1fr 70px;gap:10px;align-items:center;margin:8px 0}.row b{text-align:right;font-variant-numeric:tabular-nums}
.tag{font-size:12px;color:var(--mute)}svg{width:100%;height:auto;display:block}svg text{fill:var(--mute);font-size:11px}
ul{list-style:none;margin:0;padding:0}li{padding:4px 0}.lw{color:var(--warn)}.lo{color:var(--ok)}.li{color:var(--info)}
.legend{display:flex;gap:16px;font-size:13px;color:var(--mute);margin-bottom:6px}.legend i{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:5px}
"#;

fn score_color(s: f64) -> &'static str {
    if s >= 75.0 {
        "var(--ok)"
    } else if s >= 55.0 {
        "var(--a)"
    } else if s >= 35.0 {
        "var(--y)"
    } else {
        "var(--bad)"
    }
}

fn e(s: &str) -> String {
    html_escape(s)
}

fn sweep_svg(d: &DiskResults, qd: u32) -> String {
    let pts: Vec<_> = d.sweep.iter().filter(|p| p.qd == qd).collect();
    if pts.is_empty() {
        return String::new();
    }
    let (w, h, l, b, t) = (860.0, 300.0, 52.0, 52.0, 10.0);
    let max = pts.iter().flat_map(|p| [p.read_mbs.unwrap_or(0.0), p.write_mbs.unwrap_or(0.0)]).fold(0.0, f64::max).max(1.0);
    // "nice" axis maximum
    let mag = 10f64.powf(max.log10().floor());
    let top = ([1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|v| *v >= max)).unwrap_or(max);
    let ph = h - b - t;
    let step = (w - l - 8.0) / pts.len() as f64;
    let mut s = format!("<svg viewBox=\"0 0 {w} {h}\" role=\"img\">");
    for i in 0..=4 {
        let v = top * i as f64 / 4.0;
        let y = h - b - ph * i as f64 / 4.0;
        let _ = write!(s, "<line x1=\"{l}\" x2=\"{}\" y1=\"{y:.1}\" y2=\"{y:.1}\" stroke=\"var(--line)\"/><text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>", w - 8.0, l - 6.0, y + 4.0, num(v));
    }
    for (i, p) in pts.iter().enumerate() {
        let x0 = l + step * i as f64 + step * 0.12;
        let bw = step * 0.38;
        for (j, (val, col)) in [(p.write_mbs, "var(--w)"), (p.read_mbs, "var(--r)")].into_iter().enumerate() {
            let v = val.unwrap_or(0.0);
            let bh = ph * (v / top);
            let _ = write!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"2\" fill=\"{col}\"><title>{} {}: {} MB/s</title></rect>", x0 + bw * j as f64, h - b - bh, bw - 1.0, bh, bs_label(p.bs), if j == 0 { "write" } else { "read" }, num(v));
        }
        let _ = write!(s, "<text x=\"{:.1}\" y=\"{}\" text-anchor=\"end\" transform=\"rotate(-40 {:.1} {})\">{}</text>", x0 + bw, h - b + 14.0, x0 + bw, h - b + 14.0, bs_label(p.bs));
    }
    s.push_str("</svg>");
    s
}

fn latency_svg(m: &crate::bench::mem::MemResults) -> String {
    if m.latency.len() < 2 {
        return String::new();
    }
    let (w, h, l, b, t) = (860.0, 260.0, 52.0, 40.0, 10.0);
    let lo = m.latency.iter().map(|p| p.ns).fold(f64::MAX, f64::min).max(0.1);
    let hi = m.latency.iter().map(|p| p.ns).fold(0.0, f64::max).max(lo * 1.5);
    let (llo, lhi) = (lo.log10().floor(), hi.log10().ceil());
    let ph = h - b - t;
    let n = m.latency.len();
    let xs = |i: usize| l + (w - l - 16.0) * i as f64 / (n - 1) as f64;
    let ys = |v: f64| t + ph * (1.0 - (v.log10() - llo) / (lhi - llo));
    let mut s = format!("<svg viewBox=\"0 0 {w} {h}\" role=\"img\">");
    let mut d = llo as i32;
    while d as f64 <= lhi {
        let v = 10f64.powi(d);
        let y = ys(v);
        let _ = write!(s, "<line x1=\"{l}\" x2=\"{}\" y1=\"{y:.1}\" y2=\"{y:.1}\" stroke=\"var(--line)\"/><text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\">{} ns</text>", w - 16.0, l - 6.0, y + 4.0, num(v));
        d += 1;
    }
    let pts: Vec<String> = m.latency.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", xs(i), ys(p.ns))).collect();
    let _ = write!(s, "<polyline points=\"{}\" fill=\"none\" stroke=\"var(--y)\" stroke-width=\"2.5\" stroke-linejoin=\"round\"/>", pts.join(" "));
    for (i, p) in m.latency.iter().enumerate() {
        let _ = write!(s, "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3.5\" fill=\"var(--y)\"><title>{}: {} ns</title></circle>", xs(i), ys(p.ns), crate::bench::mem::bs_human(p.bytes), num(p.ns));
        if i % 2 == 0 || n < 9 {
            let _ = write!(s, "<text x=\"{:.1}\" y=\"{}\" text-anchor=\"middle\">{}</text>", xs(i), h - b + 16.0, crate::bench::mem::bs_human(p.bytes));
        }
    }
    s.push_str("</svg>");
    s
}

pub fn html(rec: &Record, lang: Lang) -> String {
    let t = |k: &str| i18n::t(lang, k).to_string();
    let tf = |k: &str, a: &[String]| i18n::tf(lang, k, a);
    let mut o = String::new();
    let sys = &rec.system;
    let _ = write!(o, "<!doctype html><html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>SysCheck {}</title><style>{}</style></head><body><main>", if lang == Lang::Tr { "tr" } else { "en" }, e(&rec.run_id), CSS);
    let title = sys.label.clone().or(sys.model.clone()).unwrap_or_else(|| sys.cpu.model.clone());
    let _ = write!(o, "<h1>SysCheck — {}</h1><div class=\"sub\">{} · {} · {} · {}</div>", e(&title), e(&rec.created_utc), e(&rec.tool.spec), e(&rec.mode), e(&rec.run_id));

    // scores
    o.push_str("<div class=\"card\">");
    let _ = write!(o, "<h2>{}</h2>", e(&t("h_scores")));
    for key in ["virt", "nas", "video", "game", "ai", "gfx", "rt"] {
        let name = e(&t(&format!("cat_{key}")));
        match rec.scores.categories.get(key) {
            Some(&s) => {
                let _ = write!(o, "<div class=\"row\"><span>{name}</span><div class=\"bar\"><i style=\"width:{s:.0}%;background:{}\"></i></div><b>{s:.0}</b></div><div class=\"tag\" style=\"margin:-4px 0 4px 0\">{}</div>", score_color(s), e(&t(verdict_key(s))));
            }
            None => {
                let _ = write!(o, "<div class=\"row\"><span>{name}</span><span class=\"tag\">{}</span><b></b></div>", e(&t("msg_not_measured")));
            }
        }
    }
    o.push_str("</div>");

    // system
    o.push_str("<div class=\"card\">");
    let _ = write!(o, "<h2>{}</h2><table>", e(&t("h_system")));
    let mut row = |k: &str, v: String| {
        let _ = write!(o, "<tr><td class=\"k\">{}</td><td>{}</td></tr>", e(&t(k)), e(&v));
    };
    row("l_os", format!("{} ({} / {})", sys.os.name, sys.os.kernel, sys.os.arch));
    row("l_cpu", sys.cpu.model.clone());
    row("l_cores", format!("{} {} / {} {}", sys.cpu.usable, t("u_threads"), sys.cpu.physical, t("u_cores")));
    row("l_ram", format!("{:.1} GB", sys.mem.total_mb as f64 / 1024.0));
    row("l_disk", format!("{} {} {:.0} GB", sys.disk.kind, sys.disk.model.clone().unwrap_or_default(), sys.disk.total_gb));
    for g in &sys.gpus {
        row("l_gpu", g.name.clone());
    }
    o.push_str("</table></div>");

    // cpu
    if let Some(c) = &rec.results.cpu {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2><table><tr><th>{}</th><th class=\"n\">{}</th><th class=\"n\">{}</th><th></th></tr>", e(&t("h_cpu")), e(&t("l_kernel")), e(&t("l_single")), e(&t("l_multi")));
        for k in &c.kernels {
            let _ = write!(o, "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"tag\">{}</td></tr>", e(&t(&format!("k_{}", k.name))), num(k.single), num(k.multi), e(&k.unit));
        }
        let _ = write!(o, "<tr><td><b>{}</b></td><td class=\"n\"><b>{}</b></td><td class=\"n\"><b>{}</b></td><td></td></tr></table>", e(&t("l_geo")), num(c.single_geo), num(c.multi_geo));
        if let (Some(h), Some(f)) = (c.sha.hw_mbs, &c.sha.hw_feature) {
            let _ = write!(o, "<p class=\"tag\">SHA-256: {} → {} MB/s ({})</p>", num(c.sha.soft_mbs), num(h), e(f));
        }
        if let Some(s) = &c.simd {
            let _ = write!(o, "<p class=\"tag\">SIMD: {} → {} MFLOPS ({}, {:.1}×)</p>", num(s.base_mflops), num(s.accel_mflops), e(&s.feature), s.speedup);
        }
        o.push_str("</div>");
    }

    // memory
    if let Some(m) = &rec.results.mem {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2><table><tr><th></th><th class=\"n\">1 {}</th><th class=\"n\">{} {}</th></tr>", e(&t("h_mem")), e(&t("u_thread")), m.threads, e(&t("u_threads")));
        for (k, a, b) in [("l_read", m.single.read, m.multi.read), ("l_write", m.single.write, m.multi.write), ("l_copy", m.single.copy, m.multi.copy), ("l_triad", m.single.triad, m.multi.triad)] {
            let _ = write!(o, "<tr><td>{}</td><td class=\"n\">{} GB/s</td><td class=\"n\">{} GB/s</td></tr>", e(&t(k)), num(a), num(b));
        }
        let _ = write!(o, "</table><h2 style=\"margin-top:16px\">{}</h2>{}<p class=\"tag\">{}</p></div>", e(&t("l_latency")), latency_svg(m), e(&tf("msg_lat_summary", &[num(m.l1_ns), num(m.dram_ns)])));
    }

    // disk
    if let Some(d) = &rec.results.disk {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2>", e(&t("h_disk")));
        if let Some(er) = &d.error {
            let _ = write!(o, "<p class=\"lw\">{}: {}</p>", e(&t("msg_disk_error")), e(er));
        } else {
            let _ = write!(o, "<p class=\"tag\">{} · {} MB</p>", e(&if d.direct { t("msg_direct") } else { t("msg_buffered") }), d.file_mb);
            let mut qds: Vec<u32> = d.sweep.iter().map(|p| p.qd).collect();
            qds.dedup();
            for qd in qds {
                let _ = write!(o, "<h2 style=\"margin-top:14px\">{} QD{}</h2><div class=\"legend\"><span><i style=\"background:var(--w)\"></i>{}</span><span><i style=\"background:var(--r)\"></i>{}</span><span>MB/s</span></div>{}", e(&t("l_sweep")), qd, e(&t("l_write")), e(&t("l_read")), sweep_svg(d, qd));
            }
            let _ = write!(o, "<h2 style=\"margin-top:16px\">{}</h2><table><tr><th>{}</th><th class=\"n\">MB/s</th><th class=\"n\">IOPS</th><th class=\"n\">p50 / p99 / p99.9 µs</th></tr>", e(&t("l_scenarios")), e(&t("l_scenario")));
            for s in &d.scenarios {
                let lat = match (s.p50_us, s.p99_us, s.p999_us) {
                    (Some(a), Some(b), Some(c)) => format!("{} / {} / {}", num(a), num(b), num(c)),
                    _ => String::new(),
                };
                let _ = write!(o, "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>", e(&t(&format!("s_{}", s.key))), num(s.mbs), num(s.iops), lat);
            }
            o.push_str("</table>");
            if let Some(s) = &d.streams {
                let _ = write!(o, "<p class=\"tag\">{}</p>", e(&tf("msg_streams", &[s.streams.to_string(), num(s.total_mbs), num(s.min_stream_mbs), s.uhd_capacity.to_string()])));
            }
            if let Some(s) = &d.small_files {
                let _ = write!(o, "<p class=\"tag\">{}</p>", e(&tf("msg_smallfiles", &[s.files.to_string(), num(s.create_per_s), num(s.read_per_s), num(s.delete_per_s)])));
            }
        }
        o.push_str("</div>");
    }

    // network
    if let Some(n) = &rec.results.net {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2><p class=\"tag\">{}</p><table>", e(&t("h_net")), e(&if n.mode == "remote" { t("msg_net_remote") } else { t("msg_net_loopback") }));
        for (k, v) in [("l_up1", n.up1_mbps), ("l_up4", n.up4_mbps), ("l_down1", n.down1_mbps), ("l_down4", n.down4_mbps), ("l_bidir", n.bidir_mbps)] {
            if let Some(v) = v {
                let _ = write!(o, "<tr><td class=\"k\">{}</td><td>{} Mbit/s</td></tr>", e(&t(k)), num(v));
            }
        }
        if let Some(r) = &n.rtt {
            let _ = write!(o, "<tr><td class=\"k\">{}</td><td>p50 {} µs · p99 {} µs · {} {} µs</td></tr>", e(&t("l_rtt")), num(r.p50_us), num(r.p99_us), e(&t("l_jitter")), num(r.jitter_us));
        }
        if let Some(r) = &n.gateway_rtt {
            let _ = write!(o, "<tr><td class=\"k\">{}</td><td>p50 {} µs · p99 {} µs</td></tr>", e(&t("l_gateway")), num(r.p50_us), num(r.p99_us));
        }
        o.push_str("</table></div>");
    }

    // video encoding
    if let Some(v) = &rec.results.video {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2>", e(&t("h_video")));
        match &v.ffmpeg_version {
            None => {
                let _ = write!(o, "<p class=\"tag\">{}</p>", e(&t("msg_ffmpeg_missing")));
            }
            Some(ver) => {
                let _ = write!(o, "<p class=\"tag\">ffmpeg {} · {} {} fps</p><table><tr><th>{}</th><th>{}</th><th class=\"n\">fps</th><th class=\"n\">4× fps</th><th class=\"n\">{}</th></tr>", e(ver), e(&t("l_source_fps")), v.source_fps.map(num).unwrap_or_default(), e(&t("l_encoder")), e(&t("l_codec")), e(&t("l_rt_streams")));
                for en in &v.encoders {
                    if en.ok {
                        let _ = write!(o, "<tr><td>{} <span class=\"tag\">{}</span></td><td>{}</td><td class=\"n\">{}{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>", e(&en.name), if en.kind == "hw" { "HW" } else { "CPU" }, e(&en.codec), en.fps.map(num).unwrap_or_default(), if en.source_limited { " *" } else { "" }, en.parallel_fps.map(num).unwrap_or_default(), en.realtime_streams.map(|x| x.to_string()).unwrap_or_default());
                    } else {
                        let _ = write!(o, "<tr><td>{} <span class=\"tag\">{}</span></td><td>{}</td><td colspan=\"3\" class=\"tag\">{}</td></tr>", e(&en.name), if en.kind == "hw" { "HW" } else { "CPU" }, e(&en.codec), e(&t("msg_enc_unavailable")));
                    }
                }
                let _ = write!(o, "</table><p class=\"tag\">{}</p>", e(&t("msg_video_note")));
            }
        }
        o.push_str("</div>");
    }

    // ray tracing
    let gpu_rt: Vec<_> = rec.results.gpu.iter().flat_map(|g| g.adapters.iter()).filter(|a| a.rt.is_some()).collect();
    if rec.results.rt.is_some() || !gpu_rt.is_empty() {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2>", e(&t("h_rt")));
        let cpu = rec.results.rt.as_ref();
        let max = gpu_rt.iter().filter_map(|a| a.rt.as_ref().map(|r| r.mrays)).chain(cpu.map(|c| c.multi_mrays)).fold(0.0, f64::max).max(1e-9);
        let bar = |label: String, v: f64, col: &str| format!("<div class=\"row\"><span>{}</span><div class=\"bar\"><i style=\"width:{:.1}%;background:{col}\"></i></div><b>{}</b></div>", e(&label), (v / max * 100.0).max(0.5), num(v));
        if let Some(c) = cpu {
            o.push_str(&bar(format!("CPU · 1 {}", t("u_thread")), c.single_mrays, "var(--a)"));
            o.push_str(&bar(format!("CPU · {} {}", c.threads, t("u_threads")), c.multi_mrays, "var(--a)"));
        }
        for a in &gpu_rt {
            if let Some(r) = &a.rt {
                o.push_str(&bar(format!("GPU · {}", a.info.name), r.mrays, "var(--y)"));
            }
        }
        let _ = write!(o, "<p class=\"tag\">Mrays/s · {}</p>", e(&t("msg_rt_note")));
        if cpu.is_some() {
            let png = crate::bench::rt::preview_png(24);
            let _ = write!(o, "<p><img alt=\"path traced reference scene\" width=\"512\" height=\"288\" style=\"max-width:100%;height:auto;border-radius:8px\" src=\"data:image/png;base64,{}\"></p>", crate::bench::rt::base64(&png));
        }
        o.push_str("</div>");
    }

    // gpu
    if let Some(g) = &rec.results.gpu {
        if g.supported && !g.adapters.is_empty() {
            let _ = write!(o, "<div class=\"card\"><h2>{}</h2>", e(&t("h_gpu")));
            for a in &g.adapters {
                let _ = write!(o, "<h2 style=\"margin-top:12px\">{} <span class=\"tag\">{} · {}</span></h2>", e(&a.info.name), e(&a.info.backend), e(&a.info.device_type));
                if let Some(er) = &a.error {
                    let _ = write!(o, "<p class=\"lw\">{}: {}</p>", e(&t("msg_gpu_failed")), e(er));
                    continue;
                }
                if a.is_software() {
                    let _ = write!(o, "<p class=\"tag\">{}</p>", e(&t("msg_gpu_software")));
                }
                o.push_str("<table>");
                let mut row = |k: &str, v: String| {
                    let _ = write!(o, "<tr><td class=\"k\">{}</td><td>{}</td></tr>", e(&t(k)), e(&v));
                };
                let f = |x: Option<f64>, u: &str| x.map(|v| format!("{} {}", num(v), u)).unwrap_or_else(|| "-".into());
                row("l_fp32", f(a.fp32_gflops, "GFLOPS"));
                row("l_int", f(a.int_gops, "Gops/s"));
                row("l_membw", f(a.mem_gbs, "GB/s"));
                row("l_matmul", format!("{}{}", f(a.matmul_gflops, "GFLOPS"), match a.matmul_ok { Some(true) => " ✔", Some(false) => " ✘", None => "" }));
                row("l_transfer", format!("↑ {} GB/s · ↓ {} GB/s", a.h2d_gbs.map(num).unwrap_or_default(), a.d2h_gbs.map(num).unwrap_or_default()));
                row("l_dispatch", format!("p50 {} µs · p99 {} µs", a.latency_p50_us.map(num).unwrap_or_default(), a.latency_p99_us.map(num).unwrap_or_default()));
                o.push_str("</table>");
            }
            o.push_str("</div>");
        }
    }

    if rec.results.net.is_none() && rec.results.cpu.is_some() && rec.results.mem.is_some() && rec.results.disk.is_some() {
        let _ = write!(o, "<div class=\"card\"><h2>{}</h2><p class=\"tag\">{}</p></div>", e(&t("h_net")), e(&t("msg_net_howto")));
    }

    // findings
    o.push_str("<div class=\"card\">");
    let _ = write!(o, "<h2>{}</h2><ul>", e(&t("h_findings")));
    for (lvl, icon, cls) in [("warn", "⚠", "lw"), ("ok", "✔", "lo"), ("info", "ℹ", "li")] {
        for f in rec.findings.iter().filter(|f| f.level == lvl) {
            let _ = write!(o, "<li><span class=\"{cls}\">{icon}</span> <span class=\"tag\">[{}]</span> {}</li>", e(&t(&format!("fc_{}", f.cat))), e(&finding_text(lang, f)));
        }
    }
    let _ = write!(o, "</ul></div><p class=\"sub\">{}</p></main></body></html>", e(&t("msg_footer")));
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_self_contained_and_escaped() {
        let mut rec = crate::record::tests_support::sample_full();
        rec.system.label = Some("<script>alert(1)</script>".into());
        let h = html(&rec, Lang::En);
        assert!(h.starts_with("<!doctype html>"));
        assert!(!h.contains("<script"), "labels must be escaped and the page must not contain scripts");
        assert!(!h.contains("http://") && !h.contains("https://"), "no external resources");
        assert!(h.contains("<svg"));
        assert!(h.len() < 400_000);
    }

    #[test]
    fn html_renders_in_turkish() {
        let rec = crate::record::tests_support::sample_full();
        let h = html(&rec, Lang::Tr);
        assert!(h.contains("lang=\"tr\""));
    }
}
