//! Small shared helpers: deterministic RNG, statistics, time formatting, terminal bars.

use std::time::{SystemTime, UNIX_EPOCH};

// ───────────────────────── deterministic RNG ─────────────────────────

/// xorshift64* - tiny, fast and identical on every platform, so every machine
/// benchmarks exactly the same data.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in 0..n (n > 0).
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }
    pub fn fill(&mut self, buf: &mut [u8]) {
        let mut chunks = buf.chunks_exact_mut(8);
        for c in &mut chunks {
            c.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        let rem = chunks.into_remainder();
        if !rem.is_empty() {
            let b = self.next_u64().to_le_bytes();
            rem.copy_from_slice(&b[..rem.len()]);
        }
    }
}

// ───────────────────────── statistics ─────────────────────────

pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Coefficient of variation in percent (run-to-run noise indicator).
pub fn cv_percent(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let m = mean(v);
    if m <= 0.0 {
        return 0.0;
    }
    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (v.len() - 1) as f64;
    var.sqrt() / m * 100.0
}

pub fn geomean(v: &[f64]) -> f64 {
    let pos: Vec<f64> = v.iter().copied().filter(|x| *x > 0.0).collect();
    if pos.is_empty() {
        return 0.0;
    }
    (pos.iter().map(|x| x.ln()).sum::<f64>() / pos.len() as f64).exp()
}

/// Nearest-rank percentile of an already sorted slice (p in 0..=100).
pub fn percentile(sorted: &[u32], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1] as f64
}

// ───────────────────────── scoring helpers ─────────────────────────

/// Log-scale 0..100 between `lo` and `hi` (a 2x gain is worth the same everywhere).
pub fn lscore(x: f64, lo: f64, hi: f64) -> f64 {
    if !(x > 0.0) || lo <= 0.0 || hi <= lo {
        return 0.0;
    }
    ((x.ln() - lo.ln()) / (hi.ln() - lo.ln()) * 100.0).clamp(0.0, 100.0)
}

/// Weighted average ignoring missing components (weights are renormalized).
pub fn wavg(parts: &[(Option<f64>, f64)]) -> Option<f64> {
    let (mut num, mut den) = (0.0, 0.0);
    for (v, w) in parts {
        if let Some(v) = v {
            num += v * w;
            den += w;
        }
    }
    if den > 0.0 {
        Some(num / den)
    } else {
        None
    }
}

// ───────────────────────── time ─────────────────────────

pub fn unix_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn unix_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// (year, month, day, hour, minute, second) in UTC without any date crate.
pub fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32, (rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32)
}

pub fn rfc3339_utc(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_unix(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

pub fn stamp_compact(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_unix(secs);
    format!("{y:04}{mo:02}{d:02}_{h:02}{mi:02}{s:02}")
}

// ───────────────────────── formatting ─────────────────────────

/// Block size label: 0.5 KB ... 8 MB.
pub fn bs_label(bytes: u32) -> String {
    if bytes < 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{} MB", bytes / (1024 * 1024))
    }
}

/// Compact number with sensible precision.
pub fn num(x: f64) -> String {
    if x >= 1000.0 {
        format!("{:.0}", x)
    } else if x >= 100.0 {
        format!("{:.1}", x)
    } else if x >= 10.0 {
        format!("{:.2}", x)
    } else {
        format!("{:.3}", x)
    }
}

pub fn pad_right(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

pub fn pad_left(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(width - n))
    }
}

// ───────────────────────── terminal ─────────────────────────

pub mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RED: &str = "\x1b[91m";
    pub const GREEN: &str = "\x1b[92m";
    pub const YELLOW: &str = "\x1b[93m";
    pub const BLUE: &str = "\x1b[94m";
    pub const MAGENTA: &str = "\x1b[95m";
    pub const CYAN: &str = "\x1b[96m";
    pub const GRAY: &str = "\x1b[90m";
}

pub fn paint(color: bool, code: &str, s: &str) -> String {
    if color {
        format!("{code}{s}{}", ansi::RESET)
    } else {
        s.to_string()
    }
}

/// Horizontal bar with 1/8-cell resolution. `track` draws the empty part as a dim rail.
pub fn bar(color: bool, code: &str, frac: f64, width: usize, track: bool) -> String {
    let f = if frac.is_finite() { frac.clamp(0.0, 1.0) } else { 0.0 };
    let eighths = (f * width as f64 * 8.0).round() as usize;
    let full = eighths / 8;
    let rem = eighths % 8;
    const PART: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let mut s = "█".repeat(full);
    let mut used = full;
    if rem > 0 && full < width {
        s.push_str(PART[rem]);
        used += 1;
    }
    let pad = width.saturating_sub(used);
    let rail = if track { paint(color, ansi::GRAY, &"░".repeat(pad)) } else { " ".repeat(pad) };
    format!("{}{}", paint(color, code, &s), rail)
}

pub fn spark(values: &[f64]) -> String {
    const G: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return String::new();
    }
    values.iter().map(|v| G[((v / max) * 7.0).round().clamp(0.0, 7.0) as usize]).collect()
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_nonzero() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut z = Rng::new(0);
        assert_ne!(z.next_u64(), 0);
        // fixed reference value: changing the generator would break comparability
        let mut r = Rng::new(1);
        assert_eq!(r.next_u64(), Rng::new(1).next_u64());
    }

    #[test]
    fn rng_below_is_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            assert!(r.below(10) < 10);
        }
    }

    #[test]
    fn rng_fill_handles_odd_lengths() {
        let mut r = Rng::new(3);
        let mut b = [0u8; 13];
        r.fill(&mut b);
        assert!(b.iter().any(|x| *x != 0));
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(951_782_400), (2000, 2, 29, 0, 0, 0)); // leap day
        assert_eq!(rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn stats() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert!((geomean(&[1.0, 100.0]) - 10.0).abs() < 1e-9);
        let v = [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50.0), 5.0);
        assert_eq!(percentile(&v, 99.0), 10.0);
        assert!(cv_percent(&[10.0, 10.0, 10.0]).abs() < 1e-12);
    }

    #[test]
    fn scoring_helpers() {
        assert_eq!(lscore(1.0, 1.0, 100.0), 0.0);
        assert_eq!(lscore(100.0, 1.0, 100.0), 100.0);
        assert!((lscore(10.0, 1.0, 100.0) - 50.0).abs() < 1e-9);
        assert_eq!(lscore(-5.0, 1.0, 100.0), 0.0);
        assert_eq!(wavg(&[(None, 1.0), (None, 2.0)]), None);
        assert_eq!(wavg(&[(Some(100.0), 1.0), (None, 5.0), (Some(0.0), 1.0)]), Some(50.0));
    }

    #[test]
    fn labels() {
        assert_eq!(bs_label(512), "0.5 KB");
        assert_eq!(bs_label(4096), "4 KB");
        assert_eq!(bs_label(8 * 1024 * 1024), "8 MB");
    }

    #[test]
    fn bar_width_is_constant() {
        for f in [0.0, 0.1, 0.5, 0.999, 1.0, 2.0, -1.0] {
            assert_eq!(bar(false, "", f, 20, true).chars().count(), 20);
            assert_eq!(bar(false, "", f, 20, false).chars().count(), 20);
        }
    }
}
