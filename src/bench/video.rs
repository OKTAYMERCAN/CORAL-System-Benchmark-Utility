//! Video transcode benchmark through the `ffmpeg` command-line tool (not linked: it is run as a
//! separate program, so nothing of ffmpeg ends up inside SysCheck).
//!
//! Fixed workload: ffmpeg's synthetic `testsrc2` 1920x1080 @ 30 fps, encoded to H.264 (and HEVC in
//! the full run) with the CPU encoder (libx264 / libx265) and with every hardware encoder that
//! actually works on this machine (NVENC, Quick Sync, VAAPI, AMF, VideoToolbox, MediaCodec).
//! Speed is measured as a difference between two runs of different length, so the encoder start-up
//! time cancels out. The source generator also costs CPU time; its own speed is measured
//! (`source_fps`) and an encoder that gets close to it is flagged `source_limited`.

use crate::bench::Profile;
use crate::ui::Ui;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const WARM_FRAMES: u32 = 30;
const PARALLEL: u32 = 4;
const TIMEOUT_SECS: u64 = 120;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EncoderResult {
    pub name: String,
    /// h264 | hevc
    pub codec: String,
    /// cpu | hw
    pub kind: String,
    /// false = the encoder is built into ffmpeg but did not work here (no such hardware / driver).
    pub ok: bool,
    pub fps: Option<f64>,
    /// total fps of 4 parallel encodes (full run only)
    pub parallel_fps: Option<f64>,
    /// How many real-time 1080p30 streams this encoder can sustain.
    pub realtime_streams: Option<u32>,
    pub source_limited: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VideoResults {
    /// None = ffmpeg was not found.
    pub ffmpeg_version: Option<String>,
    pub source_fps: Option<f64>,
    pub frames: u32,
    pub encoders: Vec<EncoderResult>,
}

impl VideoResults {
    pub fn cpu_h264(&self) -> Option<&EncoderResult> {
        self.encoders.iter().find(|e| e.name == "libx264" && e.ok)
    }
    /// Fastest working hardware encoder (H.264 preferred for comparability).
    pub fn best_hw(&self) -> Option<&EncoderResult> {
        self.encoders
            .iter()
            .filter(|e| e.ok && e.kind == "hw" && e.codec == "h264")
            .max_by(|a, b| a.fps.unwrap_or(0.0).partial_cmp(&b.fps.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal))
    }
    #[cfg(test)]
    pub fn any_hw_working(&self) -> bool {
        self.encoders.iter().any(|e| e.ok && e.kind == "hw")
    }
}

struct Enc {
    name: &'static str,
    codec: &'static str,
    kind: &'static str,
}

const ENCODERS: [Enc; 14] = [
    Enc { name: "libx264", codec: "h264", kind: "cpu" },
    Enc { name: "h264_nvenc", codec: "h264", kind: "hw" },
    Enc { name: "h264_qsv", codec: "h264", kind: "hw" },
    Enc { name: "h264_vaapi", codec: "h264", kind: "hw" },
    Enc { name: "h264_amf", codec: "h264", kind: "hw" },
    Enc { name: "h264_videotoolbox", codec: "h264", kind: "hw" },
    Enc { name: "h264_mediacodec", codec: "h264", kind: "hw" },
    Enc { name: "libx265", codec: "hevc", kind: "cpu" },
    Enc { name: "hevc_nvenc", codec: "hevc", kind: "hw" },
    Enc { name: "hevc_qsv", codec: "hevc", kind: "hw" },
    Enc { name: "hevc_vaapi", codec: "hevc", kind: "hw" },
    Enc { name: "hevc_amf", codec: "hevc", kind: "hw" },
    Enc { name: "hevc_videotoolbox", codec: "hevc", kind: "hw" },
    Enc { name: "hevc_mediacodec", codec: "hevc", kind: "hw" },
];

/// Names of the encoders listed by `ffmpeg -encoders`.
pub fn parse_encoder_list(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            let name = it.next()?;
            (flags.len() == 6 && flags.starts_with('V') && name != "=").then(|| name.to_string())
        })
        .collect()
}

#[cfg(unix)]
fn render_node() -> Option<String> {
    let mut v: Vec<_> = std::fs::read_dir("/dev/dri").ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("renderD")).collect();
    v.sort();
    v.first().map(|n| format!("/dev/dri/{n}"))
}
#[cfg(not(unix))]
fn render_node() -> Option<String> {
    None
}

/// (options before the input, options after the input) for an encoder.
pub fn encoder_args(name: &str) -> (Vec<String>, Vec<String>) {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match name {
        "libx264" => (vec![], s(&["-c:v", "libx264", "-preset", "medium", "-crf", "23", "-pix_fmt", "yuv420p"])),
        "libx265" => (vec![], s(&["-c:v", "libx265", "-preset", "veryfast", "-crf", "26", "-pix_fmt", "yuv420p", "-x265-params", "log-level=error"])),
        n if n.ends_with("_nvenc") => (vec![], s(&["-c:v", n, "-preset", "p4", "-b:v", "8M", "-pix_fmt", "yuv420p"])),
        n if n.ends_with("_qsv") => (s(&["-init_hw_device", "qsv=hw", "-filter_hw_device", "hw"]), s(&["-vf", "format=nv12,hwupload=extra_hw_frames=64", "-c:v", n, "-b:v", "8M"])),
        n if n.ends_with("_vaapi") => {
            let dev = render_node().unwrap_or_else(|| "/dev/dri/renderD128".into());
            (vec!["-vaapi_device".into(), dev], s(&["-vf", "format=nv12,hwupload", "-c:v", n, "-b:v", "8M"]))
        }
        n => (vec![], s(&["-c:v", n, "-b:v", "8M", "-pix_fmt", "yuv420p"])),
    }
}

/// Run ffmpeg on `frames` frames of the test source. Returns the wall time when it succeeded.
fn run_ffmpeg(ffmpeg: &str, pre: &[String], post: &[String], frames: u32) -> Option<f64> {
    let mut c = Command::new(ffmpeg);
    c.args(["-hide_banner", "-nostdin", "-loglevel", "error"]).args(pre);
    c.args(["-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30", "-an", "-frames:v", &frames.to_string()]).args(post).args(["-f", "null", "-"]);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let t = Instant::now();
    let mut child = c.spawn().ok()?;
    wait(&mut child, t).map(|_| t.elapsed().as_secs_f64())
}

fn wait(child: &mut std::process::Child, t: Instant) -> Option<()> {
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success().then_some(()),
            Ok(None) => {
                if t.elapsed() > Duration::from_secs(TIMEOUT_SECS) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => return None,
        }
    }
}

/// Frames per second from two runs of different length (start-up time cancels out).
pub fn fps_from(n1: u32, t1: f64, n2: u32, t2: f64) -> f64 {
    if t2 > t1 + 0.05 {
        (n2 - n1) as f64 / (t2 - t1)
    } else if t2 > 0.0 {
        n2 as f64 / t2
    } else {
        0.0
    }
}

fn measure(ffmpeg: &str, pre: &[String], post: &[String], frames: u32) -> Option<f64> {
    let t1 = run_ffmpeg(ffmpeg, pre, post, WARM_FRAMES)?;
    let n2 = WARM_FRAMES + frames;
    let t2 = run_ffmpeg(ffmpeg, pre, post, n2)?;
    Some(fps_from(WARM_FRAMES, t1, n2, t2))
}

/// Total fps of `PARALLEL` simultaneous encodes.
fn measure_parallel(ffmpeg: &str, pre: &[String], post: &[String], frames: u32) -> Option<f64> {
    let t = Instant::now();
    let mut kids = Vec::new();
    for _ in 0..PARALLEL {
        let mut c = Command::new(ffmpeg);
        c.args(["-hide_banner", "-nostdin", "-loglevel", "error"]).args(pre);
        c.args(["-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30", "-an", "-frames:v", &frames.to_string()]).args(post).args(["-f", "null", "-"]);
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        kids.push(c.spawn().ok()?);
    }
    let mut ok = true;
    for k in kids.iter_mut() {
        ok &= wait(k, t).is_some();
    }
    ok.then(|| (PARALLEL * frames) as f64 / t.elapsed().as_secs_f64())
}

fn ffmpeg_version(ffmpeg: &str) -> Option<String> {
    let out = Command::new(ffmpeg).arg("-version").stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&out.stdout).lines().next()?.to_string();
    first.split_whitespace().nth(2).map(|v| v.to_string())
}

pub fn run(ui: &Ui, prof: &Profile) -> VideoResults {
    let ffmpeg = "ffmpeg";
    let mut res = VideoResults { frames: prof.video_frames, ..Default::default() };
    let Some(ver) = ffmpeg_version(ffmpeg) else {
        return res;
    };
    res.ffmpeg_version = Some(ver);
    let listed = Command::new(ffmpeg).args(["-hide_banner", "-encoders"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().map(|o| parse_encoder_list(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default();

    ui.progress(&format!("{}: source", ui.t("prog_video")));
    let src_post: Vec<String> = ["-pix_fmt", "yuv420p", "-c:v", "rawvideo"].iter().map(|x| x.to_string()).collect();
    res.source_fps = measure(ffmpeg, &[], &src_post, prof.video_frames);

    let todo: Vec<&Enc> = ENCODERS.iter().filter(|e| (prof.video_hevc || e.codec == "h264") && listed.iter().any(|n| n == e.name)).collect();
    for (i, e) in todo.iter().enumerate() {
        ui.progress(&format!("{}: {} [{}/{}]", ui.t("prog_video"), e.name, i + 1, todo.len()));
        let (pre, post) = encoder_args(e.name);
        // HEVC software encoding is far slower: fewer frames keep the run time reasonable
        let frames = if e.name == "libx265" { (prof.video_frames / 3).max(30) } else { prof.video_frames };
        let mut r = EncoderResult { name: e.name.into(), codec: e.codec.into(), kind: e.kind.into(), ..Default::default() };
        if let Some(fps) = measure(ffmpeg, &pre, &post, frames) {
            r.ok = true;
            r.fps = Some(fps);
            if prof.video_parallel {
                r.parallel_fps = measure_parallel(ffmpeg, &pre, &post, frames.min(120));
            }
            let best = fps.max(r.parallel_fps.unwrap_or(0.0));
            r.realtime_streams = Some((best / 30.0).floor() as u32);
            r.source_limited = res.source_fps.map(|s| fps > 0.8 * s).unwrap_or(false);
        }
        res.encoders.push(r);
    }
    ui.clear_progress();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_list_parsing() {
        let out = " V....D libx264              libx264 H.264\n V..... h264_qsv    H.264 (Intel)\n A....D aac   AAC\n ------\n Encoders:\n V..... = Video\n";
        assert_eq!(parse_encoder_list(out), vec!["libx264", "h264_qsv"]);
    }

    #[test]
    fn fps_calculation_cancels_startup() {
        // 1 s start-up + 100 fps: 30 frames take 1.3 s, 270 frames take 3.7 s
        let f = fps_from(30, 1.3, 270, 3.7);
        assert!((f - 100.0).abs() < 1e-9);
        // degenerate timings fall back to the plain average
        assert!((fps_from(30, 1.0, 60, 1.0) - 60.0).abs() < 1e-9);
        assert_eq!(fps_from(1, 0.0, 2, 0.0), 0.0);
    }

    #[test]
    fn encoder_arguments_are_well_formed() {
        for e in ENCODERS.iter() {
            let (pre, post) = encoder_args(e.name);
            assert!(post.iter().any(|a| a == e.name), "{} must select its own codec", e.name);
            assert_eq!(pre.len() % 2, 0);
        }
        assert!(encoder_args("h264_qsv").0.contains(&"-init_hw_device".to_string()));
        assert!(encoder_args("h264_vaapi").0.contains(&"-vaapi_device".to_string()));
    }

    #[test]
    fn results_helpers() {
        let mk = |n: &str, k: &str, fps: f64, ok: bool| EncoderResult { name: n.into(), codec: "h264".into(), kind: k.into(), ok, fps: Some(fps), ..Default::default() };
        let r = VideoResults { encoders: vec![mk("libx264", "cpu", 50.0, true), mk("h264_nvenc", "hw", 400.0, true), mk("h264_qsv", "hw", 900.0, false), mk("h264_vaapi", "hw", 200.0, true)], ..Default::default() };
        assert_eq!(r.cpu_h264().unwrap().fps, Some(50.0));
        assert_eq!(r.best_hw().unwrap().name, "h264_nvenc", "failed encoders must be ignored");
        assert!(r.any_hw_working());
    }

    #[test]
    fn real_encode_when_ffmpeg_is_present() {
        if ffmpeg_version("ffmpeg").is_none() {
            return;
        }
        let ui = Ui::new(crate::i18n::Lang::En, true);
        let mut prof = Profile::new(true);
        prof.video_frames = 30;
        let r = run(&ui, &prof);
        assert!(r.ffmpeg_version.is_some() && r.source_fps.unwrap_or(0.0) > 0.0);
        if let Some(x) = r.cpu_h264() {
            assert!(x.fps.unwrap() > 1.0 && x.realtime_streams.is_some());
        }
    }

    #[test]
    fn missing_ffmpeg_is_reported_not_fatal() {
        assert!(ffmpeg_version("definitely-not-ffmpeg-xyz").is_none());
    }
}
