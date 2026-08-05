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
    about = "run, capture, and paste into tmux sessions or Herdr tabs",
    long_about = "tpp is a terminal-workspace wrapper for humans and agents.\n\n\
        List all tpp sessions, run commands in detached sessions, capture and \
        follow their output, and paste prompts in verbatim. tmux is the default backend; \
        `herdr-mode = true` puts sessions in named tabs inside one `tpp` workspace in Herdr.",
    disable_help_subcommand = true,
    propagate_version = true
)]
pub struct Cli {
    /// tmux socket name (`tmux -L`); ignored by the Herdr backend.
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

    /// Generate fresh petnames without creating sessions.
    Name(NameArgs),

    /// Inspect or control per-session stuck-screen watchers.
    Watch(WatchArgs),

    /// List all tpp sessions.
    #[command(visible_aliases = ["l", "list"])]
    Ls(LsArgs),

    /// List tpp sessions spawned from a pane or session.
    Children(ChildrenArgs),

    /// Send, list, and read durable file-backed messages.
    Mail(MailArgs),

    /// Reply to a message in the caller's inbox.
    Reply(ReplyArgs),

    /// Attach to a session (interactive).
    #[command(visible_alias = "a")]
    Attach(AttachArgs),

    /// Send typed text (optionally Enter) or keys to a session.
    #[command(visible_alias = "s")]
    Send(SendArgs),

    /// Paste text into a session verbatim (bracketed) and press Enter.
    Paste(PasteArgs),

    /// Bind a name to a multiplexer pane.
    Bind(BindArgs),

    /// Remove a named pane binding.
    Unbind(UnbindArgs),

    /// List named pane bindings.
    Targets(TargetsArgs),

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

    /// Remove stale detached sessions.
    Reap(ReapArgs),

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

    /// Check the selected backend and print resolved paths.
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
    /// Session name (a dated petname is generated if omitted).
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
    /// Watch this command for blocked interactive prompts.
    #[arg(long)]
    pub watch: bool,
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
    /// Session name (a dated petname is generated if omitted).
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
    /// Shell command to run once when this session's root command exits.
    #[arg(long, value_name = "CMD")]
    pub on_exit: Option<String>,
    /// Disable the per-session stuck-screen watcher.
    #[arg(long)]
    pub no_watch: bool,
    /// Pane to nudge if the session stalls (default: the calling pane).
    #[arg(long, value_name = "PANE")]
    pub parent_pane: Option<String>,
    /// Command to run (defaults to your shell).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub command: Vec<String>,
}

#[derive(Args, Debug)]
pub struct NameArgs {
    /// Number of mutually unique petnames to print.
    #[arg(
        short = 'n',
        long,
        default_value_t = 1,
        value_name = "N",
        value_parser = parse_positive_usize
    )]
    pub count: usize,
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| "count must be a positive integer".to_string())
        .and_then(|count| {
            if count == 0 {
                Err("count must be at least 1".to_string())
            } else {
                Ok(count)
            }
        })
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    #[command(subcommand)]
    pub action: WatchCommand,
}

#[derive(Subcommand, Debug)]
pub enum WatchCommand {
    /// Run a session watcher in the foreground.
    Run(WatchTargetArgs),
    /// List active session watchers.
    Ls,
    /// Print effective watch rules in matching order.
    Rules,
    /// Stop a session watcher.
    Stop(WatchTargetArgs),
}

#[derive(Args, Debug)]
pub struct WatchTargetArgs {
    /// Session to watch or stop watching.
    #[arg(short = 't', long, value_name = "SESSION")]
    pub target: String,
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
pub struct ChildrenArgs {
    /// Query children of this pane instead of the caller's current pane.
    #[arg(long, value_name = "TMUX_TARGET", conflicts_with = "target")]
    pub pane: Option<String>,
    /// Query children spawned from this session's startup pane.
    #[arg(short = 't', long, value_name = "SESSION", conflicts_with = "pane")]
    pub target: Option<String>,
}

#[derive(Args, Debug)]
pub struct MailArgs {
    /// Recipient session, or one of the reserved verbs: send, ls, read.
    #[arg(value_name = "TARGET|VERB")]
    pub target_or_verb: String,
    /// Arguments for the selected mail operation.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Parser, Debug)]
#[command(
    name = "tpp mail send",
    no_binary_name = true,
    disable_version_flag = true
)]
pub struct MailSendArgs {
    /// Recipient session, or parent.
    #[arg(value_name = "TARGET")]
    pub target: String,
    /// Use this text as the message body.
    #[arg(
        short = 'm',
        long = "message",
        value_name = "TEXT",
        conflicts_with_all = ["file", "stdin"]
    )]
    pub message: Option<String>,
    /// Read the message body from a file.
    #[arg(
        short = 'f',
        long = "file",
        value_name = "PATH",
        conflicts_with = "stdin"
    )]
    pub file: Option<PathBuf>,
    /// Read the message body from stdin.
    #[arg(long, conflicts_with = "file")]
    pub stdin: bool,
    /// Add a Subject header and use it as the ping excerpt.
    #[arg(long, value_name = "SUBJECT")]
    pub subject: Option<String>,
    /// Write the mail without pasting a notification.
    #[arg(long)]
    pub no_ping: bool,
    /// Suppress the recipient inbox path.
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(Parser, Debug)]
#[command(
    name = "tpp mail ls",
    no_binary_name = true,
    disable_version_flag = true
)]
pub struct MailLsArgs {
    /// Read another session's mailbox.
    #[arg(short = 't', long, value_name = "SESSION")]
    pub target: Option<String>,
    /// Show only unread inbox messages.
    #[arg(long)]
    pub unread: bool,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
    /// Print message ids only.
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(Parser, Debug)]
#[command(
    name = "tpp mail read",
    no_binary_name = true,
    disable_version_flag = true
)]
pub struct MailReadArgs {
    /// Message id in the selected inbox.
    #[arg(value_name = "ID")]
    pub id: String,
    /// Read another session's mailbox.
    #[arg(short = 't', long, value_name = "SESSION")]
    pub target: Option<String>,
    /// Emit the parsed message as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ReplyArgs {
    /// Message id in the caller's inbox.
    #[arg(value_name = "ID")]
    pub id: String,
    /// Use this text as the reply body.
    #[arg(
        short = 'm',
        long = "message",
        value_name = "TEXT",
        conflicts_with_all = ["file", "stdin"]
    )]
    pub message: Option<String>,
    /// Read the reply body from a file.
    #[arg(
        short = 'f',
        long = "file",
        value_name = "PATH",
        conflicts_with = "stdin"
    )]
    pub file: Option<PathBuf>,
    /// Read the reply body from stdin.
    #[arg(long, conflicts_with = "file")]
    pub stdin: bool,
    /// Write the reply without pasting a notification.
    #[arg(long)]
    pub no_ping: bool,
}

#[derive(Args, Debug)]
pub struct AttachArgs {
    /// Session to attach to. If omitted, pick (fzf when available, else the sole session).
    pub session: Option<String>,
}

#[derive(Args, Debug)]
pub struct SendArgs {
    /// Target session startup pane, pane:<NAME>, or parent (default: sole session or picker).
    #[arg(short = 't', long, value_name = "TARGET")]
    pub target: Option<String>,
    /// Read text from a file.
    #[arg(short = 'f', long, value_name = "PATH", conflicts_with = "stdin")]
    pub file: Option<PathBuf>,
    /// Read text from stdin.
    #[arg(long, conflicts_with = "file")]
    pub stdin: bool,
    /// Interpret args as key names (Enter, C-c, Escape) instead of literal text.
    #[arg(short = 'k', long)]
    pub keys: bool,
    /// Use bracketed paste (verbatim multi-line; good for TUIs).
    #[arg(short = 'p', long)]
    pub paste: bool,
    /// Press Enter after sending typed text.
    #[arg(short = 'e', long)]
    pub enter: bool,
    /// After Enter, confirm Claude/Codex did not leave pasted text unsubmitted.
    #[arg(long)]
    pub verify: bool,
    /// Text to send (literal unless --keys; use -- before option-looking text).
    #[arg(value_name = "TEXT")]
    pub text: Vec<String>,
}

#[derive(Args, Debug)]
pub struct PasteArgs {
    /// Target session startup pane, pane:<NAME>, or parent (default: sole session or picker).
    #[arg(short = 't', long, value_name = "TARGET")]
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
    /// Skip Claude/Codex pasted-content submission verification.
    #[arg(long)]
    pub no_verify: bool,
    /// Text to paste.
    #[arg(value_name = "TEXT")]
    pub text: Vec<String>,
}

#[derive(Args, Debug)]
pub struct BindArgs {
    /// Pane target name, used as pane:<NAME>.
    pub name: String,
    /// Bind the current pane from the selected backend.
    #[arg(long, conflicts_with = "pane")]
    pub here: bool,
    /// Bind an explicit pane target, such as %5, sess:1.0, or w5:p1.
    #[arg(long, value_name = "TMUX_TARGET")]
    pub pane: Option<String>,
    /// Role metadata stored on the pane.
    #[arg(long, default_value = "pane", value_name = "ROLE")]
    pub role: String,
}

#[derive(Args, Debug)]
pub struct UnbindArgs {
    /// Pane target name to remove.
    pub name: String,
}

#[derive(Args, Debug)]
pub struct TargetsArgs {}

#[derive(Args, Debug)]
pub struct CatArgs {
    /// Session, pane:<NAME>, or parent to print. Positional targets are still accepted.
    #[arg(short = 't', long, value_name = "TARGET")]
    pub target: Option<String>,
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
    /// Single session, pane:<NAME>, or parent to follow.
    #[arg(short = 't', long, value_name = "TARGET", conflicts_with = "sessions")]
    pub target: Option<String>,
    /// Sessions or targets to follow (default: the sole session, or a picker).
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
    /// Target session startup pane, pane:<NAME>, or parent (default: sole session or picker).
    #[arg(short = 't', long, value_name = "TARGET")]
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
pub struct ReapArgs {
    /// Show matching sessions without killing them.
    #[arg(long)]
    pub dry_run: bool,
    /// Idle threshold override for detached live sessions (examples: 1h, 90m, 1d, 0).
    #[arg(long, value_name = "DURATION")]
    pub ttl: Option<String>,
    /// Record output before killing, overriding config.
    #[arg(long, conflicts_with = "no_record")]
    pub record: bool,
    /// Skip recording output before killing, overriding config.
    #[arg(long)]
    pub no_record: bool,
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
