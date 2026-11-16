# Reading the Report

The report is printed in the terminal and saved as text and HTML. It has the same sections in both.

## Header

Program name and version, date, the mode (quick or full), and your `--label` if you gave one. A warning appears when the program is a debug build, because its numbers are much too low.

## System

What the program found: device model (where the system tells it), operating system and kernel, CPU (model, cores, threads, cache, features, virtualization support), memory (size, ECC where known), the tested disk (type, file system, size, free space), network adapters, and GPUs. Entries marked `?` or "unknown" are things the system does not tell.

## Scores

The 0-100 score of every category with a bar and a verdict word (excellent, good, limited, poor). See [Scores and Verdicts](Scores-and-Verdicts.md).

## Test sections

One section per test that ran, with the raw numbers: [CPU and Memory](CPU-and-Memory.md), [Disk Test](Disk-Test.md), [Network Test](Network-Test.md), [Video Encoding](Video-Encoding.md), [Ray Tracing and GPU](Ray-Tracing-and-GPU.md). The HTML report adds charts: the block-size sweep, the memory latency curve, and ray tracing bars with a preview picture of the rendered scene. The sustained write speed is shown as numbers.

## Findings

Short plain-language statements with a symbol:

| Symbol | Meaning |
| --- | --- |
| ✔ (ok) | Something good for the machine's job, for example "Hardware encoder works" |
| ⚠ (warn) | Something that may be a problem, for example "no virtualization extension" or "disk looks like an HDD" |
| ℹ (info) | A fact worth knowing, for example "no IOMMU groups: PCIe passthrough may not work" |

Findings are grouped by what they matter for (virtualization, NAS, video, AI, general). Examples of what the program says:

* The CPU has no VT-x / AMD-V, or you run inside a virtual machine without nested virtualization, so it cannot host virtual machines.
* Less than 8 GB of RAM is little for virtual machines.
* The disk is a RAM disk, or the test fell back to buffered I/O.
* The CPU results were noisy (more than 5 %): close other programs and run again.
* A sustained write dropped by more than 40 %.
* No hardware video encoder works, so transcoding will use the CPU.
* A GPU is a software renderer, or its matrix result did not match the CPU.
* The network was not tested, or only on loopback.

## Footer

A short reminder that the scores are provisional and that results should be compared only between runs with the same spec version.

## Seeing a result again

Every run is saved, so you can show it again later in any language:

```
syscheck report syscheck-results/<file>.json --lang tr --html report.html
```
