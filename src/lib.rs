//! tpp — tmux++. An ergonomic wrapper around the `tmux` binary for sharing, running,
//! capturing, and pasting into sessions. See `docs/DESIGN.md` for the model.

pub mod cli;
pub mod commands;
pub mod config;
pub mod output;
pub mod paths;
pub mod session;
pub mod store;
pub mod tmux;
pub mod watch;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Cmd, LsArgs};
use commands::{compat, io, lifecycle, meta, pane, Ctx};
use config::Config;
use paths::Paths;
use store::Store;
use tmux::Tmux;

/// Parse args, build the shared context, and dispatch.
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let paths = Paths::from_env()?;
    let config_path = cli.config.clone().unwrap_or_else(|| paths.config_file());
    let cfg = Config::load(&config_path)?;

    // CLI flag wins over config for the socket; empty falls back to the shared tmux server.
    let socket = cli.socket.clone().or_else(|| cfg.socket.clone());
    let tmux = Tmux::new(socket);

    // Forget stale exited records (best-effort; never fails a command).
    let store_socket = tmux.store_socket();
    let _ = Store::new(&paths, store_socket.as_deref()).prune(cfg.exit.prune_hours);

    let ctx = Ctx {
        tmux,
        cfg,
        paths,
        config_path,
        json: cli.json,
        quiet: cli.quiet,
    };

    // Bare `tpp` lists every tpp session on the selected tmux server.
    let cmd = cli.cmd.unwrap_or_else(|| Cmd::Ls(LsArgs::default()));

    match cmd {
        Cmd::Run(a) => lifecycle::run(&ctx, a),
        Cmd::New(a) => lifecycle::new(&ctx, a),
        Cmd::Ls(a) => lifecycle::ls(&ctx, a),
        Cmd::Attach(a) => lifecycle::attach(&ctx, a),
        Cmd::Send(a) => io::send(&ctx, a),
        Cmd::Paste(a) => io::paste(&ctx, a),
        Cmd::Bind(a) => pane::bind(&ctx, a),
        Cmd::Unbind(a) => pane::unbind(&ctx, a),
        Cmd::Targets(a) => pane::targets(&ctx, a),
        Cmd::Cat(a) => io::cat(&ctx, a),
        Cmd::Tail(a) => io::tail(&ctx, a),
        Cmd::Wait(a) => io::wait(&ctx, a),
        Cmd::Rm(a) => lifecycle::rm(&ctx, a),
        Cmd::Reap(a) => lifecycle::reap(&ctx, a),
        Cmd::Exit(a) => lifecycle::exit(&ctx, a),
        Cmd::Clear => lifecycle::clear(&ctx),
        Cmd::Has(a) => lifecycle::has(&ctx, a),
        Cmd::Rename(a) => lifecycle::rename(&ctx, a),
        Cmd::Config(a) => meta::config(&ctx, a),
        Cmd::Init(a) => meta::init(&ctx, a),
        Cmd::Doctor => meta::doctor(&ctx),
        Cmd::Completions(a) => meta::completions(a),

        Cmd::HasSession(r) => compat::has_session(&ctx, r),
        Cmd::NewSession(r) => compat::new_session(&ctx, r),
        Cmd::AttachSession(r) => compat::attach_session(&ctx, r),
        Cmd::KillSession(r) => compat::forward(&ctx, "kill-session", r),
        Cmd::ListSessions(r) => compat::forward(&ctx, "list-sessions", r),
        Cmd::SetBuffer(r) => compat::forward(&ctx, "set-buffer", r),
        Cmd::PasteBuffer(r) => compat::forward(&ctx, "paste-buffer", r),
        Cmd::SendKeys(r) => compat::forward(&ctx, "send-keys", r),
        Cmd::CapturePane(r) => compat::forward(&ctx, "capture-pane", r),
        Cmd::X(r) => compat::raw(&ctx, r),
    }
}
