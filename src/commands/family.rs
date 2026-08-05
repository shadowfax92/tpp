use anyhow::Result;
use serde::Serialize;

use crate::cli::ChildrenArgs;
use crate::commands::{code, die, no_such_session, Ctx};
use crate::output::{paint, print_json, Style};
use crate::session::now_epoch;

pub const PARENT_TARGET: &str = "parent";

pub fn is_parent_target(target: &str) -> bool {
    target.trim() == PARENT_TARGET
}

pub fn resolve_parent_pane(ctx: &Ctx) -> String {
    let caller = ctx.backend.current_pane().unwrap_or_else(|| {
        die(
            code::USAGE,
            format!(
                "the parent target requires running inside {}",
                ctx.backend.target_description()
            ),
        )
    });
    let session_name = ctx.backend.session_for_pane(&caller).unwrap_or_else(|| {
        die(
            code::USAGE,
            "the current pane does not belong to a tpp session",
        )
    });
    let parent = ctx
        .backend
        .parent_pane(&session_name)
        .unwrap_or_else(|| die(code::NOT_FOUND, "no parent recorded"));
    ctx.backend
        .canonical_pane(&parent)
        .unwrap_or_else(|| die(code::NOT_FOUND, "parent pane gone"))
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
        return ctx
            .backend
            .canonical_pane(target)
            .unwrap_or_else(|| die(code::NOT_FOUND, format!("pane not found: {target}")));
    }
    if let Some(target) = &args.target {
        let name = ctx.backend.resolve_name(&ctx.cfg, target);
        if !ctx.backend.exists(&name) {
            no_such_session(&name);
        }
        return ctx.backend.origin_pane(&name).unwrap_or_else(|| {
            die(
                code::NOT_FOUND,
                format!("no origin pane recorded for {name}"),
            )
        });
    }
    ctx.backend.current_pane().unwrap_or_else(|| {
        die(
            code::USAGE,
            "children requires a multiplexer pane; use --pane %N or -t SESSION otherwise",
        )
    })
}

pub fn children(ctx: &Ctx, args: ChildrenArgs) -> Result<()> {
    let parent = query_pane(ctx, &args);
    let now = now_epoch();
    let rows: Vec<ChildRow> = ctx
        .backend
        .list()?
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
