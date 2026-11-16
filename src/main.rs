//! SysCheck - a portable system benchmark and suitability analyzer
//! (virtualization, NAS, media server, game server, AI/LLM).
//!
//! Fixed native workloads, results saved as JSON (+ text, HTML, optional CSV row) so devices of
//! different architectures (x86-64, ARM64, RISC-V) and systems can be compared.

mod bench;
mod html;
mod hw;
mod i18n;
mod record;
mod render;
mod score;
mod ui;
mod util;
#[cfg(windows)]
mod winsys;

use clap::{Args, Parser, Subcommand};
use i18n::Lang;
use record::{Record, Results};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "syscheck", version, about = "Portable system benchmark: is this machine suitable for virtualization, NAS, media, game, AI workloads?")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the benchmark (default when no command is given)
    Run(RunArgs),
    /// Start the network test server on this machine
    Serve {
        #[arg(long, default_value_t = bench::net::DEFAULT_PORT)]
        port: u16,
    },
    /// Run only the network test against a `syscheck serve` instance
    Net {
        /// Server address, `IP` or `IP:PORT`
        #[arg(long)]
        server: String,
        #[command(flatten)]
        run: RunArgs,
    },
    /// Show a saved JSON result again (any language), optionally as HTML
    Report {
        file: PathBuf,
        #[arg(long)]
        lang: Option<String>,
        /// Also write an HTML report to this path
        #[arg(long)]
        html: Option<PathBuf>,
    },
    /// Merge all saved JSON results of a directory into one CSV file
    Export {
        #[arg(long, default_value = "syscheck-results")]
        dir: PathBuf,
        #[arg(long, default_value = "syscheck-all.csv")]
        out: PathBuf,
    },
}

#[derive(Args, Clone, Default)]
struct RunArgs {
    /// Language: en (default) or tr. Asked interactively when omitted
    #[arg(long)]
    lang: Option<String>,
    /// Shorter run: lower precision
    #[arg(long)]
    quick: bool,
    /// Directory (on the disk you want to test) for the disk test file
    #[arg(long, default_value = ".")]
    path: PathBuf,
    /// Where to save result files
    #[arg(long, default_value = "syscheck-results")]
    out_dir: PathBuf,
    /// Do not save any files
    #[arg(long)]
    no_save: bool,
    /// Append a flat result row to this CSV file
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Run only these tests: cpu,mem,disk,net,video,rt,gpu
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// Disk queue depths, e.g. 1,4,32
    #[arg(long, value_delimiter = ',')]
    qd: Vec<usize>,
    /// Disk test file size in MB
    #[arg(long)]
    disk_size_mb: Option<u64>,
    /// Also run a sustained write test for this many seconds
    #[arg(long, value_name = "SECS")]
    sustained: Option<u32>,
    /// Use synchronous writes (O_DSYNC) in the disk test
    #[arg(long)]
    disk_sync: bool,
    /// Network test server (`syscheck serve` on another machine). Without it, the network test is skipped
    #[arg(long)]
    net_server: Option<String>,
    /// Also run the network test against a local loopback server (measures this machine's TCP stack, not the network)
    #[arg(long)]
    net_loopback: bool,
    /// Your own name for this device, e.g. "Pixel 8" (stored in the result)
    #[arg(long)]
    label: Option<String>,
    /// Free-text notes stored in the result
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    no_color: bool,
}

fn ask_language() -> Lang {
    if !std::io::stdin().is_terminal() {
        return Lang::En;
    }
    println!("Language / Dil:\n  1) English (default)\n  2) Türkçe");
    print!("> ");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().lock().read_line(&mut s);
    Lang::parse(&s).unwrap_or(Lang::En)
}

fn pick_lang(arg: &Option<String>) -> Lang {
    match arg {
        Some(a) => Lang::parse(a).unwrap_or(Lang::En),
        None => ask_language(),
    }
}

fn die(msg: String) -> ! {
    eprintln!("syscheck: {msg}");
    std::process::exit(1);
}

fn do_run(args: RunArgs) {
    let lang = pick_lang(&args.lang);
    let ui = ui::Ui::new(lang, args.no_color);
    let tools = record::ToolInfo::current();
    if tools.build_profile == "debug" {
        println!("{}", util::paint(ui.color, util::ansi::YELLOW, &ui.t("msg_debug_warn")));
    }

    let mut prof = bench::Profile::new(args.quick);
    if !args.qd.is_empty() {
        prof.disk_qds = args.qd.iter().map(|q| (*q).clamp(1, 256)).collect();
    }
    if let Some(mb) = args.disk_size_mb {
        prof.disk_file_mb = mb.max(16);
    }
    if let Some(s) = args.sustained {
        prof.sustained_secs = s;
    }
    prof.disk_sync = args.disk_sync;

    // The network test runs only when asked for: with a server (real network) or --net-loopback.
    let net_wanted = args.net_server.is_some() || args.net_loopback;
    let all = ["cpu", "mem", "disk", "net", "video", "rt", "gpu"];
    let want: Vec<&str> = if args.only.is_empty() {
        all.iter().copied().filter(|k| *k != "net" || net_wanted).collect()
    } else {
        all.iter().copied().filter(|k| args.only.iter().any(|o| o.eq_ignore_ascii_case(k))).collect()
    };
    if want.is_empty() {
        die(ui.t("msg_no_tests"));
    }
    if want.contains(&"net") && !net_wanted {
        die(ui.t("msg_net_need_server"));
    }

    println!("{}", util::paint(ui.color, util::ansi::GRAY, &ui.t("msg_detecting")));
    let sys = hw::detect(args.label.clone(), &args.path);
    let mut results = Results::default();
    let total = want.len();
    for (i, k) in want.iter().enumerate() {
        ui.section(i + 1, total, &ui.t(&format!("h_{k}")));
        match *k {
            "cpu" => results.cpu = Some(bench::cpu::run(&ui, &sys.cpu, &prof)),
            "mem" => results.mem = Some(bench::mem::run(&ui, &sys.cpu, &sys.mem, &prof)),
            "disk" => results.disk = Some(bench::disk::run(&ui, &args.path, &sys.disk, &prof)),
            "video" => results.video = Some(bench::video::run(&ui, &prof)),
            "rt" => {
                results.rt = Some(bench::rt::run_cpu(&ui, &sys.cpu, &prof));
                // the GPU path tracer runs together with the GPU tests when those are selected too
                if !want.contains(&"gpu") {
                    results.gpu = Some(bench::gpu::run(&ui, &prof, bench::gpu::Stages { compute: false, rt: true }));
                }
            }
            "gpu" => results.gpu = Some(bench::gpu::run(&ui, &prof, bench::gpu::Stages { compute: true, rt: want.contains(&"rt") })),
            _ => {
                let gw = hw::default_gateway().map(|(_, ip)| ip);
                let link = sys.nics.iter().find(|n| n.default_route).and_then(|n| n.speed_mbps);
                results.net = Some(bench::net::run(&ui, args.net_server.as_deref(), gw, link, &prof));
            }
        }
        ui.clear_progress();
        println!("  {}", util::paint(ui.color, util::ansi::GREEN, &ui.t("msg_done")));
    }

    let rec = Record::new(args.quick, sys, results, args.notes.clone());
    println!("\n{}", render::text(&rec, lang, ui.color));
    if args.no_save {
        return;
    }
    save_all(&ui, &rec, lang, &args.out_dir, args.csv.as_deref());
}

fn save_all(ui: &ui::Ui, rec: &Record, lang: Lang, dir: &Path, csv: Option<&Path>) {
    let json = match rec.save_json(dir) {
        Ok(p) => p,
        Err(e) => die(format!("{}: {e}", ui.t("msg_save_failed"))),
    };
    let txt = json.with_extension("txt");
    let htm = json.with_extension("html");
    let mut saved = vec![json.clone()];
    if std::fs::write(&txt, render::text(rec, lang, false)).is_ok() {
        saved.push(txt);
    }
    if std::fs::write(&htm, html::html(rec, lang)).is_ok() {
        saved.push(htm);
    }
    for p in &saved {
        println!("  {}", ui.tf("msg_saved", &[p.display().to_string()]));
    }
    if let Some(c) = csv {
        match record::append_csv(c, rec) {
            Ok(()) => println!("  {}", ui.tf("msg_csv_added", &[c.display().to_string()])),
            Err(e) => eprintln!("syscheck: {e}"),
        }
    }
}

fn main() {
    // Behave like a normal Unix tool when output is piped into `head` etc. (no "broken pipe" panic).
    #[cfg(unix)]
    // SAFETY: resetting a signal disposition at startup, before any thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    // Windows console: UTF-8 output and ANSI colors; without them, keep the output plain.
    #[cfg(windows)]
    if !winsys::enable_ansi() {
        std::env::set_var("NO_COLOR", "1");
    }
    // Hidden entry point: the GPU tests run in a child process (see bench/gpu.rs).
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("__gpu") {
        #[cfg(feature = "gpu")]
        bench::gpu_worker::worker_main(&argv[2..]);
        #[cfg(not(feature = "gpu"))]
        println!("[]");
        return;
    }
    let cli = Cli::parse();
    match cli.cmd {
        None => do_run(cli.run),
        Some(Cmd::Run(a)) => do_run(a),
        Some(Cmd::Serve { port }) => {
            if let Err(e) = bench::net::serve(port) {
                die(e.to_string());
            }
        }
        Some(Cmd::Net { server, mut run }) => {
            run.net_server = Some(server);
            run.only = vec!["net".into()];
            do_run(run);
        }
        Some(Cmd::Report { file, lang, html: html_out }) => {
            let lang = lang.as_deref().and_then(Lang::parse).unwrap_or(Lang::En);
            let rec = Record::load(&file).unwrap_or_else(|e| die(e.to_string()));
            let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
            print!("{}", render::text(&rec, lang, color));
            if let Some(p) = html_out {
                std::fs::write(&p, html::html(&rec, lang)).unwrap_or_else(|e| die(e.to_string()));
                println!("  {}", i18n::tf(lang, "msg_saved", &[p.display().to_string()]));
            }
        }
        Some(Cmd::Export { dir, out }) => match record::export_dir(&dir, &out) {
            Ok(n) => println!("{} → {}", n, out.display()),
            Err(e) => die(e.to_string()),
        },
    }
}
