//! Hidden tmux-compat verbs. Each forwards to the real `tmux` (through tpp's socket) so an
//! existing `rmux`-based script works after `s/rmux/tpp/`. `new-session` additionally stamps
//! the tpp tags so sessions created this way show up in `tpp ls`.

use anyhow::Result;

use crate::cli::RawArgs;
use crate::commands::Ctx;
use crate::session::{self, now_epoch};
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

fn prefix_flag_value<F>(args: &mut [String], flag: &str, rewrite: F)
where
    F: Fn(&str) -> String,
{
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == flag {
            args[i + 1] = rewrite(&args[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }
}

fn prefix_new_session_args(cfg: &crate::config::Config, mut args: Vec<String>) -> Vec<String> {
    prefix_flag_value(&mut args, "-s", |name| session::prefixed_name(cfg, name));
    args
}

fn prefix_target_args(cfg: &crate::config::Config, mut args: Vec<String>) -> Vec<String> {
    prefix_flag_value(&mut args, "-t", |target| {
        session::prefixed_target(cfg, target)
    });
    args
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
    args.extend(prefix_target_args(&ctx.cfg, raw.args));
    std::process::exit(if ctx.tmux.ok(args) { 0 } else { 1 });
}

pub fn new_session(ctx: &Ctx, raw: RawArgs) -> Result<()> {
    let raw_args = prefix_new_session_args(&ctx.cfg, raw.args.clone());
    let mut args = vec!["new-session".to_string()];
    args.extend(raw_args.clone());
    ctx.tmux.run(args)?;

    if let Some(name) = flag_value(&raw_args, "-s") {
        let target = tgt(&name);
        if ctx.cfg.new.remain_on_exit {
            let _ = ctx
                .tmux
                .run(["set-option", "-t", &target, "-w", "remain-on-exit", "on"]);
        }
        let dir = flag_value(&raw_args, "-c").unwrap_or_default();
        let cmd = trailing_command(&raw_args)
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
    let raw_args = prefix_target_args(&ctx.cfg, raw.args);
    // Inside tmux, switch the client instead of nesting an attach.
    if std::env::var_os("TMUX").is_some() {
        if let Some(t) = flag_value(&raw_args, "-t") {
            ctx.tmux.run(["switch-client", "-t", &exact(&t)])?;
            return Ok(());
        }
    }
    let mut args = vec!["attach-session".to_string()];
    args.extend(raw_args);
    ctx.tmux.exec(args)
}

pub fn forward(ctx: &Ctx, verb: &str, raw: RawArgs) -> Result<()> {
    forward_print(ctx, verb, prefix_target_args(&ctx.cfg, raw.args))
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

#[cfg(test)]
mod tests {
    use super::{prefix_new_session_args, prefix_target_args};
    use crate::config::Config;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn prefix_new_session_args_rewrites_session_name_flag() {
        let rewritten = prefix_new_session_args(
            &Config::default(),
            args(&["-d", "-s", "api", "-c", "/tmp", "cmd"]),
        );

        assert_eq!(
            rewritten,
            args(&["-d", "-s", "tpp/api", "-c", "/tmp", "cmd"])
        );
    }

    #[test]
    fn prefix_target_args_rewrites_target_session_component() {
        let rewritten = prefix_target_args(&Config::default(), args(&["-t", "api:0", "-p"]));

        assert_eq!(rewritten, args(&["-t", "tpp/api:0", "-p"]));
    }
}
