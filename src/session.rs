//! Session model: create, list, and inspect tpp-managed tmux sessions.
//!
//! Discovery is stateless — tmux itself holds the truth. Each session carries user-options
//! (`@tpp*`) we read back with a single `list-sessions -F`. A leading `=` makes every target
//! an exact match.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::tmux::{exact, tgt, Tmux, TmuxError};

/// Field separator inside the `list-sessions` format.
const SEP: char = '\u{1f}';
const ORIGIN_PANE_OPT: &str = "@tpp_origin_pane";

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub dir: String,
    pub command: String,
    pub created: i64,
    pub attached: bool,
    pub windows: u32,
    /// The command in the active pane has exited (kept visible by remain-on-exit).
    pub dead: bool,
    /// Always present for live sessions; mirrors the `exited` record flag for dead ones.
    #[serde(default)]
    pub exited: bool,
}

impl SessionInfo {
    pub fn status(&self) -> &'static str {
        if self.dead {
            "exited"
        } else if self.attached {
            "attached"
        } else {
            "running"
        }
    }
}

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// True if a session with this exact name exists on the socket.
pub fn exists(tmux: &Tmux, name: &str) -> bool {
    tmux.ok(["has-session", "-t", &exact(name)])
}

fn safe_session_name(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, ':' | '.') { '_' } else { c })
        .collect()
}

/// Apply the configured tpp session prefix to a session name, unless already present.
pub fn prefixed_name(cfg: &Config, name: &str) -> String {
    let name = tgt(name);
    let prefix = cfg.session_prefix.as_str();
    let safe_prefix = safe_session_name(prefix);
    let already_prefixed =
        !prefix.is_empty() && (name.starts_with(prefix) || name.starts_with(&safe_prefix));
    let prefixed = if prefix.is_empty() || already_prefixed {
        name
    } else {
        format!("{prefix}{name}")
    };
    safe_session_name(&prefixed)
}

/// Apply the configured prefix to the session component of a tmux target.
pub fn prefixed_target(cfg: &Config, target: &str) -> String {
    let target = target.trim();
    let exact = target.starts_with('=');
    let raw = target.trim_start_matches('=');
    if raw.starts_with(['%', '@', '$', '{', '!']) {
        return target.to_string();
    }
    let safe_prefix = safe_session_name(&cfg.session_prefix);
    let search_start = if !cfg.session_prefix.is_empty() && raw.starts_with(&cfg.session_prefix) {
        cfg.session_prefix.len()
    } else if !safe_prefix.is_empty() && raw.starts_with(&safe_prefix) {
        safe_prefix.len()
    } else {
        0
    };
    let split_at = raw[search_start..]
        .find([':', '.'])
        .map(|i| search_start + i)
        .unwrap_or(raw.len());
    let (session, suffix) = raw.split_at(split_at);
    if session.is_empty() {
        return target.to_string();
    }

    let marker = if exact { "=" } else { "" };
    format!("{marker}{}{}", prefixed_name(cfg, session), suffix)
}

/// Resolve a user-supplied logical name to an existing prefixed session when possible.
pub fn resolve_existing_name(tmux: &Tmux, cfg: &Config, name: &str) -> String {
    let prefixed = prefixed_name(cfg, name);
    if exists(tmux, &prefixed) {
        return prefixed;
    }

    let raw = tgt(name);
    if raw != prefixed && exists(tmux, &raw) {
        raw
    } else {
        prefixed
    }
}

/// Return the startup pane id stored for this tpp session, if it has one.
pub fn origin_pane(tmux: &Tmux, name: &str) -> Option<String> {
    let target = tgt(name);
    if target.starts_with(['%', '@', '$', '{', '!']) {
        return None;
    }
    tmux.run(["show-option", "-qv", "-t", &target, ORIGIN_PANE_OPT])
        .ok()
        .map(|pane| pane.trim().to_string())
        .filter(|pane| !pane.is_empty())
}

/// Store the session's startup pane id for later output capture.
pub fn stamp_origin_pane(tmux: &Tmux, target: &str) {
    if let Ok(pane) = tmux.run(["display-message", "-p", "-t", target, "#{pane_id}"]) {
        let pane = pane.trim();
        if !pane.is_empty() {
            set_opt(tmux, target, ORIGIN_PANE_OPT, pane);
        }
    }
}

/// List tpp-managed sessions on the selected tmux socket.
pub fn list(tmux: &Tmux) -> Result<Vec<SessionInfo>> {
    let fmt = [
        "#{session_name}",
        "#{@tpp}",
        "#{@tpp_dir}",
        "#{@tpp_cmd}",
        "#{session_created}",
        "#{session_attached}",
        "#{session_windows}",
        "#{pane_dead}",
    ]
    .join(&SEP.to_string());

    let raw = match tmux.run(["list-sessions", "-F", &fmt]) {
        Ok(s) => s,
        Err(TmuxError::NoServer) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut out = Vec::new();
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(SEP).collect();
        if f.len() < 8 {
            continue;
        }
        if f[1] != "1" {
            continue; // not a tpp session
        }
        let s = SessionInfo {
            name: f[0].to_string(),
            dir: f[2].to_string(),
            command: f[3].to_string(),
            created: f[4].parse().unwrap_or(0),
            attached: f[5] == "1",
            windows: f[6].parse().unwrap_or(1),
            dead: f[7] == "1",
            exited: f[7] == "1",
        };
        out.push(s);
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(out)
}

pub struct NewOpts {
    pub name: String,
    pub dir: Option<String>,
    pub command: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Create a detached, tpp-tagged session. Caller guarantees the name is free (or uses
/// `exists` first); returns the created name.
pub fn create(tmux: &Tmux, cfg: &Config, opts: NewOpts) -> Result<String> {
    // Operations target the plain name (tmux rejects `=name` for set-option); the session
    // exists exactly once we've created it, so a bare name matches it unambiguously.
    let target = tgt(&opts.name);

    // Empty command means "a shell": honor a configured shell, else tmux's default.
    let command: Vec<String> = if opts.command.is_empty() {
        cfg.shell
            .clone()
            .filter(|s| !s.trim().is_empty())
            .map(|s| vec![s])
            .unwrap_or_default()
    } else {
        opts.command.clone()
    };

    // Start a detached *shell* session first, set options, THEN swap in the command via
    // respawn-pane. This guarantees remain-on-exit is in effect before the command runs, so
    // even an instantly-exiting command leaves its output on screen for cat/tail (no race).
    let mut args: Vec<String> = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        opts.name.clone(),
    ];
    if let Some(dir) = &opts.dir {
        args.push("-c".into());
        args.push(dir.clone());
    }
    if let (Some(x), Some(y)) = (opts.width, opts.height) {
        args.push("-x".into());
        args.push(x.to_string());
        args.push("-y".into());
        args.push(y.to_string());
    }
    tmux.run(args)?;

    if cfg.new.remain_on_exit {
        let _ = tmux.run(["set-option", "-t", &target, "-w", "remain-on-exit", "on"]);
    }
    if cfg.new.history_limit > 0 {
        let _ = tmux.run([
            "set-option",
            "-t",
            &target,
            "history-limit",
            &cfg.new.history_limit.to_string(),
        ]);
    }

    let cmd_label = if command.is_empty() {
        "shell".to_string()
    } else {
        command.join(" ")
    };
    set_opt(tmux, &target, "@tpp", "1");
    set_opt(tmux, &target, "@tpp_dir", opts.dir.as_deref().unwrap_or(""));
    set_opt(tmux, &target, "@tpp_cmd", &cmd_label);
    set_opt(tmux, &target, "@tpp_created", &now_epoch().to_string());
    stamp_origin_pane(tmux, &target);

    if !command.is_empty() {
        let mut r: Vec<String> = vec!["respawn-pane".into(), "-k".into(), "-t".into(), target];
        if let Some(dir) = &opts.dir {
            r.push("-c".into());
            r.push(dir.clone());
        }
        r.extend(command);
        tmux.run(r)?;
    }

    Ok(opts.name)
}

fn set_opt(tmux: &Tmux, target: &str, key: &str, value: &str) {
    let _ = tmux.run(["set-option", "-t", target, key, value]);
}

#[cfg(test)]
mod tests {
    use super::{prefixed_name, prefixed_target};
    use crate::config::Config;

    fn cfg(prefix: &str) -> Config {
        Config {
            session_prefix: prefix.to_string(),
            ..Config::default()
        }
    }

    #[test]
    fn prefixed_name_adds_default_prefix() {
        assert_eq!(prefixed_name(&Config::default(), "api"), "tpp/api");
    }

    #[test]
    fn prefixed_name_does_not_double_prefix() {
        assert_eq!(prefixed_name(&Config::default(), "tpp/api"), "tpp/api");
    }

    #[test]
    fn prefixed_name_allows_empty_prefix() {
        assert_eq!(prefixed_name(&cfg(""), "api"), "api");
    }

    #[test]
    fn prefixed_name_normalizes_tmux_target_separators() {
        assert_eq!(prefixed_name(&cfg("team."), "api"), "team_api");
        assert_eq!(prefixed_name(&cfg("team:"), "api"), "team_api");
    }

    #[test]
    fn prefixed_name_recognizes_safe_prefix() {
        assert_eq!(prefixed_name(&cfg("team."), "team_api"), "team_api");
    }

    #[test]
    fn prefixed_target_preserves_window_and_pane_suffix() {
        assert_eq!(
            prefixed_target(&Config::default(), "api:1.2"),
            "tpp/api:1.2"
        );
    }

    #[test]
    fn prefixed_target_keeps_exact_marker() {
        assert_eq!(prefixed_target(&Config::default(), "=api"), "=tpp/api");
    }

    #[test]
    fn prefixed_target_leaves_tmux_ids_unchanged() {
        assert_eq!(prefixed_target(&Config::default(), "%0"), "%0");
        assert_eq!(prefixed_target(&Config::default(), "@1"), "@1");
        assert_eq!(prefixed_target(&Config::default(), "$2"), "$2");
        assert_eq!(prefixed_target(&Config::default(), "=%0"), "=%0");
    }

    #[test]
    fn prefixed_target_does_not_double_prefix_with_dot_prefix() {
        let cfg = cfg("team.");

        assert_eq!(prefixed_target(&cfg, "api:0"), "team_api:0");
        assert_eq!(prefixed_target(&cfg, "team.api:0"), "team_api:0");
        assert_eq!(prefixed_target(&cfg, "team_api:0"), "team_api:0");
    }
}
