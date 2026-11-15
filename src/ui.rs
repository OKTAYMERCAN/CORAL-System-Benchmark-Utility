//! Terminal output helpers shared by all benchmarks.

use crate::i18n::{self, Lang};
use crate::util::{ansi, paint};
use std::io::{IsTerminal, Write};

pub struct Ui {
    pub lang: Lang,
    pub color: bool,
    pub tty: bool,
}

impl Ui {
    pub fn new(lang: Lang, no_color: bool) -> Self {
        let tty = std::io::stdout().is_terminal();
        let color = tty && !no_color && std::env::var_os("NO_COLOR").is_none();
        Ui { lang, color, tty }
    }

    pub fn t(&self, key: &str) -> String {
        i18n::t(self.lang, key).to_string()
    }

    pub fn tf(&self, key: &str, args: &[String]) -> String {
        i18n::tf(self.lang, key, args)
    }

    /// Transient one-line status (only on a real terminal, so logs stay clean).
    pub fn progress(&self, msg: &str) {
        if self.tty {
            print!("\r\x1b[2K  {} {}", paint(self.color, ansi::CYAN, "›"), msg);
            let _ = std::io::stdout().flush();
        }
    }

    pub fn clear_progress(&self) {
        if self.tty {
            print!("\r\x1b[2K");
            let _ = std::io::stdout().flush();
        }
    }

    pub fn section(&self, n: usize, total: usize, title: &str) {
        let line = "─".repeat(72);
        println!("\n{}", paint(self.color, ansi::CYAN, &line));
        println!("{}", paint(self.color, &format!("{}{}", ansi::BOLD, ansi::CYAN), &format!("  {n}/{total}  {title}")));
        println!("{}", paint(self.color, ansi::CYAN, &line));
    }
}
