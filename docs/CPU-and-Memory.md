# CPU and Memory

## CPU test

Six fixed kernels run first on **one core** (pinned to the fastest core) and then on **all usable cores** (one pinned thread per CPU). The kernels are deterministic: the same inputs and the same work on every machine.

| Kernel | What it does | Unit |
| --- | --- | --- |
| `int_alu` | Integer arithmetic | Mops/s |
| `fp_f64` | 64-bit floating point | MFLOPS |
| `matmul_f32` | 32-bit floating point matrix multiply | MFLOPS |
| `sort_u32` | Sorting 32-bit integers | Melem/s |
| `sha256_soft` | SHA-256 hashing in software | MB/s |
| `deflate` | Compression | MB/s |

The single-core and multi-core scores are the geometric mean of the six kernels. The report also shows the **scaling** (how much faster all cores are than one core) and the **noise**, the spread between repetitions.

### Hardware acceleration checks

* **SHA:** the same hashing with the CPU's SHA instructions (when present) against the software version. A large speed-up is good for checksums and encryption on a NAS.
* **SIMD:** on x86-64, the matrix multiply with AVX2 + FMA against the baseline code. The speed-up shows how much a CPU helps with AI inference.
* **Hybrid and big.LITTLE CPUs:** the cores are split into groups (for example performance and efficiency cores) and each group is tested on its own.

### Detected hardware

The report lists the CPU model, core and thread counts, cache sizes, maximum frequency (where the system tells it), feature flags (for example `avx2`, `aes`, `sha_ni`, `vmx`), virtualization support (VT-x / AMD-V, or KVM on ARM and RISC-V), whether you are running inside a virtual machine, and IOMMU groups on Linux.

## Memory test

### Bandwidth

Four sequential operations on large arrays, each run on one thread and then on all threads at once:

| Operation | Meaning |
| --- | --- |
| Read | Read the array |
| Write | Fill the array |
| Copy | Copy one array to another (bytes read plus bytes written are counted) |
| Triad | `a = b + 3 * c`, the classic memory-system stress |

Speeds are in decimal GB/s (1 GB = 10^9 bytes). The best of several repetitions is used. Each array is sized relative to the last-level cache so it cannot fit in it (at least 16 MB, at most 256 MB for one thread and 128 MB per thread when all threads run), and the three arrays of all threads together never use more than about 40 % of the free RAM.

### Latency curve

A pointer-chasing test: every step must wait for the previous load, so the time per step is the real access latency. It runs for working sets from 4 KB to 256 MB (64 MB with `--quick`), doubling each time. The curve shows steps where the working set leaves L1, then L2, then L3, and finally reaches RAM. The report prints the L1 latency (smallest size) and the DRAM latency (largest size, including the cost of address translation).

Low DRAM latency matters most for game servers and databases. High bandwidth matters most for AI inference and virtual machines.
