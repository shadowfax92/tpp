//! Tiny output helpers: tty-gated color and JSON printing. No color crate — a handful of
//! ANSI codes behind an `is_terminal()` check keeps the binary lean and matches house style.

use std::io::IsTerminal;
use std::sync::OnceLock;

use serde::Serialize;

static COLOR: OnceLock<bool> = OnceLock::new();

/// Whether to emit ANSI color: stdout is a tty and `NO_COLOR` is unset.
pub fn use_color() -> bool {
    *COLOR.get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

pub enum Style {
    Dim,
    Bold,
    Green,
    Yellow,
    Red,
    Cyan,
}

impl Style {
    fn code(&self) -> &'static str {
        match self {
            Style::Dim => "2",
            Style::Bold => "1",
            Style::Green => "32",
            Style::Yellow => "33",
            Style::Red => "31",
            Style::Cyan => "36",
        }
    }
}

pub fn paint(s: &str, style: Style) -> String {
    if use_color() {
        format!("\x1b[{}m{}\x1b[0m", style.code(), s)
    } else {
        s.to_string()
    }
}

/// Print a value as pretty JSON to stdout.
pub fn print_json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    println!("{s}");
    Ok(())
}
