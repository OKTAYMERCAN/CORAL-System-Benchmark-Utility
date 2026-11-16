# Scores and Verdicts

The report turns raw measurements into scores from 0 to 100 so you can see at a glance what a machine is good for. **The scores are provisional.** The raw values are always stored in the result file, so scores can be recalculated later without running anything again, for example after results from many devices have been collected and the ranges can be tuned. Compare scores only between results made with the same program version, and use the raw values for serious comparisons.

## Verdicts

| Score | Verdict |
| --- | --- |
| 75 and above | Excellent |
| 55 to 75 | Good |
| 35 to 55 | Limited |
| below 35 | Poor |

## Step 1: components (log scale)

Each measurement becomes a component score between two anchors: the value that is worth 0 points and the value that is worth 100 points. The scale is **logarithmic**, so a machine that is twice as fast gets the same number of extra points whether it is slow or fast. Values below the lower anchor give 0 and above the upper anchor give 100.

| Component | Based on | 0 points | 100 points |
| --- | --- | --- | --- |
| `cpu_single` | Single-core geometric mean of the six kernels | 100 | 2000 |
| `cpu_multi` | Multi-core geometric mean | 200 | 40,000 |
| `cpu_fp` | Multi-core f32 matrix multiply (MFLOPS) | 500 | 400,000 |
| `crypto` | SHA-256 speed, hardware if present (MB/s) | 50 | 4,000 |
| `ram` | Installed RAM (GB) | 1 | 128 |
| `mem_bw` | Memory triad bandwidth, best of single and multi thread (GB/s) | 3 | 150 |
| `mem_lat` | DRAM latency (lower is better) | 300 ns | 60 ns |
| `disk_seq` | Geometric mean of 1 MB sequential read and write (MB/s) | 50 | 7,000 |
| `disk_iops` | 4K random read and write at queue depth 32 (IOPS) | 1,000 / 500 | 800,000 / 500,000 |
| `disk_qd1` | 4K random read, queue depth 1 (IOPS) | 100 | 20,000 |
| `streams` | Number of 4K video streams the disk can feed | 1 | 200 |
| `net` | Best TCP throughput, **real network only** (Mbit/s) | 50 | 25,000 |
| `cpu_encode` | CPU H.264 encoding speed (fps) | 5 | 300 |
| `hw_encode` | Real-time 1080p30 streams of the best hardware encoder | 1 | 40 |
| `rt_cpu` | CPU ray tracing, all threads (Mrays/s) | 2 | 1,500 |
| `gpu_fp32` | GPU FP32 throughput (GFLOPS) | 200 | 80,000 |
| `gpu_mem` | GPU memory bandwidth (GB/s) | 10 | 1,500 |
| `gpu_matmul` | GPU matrix multiply (GFLOPS), only when the result was correct | 100 | 40,000 |
| `gpu_rt` | GPU ray tracing (Mrays/s), only when the picture matched the CPU | 20 | 20,000 |
| `gpu_vram` | GPU memory size (GB), when known | 2 | 48 |

A component that was not measured (a test you skipped, a failed test, no GPU) is left out.

## Step 2: categories (weighted average)

Each category is a weighted average of components. If a component is missing, the remaining weights are scaled up so the category is still a fair 0-100 value. A category with no data at all is not shown.

| Category | Components and weights |
| --- | --- |
| **Virtualization host** | `cpu_multi` 25 %, `ram` 25 %, `disk_iops` 20 %, `cpu_single` 10 %, `mem_bw` 10 %, `disk_qd1` 10 %. ECC memory adds 3 points. If the CPU has no virtualization extension (and no KVM), the score is capped at 20 |
| **NAS** | `disk_seq` 25 %, `net` 25 %, `ram` 15 %, `crypto` 15 %, `cpu_multi` 10 %, `cpu_single` 10 % |
| **Media / video server** | `cpu_multi` 15 %, `cpu_encode` 25 %, `hw_encode` 25 %, `streams` 10 %, `cpu_single` 10 %, `mem_bw` 5 %, `ram` 5 %, `disk_seq` 5 % |
| **Game server** | `cpu_single` 40 %, `mem_lat` 15 %, `ram` 15 %, `disk_qd1` 15 %, `cpu_multi` 15 % |
| **AI / LLM** | With a usable GPU: `gpu_matmul` 35 %, `gpu_mem` 20 %, `gpu_vram` 15 %, `ram` 10 %, `cpu_fp` 10 %, `mem_bw` 10 %. Without one: `mem_bw` 40 %, `cpu_fp` 30 %, `ram` 30 % |
| **Graphics (GPU)** | `gpu_fp32` 40 %, `gpu_mem` 30 %, `gpu_matmul` 30 % |
| **Ray tracing** | `gpu_rt` only |
| **General** | `cpu_single` 30 %, `cpu_multi` 30 %, `ram` 15 %, `mem_bw` 10 %, `disk_seq` 7.5 %, `disk_qd1` 7.5 % |

Notes:

* The network component counts only for a test against a **real server**. A loopback test is never scored.
* Software renderers (for example `llvmpipe`) are never used for GPU scores.
* The CPU ray tracing result (`rt_cpu`) is stored and shown, but it is not part of a category: the Ray tracing score is about the GPU.
* A machine without a GPU test result simply has no graphics and ray tracing score. It is not scored as zero.

## Why log scales and provisional ranges?

Hardware speeds span a huge range (a single-board computer to a server), so a log scale keeps both ends readable. The anchors are educated first values. When many results exist they will be adjusted, and the `spec` version in each result tells you whether two results measured the same workload (see [Comparing Devices](Comparing-Devices.md)).
