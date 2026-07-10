//! Session model: create, list, and inspect tpp-managed tmux sessions.
//!
//! Discovery is stateless — tmux itself holds the truth. Each session carries user-options
//! (`@tpp*`) we read back with a single `list-sessions -F`. A leading `=` makes every target
//! an exact match.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::paths::{create_private_dir_all, Paths};
use crate::tmux::{exact, tgt, Tmux, TmuxError};

/// Field separator inside the `list-sessions` format.
const SEP: char = '\u{1f}';
const ORIGIN_PANE_OPT: &str = "@tpp_origin_pane";
const ON_EXIT_CMD_FILE_OPT: &str = "@tpp_on_exit_cmd_file";
const ON_EXIT_MARKER_OPT: &str = "@tpp_on_exit_marker";
const ON_EXIT_LOG_OPT: &str = "@tpp_on_exit_log";
const ON_EXIT_HOOK_INDEX_OPT: &str = "@tpp_on_exit_hook_index";
const ON_EXIT_RUNNER_OPT: &str = "@tpp_on_exit_runner";

#[derive(Debug, Clone, Serialize)]
pub struct PaneState {
    pub dead: bool,
    pub pid: Option<u32>,
    pub exit_status: Option<i32>,
    pub activity: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct OnExitHook {
    command_file: PathBuf,
    marker: PathBuf,
    log: PathBuf,
    runner: PathBuf,
    hook_index: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub dir: String,
    pub command: String,
    pub created: i64,
    pub activity: i64,
    pub attached: bool,
    pub windows: u32,
    /// The command in the startup pane has exited (kept visible by remain-on-exit).
    pub dead: bool,
    pub pid: Option<u32>,
    pub exit_status: Option<i32>,
    /// Always present for live sessions; mirrors the `exited` record flag for dead ones.
    #[serde(default)]
    pub exited: bool,
}

impl SessionInfo {
    pub fn state(&self) -> &'static str {
        if self.dead {
            "exited"
        } else {
            "running"
        }
    }

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

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// True if a session with this exact name exists on the socket.
pub fn exists(tmux: &Tmux, name: &str) -> bool {
    tmux.ok(["has-session", "-t", &exact(name)])
}

fn encode_component(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => s.push(b as char),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

fn safe_session_name(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, ':' | '.') { '_' } else { c })
        .collect()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn tmux_double_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn unique_hook_index() -> String {
    let pid = std::process::id() as u128;
    let raw = now_nanos().wrapping_add(pid * 1_000_003);
    let index = (raw % 2_147_483_647).max(1);
    index.to_string()
}

fn hook_root(paths: &Paths, socket: Option<&str>) -> PathBuf {
    let socket = socket
        .map(encode_component)
        .unwrap_or_else(|| "default".into());
    paths.state_dir.join("hooks").join(socket)
}

impl OnExitHook {
    /// Persist the opaque hook command and reserve the once-marker paths used by tmux hooks.
    pub fn new(
        paths: &Paths,
        socket: Option<&str>,
        session_name: &str,
        command: String,
    ) -> Result<Self> {
        let hook_index = unique_hook_index();
        let base = hook_root(paths, socket);
        create_private_dir_all(&base)?;
        let stem = format!("{}-{hook_index}", encode_component(session_name));
        let command_file = base.join(format!("{stem}.cmd"));
        let runner = base.join(format!("{stem}.sh"));
        let marker = base.join(format!("{stem}.once"));
        let log = base.join("on-exit.log");
        std::fs::write(&command_file, command)?;
        Ok(Self {
            command_file,
            marker,
            log,
            runner,
            hook_index,
        })
    }

    fn write_runner(&self, expected_pane: &str, expected_session: &str) -> Result<()> {
        let script = format!(
            r#"#!/bin/sh
expected_pane={}
expected_session={}
marker={}
command_file={}
log={}
hook_index={}
actual_pane=${{1:-}}
actual_session=${{2:-}}
session_name=${{3:-}}
exit_status=${{4:-}}
socket_path=${{5:-}}
if [ -n "$actual_pane" ] && [ "$actual_pane" != "$expected_pane" ]; then exit 0; fi
if [ -n "$actual_session" ] && [ "$actual_session" != "$expected_session" ]; then exit 0; fi
parent=$(dirname "$marker")
mkdir -p "$parent" 2>/dev/null || true
if mkdir "$marker" 2>/dev/null; then
  cmd=$(cat "$command_file" 2>/dev/null) || cmd=
  if [ -n "$cmd" ]; then
    env TPP_SESSION_NAME="$session_name" TPP_SESSION="$session_name" TPP_EXIT_STATUS="$exit_status" sh -c "$cmd"
    status=$?
    if [ "$status" -ne 0 ]; then
      mkdir -p "$(dirname "$log")" 2>/dev/null || true
      printf '%s hook failed for %s: exit %s\n' "$(date -u +%FT%TZ)" "$session_name" "$status" >> "$log"
    fi
  fi
fi
if [ -n "$hook_index" ] && [ -n "$socket_path" ]; then
  tmux -S "$socket_path" set-hook -gu "session-closed[$hook_index]" >/dev/null 2>&1 || true
fi
exit 0
"#,
            shell_quote(expected_pane),
            shell_quote(expected_session),
            shell_quote(&self.marker.to_string_lossy()),
            shell_quote(&self.command_file.to_string_lossy()),
            shell_quote(&self.log.to_string_lossy()),
            shell_quote(&self.hook_index),
        );
        std::fs::write(&self.runner, script)?;
        Ok(())
    }

    fn hook_command(&self, args: &[&str], background: bool) -> String {
        let mut shell = format!("sh {}", shell_quote(&self.runner.to_string_lossy()));
        for arg in args {
            shell.push(' ');
            shell.push_str(&shell_quote(arg));
        }
        let flag = if background { " -b" } else { "" };
        format!("run-shell{flag} {}", tmux_double_quote(&shell))
    }

    fn store_options(&self, tmux: &Tmux, target: &str) {
        set_opt(
            tmux,
            target,
            ON_EXIT_CMD_FILE_OPT,
            &self.command_file.to_string_lossy(),
        );
        set_opt(
            tmux,
            target,
            ON_EXIT_MARKER_OPT,
            &self.marker.to_string_lossy(),
        );
        set_opt(tmux, target, ON_EXIT_LOG_OPT, &self.log.to_string_lossy());
        set_opt(tmux, target, ON_EXIT_HOOK_INDEX_OPT, &self.hook_index);
        set_opt(
            tmux,
            target,
            ON_EXIT_RUNNER_OPT,
            &self.runner.to_string_lossy(),
        );
    }
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

fn parse_optional_u32(value: &str) -> Option<u32> {
    value.trim().parse().ok()
}

fn parse_optional_i32(value: &str) -> Option<i32> {
    value.trim().parse().ok()
}

fn pane_state_for_target(tmux: &Tmux, target: &str) -> Option<PaneState> {
    let fmt = [
        "#{pane_dead}",
        "#{pane_pid}",
        "#{pane_dead_status}",
        "#{window_activity}",
    ]
    .join(&SEP.to_string());
    let raw = tmux
        .run(["display-message", "-p", "-t", &tgt(target), &fmt])
        .ok()?;
    let f: Vec<&str> = raw.split(SEP).collect();
    if f.len() < 4 {
        return None;
    }
    let dead = match f[0].trim() {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    Some(PaneState {
        dead,
        pid: parse_optional_u32(f[1]),
        exit_status: parse_optional_i32(f[2]),
        activity: f[3].trim().parse().ok(),
    })
}

fn missing_root_pane_state() -> PaneState {
    PaneState {
        dead: true,
        pid: None,
        exit_status: None,
        activity: None,
    }
}

/// Return process state for the startup pane that defines this tpp session's liveness.
pub fn root_pane_state(tmux: &Tmux, name: &str) -> Option<PaneState> {
    if let Some(target) = origin_pane(tmux, name) {
        return Some(pane_state_for_target(tmux, &target).unwrap_or_else(missing_root_pane_state));
    }
    Some(missing_root_pane_state())
}

/// True when the session exists and its startup pane process has not exited.
pub fn is_alive(tmux: &Tmux, name: &str) -> bool {
    exists(tmux, name) && root_pane_state(tmux, name).is_some_and(|pane| !pane.dead)
}

fn show_opt(tmux: &Tmux, target: &str, key: &str) -> Option<String> {
    tmux.run(["show-option", "-qv", "-t", &tgt(target), key])
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn append_hook_log(log: &Path, message: &str) {
    if let Some(parent) = log.parent() {
        let _ = create_private_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(file, "{message}");
    }
}

fn claim_marker(marker: &Path) -> bool {
    if let Some(parent) = marker.parent() {
        let _ = create_private_dir_all(parent);
    }
    std::fs::create_dir(marker).is_ok()
}

struct StoredOnExitHook {
    command_file: PathBuf,
    marker: PathBuf,
    log: PathBuf,
    hook_index: String,
}

/// Cached on-exit metadata that can still run after the tmux session is killed.
pub struct PreparedOnExitHook {
    hook: StoredOnExitHook,
    exit_status: Option<i32>,
}

impl StoredOnExitHook {
    fn load(tmux: &Tmux, target: &str) -> Option<Self> {
        Some(Self {
            command_file: PathBuf::from(show_opt(tmux, target, ON_EXIT_CMD_FILE_OPT)?),
            marker: PathBuf::from(show_opt(tmux, target, ON_EXIT_MARKER_OPT)?),
            log: PathBuf::from(show_opt(tmux, target, ON_EXIT_LOG_OPT)?),
            hook_index: show_opt(tmux, target, ON_EXIT_HOOK_INDEX_OPT)?,
        })
    }

    fn fire(&self, session_name: &str, exit_status: Option<i32>) {
        if !claim_marker(&self.marker) {
            return;
        }
        let command = match std::fs::read_to_string(&self.command_file) {
            Ok(command) => command,
            Err(e) => {
                append_hook_log(
                    &self.log,
                    &format!("reading hook command for {session_name}: {e}"),
                );
                return;
            }
        };
        let exit_status = exit_status
            .map(|status| status.to_string())
            .unwrap_or_default();
        match Command::new("sh")
            .arg("-c")
            .arg(command)
            .env("TPP_SESSION_NAME", session_name)
            .env("TPP_SESSION", session_name)
            .env("TPP_EXIT_STATUS", exit_status)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => append_hook_log(
                &self.log,
                &format!(
                    "hook failed for {session_name}: exit {}",
                    status.code().unwrap_or(1)
                ),
            ),
            Err(e) => append_hook_log(&self.log, &format!("running hook for {session_name}: {e}")),
        }
    }
}

impl PreparedOnExitHook {
    /// Remove the raw-kill hook when tpp itself is about to run the post-kill fallback.
    pub fn disable_session_closed_hook(&self, tmux: &Tmux) {
        let hook = format!("session-closed[{}]", self.hook.hook_index);
        let _ = tmux.run(["set-hook", "-gu", &hook]);
    }

    /// Run the cached hook unless another lifecycle path already claimed its marker.
    pub fn fire(&self, session_name: &str) {
        self.hook.fire(session_name, self.exit_status);
    }
}

/// Capture hook metadata before teardown so tpp can run a post-kill fallback if needed.
pub fn prepare_on_exit_hook(tmux: &Tmux, name: &str) -> Option<PreparedOnExitHook> {
    let hook = StoredOnExitHook::load(tmux, name)?;
    let exit_status = root_pane_state(tmux, name).and_then(|pane| pane.exit_status);
    Some(PreparedOnExitHook { hook, exit_status })
}

fn install_on_exit_hook(tmux: &Tmux, target: &str, hook: &OnExitHook) -> Result<()> {
    let origin = origin_pane(tmux, target).unwrap_or_else(|| tgt(target));
    let session_id = tmux.run(["display-message", "-p", "-t", target, "#{session_id}"])?;
    hook.write_runner(&origin, session_id.trim())?;
    hook.store_options(tmux, target);

    let pane_hook = format!("pane-died[{}]", hook.hook_index);
    let pane_command = hook.hook_command(
        &[
            "#{hook_pane}",
            "",
            "#{session_name}",
            "#{pane_dead_status}",
            "#{socket_path}",
        ],
        true,
    );
    tmux.run(["set-hook", "-w", "-t", target, &pane_hook, &pane_command])?;

    let closed_hook = format!("session-closed[{}]", hook.hook_index);
    let closed_command = hook.hook_command(
        &[
            "",
            "#{hook_session}",
            "#{hook_session_name}",
            "",
            "#{socket_path}",
        ],
        false,
    );
    tmux.run(["set-hook", "-g", &closed_hook, &closed_command])?;
    Ok(())
}

/// List tpp-managed sessions on the selected tmux socket.
pub fn list(tmux: &Tmux) -> Result<Vec<SessionInfo>> {
    let fmt = [
        "#{session_name}",
        "#{@tpp}",
        "#{@tpp_dir}",
        "#{@tpp_cmd}",
        "#{session_created}",
        "#{session_activity}",
        "#{session_attached}",
        "#{session_windows}",
        "#{@tpp_origin_pane}",
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
            continue;
        }
        let pane_state = if f[8].is_empty() {
            Some(missing_root_pane_state())
        } else {
            Some(pane_state_for_target(tmux, f[8]).unwrap_or_else(missing_root_pane_state))
        };
        let session_activity = f[5].parse().unwrap_or(0);
        let dead = pane_state.as_ref().map(|pane| pane.dead).unwrap_or(false);
        let s = SessionInfo {
            name: f[0].to_string(),
            dir: f[2].to_string(),
            command: f[3].to_string(),
            created: f[4].parse().unwrap_or(0),
            activity: pane_state
                .as_ref()
                .and_then(|pane| pane.activity)
                .unwrap_or(0)
                .max(session_activity),
            attached: f[6] == "1",
            windows: f[7].parse().unwrap_or(1),
            dead,
            pid: pane_state.as_ref().and_then(|pane| pane.pid),
            exit_status: pane_state.and_then(|pane| pane.exit_status),
            exited: dead,
        };
        out.push(s);
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.created));
    Ok(out)
}

pub struct NewOpts {
    pub name: String,
    pub dir: Option<String>,
    pub command: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub on_exit: Option<OnExitHook>,
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

    if cfg.new.remain_on_exit || opts.on_exit.is_some() {
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
    if let Some(hook) = &opts.on_exit {
        install_on_exit_hook(tmux, &target, hook)?;
    }

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
