# CORAL System Benchmark Utility

CORAL System Benchmark Utility (command name: `syscheck`) is a native program written in Rust that measures a computer and answers one question: **what is this machine good for?**

It rates the machine as a virtualization host, a NAS, a media / video server, a game server, a local AI (LLM) machine, a graphics (GPU) machine and a ray tracing machine. Every test is a fixed, deterministic workload, so the numbers from a PC, a phone and a single-board computer can be compared with each other. Results are saved as JSON, text and HTML, and can be merged into one CSV file.

## Quick start

1. Download the program for your system from the [Releases page](https://github.com/OKTAYMERCAN/CORAL-System-Benchmark-Utility/releases) (or [build it yourself](Installation.md)).
2. Run it in a terminal:

```
syscheck --quick
```

3. Pick a language when asked (1 = English, 2 = Türkçe). A quick run takes about 30 seconds and prints a report. The full run (`syscheck`) is more precise and takes a few minutes.
4. Open the `.html` report in the `syscheck-results` folder for charts.

The program does not install anything and does not need administrator rights. It writes a temporary file for the disk test and removes it afterwards.

## Pages

| Page | What is in it |
| --- | --- |
| [Installation](Installation.md) | Downloads, building from source on every system, optional tools |
| [Command-Line Reference](Command-Line-Reference.md) | Every command and option, with examples |
| [What Is Measured](What-Is-Measured.md) | Overview of all tests and how long they take |
| [CPU and Memory](CPU-and-Memory.md) | The CPU kernels, memory bandwidth and latency |
| [Disk Test](Disk-Test.md) | Block-size sweep, scenarios, small files, sustained write |
| [Network Test](Network-Test.md) | How to run a real network test between two machines |
| [Video Encoding](Video-Encoding.md) | CPU and hardware encoders, real-time stream count |
| [Ray Tracing and GPU](Ray-Tracing-and-GPU.md) | The path tracer and the GPU compute tests |
| [Scores and Verdicts](Scores-and-Verdicts.md) | How the 0-100 scores are calculated |
| [Reading the Report](Reading-the-Report.md) | What every section and finding means |
| [Result Files](Result-Files.md) | JSON, text, HTML, CSV, and what is (not) stored |
| [Comparing Devices](Comparing-Devices.md) | Collecting results from many devices |
| [Platform Notes](Platform-Notes.md) | Linux, Windows, macOS, Android, ARM, RISC-V |
| [Troubleshooting and FAQ](Troubleshooting-and-FAQ.md) | Common problems and questions |
| [Development](Development.md) | Source layout, tests, rules for changing a workload |

## Good to know

* **Always use a release build.** A debug build is 10 to 50 times slower and the program warns you about it.
* **Close other heavy programs** while it runs. The report warns when the CPU results are noisy.
* **Scores are provisional.** The raw values are always stored, so scores can be recalculated later when results from many devices are collected.
* **Platform status.** The author has run the program on Linux x86-64 only. Windows, macOS, Linux ARM64 / RISC-V and Android builds compile in CI but need testing on real hardware. See [Platform Notes](Platform-Notes.md).
