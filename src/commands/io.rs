//! Input and output: `send`, `paste`, `cat`, `tail`, `wait`.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::{CatArgs, PasteArgs, SendArgs, TailArgs, WaitArgs};
use crate::commands::{
    capture, code, die, last_lines, no_such_session, pane, pane_dead, pane_dead_status,
    require_session_pane_target, select, session_pane_target, trim_trailing_blank, Ctx,
};
use crate::output::{paint, print_json, Style};
use crate::session;
use crate::store::Store;
use crate::tmux::{tgt, Tmux};

const VERIFY_CAPTURE_LINES: u32 = 80;
const VERIFY_TAIL_LINES: usize = 20;
const VERIFY_RETRIES: usize = 4;
const VERIFY_SETTLE_MS: u64 = 150;

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnsentDelivery {
    target: String,
    tail: String,
}

impl fmt::Display for UnsentDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "delivery to {} appears unsent; pasted-content marker is still visible:\n{}",
            self.target, self.tail
        )
    }
}

impl Error for UnsentDelivery {}

#[derive(Debug, Clone)]
enum IoTarget {
    Session { name: String, pane_target: String },
    Pane { name: String, pane_id: String },
}

impl IoTarget {
    fn tmux_target(&self) -> &str {
        match self {
            IoTarget::Session { pane_target, .. } => pane_target,
            IoTarget::Pane { pane_id, .. } => pane_id,
        }
    }

    fn display(&self) -> String {
        match self {
            IoTarget::Session { name, .. } => name.clone(),
            IoTarget::Pane { name, .. } => format!("pane:{name}"),
        }
    }
}

fn no_such_pane_target(name: &str) -> ! {
    die(code::NOT_FOUND, format!("No such pane target pane:{name}"))
}

fn resolve_pane_target(ctx: &Ctx, name: &str) -> Result<IoTarget> {
    if let Err(err) = pane::validate_name(name) {
        die(code::USAGE, err.to_string());
    }
    match pane::resolve_bound_pane(&ctx.tmux, name)? {
        Some(bound) => Ok(IoTarget::Pane {
            name: bound.name,
            pane_id: bound.pane_id,
        }),
        None => no_such_pane_target(name),
    }
}

fn resolve_io_target(ctx: &Ctx, explicit: Option<&str>, action: &str) -> Result<IoTarget> {
    if let Some(raw) = explicit {
        if let Some(name) = pane::pane_target_name(raw) {
            return resolve_pane_target(ctx, name);
        }
    }
    let name = select::one(ctx, explicit, action)?;
    let pane_target = require_session_pane_target(&ctx.tmux, &name);
    Ok(IoTarget::Session { name, pane_target })
}

fn target_exists(ctx: &Ctx, target: &IoTarget) -> bool {
    match target {
        IoTarget::Session { name, .. } => session::exists(&ctx.tmux, name),
        IoTarget::Pane { pane_id, .. } => {
            ctx.tmux
                .ok(["display-message", "-p", "-t", pane_id, "#{pane_id}"])
        }
    }
}

fn ensure_target_exists(ctx: &Ctx, target: &IoTarget) {
    match target {
        IoTarget::Session { name, .. } => {
            if !session::exists(&ctx.tmux, name) {
                no_such_session(name);
            }
        }
        IoTarget::Pane { name, pane_id } => {
            if !ctx
                .tmux
                .ok(["display-message", "-p", "-t", pane_id, "#{pane_id}"])
            {
                no_such_pane_target(name);
            }
        }
    }
}

fn ensure_origin_available(ctx: &Ctx, target: &IoTarget) {
    if let IoTarget::Session { name, .. } = target {
        if session::exists(&ctx.tmux, name) {
            require_session_pane_target(&ctx.tmux, name);
        }
    }
}

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
pub(crate) fn bracketed_paste(tmux: &Tmux, target: &str, body: &str) -> Result<()> {
    let buf = format!("tpp-{}", std::process::id());
    tmux.run_stdin(["load-buffer", "-b", &buf, "-"], body)?;
    tmux.run(["paste-buffer", "-t", &tgt(target), "-b", &buf, "-p", "-d"])?;
    Ok(())
}

pub(crate) fn strip_ansi(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for escaped in chars.by_ref() {
                if ('@'..='~').contains(&escaped) {
                    break;
                }
            }
        } else {
            stripped.push(ch);
        }
    }
    stripped
}

fn has_pasted_marker(text: &str) -> bool {
    text.contains("[Pasted Content") || text.contains("[Pasted text")
}

fn verify_delay(retry: usize) -> Duration {
    Duration::from_millis(VERIFY_SETTLE_MS * (retry as u64 + 1))
}

/// Confirm a bracketed paste marker disappeared after submission, retrying Enter if needed.
fn verify_submitted<C, E, S>(
    target_label: &str,
    mut capture_target: C,
    mut send_enter: E,
    sleep: S,
) -> Result<()>
where
    C: FnMut() -> Result<String>,
    E: FnMut() -> Result<()>,
    S: Fn(Duration),
{
    sleep(verify_delay(0));
    let mut captured = strip_ansi(&capture_target()?);
    for retry in 0..VERIFY_RETRIES {
        if !has_pasted_marker(&captured) {
            return Ok(());
        }
        send_enter()?;
        sleep(verify_delay(retry + 1));
        captured = strip_ansi(&capture_target()?);
    }
    if has_pasted_marker(&captured) {
        return Err(UnsentDelivery {
            target: target_label.to_string(),
            tail: last_lines(&captured, VERIFY_TAIL_LINES),
        }
        .into());
    }
    Ok(())
}

fn handle_delivery_result(result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.downcast_ref::<UnsentDelivery>().is_some() => {
            die(code::UNSENT, err.to_string());
        }
        Err(err) => Err(err),
    }
}

/// Deliver input to a target as literal text, bracketed paste, or raw key names.
#[allow(clippy::too_many_arguments)]
fn deliver(
    tmux: &Tmux,
    target: &str,
    target_label: &str,
    body: &str,
    as_keys: bool,
    key_words: &[String],
    use_paste: bool,
    enter: bool,
    enter_delay_ms: u64,
    verify: bool,
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
    if verify {
        verify_submitted(
            target_label,
            || {
                Ok(capture(
                    tmux,
                    target,
                    Some(VERIFY_CAPTURE_LINES),
                    true,
                    false,
                )?)
            },
            || {
                tmux.run(["send-keys", "-t", &tgt(target), "Enter"])?;
                Ok(())
            },
            std::thread::sleep,
        )?;
    }
    Ok(())
}

pub fn send(ctx: &Ctx, args: SendArgs) -> Result<()> {
    if args.verify && !args.keys && !args.enter {
        die(code::USAGE, "send --verify requires --enter");
    }
    let target = resolve_io_target(ctx, args.target.as_deref(), "send to")?;
    ensure_target_exists(ctx, &target);
    let body = if args.keys {
        String::new()
    } else {
        body_text(args.file.as_deref(), args.stdin, &args.text)?
    };
    let use_paste = args.paste || (ctx.cfg.send.bracketed_paste && body.contains('\n'));
    let delivery = deliver(
        &ctx.tmux,
        target.tmux_target(),
        &target.display(),
        &body,
        args.keys,
        &args.text,
        use_paste,
        args.enter,
        ctx.cfg.send.enter_delay_ms,
        args.verify && !args.keys,
    );
    if delivery.is_err() {
        ensure_origin_available(ctx, &target);
    }
    handle_delivery_result(delivery)?;
    if !ctx.quiet {
        eprintln!("sent to {}", target.display());
    }
    Ok(())
}

pub fn paste(ctx: &Ctx, args: PasteArgs) -> Result<()> {
    let target = resolve_io_target(ctx, args.target.as_deref(), "paste into")?;
    ensure_target_exists(ctx, &target);
    let body = body_text(args.file.as_deref(), args.stdin, &args.text)?;
    let verify = !args.no_verify && !args.no_enter;
    let delivery = deliver(
        &ctx.tmux,
        target.tmux_target(),
        &target.display(),
        &body,
        false,
        &[],
        true,
        !args.no_enter,
        ctx.cfg.send.enter_delay_ms,
        verify,
    );
    if delivery.is_err() {
        ensure_origin_available(ctx, &target);
    }
    handle_delivery_result(delivery)?;
    if !ctx.quiet {
        eprintln!("pasted into {}", target.display());
    }
    Ok(())
}

#[derive(Serialize)]
struct CatJson {
    session: String,
    status: String,
    output: String,
}

struct CatTarget {
    resolved: String,
    raw: Option<String>,
    display: String,
    pane_name: Option<String>,
}

/// Build the implicit `cat` picker from live sessions plus socket-scoped recorded transcripts.
fn cat_picker_candidates(
    ctx: &Ctx,
    store: &Store,
    include_all_recorded: bool,
) -> Result<Vec<String>> {
    let live = session::list(&ctx.tmux)?;
    let mut names: Vec<String> = live.iter().map(|s| s.name.clone()).collect();
    let mut seen: HashSet<String> = names.iter().cloned().collect();
    let recorded = if include_all_recorded {
        store.list()?
    } else {
        store.recent(ctx.cfg.ls.show_exited_hours)?
    };

    for rec in recorded {
        if seen.insert(rec.name.clone()) {
            names.push(rec.name);
        }
    }

    Ok(names)
}

fn cat_picker_targets(
    ctx: &Ctx,
    store: &Store,
    include_all_recorded: bool,
) -> Result<Vec<CatTarget>> {
    let names = cat_picker_candidates(ctx, store, include_all_recorded)?;
    Ok(
        select::from_candidates(names, select::SelectionMode::Single, "print")?
            .into_iter()
            .map(|name| CatTarget {
                resolved: name.clone(),
                display: name,
                raw: None,
                pane_name: None,
            })
            .collect(),
    )
}

fn cat_explicit_target(ctx: &Ctx, name: &str) -> Result<CatTarget> {
    if let Some(pane_name) = pane::pane_target_name(name) {
        let target = resolve_pane_target(ctx, pane_name)?;
        return Ok(CatTarget {
            resolved: target.tmux_target().to_string(),
            raw: None,
            display: target.display(),
            pane_name: Some(pane_name.to_string()),
        });
    }
    let resolved = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, name);
    Ok(CatTarget {
        resolved: resolved.clone(),
        raw: Some(tgt(name)),
        display: resolved,
        pane_name: None,
    })
}

fn recorded_cat_output(
    store: &Store,
    name: &str,
    lines: u32,
    all_history: bool,
) -> Result<Option<String>> {
    Ok(store.read_log(name)?.map(|log| {
        let out = if all_history {
            log
        } else {
            last_lines(&log, lines as usize)
        };
        trim_trailing_blank(&out)
    }))
}

fn recorded_or_origin_gone(
    store: &Store,
    name: &str,
    lines: u32,
    all_history: bool,
    error: impl std::fmt::Display,
) -> Result<(String, String)> {
    if let Some(output) = recorded_cat_output(store, name, lines, all_history)? {
        return Ok(("exited".to_string(), output));
    }
    die(code::NOT_FOUND, error.to_string())
}

/// Capture a live session or replay its record when its stamped origin has vanished.
fn live_session_cat_output(
    ctx: &Ctx,
    store: &Store,
    name: &str,
    lines: u32,
    escape: bool,
    all_history: bool,
) -> Result<(String, String)> {
    let pane_target = match session_pane_target(&ctx.tmux, name) {
        Ok(target) => target,
        Err(error) => {
            return recorded_or_origin_gone(store, name, lines, all_history, error);
        }
    };
    let raw = match capture(&ctx.tmux, &pane_target, Some(lines), escape, all_history) {
        Ok(raw) => raw,
        Err(capture_error) => match session_pane_target(&ctx.tmux, name) {
            Ok(_) => return Err(capture_error.into()),
            Err(error) => {
                return recorded_or_origin_gone(store, name, lines, all_history, error);
            }
        },
    };
    if let Err(error) = session_pane_target(&ctx.tmux, name) {
        return recorded_or_origin_gone(store, name, lines, all_history, error);
    }
    let trimmed = trim_trailing_blank(&raw);
    let output = if all_history {
        trimmed
    } else {
        last_lines(&trimmed, lines as usize)
    };
    let status = if pane_dead(&ctx.tmux, &pane_target) {
        "exited"
    } else {
        "running"
    };
    Ok((status.to_string(), output))
}

pub fn cat(ctx: &Ctx, args: CatArgs) -> Result<()> {
    let lines = args.lines.unwrap_or(ctx.cfg.capture.lines);
    let store_socket = ctx.tmux.store_socket();
    let store = Store::new(&ctx.paths, store_socket.as_deref());
    if args.target.is_some() && !args.sessions.is_empty() {
        die(
            code::USAGE,
            "use either cat -t/--target or positional sessions, not both",
        );
    }
    let explicit = args
        .target
        .as_ref()
        .map(|target| vec![target.clone()])
        .unwrap_or(args.sessions);
    let targets: Vec<CatTarget> = if explicit.is_empty() {
        cat_picker_targets(ctx, &store, args.all)?
    } else {
        explicit
            .iter()
            .map(|name| cat_explicit_target(ctx, name))
            .collect::<Result<Vec<_>>>()?
    };
    let multi = targets.len() > 1;
    let mut json_items = Vec::new();

    for target in &targets {
        let mut display_name = target.display.as_str();
        let (status, output) = if target.pane_name.is_some() {
            let raw = capture(
                &ctx.tmux,
                &target.resolved,
                Some(lines),
                args.escape,
                args.all_history,
            )
            .unwrap_or_else(|_| {
                no_such_pane_target(target.pane_name.as_deref().unwrap_or_default())
            });
            let trimmed = trim_trailing_blank(&raw);
            let out = if args.all_history {
                trimmed
            } else {
                last_lines(&trimmed, lines as usize)
            };
            let status = if pane_dead(&ctx.tmux, &target.resolved) {
                "exited"
            } else {
                "running"
            };
            (status.to_string(), out)
        } else if session::exists(&ctx.tmux, &target.resolved) {
            live_session_cat_output(
                ctx,
                &store,
                &target.resolved,
                lines,
                args.escape,
                args.all_history,
            )?
        } else if let Some(output) =
            recorded_cat_output(&store, &target.resolved, lines, args.all_history)?
        {
            ("exited".to_string(), output)
        } else if let Some(raw_name) = target
            .raw
            .as_ref()
            .filter(|raw_name| *raw_name != &target.resolved)
        {
            if let Some(output) = recorded_cat_output(&store, raw_name, lines, args.all_history)? {
                display_name = raw_name;
                ("exited".to_string(), output)
            } else {
                no_such_session(&target.resolved);
            }
        } else {
            no_such_session(&target.resolved);
        };

        if ctx.json {
            json_items.push(CatJson {
                session: display_name.to_string(),
                status,
                output,
            });
        } else {
            if multi {
                println!(
                    "{}",
                    paint(&format!("== {display_name} [{status}] =="), Style::Cyan)
                );
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
    let targets = select::many(ctx, &args.sessions, "tail")?;
    for name in &targets {
        if !session::exists(&ctx.tmux, name) {
            no_such_session(name);
        }
    }
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
        let pane_target = require_session_pane_target(&ctx.tmux, name);
        let snap =
            capture(&ctx.tmux, &pane_target, Some(window), false, false).unwrap_or_else(|_| {
                require_session_pane_target(&ctx.tmux, name);
                String::new()
            });
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
            let pane_target = require_session_pane_target(&ctx.tmux, name);
            let cur = match capture(&ctx.tmux, &pane_target, Some(window), false, false) {
                Ok(s) => s,
                Err(_) => {
                    require_session_pane_target(&ctx.tmux, name);
                    continue;
                }
            };
            let new = appended(&last[i], &cur);
            if !new.is_empty() {
                let new = new.strip_prefix('\n').unwrap_or(new);
                for line in new.lines() {
                    println!("{}{line}", label(name));
                }
            }
            last[i] = cur;
            if !pane_dead(&ctx.tmux, &pane_target) {
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
    let target = resolve_io_target(ctx, args.target.as_deref(), "wait on")?;
    ensure_target_exists(ctx, &target);
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
        ensure_origin_available(ctx, &target);
        if args.exit && pane_dead(&ctx.tmux, target.tmux_target()) {
            break "exited";
        }
        let cur = capture(&ctx.tmux, target.tmux_target(), Some(400), false, false).unwrap_or_else(
            |_| {
                ensure_origin_available(ctx, &target);
                String::new()
            },
        );
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
        if !target_exists(ctx, &target) {
            break "gone";
        }
        if let Some(t) = timeout {
            if start.elapsed() >= t {
                if ctx.json {
                    print_json(&WaitJson {
                        session: target.display(),
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
            session: target.display(),
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

fn recordable_output(raw: &str, line_limit: usize) -> String {
    last_lines(&trim_trailing_blank(raw), line_limit)
}

/// Record a session's current output as an exited record (used by `exit`/`rm --record`).
pub fn record_session(ctx: &Ctx, name: &str) -> Result<()> {
    let info = crate::commands::find_session(&ctx.tmux, name);
    let output = capture(
        &ctx.tmux,
        name,
        Some(ctx.cfg.exit.record_lines),
        false,
        false,
    )
    .map(|s| recordable_output(&s, ctx.cfg.exit.record_lines as usize))
    .unwrap_or_default();
    let rec = crate::store::ExitedRecord {
        name: name.to_string(),
        dir: info.as_ref().map(|i| i.dir.clone()).unwrap_or_default(),
        command: info.as_ref().map(|i| i.command.clone()).unwrap_or_default(),
        exited_at: session::now_epoch(),
    };
    let store_socket = ctx.tmux.store_socket();
    Store::new(&ctx.paths, store_socket.as_deref()).record(&rec, &output)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::{
        appended, has_pasted_marker, overlap, recordable_output, strip_ansi, verify_submitted,
        UnsentDelivery,
    };

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

    #[test]
    fn recordable_output_keeps_last_configured_lines() {
        assert_eq!(recordable_output("1\n2\n3\n4\n5", 3), "3\n4\n5");
    }

    #[test]
    fn recordable_output_trims_blank_padding_before_limiting() {
        assert_eq!(recordable_output("1\n2\n\n\n", 10), "1\n2");
    }

    #[test]
    fn pasted_marker_detection_ignores_ansi_sequences() {
        let stripped = strip_ansi("ok\n[Pasted \u{1b}[31mContent 123]\n");
        assert!(has_pasted_marker(&stripped));
        assert!(has_pasted_marker("[Pasted text 42]"));
        assert!(!has_pasted_marker("plain submitted text"));
    }

    #[test]
    fn verify_submitted_accepts_clean_capture_without_retry() {
        let captures = RefCell::new(VecDeque::from(["submitted".to_string()]));
        let enters = RefCell::new(0);
        let sleeps = RefCell::new(Vec::new());

        verify_submitted(
            "pane:agent",
            || Ok(captures.borrow_mut().pop_front().unwrap()),
            || {
                *enters.borrow_mut() += 1;
                Ok(())
            },
            |duration| sleeps.borrow_mut().push(duration),
        )
        .unwrap();

        assert_eq!(*enters.borrow(), 0);
        assert_eq!(*sleeps.borrow(), vec![Duration::from_millis(150)]);
    }

    #[test]
    fn verify_submitted_retries_enter_until_marker_disappears() {
        let captures = RefCell::new(VecDeque::from([
            "[Pasted Content 99]".to_string(),
            "submitted".to_string(),
        ]));
        let enters = RefCell::new(0);
        let sleeps = RefCell::new(Vec::new());

        verify_submitted(
            "pane:agent",
            || Ok(captures.borrow_mut().pop_front().unwrap()),
            || {
                *enters.borrow_mut() += 1;
                Ok(())
            },
            |duration| sleeps.borrow_mut().push(duration),
        )
        .unwrap();

        assert_eq!(*enters.borrow(), 1);
        assert_eq!(
            *sleeps.borrow(),
            vec![Duration::from_millis(150), Duration::from_millis(300)]
        );
    }

    #[test]
    fn verify_submitted_reports_unsent_tail_after_retries() {
        let lines = (1..=25)
            .map(|n| {
                if n == 25 {
                    "line25 [Pasted text 123]".to_string()
                } else {
                    format!("line{n}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let captures = RefCell::new(VecDeque::from(vec![lines; 5]));
        let enters = RefCell::new(0);

        let err = verify_submitted(
            "pane:agent",
            || Ok(captures.borrow_mut().pop_front().unwrap()),
            || {
                *enters.borrow_mut() += 1;
                Ok(())
            },
            |_| {},
        )
        .unwrap_err();
        let unsent = err.downcast_ref::<UnsentDelivery>().unwrap();

        assert_eq!(*enters.borrow(), 4);
        assert_eq!(unsent.target, "pane:agent");
        assert!(unsent.tail.contains("line6"), "{}", unsent.tail);
        assert!(
            unsent.tail.contains("line25 [Pasted text 123]"),
            "{}",
            unsent.tail
        );
        assert!(!unsent.tail.contains("line5\n"), "{}", unsent.tail);
    }
}
