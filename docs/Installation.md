# Installation

## Download a ready-made build

Go to the [Releases page](https://github.com/OKTAYMERCAN/CORAL-System-Benchmark-Utility/releases) and download the archive for your system:

| System | File |
| --- | --- |
| Linux x86-64 | `syscheck-<version>-linux-x86_64.tar.gz` |
| Linux ARM64 | `syscheck-<version>-linux-aarch64.tar.gz` |
| Windows x86-64 | `syscheck-<version>-windows-x86_64.zip` |
| Windows ARM64 | `syscheck-<version>-windows-arm64.zip` |
| macOS (Apple Silicon) | `syscheck-<version>-macos-arm64.tar.gz` |

Unpack it and run `syscheck` (`syscheck.exe` on Windows) from a terminal. Check the download against `SHA256SUMS` if you like:

```
sha256sum -c SHA256SUMS --ignore-missing
```

* **macOS:** the program is not signed. If macOS refuses to open it, run `xattr -d com.apple.quarantine syscheck` once.
* **Windows:** SmartScreen may warn about an unknown publisher. Choose "More info" and "Run anyway", or build from source.
* **Other systems** (Linux RISC-V / ARMv7, Android, Intel Mac): build from source, see below.

## Build from source

You need the Rust toolchain. Install it from <https://rustup.rs>: it gives you `rustc` (the compiler) and `cargo` (the build tool and package manager, comparable to CMake plus a package manager). The same command works on every system:

```
git clone https://github.com/OKTAYMERCAN/CORAL-System-Benchmark-Utility
cd CORAL-System-Benchmark-Utility
cargo build --release
```

The program is `target/release/syscheck` (`target\release\syscheck.exe` on Windows). Run the tests with `cargo test --release`.

What else each system needs before `cargo build`:

| System | Needs |
| --- | --- |
| Linux | A C compiler / linker (for example `build-essential` on Debian and Ubuntu) |
| Windows | "Visual Studio Build Tools" with the "Desktop development with C++" workload (the MSVC toolchain) |
| macOS | Command line tools: `xcode-select --install` |
| Android | Install [Termux](https://termux.dev), then `pkg install rust` and build inside Termux |
| Linux RISC-V / ARM boards | Build on the board itself, or cross-build with [`cross`](https://github.com/cross-rs/cross) as the CI workflow does |

Building the first time downloads and compiles the dependencies and takes a few minutes. Later builds are fast.

### Build without the GPU tests

```
cargo build --release --no-default-features
```

This leaves out the GPU and GPU ray tracing tests and their graphics dependencies. The rest of the program is unchanged.

## Optional tools and drivers

| Tool | Used for | Without it |
| --- | --- | --- |
| `ffmpeg` in `PATH` (`ffmpeg.exe` on Windows) | The [video encoding test](Video-Encoding.md) | That test is skipped and the report says so |
| A graphics driver with Vulkan (Linux, Windows, Android), Metal (macOS) or DirectX 12 (Windows) | The [GPU tests](Ray-Tracing-and-GPU.md) | The GPU tests are skipped; a software renderer is shown but not scored |

`ffmpeg` is started as a separate program. Nothing of it is built into `syscheck`.

## Update and uninstall

The program is a single file. To update, replace it with the new one. To uninstall, delete it, and delete the `syscheck-results` folder if you no longer need the saved results.
