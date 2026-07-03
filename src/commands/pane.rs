//! Named pane targets backed by tmux pane user-options.

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::cli::{BindArgs, TargetsArgs, UnbindArgs};
use crate::commands::{code, die, Ctx};
use crate::output::print_json;
use crate::tmux::{tgt, Tmux, TmuxError};

const SEP: char = '\u{1f}';
const PANE_NAME_OPT: &str = "@tpp_name";
const PANE_ROLE_OPT: &str = "@tpp_role";
pub const PANE_TARGET_PREFIX: &str = "pane:";

#[derive(Debug, Clone)]
struct PaneLocation {
    pane_id: String,
    session: String,
    window: String,
    pane: String,
}

impl PaneLocation {
    fn location(&self) -> String {
        format!("{}:{}.{}", self.session, self.window, self.pane)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundPane {
    pub name: String,
    pub role: String,
    pub pane_id: String,
    pub location: String,
    pub session: String,
    pub window: String,
    pub pane: String,
    pub status: String,
}

fn is_path_safe_token(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn validate_name(value: &str) -> Result<()> {
    if is_path_safe_token(value) {
        Ok(())
    } else {
        bail!("pane target names must be single path-safe tokens")
    }
}

fn validate_role(value: &str) -> Result<()> {
    if is_path_safe_token(value) {
        Ok(())
    } else {
        bail!("pane roles must be single path-safe tokens")
    }
}

fn require_valid_name(value: &str) {
    if let Err(err) = validate_name(value) {
        die(code::USAGE, err.to_string());
    }
}

fn require_valid_role(value: &str) {
    if let Err(err) = validate_role(value) {
        die(code::USAGE, err.to_string());
    }
}

pub fn pane_target_name(raw: &str) -> Option<&str> {
    raw.strip_prefix(PANE_TARGET_PREFIX)
}

fn parse_dead(value: &str) -> bool {
    value.trim() == "1"
}

fn inspect_pane(tmux: &Tmux, target: &str) -> Result<PaneLocation> {
    let fmt = [
        "#{pane_id}",
        "#{session_name}",
        "#{window_index}",
        "#{pane_index}",
    ]
    .join(&SEP.to_string());
    let raw = tmux.run(["display-message", "-p", "-t", &tgt(target), &fmt])?;
    let fields: Vec<&str> = raw.split(SEP).collect();
    if fields.len() < 4 || fields[0].trim().is_empty() {
        bail!("tmux did not return pane metadata for {target}");
    }
    Ok(PaneLocation {
        pane_id: fields[0].to_string(),
        session: fields[1].to_string(),
        window: fields[2].to_string(),
        pane: fields[3].to_string(),
    })
}

fn parse_bound_pane(line: &str) -> Option<BoundPane> {
    let fields: Vec<&str> = line.split(SEP).collect();
    if fields.len() < 7 {
        return None;
    }
    let name = fields[5].trim();
    if name.is_empty() {
        return None;
    }
    let location = format!("{}:{}.{}", fields[1], fields[2], fields[3]);
    Some(BoundPane {
        name: name.to_string(),
        role: fields[6].trim().to_string(),
        pane_id: fields[0].to_string(),
        location,
        session: fields[1].to_string(),
        window: fields[2].to_string(),
        pane: fields[3].to_string(),
        status: if parse_dead(fields[4]) {
            "dead".to_string()
        } else {
            "live".to_string()
        },
    })
}

/// Return every pane currently carrying a tpp pane binding.
pub fn list_bound_panes(tmux: &Tmux) -> Result<Vec<BoundPane>> {
    let fmt = [
        "#{pane_id}",
        "#{session_name}",
        "#{window_index}",
        "#{pane_index}",
        "#{pane_dead}",
        "#{@tpp_name}",
        "#{@tpp_role}",
    ]
    .join(&SEP.to_string());
    let raw = match tmux.run(["list-panes", "-a", "-F", &fmt]) {
        Ok(raw) => raw,
        Err(TmuxError::NoServer) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    Ok(raw.lines().filter_map(parse_bound_pane).collect())
}

/// Resolve `pane:<name>` to the first matching tmux pane binding.
pub fn resolve_bound_pane(tmux: &Tmux, name: &str) -> Result<Option<BoundPane>> {
    validate_name(name)?;
    Ok(list_bound_panes(tmux)?
        .into_iter()
        .find(|pane| pane.name == name))
}

fn unset_binding(tmux: &Tmux, pane_id: &str) -> Result<()> {
    tmux.run(["set-option", "-p", "-u", "-t", pane_id, PANE_NAME_OPT])?;
    let _ = tmux.run(["set-option", "-p", "-u", "-t", pane_id, PANE_ROLE_OPT]);
    Ok(())
}

fn clear_name(tmux: &Tmux, name: &str) -> Result<Vec<BoundPane>> {
    let existing = list_bound_panes(tmux)?
        .into_iter()
        .filter(|pane| pane.name == name)
        .collect::<Vec<_>>();
    for pane in &existing {
        unset_binding(tmux, &pane.pane_id)?;
    }
    Ok(existing)
}

fn set_binding(tmux: &Tmux, pane_id: &str, name: &str, role: &str) -> Result<()> {
    tmux.run(["set-option", "-p", "-t", pane_id, PANE_NAME_OPT, name])?;
    tmux.run(["set-option", "-p", "-t", pane_id, PANE_ROLE_OPT, role])?;
    Ok(())
}

fn source_from_args(args: &BindArgs) -> String {
    let explicit = args
        .pane
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (args.here, explicit) {
        (true, None) => {
            if std::env::var_os("TMUX").is_none() {
                die(
                    code::USAGE,
                    "--here requires TMUX and TMUX_PANE; run from inside tmux or use --pane",
                );
            }
            std::env::var("TMUX_PANE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    die(
                        code::USAGE,
                        "inside tmux but TMUX_PANE is empty; use --pane",
                    )
                })
        }
        (false, Some(target)) => target.to_string(),
        _ => die(
            code::USAGE,
            "choose exactly one pane source: --here or --pane TMUX_TARGET",
        ),
    }
}

/// Bind a server-wide name to a tmux pane by writing pane user-options.
pub fn bind(ctx: &Ctx, args: BindArgs) -> Result<()> {
    require_valid_name(&args.name);
    require_valid_role(&args.role);
    let source = source_from_args(&args);
    let pane = inspect_pane(&ctx.tmux, &source)
        .with_context(|| format!("resolving pane target {source}"))?;
    let previous = clear_name(&ctx.tmux, &args.name)?;
    set_binding(&ctx.tmux, &pane.pane_id, &args.name, &args.role)?;

    if !ctx.quiet {
        if previous.is_empty() {
            eprintln!(
                "bound pane:{} -> {} ({})",
                args.name,
                pane.pane_id,
                pane.location()
            );
        } else {
            let old = previous
                .iter()
                .map(|pane| format!("{} ({})", pane.pane_id, pane.location))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "rebound pane:{}: {} -> {} ({})",
                args.name,
                old,
                pane.pane_id,
                pane.location()
            );
        }
    }
    Ok(())
}

/// Remove a pane binding wherever that name currently appears.
pub fn unbind(ctx: &Ctx, args: UnbindArgs) -> Result<()> {
    require_valid_name(&args.name);
    let removed = clear_name(&ctx.tmux, &args.name)?;
    if removed.is_empty() {
        die(
            code::NOT_FOUND,
            format!("No such pane target pane:{}", args.name),
        );
    }
    if !ctx.quiet {
        eprintln!("unbound pane:{}", args.name);
    }
    Ok(())
}

/// Print the current tmux pane bindings and their pane_dead status.
pub fn targets(ctx: &Ctx, _args: TargetsArgs) -> Result<()> {
    let panes = list_bound_panes(&ctx.tmux)?;
    if ctx.json {
        return print_json(&panes);
    }
    if ctx.quiet {
        for pane in &panes {
            println!("{}", pane.name);
        }
        return Ok(());
    }
    if panes.is_empty() {
        eprintln!("no pane targets");
        return Ok(());
    }
    println!(
        "{:<16} {:<10} {:<8} {:<14} STATUS",
        "NAME", "ROLE", "PANE", "LOCATION"
    );
    for pane in panes {
        println!(
            "{:<16} {:<10} {:<8} {:<14} {}",
            pane.name, pane.role, pane.pane_id, pane.location, pane.status
        );
    }
    Ok(())
}
