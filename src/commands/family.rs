//! Stateless parent/child relationships derived from tmux user-options.

use anyhow::Result;
use serde::Serialize;

use crate::cli::ChildrenArgs;
use crate::commands::{code, die, no_such_session, require_session_pane_target, Ctx};
use crate::output::{paint, print_json, Style};
use crate::session::{self, now_epoch};
use crate::tmux::Tmux;

pub const PARENT_TARGET: &str = "parent";

pub fn is_parent_target(target: &str) -> bool {
    target.trim() == PARENT_TARGET
}

/// Resolve a tmux target to its canonical raw `%pane_id`.
pub fn canonical_pane(tmux: &Tmux, target: &str) -> Option<String> {
    tmux.run(["display-message", "-p", "-t", target, "#{pane_id}"])
        .ok()
        .map(|pane| pane.trim().to_string())
        .filter(|pane| !pane.is_empty())
}

fn caller_pane_from_env(message: &str) -> String {
    if std::env::var_os("TMUX").is_none() {
        die(code::USAGE, message);
    }
    std::env::var("TMUX_PANE")
        .ok()
        .map(|pane| pane.trim().to_string())
        .filter(|pane| !pane.is_empty())
        .unwrap_or_else(|| die(code::USAGE, message))
}

/// Resolve the calling tpp session's recorded parent to a live raw pane id.
///
/// This is the shared resolution chain used by pane I/O and future commands such as mail.
pub fn resolve_parent_pane(ctx: &Ctx) -> String {
    let caller =
        caller_pane_from_env("the parent target requires running inside tmux with TMUX_PANE set");
    let caller = canonical_pane(&ctx.tmux, &caller).unwrap_or_else(|| {
        die(
            code::USAGE,
            "could not resolve the caller's TMUX_PANE in tmux",
        )
    });
    let session_name = ctx
        .tmux
        .run(["display-message", "-p", "-t", &caller, "#{session_name}"])
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| die(code::USAGE, "could not resolve the caller's tmux session"));
    let parent = session::parent_pane(&ctx.tmux, &session_name)
        .unwrap_or_else(|| die(code::NOT_FOUND, "no parent recorded"));
    canonical_pane(&ctx.tmux, &parent).unwrap_or_else(|| die(code::NOT_FOUND, "parent pane gone"))
}

#[derive(Debug, Serialize)]
struct ChildRow {
    name: String,
    dir: String,
    command: String,
    created: i64,
    state: String,
    status: String,
    age: String,
}

fn humanize_age(secs: i64) -> String {
    let secs = secs.max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

fn query_pane(ctx: &Ctx, args: &ChildrenArgs) -> String {
    if let Some(target) = &args.pane {
        return canonical_pane(&ctx.tmux, target)
            .unwrap_or_else(|| die(code::NOT_FOUND, format!("pane not found: {target}")));
    }
    if let Some(target) = &args.target {
        let name = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, target);
        if !session::exists(&ctx.tmux, &name) {
            no_such_session(&name);
        }
        let origin = require_session_pane_target(&ctx.tmux, &name);
        return canonical_pane(&ctx.tmux, &origin)
            .unwrap_or_else(|| die(code::NOT_FOUND, format!("origin pane gone for {name}")));
    }

    let caller = caller_pane_from_env(
        "children requires tmux; use --pane %N or -t SESSION when calling from outside tmux",
    );
    canonical_pane(&ctx.tmux, &caller).unwrap_or_else(|| {
        die(
            code::USAGE,
            "could not resolve TMUX_PANE; use --pane %N or -t SESSION",
        )
    })
}

pub fn children(ctx: &Ctx, args: ChildrenArgs) -> Result<()> {
    let parent = query_pane(ctx, &args);
    let now = now_epoch();
    let rows: Vec<ChildRow> = session::list(&ctx.tmux)?
        .into_iter()
        .filter(|session| session.parent_pane.as_deref() == Some(parent.as_str()))
        .map(|session| {
            let state = session.state().to_string();
            let status = session.status().to_string();
            ChildRow {
                name: session.name,
                dir: session.dir,
                command: session.command,
                created: session.created,
                state,
                status,
                age: humanize_age(now - session.created),
            }
        })
        .collect();

    if ctx.json {
        return print_json(&rows);
    }
    if ctx.quiet {
        for row in &rows {
            println!("{}", row.name);
        }
        return Ok(());
    }
    if rows.is_empty() {
        eprintln!("no child sessions");
        return Ok(());
    }

    let name_width = rows
        .iter()
        .map(|row| row.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let status_width = rows
        .iter()
        .map(|row| row.status.len())
        .max()
        .unwrap_or(6)
        .max(6);
    for row in &rows {
        let status = match row.status.as_str() {
            "running" => paint(&row.status, Style::Green),
            "attached" => paint(&row.status, Style::Cyan),
            "exited" => paint(&row.status, Style::Yellow),
            _ => row.status.clone(),
        };
        let status_pad = " ".repeat(status_width.saturating_sub(row.status.len()));
        println!(
            "{:<name_width$}  {}{}  {:>4}  {}",
            row.name,
            status,
            status_pad,
            paint(&row.age, Style::Dim),
            paint(&row.command, Style::Dim),
            name_width = name_width,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_parent_target;

    #[test]
    fn parent_keyword_is_reserved_but_prefixed_name_is_not() {
        assert!(is_parent_target("parent"));
        assert!(!is_parent_target("tpp/parent"));
        assert!(!is_parent_target("parent:1.0"));
    }
}
