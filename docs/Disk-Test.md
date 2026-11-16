# Disk Test

The disk test measures the disk (or network share) that holds the folder you give with `--path`. By default that is the current folder, so run the program from the disk you want to test, or use `--path`.

```
syscheck --only disk --path /mnt/nas
```

## How it works

* The test creates one file in the folder (name `.syscheck_<number>.tmp`) and removes it at the end. The file is 512 MB by default (128 MB with `--quick`). If the disk has little free space the size is reduced so the test never uses more than about 40 % of the free space. Use `--disk-size-mb` to choose the size yourself.
* **Unbuffered I/O** is used (`O_DIRECT` on Linux and Android, `F_NOCACHE` on macOS, `FILE_FLAG_NO_BUFFERING` on Windows), so the operating system's cache does not inflate the numbers.
* **Queue depth** (how many requests are in flight at the same time) is created with that many threads, each with one request outstanding.
* If unbuffered I/O is not available (for example a RAM disk or some network file systems), the test falls back to buffered I/O and the report says so with a warning.
* `--disk-sync` makes every write synchronous (`O_DSYNC` / write-through). Use it to see how the disk behaves for databases and NFS-style workloads.

## Block-size sweep

Sequential read and write speed for 15 block sizes from 0.5 KB to 8 MB (each size twice as large as the one before), at the chosen queue depths. The report draws the curve. It shows where throughput stops growing, which tells you the block size at which the disk reaches its full speed.

## Scenarios

| Scenario | Block size | Queue depth | Why it matters |
| --- | --- | --- | --- |
| 4K random read | 4 KB | 1 | Responsiveness: operating system boot, small databases. Reports latency percentiles p50, p99 and p99.9 |
| 4K random read | 4 KB | 32 | Peak read IOPS: many virtual machines, busy servers |
| 4K random write | 4 KB | 32 | Peak write IOPS |
| Mixed 8K | 8 KB, 70 % read / 30 % write | 32 | Typical database and virtual machine mix |
| 1 MB sequential read | 1 MB | 4 | Copying and streaming large files |
| 1 MB sequential write | 1 MB | 4 | Backups, ingesting large files |

### Parallel streams ("4K video capacity")

Eight readers each read their own 256 KB stream at the same time. The report shows the total speed, the slowest single stream, and an estimate of how many 4K video streams the disk can feed, counted as the total speed divided by 10 MB/s per stream. This is the number used by the media server score.

## Small files

Creates, reads and deletes many small files (4 KB each; 2000 files, or 500 with `--quick`) and reports files per second. File systems and network shares with slow metadata (network drives, some USB disks) show here.

## Sustained write (optional)

```
syscheck --only disk --sustained 60
```

Writes continuously for the given number of seconds with four writers and records the speed for every second. The report shows the start speed, the end speed, the minimum, the average and how much the speed dropped. Many SSDs are fast at first and slow down when their fast cache is full; a drop of more than 40 % is flagged. The test file is larger for this test (up to a quarter of the free space, at most 8 GB) so that the cache really fills.

## Disk information

The report also shows the file system, the disk type (NVMe, SSD, HDD, eMMC, SD card, virtio, RAM disk or unknown), model, size and free space, and the block sizes. Detection of the disk type is based on information the operating system gives (on Linux, `/sys`); on Windows it is not available yet and shows "unknown".

## Tips

* Test the disk you care about: for a NAS, point `--path` at the NAS volume; for a server, at the data disk.
* Do not test a disk that is almost full. Free space helps SSDs keep their speed.
* Run the test twice if you suspect other activity on the disk. Background reads and writes lower the numbers.
* Hitting Ctrl-C during the disk test can leave the `.syscheck_<number>.tmp` file behind. Delete it by hand.
