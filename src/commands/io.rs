//! Input and output: `send`, `paste`, `cat`, `tail`, `wait`.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::{CatArgs, PasteArgs, SendArgs, TailArgs, WaitArgs};
use crate::commands::{
    capture, code, die, last_lines, pane_dead, pane_dead_status, resolve_one_target,
    trim_trailing_blank, Ctx,
};
use crate::output::{paint, print_json, Style};
use crate::session;
use crate::store::Store;
use crate::tmux::{tgt, Tmux};

/// Gather the body text for a send/paste from `--file`, `--stdin`, or positional args.
fn body_text(file: Option<&Path>, stdin: bool, words: &[String]) -> Result<String> {
    if let Some(path) = file {
        return std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()));
    }
    if stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .context("reading stdin")?;
        return Ok(s);
    }
    Ok(words.join(" "))
}

/// Bracketed paste of arbitrary content: stage it in a tmux buffer via stdin (no arg
/// escaping), paste with `-p` (bracketed) and `-d` (drop the buffer after).
fn bracketed_paste(tmux: &Tmux, target: &str, body: &str) -> Result<()> {
    let buf = format!("tpp-{}", std::process::id());
    tmux.run_stdin(["load-buffer", "-b", &buf, "-"], body)?;
    tmux.run(["paste-buffer", "-t", &tgt(target), "-b", &buf, "-p", "-d"])?;
    Ok(())
}

/// Deliver input to a session: literal text, bracketed paste, or raw key names; optionally
/// followed by Enter.
#[allow(clippy::too_many_arguments)]
fn deliver(
    tmux: &Tmux,
    target: &str,
    body: &str,
    as_keys: bool,
    key_words: &[String],
    use_paste: bool,
    enter: bool,
    enter_delay_ms: u64,
) -> Result<()> {
    if as_keys {
        let mut args: Vec<String> = vec!["send-keys".into(), "-t".into(), tgt(target)];
        args.extend(key_words.iter().cloned());
        tmux.run(args)?;
    } else if !body.is_empty() {
        if use_paste || body.contains('\n') {
            bracketed_paste(tmux, target, body)?;
        } else {
            tmux.run(["send-keys", "-t", &tgt(target), "-l", "--", body])?;
        }
    }
    if enter {
        if enter_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(enter_delay_ms));
        }
        tmux.run(["send-keys", "-t", &tgt(target), "Enter"])?;
    }
    Ok(())
}

pub fn send(ctx: &Ctx, args: SendArgs) -> Result<()> {
    let target = resolve_one_target(ctx, args.target.as_deref());
    if !session::exists(&ctx.tmux, &target) {
        die(code::NOT_FOUND, format!("no such session: {target}"));
    }
    let body = if args.keys {
        String::new()
    } else {
        body_text(args.file.as_deref(), args.stdin, &args.text)?
    };
    let use_paste = args.paste || (ctx.cfg.send.bracketed_paste && body.contains('\n'));
    deliver(
        &ctx.tmux,
        &target,
        &body,
        args.keys,
        &args.text,
        use_paste,
        args.enter,
        ctx.cfg.send.enter_delay_ms,
    )?;
    if !ctx.quiet {
        eprintln!("sent to {target}");
    }
    Ok(())
}

pub fn paste(ctx: &Ctx, args: PasteArgs) -> Result<()> {
    let target = resolve_one_target(ctx, args.target.as_deref());
    if !session::exists(&ctx.tmux, &target) {
        die(code::NOT_FOUND, format!("no such session: {target}"));
    }
    let body = body_text(args.file.as_deref(), args.stdin, &args.text)?;
    deliver(
        &ctx.tmux,
        &target,
        &body,
        false,
        &[],
        true,
        !args.no_enter,
        ctx.cfg.send.enter_delay_ms,
    )?;
    if !ctx.quiet {
        eprintln!("pasted into {target}");
    }
    Ok(())
}

#[derive(Serialize)]
struct CatJson {
    session: String,
    status: String,
    output: String,
}

pub fn cat(ctx: &Ctx, args: CatArgs) -> Result<()> {
    let targets: Vec<String> = if args.sessions.is_empty() {
        vec![resolve_one_target(ctx, None)]
    } else {
        args.sessions.clone()
    };
    let lines = args.lines.unwrap_or(ctx.cfg.capture.lines);
    let store = Store::new(&ctx.paths);
    let multi = targets.len() > 1;
    let mut json_items = Vec::new();

    for name in &targets {
        let (status, output) = if session::exists(&ctx.tmux, name) {
            let raw = capture(&ctx.tmux, name, Some(lines), args.escape, args.all_history)?;
            let trimmed = trim_trailing_blank(&raw);
            let out = if args.all_history {
                trimmed
            } else {
                last_lines(&trimmed, lines as usize)
            };
            let status = if pane_dead(&ctx.tmux, name) {
                "exited"
            } else {
                "running"
            };
            (status.to_string(), out)
        } else if let Some(log) = store.read_log(name)? {
            let out = if args.all_history {
                log
            } else {
                last_lines(&log, lines as usize)
            };
            ("exited".to_string(), trim_trailing_blank(&out))
        } else {
            die(code::NOT_FOUND, format!("no such session: {name}"));
        };

        if ctx.json {
            json_items.push(CatJson {
                session: name.clone(),
                status,
                output,
            });
        } else {
            if multi {
                println!("{}", paint(&format!("== {name} [{status}] =="), Style::Cyan));
            }
            println!("{output}");
        }
    }

    if ctx.json {
        if multi {
            print_json(&json_items)?;
        } else if let Some(item) = json_items.into_iter().next() {
            print_json(&item)?;
        }
    }
    Ok(())
}

/// Longest suffix of `prev` that is a prefix of `cur` — used to find what's new across a
/// capture window that may have scrolled.
fn overlap(prev: &str, cur: &str) -> usize {
    let max = prev.len().min(cur.len());
    for len in (1..=max).rev() {
        if prev.as_bytes()[prev.len() - len..] == cur.as_bytes()[..len] {
            return len;
        }
    }
    0
}

/// What's new in `cur` given we last saw `prev`.
fn appended<'a>(prev: &str, cur: &'a str) -> &'a str {
    if cur == prev {
        return "";
    }
    if let Some(rest) = cur.strip_prefix(prev) {
        return rest;
    }
    let o = overlap(prev, cur);
    if o > 0 {
        &cur[o..]
    } else {
        cur
    }
}

/// Follow one or more sessions. Polls a capture window each tick and prints the delta. Stops
/// when all targets are gone or dead.
pub fn tail(ctx: &Ctx, args: TailArgs) -> Result<()> {
    let targets: Vec<String> = if args.sessions.is_empty() {
        vec![resolve_one_target(ctx, None)]
    } else {
        args.sessions.clone()
    };
    let interval = Duration::from_millis(args.interval.unwrap_or(ctx.cfg.tail.interval_ms).max(50));
    let window: u32 = 500;
    let multi = targets.len() > 1;

    let label = |name: &str| {
        if multi {
            paint(&format!("[{name}] "), Style::Cyan)
        } else {
            String::new()
        }
    };

    // Seed with an initial snapshot so we only stream genuinely new output afterwards.
    let mut last: Vec<String> = Vec::with_capacity(targets.len());
    let initial = args.lines.unwrap_or(0);
    for name in &targets {
        let snap = capture(&ctx.tmux, name, Some(window), false, false).unwrap_or_default();
        if initial > 0 {
            let shown = trim_trailing_blank(&last_lines(&snap, initial as usize));
            for line in shown.lines() {
                println!("{}{line}", label(name));
            }
        }
        last.push(snap);
    }

    loop {
        let mut any_alive = false;
        for (i, name) in targets.iter().enumerate() {
            if !session::exists(&ctx.tmux, name) {
                continue;
            }
            let cur = match capture(&ctx.tmux, name, Some(window), false, false) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let new = appended(&last[i], &cur);
            if !new.is_empty() {
                let new = new.strip_prefix('\n').unwrap_or(new);
                for line in new.lines() {
                    println!("{}{line}", label(name));
                }
            }
            last[i] = cur;
            if !pane_dead(&ctx.tmux, name) {
                any_alive = true;
            }
        }
        if !any_alive {
            break;
        }
        std::thread::sleep(interval);
    }
    Ok(())
}

#[derive(Serialize)]
struct WaitJson {
    session: String,
    outcome: String,
    elapsed_ms: u128,
}

/// Block until a condition holds: text appears, output goes idle, or the pane exits. Exits 4
/// on timeout.
pub fn wait(ctx: &Ctx, args: WaitArgs) -> Result<()> {
    let target = resolve_one_target(ctx, args.target.as_deref());
    if !session::exists(&ctx.tmux, &target) {
        die(code::NOT_FOUND, format!("no such session: {target}"));
    }
    let stable_for =
        Duration::from_millis(args.stable_for.unwrap_or(ctx.cfg.wait.stable_for_ms).max(1));
    let timeout_ms = args.timeout.unwrap_or(ctx.cfg.wait.timeout_ms);
    let timeout = if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    };
    // Default to idle when no explicit condition is requested.
    let want_idle = args.idle || (!args.exit && args.text.is_none());

    let start = Instant::now();
    let poll = Duration::from_millis(100);
    let mut prev = String::new();
    let mut last_change = Instant::now();

    let outcome = loop {
        if args.exit && pane_dead(&ctx.tmux, &target) {
            break "exited";
        }
        let cur = capture(&ctx.tmux, &target, Some(400), false, false).unwrap_or_default();
        if let Some(text) = &args.text {
            if cur.contains(text.as_str()) {
                break "text";
            }
        }
        if cur != prev {
            prev = cur;
            last_change = Instant::now();
        } else if want_idle && last_change.elapsed() >= stable_for {
            break "idle";
        }
        if !session::exists(&ctx.tmux, &target) {
            break "gone";
        }
        if let Some(t) = timeout {
            if start.elapsed() >= t {
                if ctx.json {
                    print_json(&WaitJson {
                        session: target.clone(),
                        outcome: "timeout".into(),
                        elapsed_ms: start.elapsed().as_millis(),
                    })?;
                } else if !ctx.quiet {
                    eprintln!("tpp: wait timed out after {timeout_ms}ms");
                }
                std::process::exit(code::TIMEOUT);
            }
        }
        std::thread::sleep(poll);
    };

    if ctx.json {
        print_json(&WaitJson {
            session: target,
            outcome: outcome.into(),
            elapsed_ms: start.elapsed().as_millis(),
        })?;
    } else if !ctx.quiet {
        eprintln!("{outcome}");
    }
    Ok(())
}

/// Strip tmux's remain-on-exit banner and trailing blank padding so a streamed capture reads
/// like real command output.
fn clean_stream(s: &str) -> String {
    let mut lines: Vec<&str> = s
        .lines()
        .filter(|l| !l.trim_start().starts_with("Pane is dead"))
        .collect();
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

/// Print whatever is new in `cur` relative to `last`, then adopt `cur` as the new baseline.
fn emit_new(last: &mut String, cur: String) {
    use std::io::Write;
    let new = appended(last, &cur);
    if !new.is_empty() {
        let new = new.strip_prefix('\n').unwrap_or(new);
        for line in new.lines() {
            println!("{line}");
        }
        let _ = std::io::stdout().flush();
    }
    *last = cur;
}

/// Stream a session's output until its command exits; return the command's exit status.
/// Used by `run --wait` to behave like running the command directly.
pub fn run_wait(ctx: &Ctx, name: &str) -> Result<i32> {
    let interval = Duration::from_millis(200);
    let mut last = String::new();
    loop {
        if !session::exists(&ctx.tmux, name) {
            return Ok(0);
        }
        let cur = clean_stream(&capture(&ctx.tmux, name, None, false, true).unwrap_or_default());
        emit_new(&mut last, cur);
        if pane_dead(&ctx.tmux, name) {
            // A final pass in case output landed between the capture and the dead check.
            let fin =
                clean_stream(&capture(&ctx.tmux, name, None, false, true).unwrap_or_default());
            emit_new(&mut last, fin);
            return Ok(pane_dead_status(&ctx.tmux, name).unwrap_or(0));
        }
        std::thread::sleep(interval);
    }
}

/// Record a session's current output as an exited record (used by `exit`/`rm --record`).
pub fn record_session(ctx: &Ctx, name: &str) -> Result<()> {
    let info = crate::commands::find_session(&ctx.tmux, name);
    let output = capture(&ctx.tmux, name, Some(ctx.cfg.exit.record_lines), false, false)
        .map(|s| trim_trailing_blank(&s))
        .unwrap_or_default();
    let rec = crate::store::ExitedRecord {
        name: name.to_string(),
        scope: info
            .as_ref()
            .map(|i| i.scope.clone())
            .unwrap_or_else(|| ctx.scope.clone().unwrap_or_default()),
        dir: info.as_ref().map(|i| i.dir.clone()).unwrap_or_default(),
        command: info.as_ref().map(|i| i.command.clone()).unwrap_or_default(),
        exited_at: session::now_epoch(),
    };
    Store::new(&ctx.paths).record(&rec, &output)
}

#[cfg(test)]
mod tests {
    use super::{appended, overlap};

    #[test]
    fn overlap_finds_suffix_prefix() {
        assert_eq!(overlap("abcdef", "defxyz"), 3);
        assert_eq!(overlap("abc", "xyz"), 0);
    }

    #[test]
    fn appended_plain_suffix() {
        assert_eq!(appended("hello", "hello world"), " world");
    }

    #[test]
    fn appended_after_scroll() {
        // window scrolled: "line1\nline2" -> "line2\nline3"; only "\nline3" is new.
        assert_eq!(appended("line1\nline2", "line2\nline3"), "\nline3");
    }

    #[test]
    fn appended_full_reset_and_nochange() {
        assert_eq!(appended("aaa", "bbb"), "bbb");
        assert_eq!(appended("x", "x"), "");
    }
}
