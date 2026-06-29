//! Session model: create tpp sessions (tagged with their scope), list and inspect them.
//!
//! Discovery is stateless — tmux itself holds the truth. Each session carries user-options
//! (`@tpp*`) we read back with a single `list-sessions -F`. A leading `=` makes every target
//! an exact match.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::tmux::{exact, tgt, Tmux, TmuxError};

/// Field separator inside the `list-sessions` format — a control byte that won't appear in
/// session names, scopes, or commands.
const SEP: char = '\u{1f}';

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub scope: String,
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

/// List tpp-managed sessions. When `scope` is `Some`, only sessions stamped with that scope
/// are returned; `None` returns every tpp session. Non-tpp tmux sessions are always excluded.
pub fn list(tmux: &Tmux, scope: Option<&str>) -> Result<Vec<SessionInfo>> {
    let fmt = [
        "#{session_name}",
        "#{@tpp}",
        "#{@tpp_scope}",
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
        if f.len() < 9 {
            continue;
        }
        if f[1] != "1" {
            continue; // not a tpp session
        }
        let s = SessionInfo {
            name: f[0].to_string(),
            scope: f[2].to_string(),
            dir: f[3].to_string(),
            command: f[4].to_string(),
            created: f[5].parse().unwrap_or(0),
            attached: f[6] == "1",
            windows: f[7].parse().unwrap_or(1),
            dead: f[8] == "1",
            exited: f[8] == "1",
        };
        if let Some(scope) = scope {
            if s.scope != scope {
                continue;
            }
        }
        out.push(s);
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(out)
}

pub struct NewOpts<'a> {
    pub name: String,
    pub dir: Option<String>,
    pub command: Vec<String>,
    pub scope: Option<&'a str>,
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
    set_opt(tmux, &target, "@tpp_scope", opts.scope.unwrap_or(""));
    set_opt(tmux, &target, "@tpp_dir", opts.dir.as_deref().unwrap_or(""));
    set_opt(tmux, &target, "@tpp_cmd", &cmd_label);
    set_opt(tmux, &target, "@tpp_created", &now_epoch().to_string());

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
