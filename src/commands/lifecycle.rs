//! Session lifecycle: `run`, `new`, `ls`, `attach`, `rm`, `exit`, `clear`, `has`, `rename`.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::cli::{AttachArgs, ExitArgs, HasArgs, LsArgs, NewArgs, RenameArgs, RmArgs, RunArgs};
use crate::commands::io::{record_session, run_wait};
use crate::commands::{code, current_session, die, select, Ctx};
use crate::config::LsDefault;
use crate::output::{paint, print_json, Style};
use crate::session::{self, now_epoch, NewOpts};
use crate::store::Store;
use crate::tmux::exact;

fn cwd_string() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Turn a command/dir into a tmux-safe session slug.
fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "tpp".to_string()
    } else {
        trimmed
    }
}

/// Pick an unused session name from a base, appending -2, -3, … on collision.
fn unique_name(ctx: &Ctx, base: &str) -> String {
    let base = session::prefixed_name(&ctx.cfg, base);
    if !session::exists(&ctx.tmux, &base) {
        return base;
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if !session::exists(&ctx.tmux, &candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn auto_name_for_command(ctx: &Ctx, command: &[String]) -> String {
    let base = command
        .first()
        .map(|c| {
            Path::new(c)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(c)
        })
        .unwrap_or("shell");
    unique_name(ctx, &slug(base))
}

pub fn run(ctx: &Ctx, args: RunArgs) -> Result<()> {
    let dir = args.dir.clone().or_else(cwd_string);
    let name = match &args.name {
        Some(n) => {
            let name = session::prefixed_name(&ctx.cfg, n);
            if session::exists(&ctx.tmux, &name) {
                die(1, format!("session already exists: {n}"));
            }
            name
        }
        None => auto_name_for_command(ctx, &args.command),
    };

    session::create(
        &ctx.tmux,
        &ctx.cfg,
        NewOpts {
            name: name.clone(),
            dir,
            command: args.command.clone(),
            scope: ctx.scope.as_deref(),
            width: None,
            height: None,
        },
    )?;

    if args.wait {
        let status = run_wait(ctx, &name)?;
        if args.record {
            let _ = record_session(ctx, &name);
        }
        let _ = ctx.tmux.run(["kill-session", "-t", &exact(&name)]);
        std::process::exit(status);
    }

    // The name is the handle — stdout only, so `s=$(tpp run -- cmd)` works.
    println!("{name}");
    if !ctx.quiet {
        eprintln!(
            "started {name}  ({}attach: tpp attach {name}  ·  cat: tpp cat {name})",
            ctx.tmux.socket_flag()
        );
    }
    Ok(())
}

pub fn new(ctx: &Ctx, args: NewArgs) -> Result<()> {
    let name = match &args.name {
        Some(n) => session::prefixed_name(&ctx.cfg, n),
        None => {
            let base = cwd_string()
                .as_deref()
                .and_then(|d| Path::new(d).file_name().and_then(|s| s.to_str()).map(slug))
                .unwrap_or_else(|| "tpp".to_string());
            unique_name(ctx, &base)
        }
    };

    if session::exists(&ctx.tmux, &name) {
        if args.attach {
            println!("{name}");
            return Ok(());
        }
        die(1, format!("session already exists: {name}"));
    }

    let dir = args.dir.clone().or_else(cwd_string);
    session::create(
        &ctx.tmux,
        &ctx.cfg,
        NewOpts {
            name: name.clone(),
            dir,
            command: args.command.clone(),
            scope: ctx.scope.as_deref(),
            width: None,
            height: None,
        },
    )?;

    println!("{name}");
    if !ctx.quiet {
        eprintln!("created {name}  (attach: tpp attach {name})");
    }
    Ok(())
}

#[derive(Serialize)]
struct LsRow {
    name: String,
    status: String,
    scope: String,
    dir: String,
    command: String,
    age: String,
}

fn humanize_age(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

fn ls_scope_filter<'a>(
    _args: &LsArgs,
    _default: LsDefault,
    _current_scope: Option<&'a str>,
) -> Option<&'a str> {
    None
}

pub fn ls(ctx: &Ctx, args: LsArgs) -> Result<()> {
    let scope_filter = ls_scope_filter(&args, ctx.cfg.ls.default, ctx.scope.as_deref());

    let live = session::list(&ctx.tmux, scope_filter)?;
    let now = now_epoch();

    let mut rows: Vec<LsRow> = live
        .iter()
        .map(|s| LsRow {
            name: s.name.clone(),
            status: s.status().to_string(),
            scope: s.scope.clone(),
            dir: s.dir.clone(),
            command: s.command.clone(),
            age: humanize_age(now - s.created),
        })
        .collect();

    let show_exited = args.exited || (!args.no_exited && ctx.cfg.ls.show_exited_hours > 0);
    if show_exited {
        let store_socket = ctx.tmux.store_socket();
        let store = Store::new(&ctx.paths, store_socket.as_deref());
        let hours = if args.exited && ctx.cfg.ls.show_exited_hours == 0 {
            24
        } else {
            ctx.cfg.ls.show_exited_hours
        };
        let live_names: std::collections::HashSet<&str> =
            live.iter().map(|s| s.name.as_str()).collect();
        for rec in store.recent(hours)? {
            if live_names.contains(rec.name.as_str()) {
                continue;
            }
            if let Some(scope) = scope_filter {
                if rec.scope != scope {
                    continue;
                }
            }
            rows.push(LsRow {
                name: rec.name,
                status: "recorded".to_string(),
                scope: rec.scope,
                dir: rec.dir,
                command: rec.command,
                age: humanize_age(now - rec.exited_at),
            });
        }
    }

    if ctx.json {
        return print_json(&rows);
    }
    if ctx.quiet {
        for r in &rows {
            println!("{}", r.name);
        }
        return Ok(());
    }
    if rows.is_empty() {
        let where_ = if let Some(scope) = scope_filter {
            format!(" in {}", crate::scope::label(scope))
        } else {
            String::new()
        };
        eprintln!("no tpp sessions{where_}");
        return Ok(());
    }

    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let status_w = rows
        .iter()
        .map(|r| r.status.len())
        .max()
        .unwrap_or(6)
        .max(6);
    for r in &rows {
        let status = match r.status.as_str() {
            "running" => paint(&r.status, Style::Green),
            "attached" => paint(&r.status, Style::Cyan),
            "exited" | "recorded" => paint(&r.status, Style::Yellow),
            _ => r.status.clone(),
        };
        // Pad on the uncolored text so columns line up regardless of ANSI codes.
        let status_pad = " ".repeat(status_w.saturating_sub(r.status.len()));
        println!(
            "{:<name_w$}  {}{}  {:>4}  {}",
            r.name,
            status,
            status_pad,
            paint(&r.age, Style::Dim),
            paint(&r.command, Style::Dim),
            name_w = name_w,
        );
    }
    Ok(())
}

pub fn attach(ctx: &Ctx, args: AttachArgs) -> Result<()> {
    let name = select::one(ctx, args.session.as_deref(), "attach to")?;

    if !session::exists(&ctx.tmux, &name) {
        die(code::NOT_FOUND, format!("no such session: {name}"));
    }

    // Inside tmux we can't nest an attach — switch the current client instead.
    if std::env::var_os("TMUX").is_some() {
        ctx.tmux.run(["switch-client", "-t", &exact(&name)])?;
        return Ok(());
    }
    ctx.tmux.exec(["attach-session", "-t", &exact(&name)])
}

pub fn rm(ctx: &Ctx, args: RmArgs) -> Result<()> {
    let targets: Vec<String> = if args.all {
        session::list(&ctx.tmux, ctx.scope.as_deref())?
            .into_iter()
            .map(|s| s.name)
            .collect()
    } else {
        select::many(ctx, &args.sessions, "remove")?
    };

    if targets.is_empty() {
        die(2, "name a session to remove, or pass --all");
    }

    let mut removed = 0;
    for name in &targets {
        if !session::exists(&ctx.tmux, name) {
            if !ctx.quiet {
                eprintln!("tpp: no such session: {name}");
            }
            continue;
        }
        if args.record {
            let _ = record_session(ctx, name);
        }
        match ctx.tmux.run(["kill-session", "-t", &exact(name)]) {
            Ok(_) => removed += 1,
            Err(e) => eprintln!("tpp: failed to remove {name}: {e}"),
        }
    }
    if !ctx.quiet {
        eprintln!("removed {removed} session(s)");
    }
    Ok(())
}

pub fn exit(ctx: &Ctx, args: ExitArgs) -> Result<()> {
    let name = if let Some(name) = args.session.as_deref() {
        select::one(ctx, Some(name), "exit")?
    } else if let Some(name) = current_session(&ctx.tmux) {
        name
    } else {
        select::one(ctx, None, "exit")?
    };
    if !args.no_record && session::exists(&ctx.tmux, &name) {
        let _ = record_session(ctx, &name);
    }
    let _ = ctx.tmux.run(["kill-session", "-t", &exact(&name)]);
    if !ctx.quiet {
        eprintln!("exited {name}");
    }
    Ok(())
}

pub fn clear(ctx: &Ctx) -> Result<()> {
    let store_socket = ctx.tmux.store_socket();
    let n = Store::new(&ctx.paths, store_socket.as_deref()).clear()?;
    if ctx.json {
        print_json(&serde_json::json!({ "cleared": n }))?;
    } else if !ctx.quiet {
        println!("cleared {n} recorded exited session(s)");
    }
    Ok(())
}

pub fn has(ctx: &Ctx, args: HasArgs) -> Result<()> {
    let name = match args.session.or(args.target) {
        Some(n) => n,
        None => die(2, "usage: tpp has <session>"),
    };
    let name = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, &name);
    std::process::exit(if session::exists(&ctx.tmux, &name) {
        0
    } else {
        1
    });
}

pub fn rename(ctx: &Ctx, args: RenameArgs) -> Result<()> {
    let (session_name, new_name) = match args.names.as_slice() {
        [session_name, new_name] => (
            session::resolve_existing_name(&ctx.tmux, &ctx.cfg, session_name),
            session::prefixed_name(&ctx.cfg, new_name),
        ),
        [new_name] => (
            select::one(ctx, None, "rename")?,
            session::prefixed_name(&ctx.cfg, new_name),
        ),
        _ => die(2, "usage: tpp rename [SESSION] <NEW_NAME>"),
    };

    if !session::exists(&ctx.tmux, &session_name) {
        die(code::NOT_FOUND, format!("no such session: {session_name}"));
    }
    ctx.tmux
        .run(["rename-session", "-t", &exact(&session_name), &new_name])?;
    if !ctx.quiet {
        eprintln!("renamed {session_name} -> {new_name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{humanize_age, ls_scope_filter, slug};
    use crate::cli::LsArgs;
    use crate::config::LsDefault;

    #[test]
    fn slug_sanitizes() {
        assert_eq!(slug("npm test"), "npm-test");
        assert_eq!(slug("/usr/bin/bash"), "usr-bin-bash");
        assert_eq!(slug("feat/build-x"), "feat-build-x");
        assert_eq!(slug("!!!"), "tpp");
    }

    #[test]
    fn age_buckets() {
        assert_eq!(humanize_age(30), "30s");
        assert_eq!(humanize_age(120), "2m");
        assert_eq!(humanize_age(7200), "2h");
        assert_eq!(humanize_age(172_800), "2d");
    }

    #[test]
    fn ls_scope_filter_is_universal() {
        let scope = Some("/tmp/worktree");

        assert_eq!(
            ls_scope_filter(&LsArgs::default(), LsDefault::Scope, scope),
            None
        );
        assert_eq!(
            ls_scope_filter(&LsArgs::default(), LsDefault::All, scope),
            None
        );
        assert_eq!(
            ls_scope_filter(
                &LsArgs {
                    all: true,
                    ..LsArgs::default()
                },
                LsDefault::Scope,
                scope,
            ),
            None
        );
    }
}
