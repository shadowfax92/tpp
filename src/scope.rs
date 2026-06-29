//! Scope resolution — the "directory" a session is shared within.
//!
//! A scope is just an absolute path string stamped onto each session. `ls` filters by it
//! so everyone working in the same directory sees the same sessions. `git` mode resolves to
//! the worktree/repo root (so a grove worktree is its own scope); `cwd` is the literal
//! directory; `none` disables scoping entirely.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::config::ScopeMode;

/// Resolve the active scope. `None` means "no scoping" (mode none, or `--scope none`).
/// An explicit `override_dir` of "none"/"all" also disables scoping.
pub fn resolve(mode: ScopeMode, override_dir: Option<&str>) -> Result<Option<String>> {
    if let Some(dir) = override_dir {
        let d = dir.trim();
        if d.eq_ignore_ascii_case("none") || d.eq_ignore_ascii_case("all") {
            return Ok(None);
        }
        return Ok(Some(canonical(Path::new(d))));
    }
    match mode {
        ScopeMode::None => Ok(None),
        ScopeMode::Cwd => Ok(Some(canonical(&cwd()?))),
        ScopeMode::Git => {
            let cwd = cwd()?;
            Ok(Some(git_toplevel(&cwd).unwrap_or_else(|| canonical(&cwd))))
        }
    }
}

fn cwd() -> Result<PathBuf> {
    std::env::current_dir().context("getting current directory")
}

/// Canonicalize if possible, else fall back to the lexical path so we still produce a
/// stable string for directories that don't exist yet.
fn canonical(p: &Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn git_toplevel(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(canonical(Path::new(&path)))
    }
}

/// A short, human label for a scope path (its final component), for compact `ls` headers.
pub fn label(scope: &str) -> &str {
    Path::new(scope)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(scope)
}
