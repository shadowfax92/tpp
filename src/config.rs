//! `config.toml` model and loading. Every field has a default, so a missing or partial
//! file is fine — `Config::load` returns defaults when the file is absent.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// tmux socket name (`tmux -L <name>`). Empty/unset = the default tmux server, so tpp
    /// sessions live alongside your normal ones. Set a name to isolate them.
    pub socket: Option<String>,
    /// Command run by `tpp new`/`tpp run` when none is given. Defaults to `$SHELL`.
    pub shell: Option<String>,
    pub scope: ScopeCfg,
    pub ls: LsCfg,
    pub send: SendCfg,
    pub new: NewCfg,
    pub capture: CaptureCfg,
    pub tail: TailCfg,
    pub exit: ExitCfg,
    pub wait: WaitCfg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScopeMode {
    /// Nearest `git rev-parse --show-toplevel` (a worktree is its own scope).
    #[default]
    Git,
    /// The exact current working directory.
    Cwd,
    /// No scoping — `ls` shows every tpp session.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScopeCfg {
    pub mode: ScopeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LsDefault {
    /// Legacy scoped-listing preference, accepted for config compatibility.
    Scope,
    /// All tpp sessions everywhere.
    #[default]
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LsCfg {
    pub default: LsDefault,
    /// Also show sessions that exited within this many hours (0 = never).
    pub show_exited_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SendCfg {
    /// Use bracketed paste for multi-line text so it lands verbatim in TUIs.
    pub bracketed_paste: bool,
    /// Pause after a paste before pressing Enter, letting a TUI settle (ms).
    pub enter_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewCfg {
    /// Keep a finished command's output on screen (so `cat`/`tail` still work) instead of
    /// letting tmux close the session the instant the command exits.
    pub remain_on_exit: bool,
    /// Per-session scrollback limit applied to panes created after the session starts.
    pub history_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureCfg {
    /// Default number of trailing lines for `cat` (0 = visible screen only).
    pub lines: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TailCfg {
    /// Poll interval for `tail` (ms).
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExitCfg {
    /// Scrollback lines recorded when a session is exited/removed with `--record`.
    pub record_lines: u32,
    /// Auto-prune recorded exited sessions older than this many hours (0 = keep forever).
    pub prune_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaitCfg {
    /// How long output must be unchanged to count as "idle" (ms).
    pub stable_for_ms: u64,
    /// Default upper bound for `wait` (ms).
    pub timeout_ms: u64,
}

impl Default for ScopeCfg {
    fn default() -> Self {
        Self {
            mode: ScopeMode::Git,
        }
    }
}
impl Default for LsCfg {
    fn default() -> Self {
        Self {
            default: LsDefault::All,
            show_exited_hours: 24,
        }
    }
}
impl Default for SendCfg {
    fn default() -> Self {
        Self {
            bracketed_paste: true,
            enter_delay_ms: 0,
        }
    }
}
impl Default for NewCfg {
    fn default() -> Self {
        Self {
            remain_on_exit: true,
            history_limit: 100_000,
        }
    }
}
impl Default for CaptureCfg {
    fn default() -> Self {
        Self { lines: 200 }
    }
}
impl Default for TailCfg {
    fn default() -> Self {
        Self { interval_ms: 1000 }
    }
}
impl Default for ExitCfg {
    fn default() -> Self {
        Self {
            record_lines: 2000,
            prune_hours: 24,
        }
    }
}
impl Default for WaitCfg {
    fn default() -> Self {
        Self {
            stable_for_ms: 750,
            timeout_ms: 30_000,
        }
    }
}

impl Config {
    /// Load config from `path`, returning defaults if the file does not exist.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading config {}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing config.toml")
    }
}

pub const STARTER_CONFIG: &str = r#"# tpp configuration  (~/.config/tpp/config.toml)
# Every setting below is shown at its default — delete what you don't need to change.

# tmux socket. Empty = the default tmux server, so tpp sessions show up in your normal
# tmux (and in `tmx`). Set a name (e.g. "tpp") to give tpp its own isolated server.
socket = ""

# Command for `tpp new`/`tpp run` when you don't pass one. Empty = $SHELL.
shell = ""

[scope]
# How sessions are grouped so they're "shared in a directory":
#   git  = nearest git toplevel (a worktree is its own scope)   [default]
#   cwd  = the exact current directory
#   none = no scoping; `ls` shows every tpp session
mode = "git"

[ls]
default = "all"          # compatibility setting; `ls` shows all sessions
show_exited_hours = 24   # also surface sessions that exited in the last N hours

[send]
bracketed_paste = true   # multi-line text pastes verbatim (good for agent TUIs)
enter_delay_ms = 0       # pause after a paste before pressing Enter

[new]
remain_on_exit = true    # keep a finished command's output on screen for cat/tail
history_limit = 100000

[capture]
lines = 200              # default trailing lines for `cat` (0 = visible screen only)

[tail]
interval_ms = 1000       # poll cadence for `tail`

[exit]
record_lines = 2000      # scrollback recorded when a session exits
prune_hours = 24         # forget recorded exited sessions after N hours

[wait]
stable_for_ms = 750      # output must be unchanged this long to count as "idle"
timeout_ms = 30000       # default upper bound for `wait`
"#;
