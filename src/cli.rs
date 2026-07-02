//! Command-line surface (clap derive).
//!
//! Two layers: the ergonomic commands humans and agents use day to day, and a set of hidden
//! `tmux-compat` verbs (`has-session`, `new-session`, `paste-buffer`, …) that forward to tmux
//! so `tpp` is a drop-in for `rmux` in existing scripts — replace the word `rmux` with `tpp`
//! and it works.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "tpp",
    version,
    about = "tmux++ — share, run, capture, and paste into tmux sessions",
    long_about = "tmux++ (tpp) — an ergonomic wrapper around tmux for humans and agents.\n\n\
        List all tpp sessions, run commands in detached sessions, capture and \
        follow their output, and paste prompts in verbatim. Sessions live in your normal \
        tmux server, so `tmx`, `grove`, and plain `tmux` all see them.",
    disable_help_subcommand = true,
    propagate_version = true
)]
pub struct Cli {
    /// tmux socket name (`tmux -L`). Default: from config, else the shared tmux server.
    #[arg(short = 'L', long, global = true, value_name = "NAME")]
    pub socket: Option<String>,

    /// Machine-readable JSON output (where supported).
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-essential output (with `ls`, print only names).
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Config file path (default: ~/.config/tpp/config.toml).
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Defaults to `ls` (all tpp sessions) when omitted.
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run a command in a new detached session (prints the session name).
    #[command(visible_alias = "r")]
    Run(RunArgs),

    /// Create a session (detached; runs your shell if no command is given).
    #[command(visible_alias = "n")]
    New(NewArgs),

    /// List all tpp sessions.
    #[command(visible_aliases = ["l", "list"])]
    Ls(LsArgs),

    /// Attach to a session (interactive).
    #[command(visible_alias = "a")]
    Attach(AttachArgs),

    /// Send typed text (optionally Enter) or keys to a session.
    #[command(visible_alias = "s")]
    Send(SendArgs),

    /// Paste text into a session verbatim (bracketed) and press Enter.
    Paste(PasteArgs),

    /// Print a session's output (live, or replayed if it has already exited).
    #[command(visible_aliases = ["cap", "capture"])]
    Cat(CatArgs),

    /// Follow a session's output as it changes.
    #[command(visible_alias = "follow")]
    Tail(TailArgs),

    /// Block until text appears, output goes idle, or the pane exits.
    Wait(WaitArgs),

    /// Remove (kill) sessions.
    #[command(visible_aliases = ["kill", "remove"])]
    Rm(RmArgs),

    /// Exit the current session: record its output, then kill it.
    #[command(visible_aliases = ["e", "quit"])]
    Exit(ExitArgs),

    /// Clear recorded exited sessions.
    #[command(visible_alias = "clr")]
    Clear,

    /// Exit 0 if a session exists, non-zero otherwise (script-friendly).
    Has(HasArgs),

    /// Rename a session.
    Rename(RenameArgs),

    /// Show, edit, or initialize configuration.
    Config(ConfigArgs),

    /// Write a starter config (and optionally install fish completions).
    Init(InitArgs),

    /// Check tmux availability and print resolved paths.
    Doctor,

    /// Generate shell completions (bash, zsh, fish, …).
    Completions(CompletionsArgs),

    // ---- tmux-compat (hidden): forwarded to tmux so tpp drops in for rmux ----
    #[command(name = "has-session", hide = true)]
    HasSession(RawArgs),
    #[command(name = "new-session", hide = true)]
    NewSession(RawArgs),
    #[command(name = "attach-session", hide = true)]
    AttachSession(RawArgs),
    #[command(name = "kill-session", hide = true)]
    KillSession(RawArgs),
    #[command(name = "list-sessions", hide = true)]
    ListSessions(RawArgs),
    #[command(name = "set-buffer", hide = true)]
    SetBuffer(RawArgs),
    #[command(name = "paste-buffer", hide = true)]
    PasteBuffer(RawArgs),
    #[command(name = "send-keys", hide = true)]
    SendKeys(RawArgs),
    #[command(name = "capture-pane", hide = true)]
    CapturePane(RawArgs),
    /// Raw passthrough to tmux (using tpp's socket).
    #[command(hide = true)]
    X(RawArgs),
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Session name (auto-generated from the command if omitted).
    #[arg(short = 's', long = "name", value_name = "NAME")]
    pub name: Option<String>,
    /// Working directory for the session.
    #[arg(short = 'c', long, value_name = "DIR")]
    pub dir: Option<String>,
    /// Wait for the command to finish, stream its output, then exit with its status.
    #[arg(short = 'w', long)]
    pub wait: bool,
    /// With --wait: also record the output as an exited session.
    #[arg(long)]
    pub record: bool,
    /// The command to run (everything after `--`).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub command: Vec<String>,
}

#[derive(Args, Debug)]
pub struct NewArgs {
    /// Session name (auto-generated from the directory if omitted).
    #[arg(short = 's', long = "name", value_name = "NAME")]
    pub name: Option<String>,
    /// Working directory for the session.
    #[arg(short = 'c', long, value_name = "DIR")]
    pub dir: Option<String>,
    /// OK if it already exists (no-op, exit 0) instead of erroring.
    #[arg(short = 'A', long)]
    pub attach: bool,
    /// Accepted for tmux symmetry; `new` is always detached.
    #[arg(short = 'd', long, hide = true)]
    pub detached: bool,
    /// Command to run (defaults to your shell).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub command: Vec<String>,
}

#[derive(Args, Debug, Default)]
pub struct LsArgs {
    /// Accepted for compatibility; `ls` already shows all tpp sessions.
    #[arg(short = 'a', long)]
    pub all: bool,
    /// Include recently exited sessions.
    #[arg(long)]
    pub exited: bool,
    /// Hide recently exited sessions.
    #[arg(long, conflicts_with = "exited")]
    pub no_exited: bool,
}

#[derive(Args, Debug)]
pub struct AttachArgs {
    /// Session to attach to. If omitted, pick (fzf when available, else the sole session).
    pub session: Option<String>,
}

#[derive(Args, Debug)]
pub struct SendArgs {
    /// Target session (default: the sole session, or a picker).
    #[arg(short = 't', long, value_name = "SESSION")]
    pub target: Option<String>,
    /// Read text from a file.
    #[arg(short = 'f', long, value_name = "PATH", conflicts_with = "stdin")]
    pub file: Option<PathBuf>,
    /// Read text from stdin.
    #[arg(long, conflicts_with = "file")]
    pub stdin: bool,
    /// Interpret args as tmux key names (Enter, C-c, Escape) instead of literal text.
    #[arg(short = 'k', long)]
    pub keys: bool,
    /// Use bracketed paste (verbatim multi-line; good for TUIs).
    #[arg(short = 'p', long)]
    pub paste: bool,
    /// Press Enter after sending typed text.
    #[arg(short = 'e', long)]
    pub enter: bool,
    /// Text to send (literal unless --keys; use -- before option-looking text).
    #[arg(value_name = "TEXT")]
    pub text: Vec<String>,
}

#[derive(Args, Debug)]
pub struct PasteArgs {
    /// Target session (default: the sole session, or a picker).
    #[arg(short = 't', long, value_name = "SESSION")]
    pub target: Option<String>,
    /// Read text from a file.
    #[arg(short = 'f', long, value_name = "PATH", conflicts_with = "stdin")]
    pub file: Option<PathBuf>,
    /// Read text from stdin.
    #[arg(long, conflicts_with = "file")]
    pub stdin: bool,
    /// Leave pasted text unsubmitted.
    #[arg(long)]
    pub no_enter: bool,
    /// Text to paste.
    #[arg(value_name = "TEXT")]
    pub text: Vec<String>,
}

#[derive(Args, Debug)]
pub struct CatArgs {
    /// Sessions to print (default: the sole session, or a picker).
    #[arg(value_name = "SESSION")]
    pub sessions: Vec<String>,
    /// Include every recorded exited session in the no-argument picker.
    #[arg(short = 'a', long)]
    pub all: bool,
    /// Trailing lines to print (0 = visible screen only; default from config).
    #[arg(short = 'n', long, value_name = "N")]
    pub lines: Option<u32>,
    /// Include escape sequences (colors).
    #[arg(short = 'e', long)]
    pub escape: bool,
    /// Print the entire scrollback.
    #[arg(short = 'S', long = "all-history")]
    pub all_history: bool,
}

#[derive(Args, Debug)]
pub struct TailArgs {
    /// Sessions to follow (default: the sole session, or a picker).
    #[arg(value_name = "SESSION")]
    pub sessions: Vec<String>,
    /// Poll interval in ms (default from config).
    #[arg(short = 'i', long, value_name = "MS")]
    pub interval: Option<u64>,
    /// Print this many trailing lines before following.
    #[arg(short = 'n', long, value_name = "N")]
    pub lines: Option<u32>,
}

#[derive(Args, Debug)]
pub struct WaitArgs {
    /// Target session (default: the sole session, or a picker).
    #[arg(short = 't', long, value_name = "SESSION")]
    pub target: Option<String>,
    /// Wait until this text appears in the pane.
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,
    /// Wait until output is unchanged for the idle threshold.
    #[arg(long)]
    pub idle: bool,
    /// Wait until the pane's command exits.
    #[arg(long)]
    pub exit: bool,
    /// Idle threshold in ms (default from config).
    #[arg(long, value_name = "MS")]
    pub stable_for: Option<u64>,
    /// Timeout in ms (default from config; 0 = no timeout).
    #[arg(long, value_name = "MS")]
    pub timeout: Option<u64>,
}

#[derive(Args, Debug)]
pub struct RmArgs {
    /// Sessions to remove.
    #[arg(value_name = "SESSION")]
    pub sessions: Vec<String>,
    /// Remove every tpp session.
    #[arg(long)]
    pub all: bool,
    /// Record output before killing.
    #[arg(long)]
    pub record: bool,
}

#[derive(Args, Debug)]
pub struct ExitArgs {
    /// Session to exit (default: the session you're calling from).
    #[arg(value_name = "SESSION")]
    pub session: Option<String>,
    /// Don't record output before killing.
    #[arg(long)]
    pub no_record: bool,
}

#[derive(Args, Debug)]
pub struct HasArgs {
    /// Session name.
    #[arg(value_name = "SESSION")]
    pub session: Option<String>,
    /// Session name (tmux-style flag form).
    #[arg(short = 't', long, value_name = "SESSION", conflicts_with = "session")]
    pub target: Option<String>,
    /// Require the session's root pane process to still be running.
    #[arg(long)]
    pub alive: bool,
}

#[derive(Args, Debug)]
pub struct RenameArgs {
    /// With one arg: new name, and pick the session. With two: SESSION NEW_NAME.
    #[arg(value_name = "SESSION_OR_NEW_NAME", num_args = 1..=2)]
    pub names: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: Option<ConfigAction>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the config file path.
    Path,
    /// Print the effective config.
    Show,
    /// Open the config in $EDITOR.
    Edit,
    /// Write a starter config.
    Init {
        /// Overwrite an existing config.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite an existing config.
    #[arg(long)]
    pub force: bool,
    /// Also install fish completions to ~/.config/fish/completions.
    #[arg(long)]
    pub fish: bool,
}

#[derive(Args, Debug)]
pub struct CompletionsArgs {
    /// Target shell.
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

/// Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim.
#[derive(Args, Debug)]
pub struct RawArgs {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    pub args: Vec<String>,
}
