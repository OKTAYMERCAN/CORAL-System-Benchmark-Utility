# What Is Measured

`syscheck` runs seven groups of tests. Each one is a fixed workload: the same inputs and the same amount of work on every machine, so the speed you measure is comparable between devices.

| Group | `--only` name | Measures | Details |
| --- | --- | --- | --- |
| CPU | `cpu` | Six kernels, single core and all cores, hardware acceleration, hybrid cores | [CPU and Memory](CPU-and-Memory.md) |
| Memory | `mem` | Sequential bandwidth and access latency from 4 KB to 256 MB | [CPU and Memory](CPU-and-Memory.md) |
| Disk | `disk` | Block-size sweep, 4K random I/O, 1 MB sequential, parallel streams, small files | [Disk Test](Disk-Test.md) |
| Network | `net` | TCP throughput, round-trip time, message-size sweep | [Network Test](Network-Test.md) |
| Video encoding | `video` | CPU and hardware video encoders through `ffmpeg` | [Video Encoding](Video-Encoding.md) |
| Ray tracing | `rt` | A fixed path-traced scene on the CPU | [Ray Tracing and GPU](Ray-Tracing-and-GPU.md) |
| GPU | `gpu` | Compute throughput, memory bandwidth, matrix multiply, and the same ray tracing scene on the GPU | [Ray Tracing and GPU](Ray-Tracing-and-GPU.md) |

If you do not use `--only`, everything runs except the network test (which needs a second machine or an explicit `--net-loopback`).

## How long does it take?

| Mode | Time | Notes |
| --- | --- | --- |
| `--quick` | about 30 seconds | Shorter measurements, one queue depth, smaller test file, no HEVC or parallel video test |
| Full (default) | about 3 minutes | Measured on a small 2-core cloud VM. Fast machines finish earlier, slow disks and slow single-board computers take longer |

The disk test and the video test take the largest share of the time.

## What the settings change

| Setting | Quick | Full |
| --- | --- | --- |
| Measurement time per CPU kernel | 0.35 s, 1 repetition | 0.6 s, 3 repetitions |
| Memory bandwidth repetitions | 2 | 5 |
| Latency curve | up to 64 MB | up to 256 MB |
| Disk test file | 128 MB | 512 MB |
| Disk queue depths | 4 | 1, 4 and 32 |
| Small files | 500 | 2000 |
| Network time per measurement | 2 s | 5 s |
| Ray tracing time | 0.6 s | 1.5 s |
| Video frames per run | 90 | 240 |
| HEVC and 4-stream video test | off | on |

`--disk-size-mb`, `--qd` and `--sustained` override the disk settings in either mode.

## Fairness rules

* Single-core tests are pinned to the fastest core, and multi-core tests use one pinned thread per usable CPU (where the system allows pinning).
* Memory tests use arrays far larger than the last-level cache, but never more than about 40 % of the free RAM.
* The disk test uses unbuffered I/O, so the operating system's cache does not inflate the numbers.
* Math in the workloads uses only operations with exactly defined results, so a CPU workload does the same work on every platform.
* The CPU tests report how noisy they were. Above 5 % noise the report warns you to close other programs and run again.
