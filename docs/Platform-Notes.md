# Platform Notes

## Status

| System | Builds in CI | Run and tested by the author |
| --- | --- | --- |
| Linux x86-64 | yes | **yes** |
| Linux ARM64 | yes | no |
| Linux RISC-V (64-bit) and ARMv7 | yes (cross build) | no |
| Windows x86-64 | yes (with tests) | no |
| Windows ARM64 | yes | no |
| macOS (Apple Silicon) | yes (with tests) | no |
| Android ARM64 | yes (cross build) | no |

"Builds in CI" means the GitHub Actions workflow compiles the program for that target. It does not prove that every test behaves correctly on real hardware. Please open an issue when something looks wrong.

## Linux

The best supported system. Disk type, device model, IOMMU groups, hybrid core groups, cache sizes, GPU memory and network adapters are read from `/proc` and `/sys`. Direct I/O uses `O_DIRECT`. Inside containers the program works, but GPU and some hardware information may be missing, and the report notes that it runs in a container.

## Windows

* CPU pinning and the CPU count cover the first processor group, which is up to 64 logical CPUs.
* Disk type (SSD or HDD), the split into performance and efficiency cores, GPU memory size and network adapter details are not detected yet and show as unknown. The measurements themselves work.
* The disk test uses unbuffered I/O (`FILE_FLAG_NO_BUFFERING`).
* The program switches the console to UTF-8 and turns on colors. If your console cannot show colors, the program turns them off (or use `--no-color`).
* For the video test, put `ffmpeg.exe` in `PATH`.
* Virtualization support is detected with the CPU's own information; when Hyper-V is active it hides VT-x from the host, so the program asks the firmware flag instead.

## macOS

Apple Silicon is built in CI. The disk test uses `F_NOCACHE`, GPU tests use Metal, and VideoToolbox is the hardware encoder. Downloaded programs are not signed: see [Installation](Installation.md) for the quarantine command. Intel Macs can build from source.

## Android

Build inside [Termux](https://termux.dev) with `pkg install rust` and `cargo build --release`. Phones throttle when hot, so run on a cool device and keep it plugged in. The program tries the MediaCodec hardware encoder through `ffmpeg`; whether it works depends on how your `ffmpeg` was built. GPU tests use Vulkan. Some Android versions restrict access to hardware information; the report shows unknown values then.

## ARM and RISC-V boards

Build on the board (`cargo build --release`) or cross-build as `.github/workflows/build.yml` does. A build on a small board can take a long time; cross-building is faster. Feature detection (SHA, NEON, atomics, dot product on ARM; extensions on RISC-V) is read from the system.

## Virtual machines

Results from inside a virtual machine are valid for that virtual machine, not for the host. The report says when it detects a hypervisor. If you want to know whether a machine can **host** virtual machines, check the virtualization finding: it needs VT-x / AMD-V (or KVM on ARM / RISC-V) and, for nested virtualization, the host must pass it through.
