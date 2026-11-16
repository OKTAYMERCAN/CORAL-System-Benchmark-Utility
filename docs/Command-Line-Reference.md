# Command-Line Reference

```
syscheck [OPTIONS] [COMMAND]
```

With no command, `syscheck` runs the benchmark (the same as `syscheck run`).

## Commands

| Command | What it does |
| --- | --- |
| `run` | Run the benchmark. This is the default. |
| `serve` | Start the network test server on this machine, see [Network Test](Network-Test.md). |
| `net` | Run only the network test against a `syscheck serve` instance. Needs `--server`. |
| `report` | Show a saved JSON result again, in any language, optionally as HTML. |
| `export` | Merge all saved JSON results of a folder into one CSV file. |
| `help` | Print help for the program or for one command. |

`-h` / `--help` and `-V` / `--version` work everywhere.

## Options of `run` (and the default run)

| Option | Meaning |
| --- | --- |
| `--lang <en\|tr>` | Language of the report: `en` (English) or `tr` (Türkçe). If omitted, the program asks. |
| `--quick` | Shorter run with lower precision (about 30 seconds instead of a few minutes). |
| `--only <list>` | Run only some tests. Comma-separated: `cpu`, `mem`, `disk`, `net`, `video`, `rt`, `gpu`. |
| `--path <dir>` | Folder for the disk test file. Put it on the disk you want to test. Default: the current folder. |
| `--disk-size-mb <MB>` | Size of the disk test file. Default 512 MB (128 MB with `--quick`). |
| `--qd <list>` | Disk queue depths, for example `1,4,32`. Default 1,4,32 (only 4 with `--quick`). |
| `--sustained <seconds>` | Also run a sustained write test of this length (see [Disk Test](Disk-Test.md)). |
| `--disk-sync` | Use synchronous writes (`O_DSYNC` / write-through) in the disk test. |
| `--net-server <ip[:port]>` | Run the network test against this server. |
| `--net-loopback` | Run the network test against a local server. Measures this machine's TCP stack, not a network. |
| `--out-dir <dir>` | Where to save the result files. Default `syscheck-results`. |
| `--no-save` | Do not write any files. |
| `--csv <file>` | Also append a flat result row to this CSV file. |
| `--label <text>` | Your own name for this device, for example `"Pixel 8"`. Stored in the result. |
| `--notes <text>` | Free text stored in the result. |
| `--no-color` | Plain output without colors. The `NO_COLOR` environment variable does the same. |

The network test is **not** part of the default run. It runs only with `--net-server` or `--net-loopback`.

## `serve`

```
syscheck serve [--port <port>]
```

Starts the test server. Default port `5201`. The server has no authentication: use it only on networks you trust.

## `net`

```
syscheck net --server <ip[:port]> [other run options]
```

Runs only the network test against the given server.

## `report`

```
syscheck report <file.json> [--lang <en|tr>] [--html <report.html>]
```

Prints a saved result again. The result stores language-independent data, so you can view an English result in Turkish and the other way round. `--html` also writes an HTML report.

## `export`

```
syscheck export [--dir <folder>] [--out <file.csv>]
```

Reads every `*.json` result in the folder (default `syscheck-results`) and writes one CSV file (default `syscheck-all.csv`), oldest first. See [Result Files](Result-Files.md) for the columns.

## Examples

```
syscheck --quick --lang en                     # fast check, English
syscheck --label "Home NAS" --csv all.csv      # full run, also append a row to all.csv
syscheck --path /mnt/nas --only disk --qd 1,4,32 --sustained 60
syscheck --only video,rt,gpu                   # only encoding, ray tracing and GPU
syscheck --only cpu,mem --no-save              # only CPU and memory, write nothing
syscheck report result.json --lang tr --html report.html
syscheck export --dir syscheck-results --out all.csv
```

## Errors and exit codes

When the program cannot read a file you gave it (for example `syscheck report missing.json`), it prints a message starting with `syscheck:` and exits with code 1. A test that fails inside a normal run (an unreachable network server, a driver problem in the GPU test, an unwritable disk folder) does not stop the run: the report shows what failed, the other results are still saved, and the exit code stays 0.
