//! Hidden tmux-compat verbs. Each forwards to the real `tmux` (through tpp's socket) so an
//! existing `rmux`-based script works after `s/rmux/tpp/`. `new-session` additionally stamps
//! the tpp tags so sessions created this way show up in `tpp ls`.

use anyhow::Result;

use crate::cli::RawArgs;
use crate::commands::Ctx;
use crate::session::now_epoch;
use crate::tmux::{exact, tgt};

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return it.next().cloned();
        }
    }
    None
}

/// Best-effort: the command token at the end of a `new-session` invocation (skip it if the
/// last token is actually a flag's value).
fn trailing_command(args: &[String]) -> Option<String> {
    let last = args.last()?;
    if last.starts_with('-') {
        return None;
    }
    let s = flag_value(args, "-s");
    let c = flag_value(args, "-c");
    if Some(last) == s.as_ref() || Some(last) == c.as_ref() {
        return None;
    }
    Some(last.clone())
}

fn forward_print(ctx: &Ctx, verb: &str, rest: Vec<String>) -> Result<()> {
    let mut args = vec![verb.to_string()];
    args.extend(rest);
    match ctx.tmux.run(args) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("tpp: {e}");
            std::process::exit(1);
        }
    }
}

pub fn has_session(ctx: &Ctx, raw: RawArgs) -> ! {
    let mut args = vec!["has-session".to_string()];
    args.extend(raw.args);
    std::process::exit(if ctx.tmux.ok(args) { 0 } else { 1 });
}

pub fn new_session(ctx: &Ctx, raw: RawArgs) -> Result<()> {
    let mut args = vec!["new-session".to_string()];
    args.extend(raw.args.clone());
    ctx.tmux.run(args)?;

    if let Some(name) = flag_value(&raw.args, "-s") {
        let target = tgt(&name);
        if ctx.cfg.new.remain_on_exit {
            let _ = ctx
                .tmux
                .run(["set-option", "-t", &target, "-w", "remain-on-exit", "on"]);
        }
        let dir = flag_value(&raw.args, "-c").unwrap_or_default();
        let cmd = trailing_command(&raw.args)
            .map(|c| {
                std::path::Path::new(&c)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&c)
                    .to_string()
            })
            .unwrap_or_else(|| "shell".to_string());
        let set = |k: &str, v: &str| {
            let _ = ctx.tmux.run(["set-option", "-t", &target, k, v]);
        };
        set("@tpp", "1");
        set("@tpp_scope", ctx.scope.as_deref().unwrap_or(""));
        set("@tpp_dir", &dir);
        set("@tpp_cmd", &cmd);
        set("@tpp_created", &now_epoch().to_string());
    }
    Ok(())
}

pub fn attach_session(ctx: &Ctx, raw: RawArgs) -> Result<()> {
    // Inside tmux, switch the client instead of nesting an attach.
    if std::env::var_os("TMUX").is_some() {
        if let Some(t) = flag_value(&raw.args, "-t") {
            ctx.tmux.run(["switch-client", "-t", &exact(&t)])?;
            return Ok(());
        }
    }
    let mut args = vec!["attach-session".to_string()];
    args.extend(raw.args);
    ctx.tmux.exec(args)
}

pub fn forward(ctx: &Ctx, verb: &str, raw: RawArgs) -> Result<()> {
    forward_print(ctx, verb, raw.args)
}

/// Raw passthrough: `tpp x -- <tmux args>`.
pub fn raw(ctx: &Ctx, raw: RawArgs) -> Result<()> {
    if raw.args.is_empty() {
        anyhow::bail!("usage: tpp x -- <tmux args...>");
    }
    match ctx.tmux.run(raw.args) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("tpp: {e}");
            std::process::exit(1);
        }
    }
}
