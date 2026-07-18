//! Command implementations and the shared [`Ctx`] they run against.

pub mod compat;
pub mod io;
pub mod lifecycle;
pub mod meta;
pub mod pane;
pub mod select;

use crate::config::Config;
use crate::paths::Paths;
use crate::session::{self, SessionInfo};
use crate::tmux::{tgt, Tmux, TmuxError};

/// Everything a command needs: tmux access, loaded config, resolved paths, and output flags.
pub struct Ctx {
    pub tmux: Tmux,
    pub cfg: Config,
    pub paths: Paths,
    pub config_path: std::path::PathBuf,
    pub json: bool,
    pub quiet: bool,
}

/// Process exit codes (documented in the README). clap emits 2 for usage errors itself.
pub mod code {
    pub const USAGE: i32 = 2;
    pub const NOT_FOUND: i32 = 3;
    pub const TIMEOUT: i32 = 4;
    pub const UNSENT: i32 = 5;
}

/// Print a message to stderr and exit with `code`.
pub fn die(code: i32, msg: impl AsRef<str>) -> ! {
    eprintln!("tpp: {}", msg.as_ref());
    std::process::exit(code);
}

pub fn no_such_session_message(name: &str) -> String {
    format!("No such session {name}")
}

/// Exit through the shared high-level missing-session error path.
pub fn no_such_session(name: &str) -> ! {
    die(code::NOT_FOUND, no_such_session_message(name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OriginPaneGone {
    session: String,
}

impl std::fmt::Display for OriginPaneGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "origin pane gone for {} (startup window closed?)",
            self.session
        )
    }
}

/// Resolve a session to its live startup pane while preserving unstamped legacy fallback.
pub(crate) fn session_pane_target(tmux: &Tmux, name: &str) -> Result<String, OriginPaneGone> {
    let Some(pane) = session::origin_pane(tmux, name) else {
        return Ok(tgt(name));
    };
    if tmux.ok(["display-message", "-p", "-t", &pane, "#{pane_id}"]) || !session::exists(tmux, name)
    {
        return Ok(pane);
    }
    Err(OriginPaneGone { session: tgt(name) })
}

/// Resolve the startup pane or exit through the stable not-found path.
pub(crate) fn require_session_pane_target(tmux: &Tmux, name: &str) -> String {
    session_pane_target(tmux, name).unwrap_or_else(|err| die(code::NOT_FOUND, err.to_string()))
}

/// Resolve the session a single-target command should act on.
pub fn resolve_one_target(ctx: &Ctx, explicit: Option<&str>) -> String {
    if let Some(name) = explicit {
        return session::resolve_existing_name(&ctx.tmux, &ctx.cfg, name);
    }
    let sessions = session::list(&ctx.tmux).unwrap_or_default();
    match sessions.len() {
        1 => sessions[0].name.clone(),
        0 => die(
            code::NOT_FOUND,
            "no sessions — name one explicitly (-t NAME)",
        ),
        _ => {
            let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
            die(
                code::NOT_FOUND,
                format!(
                    "multiple sessions — name one (-t NAME): {}",
                    names.join(", ")
                ),
            )
        }
    }
}

/// Capture a pane's contents. `lines = Some(0)` is the visible screen only; `Some(n)` reaches
/// `n` lines into history; `all_history` grabs everything.
pub fn capture(
    tmux: &Tmux,
    name: &str,
    lines: Option<u32>,
    escape: bool,
    all_history: bool,
) -> Result<String, TmuxError> {
    let target = session_pane_target(tmux, name).map_err(|_| TmuxError::NotFound)?;
    let mut args: Vec<String> = vec![
        "capture-pane".into(),
        "-p".into(),
        "-J".into(),
        "-t".into(),
        target,
    ];
    if escape {
        args.push("-e".into());
    }
    if all_history {
        args.push("-S".into());
        args.push("-".into());
    } else if let Some(n) = lines {
        if n > 0 {
            args.push("-S".into());
            args.push(format!("-{n}"));
        }
    }
    tmux.run(args)
}

/// Keep only the last `n` lines of `s` (no-op when `n == 0`).
pub fn last_lines(s: &str, n: usize) -> String {
    if n == 0 {
        return s.to_string();
    }
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Drop trailing blank lines (tmux pads the visible screen to pane height).
pub fn trim_trailing_blank(s: &str) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

/// Whether the output pane's command has exited.
pub fn pane_dead(tmux: &Tmux, name: &str) -> bool {
    let Ok(target) = session_pane_target(tmux, name) else {
        return true;
    };
    tmux.run(["display-message", "-p", "-t", &target, "#{pane_dead}"])
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

/// Exit status of a dead pane, if tmux reports one.
pub fn pane_dead_status(tmux: &Tmux, name: &str) -> Option<i32> {
    let target = session_pane_target(tmux, name).ok()?;
    tmux.run([
        "display-message",
        "-p",
        "-t",
        &target,
        "#{pane_dead_status}",
    ])
    .ok()
    .and_then(|s| s.trim().parse().ok())
}

/// The session the caller is running inside (requires `$TMUX`), if any.
pub fn current_session(tmux: &Tmux) -> Option<String> {
    std::env::var_os("TMUX")?;
    tmux.run(["display-message", "-p", "#{session_name}"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Look up a session's metadata in the current tmux server by exact name.
pub fn find_session(tmux: &Tmux, name: &str) -> Option<SessionInfo> {
    session::list(tmux)
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{resolve_one_target, Ctx};
    use crate::config::Config;
    use crate::paths::Paths;
    use crate::tmux::Tmux;

    fn ctx_with_prefix(prefix: &str) -> Ctx {
        Ctx {
            tmux: Tmux::new(Some(format!("tpp-test-{}", std::process::id()))),
            cfg: Config {
                session_prefix: prefix.to_string(),
                ..Config::default()
            },
            paths: Paths {
                config_dir: PathBuf::new(),
                state_dir: PathBuf::new(),
            },
            config_path: PathBuf::new(),
            json: false,
            quiet: true,
        }
    }

    #[test]
    fn explicit_target_applies_session_prefix() {
        assert_eq!(
            resolve_one_target(&ctx_with_prefix("tpp/"), Some("api")),
            "tpp/api"
        );
    }

    #[test]
    fn explicit_target_does_not_double_prefix() {
        assert_eq!(
            resolve_one_target(&ctx_with_prefix("tpp/"), Some("tpp/api")),
            "tpp/api"
        );
    }

    #[test]
    fn explicit_target_respects_empty_prefix() {
        assert_eq!(resolve_one_target(&ctx_with_prefix(""), Some("api")), "api");
    }
}
