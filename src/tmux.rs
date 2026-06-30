//! Thin wrapper over the `tmux` binary.
//!
//! Every call goes through [`Tmux`], which injects `-u` (UTF-8) and an optional `-L <socket>`
//! before the subcommand, captures output, and maps tmux's stderr onto typed errors. Targets
//! are matched exactly with a leading `=` so `foo` never accidentally matches `foobar`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};

#[derive(Debug)]
pub enum TmuxError {
    /// No tmux server is running on this socket.
    NoServer,
    /// `new-session` for a name that already exists.
    SessionExists,
    /// Target session/window/pane does not exist.
    NotFound,
    /// Anything else — carries tmux's stderr.
    Other(String),
}

impl std::fmt::Display for TmuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TmuxError::NoServer => write!(f, "no tmux server running"),
            TmuxError::SessionExists => write!(f, "session already exists"),
            TmuxError::NotFound => write!(f, "session not found"),
            TmuxError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for TmuxError {}

fn classify(stderr: &str) -> TmuxError {
    let s = stderr.trim();
    let low = s.to_ascii_lowercase();
    if low.contains("no server running")
        || low.contains("error connecting")
        || low.contains("no current")
    {
        TmuxError::NoServer
    } else if low.contains("duplicate session") {
        TmuxError::SessionExists
    } else if low.contains("can't find")
        || low.contains("session not found")
        || low.contains("no such")
    {
        TmuxError::NotFound
    } else {
        TmuxError::Other(if s.is_empty() {
            "tmux command failed".into()
        } else {
            s.into()
        })
    }
}

#[derive(Debug, Clone)]
pub struct Tmux {
    socket: Option<String>,
}

impl Tmux {
    pub fn new(socket: Option<String>) -> Self {
        let socket = socket.filter(|s| !s.trim().is_empty());
        Self { socket }
    }

    fn base(&self) -> Command {
        let mut c = Command::new("tmux");
        c.arg("-u");
        if let Some(sock) = &self.socket {
            c.arg("-L").arg(sock);
        }
        c
    }

    /// Run a tmux subcommand, returning trimmed stdout. Errors are classified.
    pub fn run<I, S>(&self, args: I) -> Result<String, TmuxError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.base();
        cmd.args(args);
        let out = cmd
            .stdin(Stdio::null())
            .output()
            .map_err(|e| TmuxError::Other(format!("spawning tmux: {e}")))?;
        if out.status.success() {
            let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
            while s.ends_with('\n') || s.ends_with('\r') {
                s.pop();
            }
            Ok(s)
        } else {
            Err(classify(&String::from_utf8_lossy(&out.stderr)))
        }
    }

    /// Run a tmux subcommand, feeding `input` on stdin (e.g. `load-buffer -`). Used so paste
    /// content of any size/shape goes in without shell-arg escaping.
    pub fn run_stdin<I, S>(&self, args: I, input: &str) -> Result<String, TmuxError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        use std::io::Write;
        let mut cmd = self.base();
        cmd.args(args);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TmuxError::Other(format!("spawning tmux: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .map_err(|e| TmuxError::Other(format!("writing to tmux: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| TmuxError::Other(format!("waiting for tmux: {e}")))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        } else {
            Err(classify(&String::from_utf8_lossy(&out.stderr)))
        }
    }

    /// Run a tmux subcommand only for its exit status (e.g. `has-session`).
    pub fn ok<I, S>(&self, args: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.base();
        cmd.args(args);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Replace the current process with `tmux <args>` (used for `attach`, which must own the
    /// terminal). Returns only on failure to exec.
    pub fn exec<I, S>(&self, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = self.base();
        cmd.args(args);
        Err(anyhow!("failed to exec tmux: {}", cmd.exec()))
    }

    pub fn socket(&self) -> Option<&str> {
        self.socket.as_deref()
    }

    /// Stable identity for filesystem state tied to the tmux server this wrapper will target.
    /// With no explicit `-L`, tmux honors `$TMUX` when called inside a tmux session, so that
    /// socket path must not share transcript storage with the real default server.
    pub fn store_socket(&self) -> Option<String> {
        if let Some(socket) = &self.socket {
            if let Some(path) = self.socket_path() {
                return socket_store_key(&path);
            }
            return named_socket_store_key(socket);
        }
        std::env::var("TMUX")
            .ok()
            .and_then(|value| tmux_env_socket(&value).and_then(socket_store_key))
    }

    fn socket_path(&self) -> Option<String> {
        self.run(["display-message", "-p", "#{socket_path}"])
            .ok()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
    }

    /// `-L <socket>` fragment for printing copy-pasteable attach hints; empty for default.
    pub fn socket_flag(&self) -> String {
        match &self.socket {
            Some(s) => format!("-L {s} "),
            None => String::new(),
        }
    }
}

/// Exact-match target form (`=name`). tmux honors this for *session-target* commands like
/// `has-session`, giving us a true existence check that never prefix-matches a longer name.
pub fn exact(name: &str) -> String {
    let n = name.trim().trim_start_matches('=');
    format!("={n}")
}

/// Plain target name. tmux's `=` exact prefix is rejected by pane/option-target commands
/// (`capture-pane`, `set-option`, `send-keys`, …), and a bare name already exact-matches when
/// the session exists — so operations on a known-existing session use this form.
pub fn tgt(name: &str) -> String {
    name.trim().trim_start_matches('=').to_string()
}

fn tmux_env_socket(value: &str) -> Option<&str> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|socket| !socket.is_empty())
}

fn named_socket_store_key(name: &str) -> Option<String> {
    tmux_named_socket_path(name).and_then(|path| socket_store_key(path.to_str()?))
}

fn socket_store_key(path: &str) -> Option<String> {
    let normalized = normalize_socket_path(Path::new(path));
    if tmux_named_socket_path("default")
        .map(|default_path| normalize_socket_path(&default_path) == normalized)
        .unwrap_or(false)
    {
        None
    } else {
        Some(format!("path:{normalized}"))
    }
}

fn tmux_named_socket_path(name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') {
        return None;
    }
    Some(tmux_socket_dir()?.join(name))
}

fn tmux_socket_dir() -> Option<PathBuf> {
    let base = std::env::var_os("TMUX_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let uid = current_uid()?;
    Some(base.join(format!("tmux-{uid}")))
}

fn current_uid() -> Option<String> {
    if let Ok(uid) = std::env::var("UID") {
        if !uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit()) {
            return Some(uid);
        }
    }
    let out = Command::new("id").arg("-u").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit())).then_some(uid)
}

fn normalize_socket_path(path: &Path) -> String {
    if let Ok(path) = std::fs::canonicalize(path) {
        return path.to_string_lossy().into_owned();
    }
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
        if let Ok(parent) = std::fs::canonicalize(parent) {
            return parent.join(file_name).to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{named_socket_store_key, socket_store_key, tmux_env_socket, tmux_named_socket_path};

    #[test]
    fn tmux_env_socket_parses_socket_path() {
        assert_eq!(
            tmux_env_socket("/tmp/tmux-501/default,123,0"),
            Some("/tmp/tmux-501/default")
        );
    }

    #[test]
    fn tmux_env_socket_rejects_empty_value() {
        assert_eq!(tmux_env_socket(""), None);
        assert_eq!(tmux_env_socket(",123,0"), None);
    }

    #[test]
    fn explicit_and_inherited_named_socket_keys_match() {
        let path = tmux_named_socket_path("agents").expect("current uid");

        assert_eq!(
            named_socket_store_key("agents"),
            socket_store_key(&path.to_string_lossy()),
        );
    }

    #[test]
    fn standard_default_socket_uses_legacy_default_namespace() {
        let path = tmux_named_socket_path("default").expect("current uid");

        assert_eq!(named_socket_store_key("default"), None);
        assert_eq!(socket_store_key(&path.to_string_lossy()), None);
    }
}
