use anyhow::{bail, Context, Result};

use crate::backend::{Backend, BoundPane};
use crate::cli::{BindArgs, TargetsArgs, UnbindArgs};
use crate::commands::{code, die, Ctx};
use crate::output::print_json;

pub const PANE_TARGET_PREFIX: &str = "pane:";

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
    if let Err(error) = validate_name(value) {
        die(code::USAGE, error.to_string());
    }
}

fn require_valid_role(value: &str) {
    if let Err(error) = validate_role(value) {
        die(code::USAGE, error.to_string());
    }
}

pub fn pane_target_name(raw: &str) -> Option<&str> {
    raw.strip_prefix(PANE_TARGET_PREFIX)
}

pub fn resolve_bound_pane(backend: &dyn Backend, name: &str) -> Result<Option<BoundPane>> {
    validate_name(name)?;
    Ok(backend
        .list_bindings()?
        .into_iter()
        .find(|pane| pane.name == name))
}

fn source_from_args(ctx: &Ctx, args: &BindArgs) -> String {
    let explicit = args
        .pane
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (args.here, explicit) {
        (true, None) => ctx.backend.current_pane().unwrap_or_else(|| {
            die(
                code::USAGE,
                format!(
                    "--here requires running inside {}; use --pane otherwise",
                    ctx.backend.target_description()
                ),
            )
        }),
        (false, Some(target)) => target.to_string(),
        _ => die(
            code::USAGE,
            "choose exactly one pane source: --here or --pane TARGET",
        ),
    }
}

pub fn bind(ctx: &Ctx, args: BindArgs) -> Result<()> {
    require_valid_name(&args.name);
    require_valid_role(&args.role);
    let source = source_from_args(ctx, &args);
    let pane = ctx
        .backend
        .inspect_pane(&source)
        .with_context(|| format!("resolving pane target {source}"))?;
    let previous = ctx
        .backend
        .bind_pane(&args.name, &args.role, &pane.pane_id)?;

    if !ctx.quiet {
        if previous.is_empty() {
            eprintln!(
                "bound pane:{} -> {} ({})",
                args.name, pane.pane_id, pane.location
            );
        } else {
            let old = previous
                .iter()
                .map(|pane| format!("{} ({})", pane.pane_id, pane.location))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "rebound pane:{}: {} -> {} ({})",
                args.name, old, pane.pane_id, pane.location
            );
        }
    }
    Ok(())
}

pub fn unbind(ctx: &Ctx, args: UnbindArgs) -> Result<()> {
    require_valid_name(&args.name);
    let removed = ctx.backend.unbind_pane(&args.name)?;
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

pub fn targets(ctx: &Ctx, _args: TargetsArgs) -> Result<()> {
    let panes = ctx.backend.list_bindings()?;
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
