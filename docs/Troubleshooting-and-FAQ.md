# Troubleshooting and FAQ

## The numbers look far too low

* Is it a **debug build**? The report warns about it. Build with `cargo build --release`.
* Is something else running? The report warns when CPU noise is above 5 %. Close other programs and run again.
* Is a laptop on battery or a phone hot? Use the performance power mode and let the device cool down.
* Is the disk test on the disk you wanted? Use `--path`. A RAM disk, a USB stick and a network share all look very different.

## The disk results are too good to be true

The test uses unbuffered I/O. If the report says the test fell back to **buffered** I/O (for example on tmpfs or some network file systems), the operating system's cache inflates the numbers and the report warns about that. Test a real disk with `--path`.

## The video test says `ffmpeg` was not found

Install `ffmpeg` and make sure it is in your `PATH` (`ffmpeg -version` must work in the same terminal). Without it the video test is skipped; everything else works.

## A hardware encoder shows "not working"

The program really tries every hardware encoder. Common reasons: no such hardware, missing or old driver, a container or virtual machine without access to the GPU, or an `ffmpeg` build without that encoder. The CPU encoder is still tested.

## There is no GPU result

* No graphics driver, or no Vulkan / Metal / DirectX 12 support: install the driver.
* Running inside a container or virtual machine without GPU access.
* A software renderer is shown, but not scored by design.
* The build was made with `--no-default-features`, which leaves the GPU tests out.
* To see the driver's error messages, set `SYSCHECK_GPU_DEBUG=1` and run `syscheck --only gpu`.

A crash in the GPU driver only fails the GPU test; the other results are saved normally.

## The network test says it needs a server

The network test only runs with `--net-server <ip>` (a real test, with `syscheck serve` on the other machine) or `--net-loopback`. See [Network Test](Network-Test.md). If the connection fails, check the address, the port (5201 by default) and the firewall of the machine that runs `syscheck serve`.

## A `.syscheck_*.tmp` file is left behind

Ctrl-C during the disk test can leave the test file in the test folder. Delete it. A finished run removes it by itself.

## Questions

**Does it send my results anywhere?** No. The program makes no network connection except the network test you start yourself. See [Result Files](Result-Files.md).

**Does it change anything on my system?** No. It writes the result files and a temporary disk test file, and removes the temporary file afterwards. It needs no administrator rights.

**Can I run it on a server without a screen?** Yes. It is a terminal program. Use `--lang en --no-color` for clean logs and read the `.txt` or `.json` result afterwards.

**Why is there no single score?** The score depends on the job. A machine can be an excellent NAS and a poor game server. The *General* score is a rough overall number, but the category scores are the useful part.

**Why are scores "provisional"?** The 0-100 ranges are first estimates. They will be improved once results of many devices exist. Raw values never change.

**Can I compare an English result with a Turkish one?** Yes. Results store language-independent data. Use `syscheck report <file> --lang tr` to see any result in either language.

**What does `spec` mean?** The version of the measuring method. Compare results only when it is the same.

**Is it the same as other benchmark programs?** No. It is an independent program with its own workloads, written from scratch, so its numbers cannot be compared with the numbers of other benchmark programs.

**Which license is it under?** GPL-3.0, see the `LICENSE` file in the repository.

## Reporting a problem

Open an issue in the repository and include: the operating system and version, the processor architecture, the program version (`syscheck --version`), what you ran and what happened. Attaching the `.json` result file (after checking it for text you do not want to share) helps a lot.
