//! Hidden tmux-compat verbs. Each forwards to the real `tmux` (through tpp's socket) so an
//! existing `rmux`-based script works after `s/rmux/tpp/`. `new-session` additionally stamps
//! the tpp tags so sessions created this way show up in `tpp ls`.

use anyhow::Result;

use crate::cli::RawArgs;
use crate::commands::{require_session_pane_target, Ctx};
use crate::session::{self, now_epoch};
use crate::tmux::{exact, tgt};

fn rewrite_short_flag_value<F>(
    args: &mut [String],
    index: usize,
    flag: &str,
    mut rewrite: F,
) -> usize
where
    F: FnMut(&str) -> String,
{
    if args[index] == flag {
        if index + 1 < args.len() {
            args[index + 1] = rewrite(&args[index + 1]);
        }
        return index + 2;
    }
    if let Some(value) = args[index].strip_prefix(flag) {
        if !value.is_empty() {
            args[index] = format!("{flag}{}", rewrite(value));
        }
    }
    index + 1
}

fn new_session_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-c" | "-e" | "-F" | "-f" | "-n" | "-s" | "-t" | "-x" | "-y"
    )
}

#[derive(Debug, Default)]
struct NewSessionMeta {
    name: Option<String>,
    dir: Option<String>,
    command: Option<String>,
}

fn rewrite_new_session_args(
    cfg: &crate::config::Config,
    mut args: Vec<String>,
) -> (Vec<String>, NewSessionMeta) {
    let mut meta = NewSessionMeta::default();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            meta.command = args.get(i + 1).cloned();
            break;
        }
        if !args[i].starts_with('-') {
            meta.command = Some(args[i].clone());
            break;
        }
        if args[i] == "-s" || args[i].starts_with("-s") {
            meta.name = short_flag_value_at(&args, i, "-s").map(|name| {
                let prefixed = session::prefixed_name(cfg, &name);
                if args[i] == "-s" {
                    if i + 1 < args.len() {
                        args[i + 1] = prefixed.clone();
                    }
                } else {
                    args[i] = format!("-s{prefixed}");
                }
                prefixed
            });
            i = skip_short_flag_value(&args, i, "-s");
        } else if args[i] == "-c" || args[i].starts_with("-c") {
            meta.dir = short_flag_value_at(&args, i, "-c");
            i = skip_short_flag_value(&args, i, "-c");
        } else if args[i] == "-t" || args[i].starts_with("-t") {
            i = rewrite_short_flag_value(&mut args, i, "-t", |target| {
                session::prefixed_target(cfg, target)
            });
        } else if new_session_option_takes_value(&args[i]) {
            i += 2;
        } else {
            i += 1;
        }
    }
    (args, meta)
}

fn short_flag_value_at(args: &[String], index: usize, flag: &str) -> Option<String> {
    if args[index] == flag {
        args.get(index + 1).cloned()
    } else {
        args[index]
            .strip_prefix(flag)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }
}

fn skip_short_flag_value(args: &[String], index: usize, flag: &str) -> usize {
    if args[index] == flag {
        index + 2
    } else {
        index + 1
    }
}

fn short_flag_takes_value(arg: &str, value_flags: &[&str]) -> bool {
    value_flags.contains(&arg)
}

fn find_short_flag_value(args: &[String], flag: &str, value_flags: &[&str]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            break;
        }
        if args[i] == flag || args[i].starts_with(flag) {
            return short_flag_value_at(args, i, flag);
        }
        if short_flag_takes_value(&args[i], value_flags) {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

fn rewrite_target_args_with_values<F>(
    mut args: Vec<String>,
    value_flags: &[&str],
    mut rewrite: F,
) -> Vec<String>
where
    F: FnMut(&str) -> String,
{
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            break;
        }
        if args[i] == "-t" || args[i].starts_with("-t") {
            i = rewrite_short_flag_value(&mut args, i, "-t", &mut rewrite);
        } else if short_flag_takes_value(&args[i], value_flags) {
            i += 2;
        } else {
            i += 1;
        }
    }
    args
}

fn prefix_target_args_with_values(
    cfg: &crate::config::Config,
    args: Vec<String>,
    value_flags: &[&str],
) -> Vec<String> {
    rewrite_target_args_with_values(args, value_flags, |target| {
        session::prefixed_target(cfg, target)
    })
}

fn prefix_target_args(cfg: &crate::config::Config, args: Vec<String>) -> Vec<String> {
    prefix_target_args_with_values(cfg, args, &[])
}

fn is_plain_session_target(target: &str) -> bool {
    let raw = target.trim().trim_start_matches('=');
    !raw.starts_with(['%', '@', '$', '{', '!']) && !raw.contains([':', '.'])
}

fn prefix_forward_args<F>(
    cfg: &crate::config::Config,
    verb: &str,
    args: Vec<String>,
    mut resolve_session: F,
) -> Vec<String>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut rewrite_io_targets = |args, value_flags: &[&str]| {
        rewrite_target_args_with_values(args, value_flags, |target| {
            let prefixed = session::prefixed_target(cfg, target);
            if is_plain_session_target(&prefixed) {
                resolve_session(&prefixed).unwrap_or(prefixed)
            } else {
                prefixed
            }
        })
    };
    match verb {
        "kill-session" => prefix_target_args_with_values(cfg, args, &[]),
        "paste-buffer" => rewrite_io_targets(args, &["-b", "-s"]),
        "send-keys" => rewrite_io_targets(args, &["-N"]),
        "capture-pane" => rewrite_io_targets(args, &["-b", "-E", "-S"]),
        _ => args,
    }
}

fn command_label(command: Option<&str>) -> String {
    command
        .map(|c| {
            std::path::Path::new(c)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(c)
                .to_string()
        })
        .unwrap_or_else(|| "shell".to_string())
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
    let (raw_args, meta) = rewrite_new_session_args(&ctx.cfg, raw.args.clone());
    let existed = meta
        .name
        .as_deref()
        .map(|name| session::exists(&ctx.tmux, name))
        .unwrap_or(false);
    let mut args = vec!["new-session".to_string()];
    args.extend(raw_args.clone());
    ctx.tmux.run(args)?;

    if let Some(name) = meta.name {
        let target = tgt(&name);
        if ctx.cfg.new.remain_on_exit {
            let _ = ctx
                .tmux
                .run(["set-option", "-t", &target, "-w", "remain-on-exit", "on"]);
        }
        let set = |k: &str, v: &str| {
            let _ = ctx.tmux.run(["set-option", "-t", &target, k, v]);
        };
        set("@tpp", "1");
        set("@tpp_dir", meta.dir.as_deref().unwrap_or(""));
        set("@tpp_cmd", &command_label(meta.command.as_deref()));
        set("@tpp_created", &now_epoch().to_string());
        if !existed {
            session::stamp_origin_pane(&ctx.tmux, &target);
        }
    }
    Ok(())
}

pub fn attach_session(ctx: &Ctx, raw: RawArgs) -> Result<()> {
    let raw_args = prefix_target_args_with_values(&ctx.cfg, raw.args, &["-c"]);
    // Inside tmux, switch the client instead of nesting an attach.
    if std::env::var_os("TMUX").is_some() {
        if let Some(t) = find_short_flag_value(&raw_args, "-t", &["-c"]) {
            ctx.tmux.run(["switch-client", "-t", &exact(&t)])?;
            return Ok(());
        }
    }
    let mut args = vec!["attach-session".to_string()];
    args.extend(raw_args);
    ctx.tmux.exec(args)
}

pub fn forward(ctx: &Ctx, verb: &str, raw: RawArgs) -> Result<()> {
    let args = prefix_forward_args(&ctx.cfg, verb, raw.args, |target| {
        session::exists(&ctx.tmux, target).then(|| require_session_pane_target(&ctx.tmux, target))
    });
    forward_print(ctx, verb, args)
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
    use super::{prefix_forward_args, prefix_target_args, rewrite_new_session_args};
    use crate::config::Config;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    fn prefix_new_session_args(cfg: &Config, args: Vec<String>) -> Vec<String> {
        rewrite_new_session_args(cfg, args).0
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

    #[test]
    fn prefix_new_session_args_rewrites_attached_session_name_flag() {
        let rewritten = prefix_new_session_args(&Config::default(), args(&["-d", "-sapi"]));

        assert_eq!(rewritten, args(&["-d", "-stpp/api"]));
    }

    #[test]
    fn prefix_target_args_rewrites_attached_target_flag() {
        let rewritten = prefix_target_args(&Config::default(), args(&["-tapi:0", "-p"]));

        assert_eq!(rewritten, args(&["-ttpp/api:0", "-p"]));
    }

    #[test]
    fn prefix_target_args_leaves_tmux_ids_unchanged() {
        let rewritten = prefix_target_args(&Config::default(), args(&["-t", "%0"]));

        assert_eq!(rewritten, args(&["-t", "%0"]));
    }

    #[test]
    fn prefix_new_session_args_stops_before_shell_command() {
        let rewritten = prefix_new_session_args(
            &Config::default(),
            args(&["-d", "-s", "api", "cmd", "-s", "inner"]),
        );

        assert_eq!(
            rewritten,
            args(&["-d", "-s", "tpp/api", "cmd", "-s", "inner"])
        );
    }

    #[test]
    fn prefix_rewriters_stop_at_double_dash() {
        let new_args = prefix_new_session_args(
            &Config::default(),
            args(&["-d", "-s", "api", "--", "cmd", "-s", "inner"]),
        );
        let target_args = prefix_target_args(
            &Config::default(),
            args(&["-t", "api", "--", "-t", "inner"]),
        );

        assert_eq!(
            new_args,
            args(&["-d", "-s", "tpp/api", "--", "cmd", "-s", "inner"])
        );
        assert_eq!(target_args, args(&["-t", "tpp/api", "--", "-t", "inner"]));
    }

    #[test]
    fn prefix_forward_args_does_not_rewrite_non_target_value() {
        let rewritten = prefix_forward_args(
            &Config::default(),
            "set-buffer",
            args(&["-b", "-tmp", "data"]),
            |_| None,
        );

        assert_eq!(rewritten, args(&["-b", "-tmp", "data"]));
    }

    #[test]
    fn prefix_forward_args_skips_value_operands_for_target_verbs() {
        let rewritten = prefix_forward_args(
            &Config::default(),
            "paste-buffer",
            args(&["-b", "-tmp", "-t", "api"]),
            |_| None,
        );

        assert_eq!(rewritten, args(&["-b", "-tmp", "-t", "tpp/api"]));
    }

    #[test]
    fn prefix_forward_args_resolves_only_plain_io_session_targets() {
        for target in ["api:1.2", "%5", "@7", "$2", "{last}", "!3"] {
            let rewritten = prefix_forward_args(
                &Config::default(),
                "send-keys",
                args(&["-t", target, "Enter"]),
                |_| Some("%42".to_string()),
            );

            assert_ne!(rewritten[1], "%42", "target {target}");
        }

        let rewritten = prefix_forward_args(
            &Config::default(),
            "capture-pane",
            args(&["-ttpp/api", "-p"]),
            |target| (target == "tpp/api").then(|| "%42".to_string()),
        );

        assert_eq!(rewritten, args(&["-t%42", "-p"]));
    }

    #[test]
    fn rewrite_new_session_args_returns_pre_payload_metadata() {
        let (rewritten, meta) = rewrite_new_session_args(
            &Config::default(),
            args(&[
                "-d", "-s", "api", "-c", "/tmp", "--", "sh", "-c", "pwd", "-s", "inner",
            ]),
        );

        assert_eq!(
            rewritten,
            args(&["-d", "-s", "tpp/api", "-c", "/tmp", "--", "sh", "-c", "pwd", "-s", "inner",])
        );
        assert_eq!(meta.name.as_deref(), Some("tpp/api"));
        assert_eq!(meta.dir.as_deref(), Some("/tmp"));
        assert_eq!(meta.command.as_deref(), Some("sh"));
    }

    #[test]
    fn rewrite_new_session_args_ignores_payload_flags_for_metadata() {
        let (_rewritten, meta) = rewrite_new_session_args(
            &Config::default(),
            args(&["-d", "-s", "api", "--", "sh", "-c", "pwd", "-s", "inner"]),
        );

        assert_eq!(meta.name.as_deref(), Some("tpp/api"));
        assert_eq!(meta.dir.as_deref(), None);
        assert_eq!(meta.command.as_deref(), Some("sh"));
    }

    #[test]
    fn rewrite_new_session_args_prefixes_target_group() {
        let (rewritten, meta) = rewrite_new_session_args(
            &Config::default(),
            args(&["-d", "-s", "child", "-t", "api"]),
        );

        assert_eq!(rewritten, args(&["-d", "-s", "tpp/child", "-t", "tpp/api"]));
        assert_eq!(meta.name.as_deref(), Some("tpp/child"));
    }
}
