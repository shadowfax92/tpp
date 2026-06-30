//! Meta commands: `config`, `init`, `doctor`, `completions`.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use clap::CommandFactory;

use crate::cli::{Cli, CompletionsArgs, ConfigAction, ConfigArgs, InitArgs};
use crate::commands::Ctx;
use crate::config::STARTER_CONFIG;
use crate::output::{paint, Style};
use crate::paths::create_private_dir_all;
use crate::session;

pub fn config(ctx: &Ctx, args: ConfigArgs) -> Result<()> {
    match args.action.unwrap_or(ConfigAction::Show) {
        ConfigAction::Path => {
            println!("{}", ctx.config_path.display());
        }
        ConfigAction::Show => {
            let text = toml::to_string_pretty(&ctx.cfg).context("serializing config")?;
            print!("{text}");
        }
        ConfigAction::Edit => {
            ensure_config_exists(&ctx.config_path)?;
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            let status = std::process::Command::new(editor)
                .arg(&ctx.config_path)
                .status()
                .context("launching $EDITOR")?;
            if !status.success() {
                anyhow::bail!("editor exited with an error");
            }
        }
        ConfigAction::Init { force } => write_starter(&ctx.config_path, force)?,
    }
    Ok(())
}

fn ensure_config_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        write_starter(path, false)?;
    }
    Ok(())
}

fn write_starter(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        anyhow::bail!(
            "config already exists at {} (use --force to overwrite)",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    std::fs::write(path, STARTER_CONFIG).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub fn init(ctx: &Ctx, args: InitArgs) -> Result<()> {
    write_starter(&ctx.config_path, args.force)?;
    if args.fish {
        install_fish_completions()?;
    } else {
        eprintln!("tip: `tpp init --fish` installs shell completions");
    }
    Ok(())
}

fn install_fish_completions() -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .context("HOME not set")?;
    let dir = home.join(".config/fish/completions");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("tpp.fish");
    let mut buf = Vec::new();
    let mut cmd = Cli::command();
    clap_complete::generate(clap_complete::Shell::Fish, &mut cmd, "tpp", &mut buf);
    std::fs::write(&path, &buf).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub fn completions(args: CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let mut out = std::io::stdout();
    clap_complete::generate(args.shell, &mut cmd, "tpp", &mut out);
    out.flush().ok();
    Ok(())
}

pub fn doctor(ctx: &Ctx) -> Result<()> {
    let ok = |b: bool| {
        if b {
            paint("ok", Style::Green)
        } else {
            paint("MISSING", Style::Red)
        }
    };

    let tmux_version = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    println!("{}", paint("tpp doctor", Style::Bold));
    match &tmux_version {
        Some(v) => println!("  tmux:        {}  ({v})", ok(true)),
        None => println!("  tmux:        {}  — install tmux", ok(false)),
    }

    let fzf = std::process::Command::new("fzf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    println!(
        "  fzf:         {}  (optional; powers session pickers)",
        if fzf {
            paint("ok", Style::Green)
        } else {
            paint("absent", Style::Dim)
        }
    );

    println!(
        "  socket:      {}",
        ctx.tmux.socket().unwrap_or("(default tmux server)")
    );
    let cfg_exists = ctx.config_path.exists();
    println!(
        "  config:      {}  {}",
        ctx.config_path.display(),
        if cfg_exists {
            paint("(found)", Style::Dim)
        } else {
            paint("(defaults; run `tpp init`)", Style::Dim)
        }
    );
    println!("  state:       {}", ctx.paths.state_dir.display());

    let all = session::list(&ctx.tmux).unwrap_or_default();
    println!("  sessions:    {} total", all.len());

    if tmux_version.is_none() {
        anyhow::bail!("tmux not found on PATH");
    }
    Ok(())
}
