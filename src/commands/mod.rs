//! Command implementations and the shared [`Ctx`] they run against.

pub mod compat;
pub mod family;
pub mod io;
pub mod lifecycle;
pub mod mail;
pub mod meta;
pub mod pane;
pub mod select;

use crate::backend::{Backend, BackendRef, CaptureSpec};
use crate::config::Config;
use crate::paths::Paths;
use crate::session::SessionInfo;

/// Everything a command needs: tmux access, loaded config, resolved paths, and output flags.
pub struct Ctx {
    pub backend: BackendRef,
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
pub(crate) fn session_pane_target(
    backend: &dyn Backend,
    name: &str,
) -> Result<String, OriginPaneGone> {
    let Some(pane) = backend.origin_pane(name) else {
        return Ok(name.trim().trim_start_matches('=').to_string());
    };
    let pane_resolves = backend.canonical_pane(&pane).as_deref() == Some(pane.as_str());
    if pane_resolves || !backend.exists(name) {
        return Ok(pane);
    }
    Err(OriginPaneGone {
        session: name.trim().trim_start_matches('=').to_string(),
    })
}

/// Resolve the startup pane or exit through the stable not-found path.
pub(crate) fn require_session_pane_target(backend: &dyn Backend, name: &str) -> String {
    session_pane_target(backend, name).unwrap_or_else(|err| die(code::NOT_FOUND, err.to_string()))
}

/// Resolve the session a single-target command should act on.
pub fn resolve_one_target(ctx: &Ctx, explicit: Option<&str>) -> String {
    if let Some(name) = explicit {
        return ctx.backend.resolve_name(&ctx.cfg, name);
    }
    let sessions = ctx.backend.list().unwrap_or_default();
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
    backend: &dyn Backend,
    name: &str,
    lines: Option<u32>,
    escape: bool,
    all_history: bool,
) -> anyhow::Result<String> {
    let target = session_pane_target(backend, name)
        .map_err(|_| anyhow::anyhow!("origin pane gone for {name}"))?;
    backend
        .capture(
            &target,
            CaptureSpec {
                lines,
                escape,
                all_history,
            },
        )
        .map(|raw| strip_dead_pane_overlay(&raw))
}

/// Drop tmux's dead-pane chrome when it is the capture's final non-blank line.
///
/// tmux (observed on 3.6a) writes "Pane is dead (status …)" into the bottom visible row of
/// a remain-on-exit pane, leaving a blank gulf between real output and the overlay, so
/// trailing-blank trims can never reach the content above it.
fn strip_dead_pane_overlay(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let Some(last_content) = lines.iter().rposition(|l| !l.trim().is_empty()) else {
        return s.to_string();
    };
    let stripped = io::strip_ansi(lines[last_content]);
    let trimmed = stripped.trim();
    let is_overlay = trimmed == "Pane is dead"
        || (trimmed.starts_with("Pane is dead (") && trimmed.ends_with(')'));
    if !is_overlay {
        return s.to_string();
    }
    lines[..last_content].join("\n")
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
pub fn pane_dead(backend: &dyn Backend, name: &str) -> bool {
    let Ok(target) = session_pane_target(backend, name) else {
        return true;
    };
    backend.pane_state(&target).is_some_and(|pane| pane.dead)
}

/// Exit status of a dead pane, if tmux reports one.
pub fn pane_dead_status(backend: &dyn Backend, name: &str) -> Option<i32> {
    let target = session_pane_target(backend, name).ok()?;
    backend
        .pane_state(&target)
        .and_then(|pane| pane.exit_status)
}

/// The session the caller is running inside (requires `$TMUX`), if any.
pub fn current_session(backend: &dyn Backend) -> Option<String> {
    backend.current_session()
}

/// Look up a session's metadata in the current tmux server by exact name.
pub fn find_session(backend: &dyn Backend, name: &str) -> Option<SessionInfo> {
    backend
        .list()
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{resolve_one_target, strip_dead_pane_overlay, Ctx};
    use crate::backend::{select, BackendRef};
    use crate::config::Config;
    use crate::paths::Paths;

    fn ctx_with_prefix(prefix: &str) -> Ctx {
        Ctx {
            backend: backend(prefix),
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

    fn backend(prefix: &str) -> BackendRef {
        let paths = Paths {
            config_dir: PathBuf::new(),
            state_dir: PathBuf::new(),
        };
        select(
            &Config {
                session_prefix: prefix.to_string(),
                ..Config::default()
            },
            &paths,
            PathBuf::new(),
            Some(format!("tpp-test-{}", std::process::id())),
        )
    }

    #[test]
    fn explicit_target_applies_session_prefix() {
        assert_eq!(
            resolve_one_target(&ctx_with_prefix("tpp/"), Some("api")),
            "tpp/api"
        );
    }

    #[test]
    fn dead_overlay_stripped_when_final_non_blank_line() {
        let capture = "output line\n\n\n\nPane is dead (status 127, Thu Jul 30 13:26:49 2026)";
        assert_eq!(strip_dead_pane_overlay(capture), "output line\n\n\n");
        let bare = "output line\n\nPane is dead";
        assert_eq!(strip_dead_pane_overlay(bare), "output line\n");
    }

    #[test]
    fn dead_overlay_stripped_despite_ansi_styling() {
        let capture = "output line\n\n\u{1b}[7mPane is dead (status 0, now)\u{1b}[0m";
        assert_eq!(strip_dead_pane_overlay(capture), "output line\n");
    }

    #[test]
    fn dead_overlay_kept_when_content_follows_or_shape_differs() {
        let mid = "Pane is dead (status 1, then)\nreal output after";
        assert_eq!(strip_dead_pane_overlay(mid), mid);
        let similar = "output\nPane is dead maybe";
        assert_eq!(strip_dead_pane_overlay(similar), similar);
        let empty = "\n\n";
        assert_eq!(strip_dead_pane_overlay(empty), empty);
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
