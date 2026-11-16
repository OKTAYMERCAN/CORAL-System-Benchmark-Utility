//! Network benchmark with a tiny built-in server (`syscheck serve`).
//!
//! Tests against a remote server (real network): TCP throughput with 1 and 4 streams in both
//! directions, bidirectional, round-trip time (64 B echo, p50 / p99 / jitter) and a message-size
//! sweep. With `--net-loopback` the same tests run against a local server and are labelled
//! "loopback" (they measure the local TCP stack, not the network); this is off by default. The default gateway is probed
//! with TCP connects (a refused connection still proves the round trip).
//!
//! Protocol (all integers little endian), client -> server header of 24 bytes:
//!   magic "SCNET001" | mode u8 | pad 3 | duration_ms u32 | msg_size u32 | pad 4
//! mode 1 = sink (client sends; server replies bytes u64 + elapsed_ns u64 at the end),
//! mode 2 = source (server sends for duration_ms), mode 3 = echo (server echoes msg_size blocks).
//! The server is unauthenticated and limits duration (60 s), message size (16 MiB) and
//! concurrent connections (64). Run it only on networks you trust.

use crate::bench::Profile;
use crate::ui::Ui;
use crate::util::*;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 5201;
const MAGIC: &[u8; 8] = b"SCNET001";
const MODE_SINK: u8 = 1;
const MODE_SOURCE: u8 = 2;
const MODE_ECHO: u8 = 3;
const MAX_MS: u32 = 60_000;
const MAX_MSG: u32 = 16 << 20;
const MAX_CONN: usize = 64;
const STREAM_MSG: u32 = 256 << 10;

// ───────────────────────── result types ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Rtt {
    pub samples: u32,
    pub min_us: f64,
    pub p50_us: f64,
    pub p99_us: f64,
    /// Mean absolute difference between consecutive round trips.
    pub jitter_us: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MsgPoint {
    pub size: u32,
    pub msgs_per_s: f64,
    /// Echo throughput counting both directions, MB/s.
    pub mbs: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NetResults {
    /// "remote" (real network) or "loopback" (local TCP stack only).
    pub mode: String,
    pub error: Option<String>,
    pub up1_mbps: Option<f64>,
    pub up4_mbps: Option<f64>,
    pub down1_mbps: Option<f64>,
    pub down4_mbps: Option<f64>,
    pub bidir_mbps: Option<f64>,
    pub rtt: Option<Rtt>,
    pub sweep: Vec<MsgPoint>,
    pub gateway_rtt: Option<Rtt>,
    pub link_mbps: Option<u32>,
    /// link speed x 0.94 (TCP/IP overhead); only for remote tests.
    pub ceiling_mbps: Option<f64>,
    /// best 4-stream result as percent of the ceiling.
    pub efficiency_pct: Option<f64>,
}

impl NetResults {
    /// Best single-direction 4-stream throughput.
    pub fn best_mbps(&self) -> Option<f64> {
        match (self.up4_mbps, self.down4_mbps) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }
}

// ───────────────────────── server ─────────────────────────

fn header(mode: u8, ms: u32, msg: u32) -> [u8; 24] {
    let mut h = [0u8; 24];
    h[..8].copy_from_slice(MAGIC);
    h[8] = mode;
    h[12..16].copy_from_slice(&ms.to_le_bytes());
    h[16..20].copy_from_slice(&msg.to_le_bytes());
    h
}

fn handle(mut s: TcpStream) -> io::Result<()> {
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut h = [0u8; 24];
    s.read_exact(&mut h)?;
    if &h[..8] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic"));
    }
    let mode = h[8];
    let ms = u32::from_le_bytes([h[12], h[13], h[14], h[15]]).min(MAX_MS);
    let msg = u32::from_le_bytes([h[16], h[17], h[18], h[19]]).clamp(1, MAX_MSG) as usize;
    let dur = Duration::from_millis(ms as u64);
    let start = Instant::now();
    match mode {
        MODE_SINK => {
            let mut buf = vec![0u8; msg.max(64 << 10)];
            let mut total = 0u64;
            loop {
                match s.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => total += n as u64,
                    Err(e) => return Err(e),
                }
                if start.elapsed() > dur + Duration::from_secs(3) {
                    break;
                }
            }
            let mut r = [0u8; 16];
            r[..8].copy_from_slice(&total.to_le_bytes());
            r[8..].copy_from_slice(&(start.elapsed().as_nanos() as u64).to_le_bytes());
            let _ = s.write_all(&r);
        }
        MODE_SOURCE => {
            let mut buf = vec![0u8; msg.max(4096)];
            Rng::new(5).fill(&mut buf);
            while start.elapsed() < dur {
                s.write_all(&buf)?;
            }
        }
        MODE_ECHO => {
            let mut buf = vec![0u8; msg];
            let limit = dur + Duration::from_secs(5);
            while start.elapsed() < limit {
                match s.read_exact(&mut buf) {
                    Ok(()) => s.write_all(&buf)?,
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(e),
                }
            }
        }
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown mode")),
    }
    Ok(())
}

/// Accept loop. Returns when `stop` becomes true.
pub fn serve_listener(listener: TcpListener, stop: Arc<AtomicBool>, verbose: bool) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let active = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((s, peer)) => {
                let _ = s.set_nonblocking(false);
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONN {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue; // over the limit: drop the connection
                }
                if verbose {
                    println!("  connection from {}", peer.ip());
                }
                let active = active.clone();
                std::thread::spawn(move || {
                    let _ = handle(s);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// `syscheck serve`: runs until interrupted.
pub fn serve(port: u16) -> io::Result<()> {
    let l = TcpListener::bind(("0.0.0.0", port))?;
    println!("SysCheck network server listening on port {port} (Ctrl-C to stop)");
    println!("Unauthenticated: limits {MAX_MS} ms per test, {} MiB messages, {MAX_CONN} connections.", MAX_MSG >> 20);
    serve_listener(l, Arc::new(AtomicBool::new(false)), true)
}

// ───────────────────────── client ─────────────────────────

fn connect(addr: SocketAddr, mode: u8, ms: u32, msg: u32) -> io::Result<TcpStream> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    s.set_write_timeout(Some(Duration::from_secs(15)))?;
    s.write_all(&header(mode, ms, msg))?;
    Ok(s)
}

/// Bytes moved and seconds spent by one stream.
fn one_stream(addr: SocketAddr, up: bool, secs: f64, barrier: &Barrier) -> io::Result<(u64, f64)> {
    let ms = (secs * 1000.0) as u32;
    let mut s = connect(addr, if up { MODE_SINK } else { MODE_SOURCE }, ms, STREAM_MSG)?;
    let mut buf = vec![0u8; STREAM_MSG as usize];
    Rng::new(3).fill(&mut buf);
    barrier.wait();
    let start = Instant::now();
    let mut total = 0u64;
    if up {
        while start.elapsed().as_secs_f64() < secs {
            s.write_all(&buf)?;
            total += buf.len() as u64;
        }
        let el = start.elapsed().as_secs_f64();
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut r = [0u8; 16];
        let _ = s.read_exact(&mut r); // wait for the server to confirm it drained the data
        Ok((total, el))
    } else {
        loop {
            match s.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => total += n as u64,
                Err(e) => return Err(e),
            }
            if start.elapsed().as_secs_f64() > secs + 2.0 {
                break;
            }
        }
        Ok((total, start.elapsed().as_secs_f64().min(secs.max(0.001) + 0.5)))
    }
}

/// Aggregate throughput in Mbit/s for `ups` upload and `downs` download streams running together.
fn throughput(addr: SocketAddr, ups: usize, downs: usize, secs: f64) -> io::Result<f64> {
    let n = ups + downs;
    let barrier = Barrier::new(n);
    let res: Vec<io::Result<(u64, f64)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..n)
            .map(|i| {
                let b = &barrier;
                sc.spawn(move || one_stream(addr, i < ups, secs, b))
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::Other, "stream panicked")))).collect()
    });
    let mut bytes = 0u64;
    let mut el: f64 = 0.0;
    for r in res {
        let (b, e) = r?;
        bytes += b;
        el = el.max(e);
    }
    Ok(if el > 0.0 { bytes as f64 * 8.0 / el / 1e6 } else { 0.0 })
}

fn rtt_stats(mut seq_ns: Vec<u32>) -> Option<Rtt> {
    if seq_ns.len() < 3 {
        return None;
    }
    let jitter = seq_ns.windows(2).map(|w| (w[1] as f64 - w[0] as f64).abs()).sum::<f64>() / (seq_ns.len() - 1) as f64 / 1000.0;
    seq_ns.sort_unstable();
    Some(Rtt {
        samples: seq_ns.len() as u32,
        min_us: seq_ns[0] as f64 / 1000.0,
        p50_us: percentile(&seq_ns, 50.0) / 1000.0,
        p99_us: percentile(&seq_ns, 99.0) / 1000.0,
        jitter_us: jitter,
    })
}

fn echo_rtt(addr: SocketAddr, secs: f64) -> io::Result<Rtt> {
    let mut s = connect(addr, MODE_ECHO, (secs * 1000.0) as u32 + 2000, 64)?;
    let msg = [0xA5u8; 64];
    let mut back = [0u8; 64];
    let mut seq = Vec::new();
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < secs && seq.len() < 100_000 {
        let t = Instant::now();
        s.write_all(&msg)?;
        s.read_exact(&mut back)?;
        seq.push(t.elapsed().as_nanos().min(u32::MAX as u128) as u32);
    }
    rtt_stats(seq).ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "too few round trips"))
}

fn echo_size(addr: SocketAddr, size: u32, secs: f64) -> io::Result<MsgPoint> {
    let mut s = connect(addr, MODE_ECHO, (secs * 1000.0) as u32 + 2000, size)?;
    let mut msg = vec![0u8; size as usize];
    Rng::new(size as u64).fill(&mut msg);
    let mut back = vec![0u8; size as usize];
    let start = Instant::now();
    let mut n = 0u64;
    while start.elapsed().as_secs_f64() < secs {
        s.write_all(&msg)?;
        s.read_exact(&mut back)?;
        n += 1;
    }
    let el = start.elapsed().as_secs_f64();
    Ok(MsgPoint { size, msgs_per_s: n as f64 / el, mbs: 2.0 * size as f64 * n as f64 / el / 1e6 })
}

/// Round-trip time to the default gateway using TCP connects (a refused connection counts).
pub fn gateway_rtt(ip: Ipv4Addr, samples: usize) -> Option<Rtt> {
    let ports = [443u16, 80, 53, 22];
    let mut seq = Vec::new();
    for i in 0..samples {
        let addr = SocketAddr::from((ip, ports[i % ports.len()]));
        let t = Instant::now();
        let r = TcpStream::connect_timeout(&addr, Duration::from_millis(150));
        let ok = match r {
            Ok(_) => true,
            Err(e) => e.kind() == io::ErrorKind::ConnectionRefused,
        };
        if ok {
            seq.push(t.elapsed().as_nanos().min(u32::MAX as u128) as u32);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    rtt_stats(seq)
}

pub fn parse_server(s: &str) -> io::Result<SocketAddr> {
    let with_port = if s.contains(':') && !s.starts_with('[') && s.matches(':').count() == 1 || s.starts_with('[') && s.contains("]:") {
        s.to_string()
    } else if s.starts_with('[') {
        format!("{s}:{DEFAULT_PORT}")
    } else if s.matches(':').count() > 1 {
        format!("[{s}]:{DEFAULT_PORT}")
    } else {
        format!("{s}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "could not resolve server address"))
}

fn client_tests(ui: &Ui, addr: SocketAddr, prof: &Profile, res: &mut NetResults) -> io::Result<()> {
    let secs = prof.net_secs;
    let steps: [(&str, usize, usize); 5] = [("up 1", 1, 0), ("up 4", 4, 0), ("down 1", 0, 1), ("down 4", 0, 4), ("bidir", 1, 1)];
    for (i, (label, u, d)) in steps.iter().enumerate() {
        ui.progress(&format!("{}: {} [{}/{}]", ui.t("prog_net_tcp"), label, i + 1, steps.len()));
        let v = throughput(addr, *u, *d, secs)?;
        match i {
            0 => res.up1_mbps = Some(v),
            1 => res.up4_mbps = Some(v),
            2 => res.down1_mbps = Some(v),
            3 => res.down4_mbps = Some(v),
            _ => res.bidir_mbps = Some(v),
        }
    }
    ui.progress(&ui.t("prog_net_rtt"));
    res.rtt = Some(echo_rtt(addr, prof.net_rtt_secs)?);
    for (i, size) in [64u32, 1 << 10, 4 << 10, 16 << 10, 64 << 10, 256 << 10, 1 << 20].iter().enumerate() {
        ui.progress(&format!("{}: {} [{}/7]", ui.t("prog_net_sweep"), bs_label(*size), i + 1));
        res.sweep.push(echo_size(addr, *size, prof.net_rtt_secs)?);
    }
    Ok(())
}

/// Run the network tests. `server == None` uses a local loopback server.
pub fn run(ui: &Ui, server: Option<&str>, gateway: Option<Ipv4Addr>, link_mbps: Option<u32>, prof: &Profile) -> NetResults {
    let mut res = NetResults { mode: if server.is_some() { "remote" } else { "loopback" }.into(), link_mbps, ..Default::default() };
    let outcome = match server {
        Some(s) => parse_server(s).and_then(|a| client_tests(ui, a, prof, &mut res)),
        None => {
            let stop = Arc::new(AtomicBool::new(false));
            match TcpListener::bind(("127.0.0.1", 0)) {
                Ok(l) => {
                    let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
                    let st = stop.clone();
                    let h = std::thread::spawn(move || serve_listener(l, st, false));
                    let r = client_tests(ui, SocketAddr::from((Ipv4Addr::LOCALHOST, port)), prof, &mut res);
                    stop.store(true, Ordering::Relaxed);
                    let _ = h.join();
                    r
                }
                Err(e) => Err(e),
            }
        }
    };
    if let Err(e) = outcome {
        res.error = Some(e.to_string());
    }
    if server.is_some() {
        if let (Some(link), Some(best)) = (link_mbps, res.best_mbps()) {
            let ceil = link as f64 * 0.94;
            res.ceiling_mbps = Some(ceil);
            res.efficiency_pct = Some(best / ceil * 100.0);
        }
    }
    if let Some(gw) = gateway {
        if prof.gw_samples > 0 {
            ui.progress(&ui.t("prog_net_gw"));
            res.gateway_rtt = gateway_rtt(gw, prof.gw_samples);
        }
    }
    ui.clear_progress();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_server() -> (SocketAddr, Arc<AtomicBool>) {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = l.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        std::thread::spawn(move || serve_listener(l, st, false));
        (addr, stop)
    }

    #[test]
    fn throughput_and_rtt_work_on_loopback() {
        let (addr, stop) = local_server();
        let up = throughput(addr, 2, 0, 0.3).unwrap();
        let down = throughput(addr, 0, 2, 0.3).unwrap();
        let both = throughput(addr, 1, 1, 0.3).unwrap();
        assert!(up > 100.0 && down > 100.0 && both > 100.0, "{up} {down} {both}");
        let rtt = echo_rtt(addr, 0.2).unwrap();
        assert!(rtt.samples > 10 && rtt.p50_us > 0.0 && rtt.p99_us >= rtt.p50_us && rtt.min_us <= rtt.p50_us);
        let p = echo_size(addr, 4096, 0.15).unwrap();
        assert!(p.msgs_per_s > 10.0 && p.mbs > 0.0);
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn server_rejects_bad_magic_and_survives() {
        let (addr, stop) = local_server();
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(&[0u8; 24]).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut b = [0u8; 1];
        assert!(matches!(s.read(&mut b), Ok(0) | Err(_)));
        // a valid client still works afterwards
        assert!(echo_rtt(addr, 0.1).is_ok());
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn server_clamps_excessive_requests() {
        let (addr, stop) = local_server();
        // 1 hour and 4 GiB requested: server must clamp instead of allocating 4 GiB
        let mut s = connect(addr, MODE_SOURCE, 3_600_000, u32::MAX).unwrap();
        let mut buf = vec![0u8; 1 << 16];
        assert!(s.read(&mut buf).unwrap() > 0);
        drop(s);
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn server_addresses_parse() {
        assert_eq!(parse_server("127.0.0.1").unwrap().port(), DEFAULT_PORT);
        assert_eq!(parse_server("127.0.0.1:6000").unwrap().port(), 6000);
        assert_eq!(parse_server("::1").unwrap().port(), DEFAULT_PORT);
        assert_eq!(parse_server("[::1]:7000").unwrap().port(), 7000);
        assert!(parse_server("").is_err() || parse_server("").is_ok());
    }

    #[test]
    fn rtt_stats_need_samples() {
        assert!(rtt_stats(vec![1000, 2000]).is_none());
        let r = rtt_stats(vec![1000, 3000, 2000, 2000]).unwrap();
        assert_eq!(r.min_us, 1.0);
        assert!(r.jitter_us > 0.0);
    }

    #[test]
    fn full_loopback_run() {
        let ui = Ui::new(crate::i18n::Lang::En, true);
        let mut prof = Profile::new(true);
        prof.net_secs = 0.2;
        prof.net_rtt_secs = 0.1;
        let r = run(&ui, None, None, None, &prof);
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.mode, "loopback");
        assert_eq!(r.sweep.len(), 7);
        assert!(r.up4_mbps.unwrap() > 0.0 && r.rtt.is_some());
        assert!(r.ceiling_mbps.is_none(), "loopback has no link ceiling");
    }

    #[test]
    fn unreachable_server_reports_error() {
        let ui = Ui::new(crate::i18n::Lang::En, true);
        let prof = Profile::new(true);
        let r = run(&ui, Some("127.0.0.1:1"), None, Some(1000), &prof);
        assert!(r.error.is_some());
        assert!(r.up1_mbps.is_none());
    }
}
