//! Shared interactive session selection for human-facing commands.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::commands::{code, die, Ctx};
use crate::session;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionMode {
    Single,
    Multi,
}

pub fn normalize_explicit(name: &str) -> String {
    name.trim().trim_start_matches('=').to_string()
}

pub fn normalize_explicit_many(names: &[String]) -> Vec<String> {
    names.iter().map(|name| normalize_explicit(name)).collect()
}

pub fn parse_fzf_output(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn one(ctx: &Ctx, explicit: Option<&str>, action: &str) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(session::resolve_existing_name(&ctx.tmux, &ctx.cfg, name));
    }
    let picks = from_all(ctx, SelectionMode::Single, action)?;
    picks
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no session selected"))
}

pub fn many(ctx: &Ctx, explicit: &[String], action: &str) -> Result<Vec<String>> {
    if !explicit.is_empty() {
        return Ok(explicit
            .iter()
            .map(|name| session::resolve_existing_name(&ctx.tmux, &ctx.cfg, name))
            .collect());
    }
    from_all(ctx, SelectionMode::Multi, action)
}

fn from_all(ctx: &Ctx, mode: SelectionMode, action: &str) -> Result<Vec<String>> {
    let sessions = session::list(&ctx.tmux)?;
    match sessions.len() {
        0 => die(code::NOT_FOUND, format!("no sessions to {action}")),
        1 => Ok(vec![sessions[0].name.clone()]),
        _ => {
            let names: Vec<String> = sessions.into_iter().map(|s| s.name).collect();
            if let Some(picks) = fzf_pick(&names, mode) {
                if !picks.is_empty() {
                    return Ok(picks);
                }
            }
            let prompt = match mode {
                SelectionMode::Single => format!("name a session to {action}"),
                SelectionMode::Multi => format!("name one or more sessions to {action}"),
            };
            die(code::NOT_FOUND, format!("{prompt}: {}", names.join(", ")));
        }
    }
}

fn fzf_pick(names: &[String], mode: SelectionMode) -> Option<Vec<String>> {
    let mut cmd = Command::new("fzf");
    cmd.args(["--prompt", "tpp> ", "--height", "40%", "--reverse"]);
    if mode == SelectionMode::Multi {
        cmd.arg("--multi");
    }

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(names.join("\n").as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_fzf_output(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::{normalize_explicit, parse_fzf_output};

    #[test]
    fn parses_non_empty_fzf_lines() {
        assert_eq!(
            parse_fzf_output("api\n worker \n\n"),
            vec!["api".to_string(), "worker".to_string()]
        );
    }

    #[test]
    fn normalizes_tmux_exact_prefixes() {
        assert_eq!(normalize_explicit("=api"), "api");
        assert_eq!(normalize_explicit(" worker "), "worker");
    }
}
