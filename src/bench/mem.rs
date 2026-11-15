//! Memory benchmark: sequential bandwidth (read / write / copy / triad, single thread and all
//! threads) and a pointer-chasing latency curve from 4 KiB to 256 MiB that makes the cache
//! hierarchy visible (L1 -> L2 -> L3 -> DRAM).
//!
//! Bandwidth is reported in decimal GB/s (1e9 bytes); copy counts bytes read plus bytes written.

use crate::bench::Profile;
use crate::hw::{self, CpuInfo, MemInfo};
use crate::ui::Ui;
use crate::util::*;
use serde::{Deserialize, Serialize};
use std::hint::black_box;
use std::sync::Barrier;
use std::time::Instant;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Bw {
    pub read: f64,
    pub write: f64,
    pub copy: f64,
    pub triad: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LatPoint {
    pub bytes: u64,
    pub ns: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemResults {
    /// Size of each of the three test arrays per thread (single-thread test), MiB.
    pub array_mb: u32,
    pub threads: u32,
    pub single: Bw,
    pub multi: Bw,
    pub latency: Vec<LatPoint>,
    /// Latency of the smallest working set (L1 hit), ns.
    pub l1_ns: f64,
    /// Latency of the largest working set (DRAM, includes TLB misses), ns.
    pub dram_ns: f64,
}

// ───────────────────────── bandwidth kernels ─────────────────────────

#[inline(never)]
fn op_read(a: &[u64]) -> u64 {
    let mut s = [0u64; 8];
    for c in a.chunks_exact(8) {
        for i in 0..8 {
            s[i] = s[i].wrapping_add(c[i]);
        }
    }
    s.iter().fold(0, |x, y| x ^ y)
}

#[inline(never)]
fn op_write(a: &mut [u64], v: u64) {
    a.fill(v);
}

#[inline(never)]
fn op_copy(dst: &mut [u64], src: &[u64]) {
    dst.copy_from_slice(src);
}

#[inline(never)]
fn op_triad(a: &mut [u64], b: &[u64], c: &[u64]) {
    for ((x, y), z) in a.iter_mut().zip(b).zip(c) {
        *x = (f64::from_bits(*y) + 3.0 * f64::from_bits(*z)).to_bits();
    }
}

/// Runs the four operations `reps` times on private arrays of `n` u64. Returns GB/s per rep.
fn bw_worker(n: usize, reps: usize, sync: Option<&Barrier>) -> Vec<[f64; 4]> {
    let mut a = vec![0x3FF0_0000_0000_0000u64; n]; // 1.0
    let b = vec![0x4000_0000_0000_0000u64; n]; // 2.0
    let c = vec![0x4008_0000_0000_0000u64; n]; // 3.0
    let bytes = (n * 8) as f64;
    let wait = || {
        if let Some(bar) = sync {
            bar.wait();
        }
    };
    let mut out = Vec::new();
    for _ in 0..reps.max(1) {
        let mut r = [0.0f64; 4];
        wait();
        let t = Instant::now();
        black_box(op_read(&b));
        r[0] = bytes / t.elapsed().as_secs_f64() / 1e9;
        wait();
        let t = Instant::now();
        op_write(&mut a, black_box(0x4010_0000_0000_0000));
        r[1] = bytes / t.elapsed().as_secs_f64() / 1e9;
        wait();
        let t = Instant::now();
        op_copy(&mut a, &b);
        r[2] = 2.0 * bytes / t.elapsed().as_secs_f64() / 1e9;
        wait();
        let t = Instant::now();
        op_triad(&mut a, &b, &c);
        r[3] = 3.0 * bytes / t.elapsed().as_secs_f64() / 1e9;
        black_box(&a);
        out.push(r);
    }
    out
}

/// Elements per array so that the combined working set is well beyond the last-level cache
/// while using at most ~40 % of the available RAM.
fn pick_elems(llc: u64, threads: usize, avail_bytes: u64, cap: u64) -> usize {
    let mut bytes = (llc * 4 / threads as u64).max(16 << 20).min(cap);
    while bytes > (2 << 20) && (threads as u64) * 3 * bytes > avail_bytes * 4 / 10 {
        bytes /= 2;
    }
    (bytes / 8).max(1 << 16) as usize
}

fn best(reps: &[[f64; 4]]) -> Bw {
    let m = |i: usize| reps.iter().map(|r| r[i]).fold(0.0, f64::max);
    Bw { read: m(0), write: m(1), copy: m(2), triad: m(3) }
}

// ───────────────────────── latency ─────────────────────────

#[repr(align(64))]
#[derive(Clone, Copy)]
struct Line {
    next: usize,
    _pad: [u64; 7],
}

/// Dependent-load latency (ns) over a random single-cycle chain through `bytes` of memory.
fn chase(bytes: u64, steps: usize, reps: usize) -> f64 {
    let n = ((bytes / 64) as usize).max(2);
    let mut rng = Rng::new(0xC0FFEE ^ bytes);
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    let mut lines = vec![Line { next: 0, _pad: [0; 7] }; n];
    for i in 0..n {
        lines[order[i]].next = order[(i + 1) % n];
    }
    drop(order);
    let mut idx = 0usize;
    for _ in 0..n.min(1_000_000) {
        idx = lines[idx].next; // warm-up: fault pages in, fill caches
    }
    let mut best = f64::MAX;
    for _ in 0..reps.max(1) {
        let t = Instant::now();
        for _ in 0..steps {
            idx = lines[idx].next;
        }
        best = best.min(t.elapsed().as_secs_f64() / steps as f64 * 1e9);
    }
    black_box(idx);
    best
}

pub fn run(ui: &Ui, cpu: &CpuInfo, mem: &MemInfo, prof: &Profile) -> MemResults {
    let threads = cpu.usable.max(1) as usize;
    let avail = mem.available_mb.max(256) * 1024 * 1024;
    let llc = cpu.llc_bytes();
    let reps = prof.mem_reps;

    // ---- single thread bandwidth ----------------------------------------------------------
    ui.progress(&format!("{}: 1 thread", ui.t("prog_mem_bw")));
    let n1 = pick_elems(llc, 1, avail, 256 << 20);
    let single = std::thread::scope(|sc| {
        sc.spawn(|| {
            hw::pin_to_cpu(cpu.best_cpu);
            best(&bw_worker(n1, reps, None))
        })
        .join()
        .expect("memory thread panicked")
    });

    // ---- all threads ------------------------------------------------------------------------
    let multi = if threads > 1 {
        ui.progress(&format!("{}: {} threads", ui.t("prog_mem_bw"), threads));
        let nt = pick_elems(llc, threads, avail, 128 << 20);
        let barrier = Barrier::new(threads);
        let per_thread: Vec<Vec<[f64; 4]>> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..threads)
                .map(|t| {
                    let barrier = &barrier;
                    let cpu_id = cpu.cpu_list.get(t).copied();
                    sc.spawn(move || {
                        if let Some(c) = cpu_id {
                            hw::pin_to_cpu(c);
                        }
                        bw_worker(nt, reps, Some(barrier))
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("memory thread panicked")).collect()
        });
        let per_rep: Vec<[f64; 4]> = (0..reps.max(1))
            .map(|r| {
                let mut s = [0.0; 4];
                for t in &per_thread {
                    for i in 0..4 {
                        s[i] += t[r][i];
                    }
                }
                s
            })
            .collect();
        best(&per_rep)
    } else {
        single.clone()
    };

    // ---- latency curve ----------------------------------------------------------------------
    let max_bytes = prof.lat_max_bytes.min(avail / 4).max(1 << 20);
    let mut sizes = Vec::new();
    let mut s = 4096u64;
    while s <= max_bytes {
        sizes.push(s);
        s *= 2;
    }
    let latency: Vec<LatPoint> = std::thread::scope(|sc| {
        sc.spawn(|| {
            hw::pin_to_cpu(cpu.best_cpu);
            sizes
                .iter()
                .enumerate()
                .map(|(i, &b)| {
                    ui.progress(&format!("{}: {} [{}/{}]", ui.t("prog_mem_lat"), bs_human(b), i + 1, sizes.len()));
                    LatPoint { bytes: b, ns: chase(b, prof.lat_steps, if prof.quick { 1 } else { 3 }) }
                })
                .collect()
        })
        .join()
        .expect("latency thread panicked")
    });
    ui.clear_progress();

    MemResults {
        array_mb: (n1 * 8 / (1 << 20)) as u32,
        threads: threads as u32,
        single,
        multi,
        l1_ns: latency.first().map(|p| p.ns).unwrap_or(0.0),
        dram_ns: latency.last().map(|p| p.ns).unwrap_or(0.0),
        latency,
    }
}

pub fn bs_human(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{} MiB", b >> 20)
    } else {
        format!("{} KiB", b >> 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chase_visits_a_single_cycle() {
        // a latency chain must be one cycle through every line, otherwise we would measure a cache
        let n = 1000usize;
        let mut rng = Rng::new(9);
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        let mut next = vec![0usize; n];
        for i in 0..n {
            next[order[i]] = order[(i + 1) % n];
        }
        let mut seen = vec![false; n];
        let mut idx = 0;
        for _ in 0..n {
            assert!(!seen[idx]);
            seen[idx] = true;
            idx = next[idx];
        }
        assert_eq!(idx, 0);
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn sizing_respects_memory_limit() {
        let e = pick_elems(256 << 20, 4, 1 << 30, 128 << 20);
        assert!(4 * 3 * e as u64 * 8 <= (1u64 << 30) * 4 / 10 + (4 << 20) * 12);
        assert!(e >= 1 << 16);
    }

    #[test]
    fn bandwidth_ops_work() {
        let r = bw_worker(1 << 18, 1, None);
        assert_eq!(r.len(), 1);
        assert!(r[0].iter().all(|x| *x > 0.0));
    }

    #[test]
    fn latency_is_positive() {
        assert!(chase(64 * 1024, 10_000, 1) > 0.0);
    }
}
