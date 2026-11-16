# Result Files

Unless you use `--no-save`, every run is saved in the `syscheck-results` folder (or the folder from `--out-dir`) as three files with the same name:

```
syscheck-<date>_<time>-<id>.json     complete record (the source of truth)
syscheck-<date>_<time>-<id>.txt      the report as text
syscheck-<date>_<time>-<id>.html     the report with charts (one self-contained file)
```

The HTML file has no external parts and can be opened offline or sent by e-mail.

## JSON record

One JSON document per run. Its top-level fields:

| Field | Content |
| --- | --- |
| `schema` | Version of the record layout (currently 1) |
| `tool` | Program name, version, **spec** version, build profile, compiler version and target |
| `created_utc`, `run_id` | When the run was made (UTC) and a random id |
| `mode` | `quick` or `full` |
| `notes` | The text of `--notes`, if any |
| `system` | The detected hardware: label, model, OS, CPU, memory, disk, network adapters, GPUs |
| `results` | Raw numbers of every test that ran: `cpu`, `mem`, `disk`, `net`, `video`, `rt`, `gpu` |
| `scores` | `categories`, `components` and `general` |
| `findings` | The findings as language-independent keys with arguments (translated when the report is shown) |

Because findings are keys, not sentences, the same file can be shown in English or Türkçe. The file is meant for programs too: all numbers are in plain units (MB/s, IOPS, microseconds, GFLOPS, Mrays/s).

## CSV

```
syscheck --csv all.csv                        # append one row after a run
syscheck export --dir syscheck-results --out all.csv   # build one CSV from all saved JSON files
```

The CSV has one row per run and 67 flat columns, for spreadsheets and databases:

| Group | Columns |
| --- | --- |
| Run | `schema`, `created_utc`, `run_id`, `mode`, `tool_version`, `spec`, `rustc`, `target`, `build_profile`, `label`, `notes`, `findings_warn` |
| System | `model`, `os`, `kernel`, `arch`, `cpu_model`, `cpu_usable`, `cpu_physical`, `ram_mb` |
| CPU | `cpu_single`, `cpu_multi`, `cpu_scaling`, `sha_soft_mbs`, `sha_hw_mbs`, `simd_speedup` |
| Memory | `mem_single_triad_gbs`, `mem_multi_triad_gbs`, `mem_multi_read_gbs`, `mem_l1_ns`, `mem_dram_ns` |
| Disk | `disk_kind`, `disk_direct`, `disk_seq_read_mbs`, `disk_seq_write_mbs`, `disk_rand4k_read_qd1_iops`, `disk_rand4k_read_qd32_iops`, `disk_rand4k_write_qd32_iops`, `disk_rand4k_read_qd1_p99_us`, `disk_uhd_streams`, `disk_small_create_per_s` |
| Network | `net_mode`, `net_link_mbps`, `net_up4_mbps`, `net_down4_mbps`, `net_rtt_p50_us` |
| Video | `video_x264_fps`, `video_hw_encoder`, `video_hw_fps`, `video_hw_streams` |
| Ray tracing | `cpu_pt_single_mrays`, `cpu_pt_multi_mrays`, `gpu_rt_mrays` |
| GPU | `gpu_name`, `gpu_backend`, `gpu_type`, `gpu_fp32_gflops`, `gpu_mem_gbs`, `gpu_matmul_gflops` |
| Scores | `score_virt`, `score_nas`, `score_video`, `score_game`, `score_ai`, `score_general`, `score_gfx`, `score_rt` |

Numbers use a dot as the decimal separator. Text with commas or quotes is quoted correctly.

## Privacy: what is stored

The record contains hardware and measurements. It does **not** contain the computer's host name, your user name, IP addresses, MAC addresses, serial numbers or file paths.

It does contain things you type yourself: `--label` and `--notes`. Do not put personal information there if you plan to share the file. The hardware itself (CPU model, disk model, GPU name, network adapter names) is stored, since that is what makes a result meaningful.

The program makes no network connection unless you ask for the [network test](Network-Test.md). It never uploads results anywhere.
