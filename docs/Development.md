# Development

## Build and test

```
cargo build --release
cargo test --release
```

Tests run in release mode on purpose: some of them check the exact output of the CPU workloads and run the real benchmark kernels. CI runs the tests on Linux, Windows and macOS and cross-builds the other targets (`.github/workflows/build.yml`). A tag such as `v1.0.0` builds all release packages and publishes a GitHub Release (`.github/workflows/release.yml`).

## Source layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Command line (clap), which tests run, saving results |
| `src/hw.rs` | Hardware and operating system detection, CPU pinning |
| `src/winsys.rs` | The few Windows API calls used on Windows only |
| `src/bench/cpu.rs` | The six CPU kernels and acceleration probes |
| `src/bench/mem.rs` | Memory bandwidth and latency |
| `src/bench/disk.rs` | Disk test |
| `src/bench/net.rs` | Network client and the `serve` server |
| `src/bench/video.rs` | `ffmpeg` based encoder tests |
| `src/bench/rt.rs` | CPU path tracer, reference checksum, preview picture |
| `src/bench/gpu.rs`, `gpu_worker.rs` | GPU tests: the orchestrator and the child process that talks to the driver |
| `src/score.rs` | Components, categories, findings |
| `src/record.rs` | The result record, JSON and CSV |
| `src/render.rs`, `src/html.rs` | Text and HTML reports |
| `src/i18n.rs` | English and Turkish texts |
| `src/ui.rs`, `src/util.rs` | Terminal output, helpers (random numbers, statistics, log scale) |

## Rules

### Changing a workload means a new spec

Results are comparable only within one spec (`SPEC` in `src/record.rs`). If you change what a test computes or how much work it does, bump the spec version. Changing only the scoring constants in `score.rs` does not need a bump, because raw values are stored and scores can be recomputed.

### Keep CPU workloads exact

Use only operations with exactly defined results (add, subtract, multiply, divide, square root) and integer random number generators in the workloads, so the result is the same on every platform. Do not use functions such as `sin`, `exp` or `pow` in a workload: they can differ between platforms. The ray tracing test has a reference checksum (`REF_CHECKSUM` in `rt.rs`) that must stay equal on all platforms.

### Translations

Every text is a row in `TABLE` in `src/i18n.rs`: key, English, Turkish, with `{0}`, `{1}` placeholders. The tests check that no key is missing a language, that both languages have the same placeholders, and that every key used in the source exists in the table. Findings are stored as keys, not sentences. To add a language, add the language to the parser in `i18n.rs` and a third column.

### Privacy

Do not store host names, user names, addresses or paths in the record. The only free text allowed is `--label` and `--notes`.

### Safety

* Anything that talks to a graphics driver belongs in the GPU worker process, not in the main process.
* Tests must clean up their temporary files, and fail soft: a failing test is reported and the run goes on.
* The network server must keep its limits (test length, message size, connections).

### Dependencies

Use only crates under permissive licenses (MIT, Apache-2.0, BSD and similar). Current dependencies: `clap`, `serde`, `serde_json`, `sha2`, `flate2`, `libc` (Unix), `windows-sys` (Windows), and `wgpu`, `pollster`, `bytemuck` for the GPU tests (optional feature `gpu`).

## Contributing

Issues and pull requests are welcome. Please run `cargo test --release` before sending a change, keep changes focused, and explain in the description what you tested and on which system. Edit the documentation in the `docs/` folder: it is copied to the Wiki automatically when it changes on the `main` branch.
