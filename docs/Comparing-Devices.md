# Comparing Devices

The program is built so that results from very different devices can be put side by side: a desktop PC, a laptop, a server, a phone, a single-board computer, an ARM or RISC-V board.

## The idea

1. Run `syscheck` on each device. Give each one a clear `--label`, for example `--label "Pixel 8"`.
2. Copy the `.json` files to one folder.
3. Merge them into one table:

```
syscheck export --dir all-results --out all.csv
```

4. Open `all.csv` in a spreadsheet, or load it into a database, and sort, filter and chart.

Alternatively, append a row after every run with `--csv all.csv` (the file gets a header when it is created). If the file already exists with a different column layout (for example from an older program version), the program refuses to append and asks for another file, so columns never get mixed up.

## What makes results comparable

* **The same workload everywhere.** Every test is a fixed workload with fixed input, and the CPU math is exactly defined, so a machine does the same work whatever its processor architecture is. Rates are "work per second", so a slow board and a fast server both finish in reasonable time.
* **A spec version.** Each result carries a `spec` value (currently `syscheck-spec-1`). It changes only when a workload is changed in a way that makes old and new numbers incomparable. **Compare results only when the `spec` values are equal.** The CSV has a `spec` column for filtering.
* **Raw values, not just scores.** Scores are provisional and may be recalculated later. The raw columns (`cpu_single`, `disk_rand4k_read_qd32_iops`, `gpu_fp32_gflops` and so on) do not change.
* **A record of how the program was built.** `tool_version`, `rustc`, `target` and `build_profile` are stored so you can spot a debug build or a different build.

## Make comparisons fair

* Use the same mode (`quick` or `full`) for all devices. Quick results are less precise.
* Test the same kind of disk target. A USB stick and an NVMe disk are different devices even in the same computer, so use `--path` deliberately and note it with `--notes`.
* Check `mode`, `disk_direct` and `build_profile` columns: a buffered disk test (`disk_direct` false) or a debug build should not be compared with proper results.
* Run on mains power with the performance power profile for laptops and phones, and let the device cool down between runs. Phones and small boards slow down when hot.
* Do not compare a loopback network test with a real one. The `net_mode` column tells them apart.
* Compare ray tracing results only when the CPU checksum matched. A warning in the findings shows when it did not.

## Cross-architecture notes

The CPU kernels run on x86-64, ARM64 and RISC-V. Some features exist on one architecture only: AVX2 + FMA (x86-64), hardware SHA (x86-64 and ARM64 where present). The report states which acceleration was used, and the raw SHA speed is stored for both the software and the hardware version so you can compare like with like.
