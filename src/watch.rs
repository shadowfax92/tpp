//! Per-session stable-screen detection and watchdog process lifecycle.

use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::cli::WatchTargetArgs;
use crate::commands::io::{bracketed_paste, strip_ansi};
use crate::commands::{capture, code, die, last_lines, trim_trailing_blank, Ctx};
use crate::config::{WatchAction, WatchCfg, WatchRuleCfg};
use crate::output::print_json;
use crate::paths::{create_private_dir_all, encode_state_component};
use crate::session;
use crate::tmux::tgt;

const BUILTIN_RULES: &[(&str, WatchAction)] = &[
    ("? for shortcuts", WatchAction::Ignore),
    ("Press enter to continue", WatchAction::Enter),
    ("Enter to confirm", WatchAction::Enter),
    ("Do you trust", WatchAction::Enter),
    ("trust this folder", WatchAction::Enter),
];

#[derive(Debug)]
enum RulePattern {
    Substring(String),
    Regex(Regex),
}

#[derive(Debug)]
struct CompiledRule {
    source: String,
    pattern: RulePattern,
    action: WatchAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleMatch {
    source: String,
    action: WatchAction,
}

#[derive(Debug)]
struct RuleSet {
    rules: Vec<CompiledRule>,
}

impl RuleSet {
    fn compile(configured: &[WatchRuleCfg]) -> Result<Self> {
        let mut rules = Vec::with_capacity(configured.len() + BUILTIN_RULES.len());
        for rule in configured {
            rules.push(CompiledRule::compile(&rule.pattern, rule.action)?);
        }
        for &(pattern, action) in BUILTIN_RULES {
            rules.push(CompiledRule::compile(pattern, action)?);
        }
        Ok(Self { rules })
    }

    fn matched(&self, screen: &str) -> Option<RuleMatch> {
        self.rules.iter().find_map(|rule| {
            let matches = match &rule.pattern {
                RulePattern::Substring(pattern) => screen.contains(pattern),
                RulePattern::Regex(pattern) => pattern.is_match(screen),
            };
            matches.then(|| RuleMatch {
                source: rule.source.clone(),
                action: rule.action,
            })
        })
    }
}

impl CompiledRule {
    fn compile(pattern: &str, action: WatchAction) -> Result<Self> {
        let compiled = if pattern.len() >= 2 && pattern.starts_with('/') && pattern.ends_with('/') {
            RulePattern::Regex(
                Regex::new(&pattern[1..pattern.len() - 1])
                    .with_context(|| format!("invalid watch rule regex {pattern:?}"))?,
            )
        } else {
            RulePattern::Substring(pattern.to_string())
        };
        Ok(Self {
            source: pattern.to_string(),
            pattern: compiled,
            action,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchDecision {
    Enter { pattern: String },
    Escalate { reason: String },
}

#[derive(Debug, Default)]
struct WatchState {
    screen_hash: Option<u64>,
    stable_since: Duration,
    enters: u32,
    awaiting_change: bool,
    escalated: bool,
    last_escalation: Option<Duration>,
}

pub struct WatchEngine {
    cfg: WatchCfg,
    rules: RuleSet,
    state: WatchState,
}

impl WatchEngine {
    /// Compile configured rules and initialize one session's detection state.
    pub fn new(cfg: &WatchCfg) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            rules: RuleSet::compile(&cfg.rules)?,
            state: WatchState::default(),
        })
    }

    /// Evaluate an ANSI-stripped screen tail captured at the supplied elapsed time.
    pub fn observe(
        &mut self,
        now: Duration,
        screen_hash: u64,
        screen: &str,
    ) -> Option<WatchDecision> {
        let matched = self.rules.matched(screen);
        self.state
            .observe(now, screen_hash, matched.as_ref(), &self.cfg)
    }
}

impl WatchState {
    /// Turn one captured-screen observation into at most one watchdog action.
    fn observe(
        &mut self,
        now: Duration,
        screen_hash: u64,
        matched: Option<&RuleMatch>,
        cfg: &WatchCfg,
    ) -> Option<WatchDecision> {
        if self.screen_hash != Some(screen_hash) {
            self.screen_hash = Some(screen_hash);
            self.stable_since = now;
            self.enters = 0;
            self.awaiting_change = false;
            self.escalated = false;
            return None;
        }

        if matched.is_some_and(|rule| rule.action == WatchAction::Ignore) {
            return None;
        }

        if self.awaiting_change {
            return self.escalate(
                now,
                cfg,
                "screen unchanged after automatic Enter".to_string(),
            );
        }

        let stable_for = now.saturating_sub(self.stable_since);
        match matched {
            Some(rule) if stable_for >= Duration::from_secs(cfg.prompt_stable.as_secs()) => {
                match rule.action {
                    WatchAction::Enter if cfg.auto_enter && self.enters < cfg.max_enters => {
                        self.enters += 1;
                        self.awaiting_change = true;
                        Some(WatchDecision::Enter {
                            pattern: rule.source.clone(),
                        })
                    }
                    WatchAction::Enter if !cfg.auto_enter => self.escalate(
                        now,
                        cfg,
                        format!("matched {:?}, but auto-Enter is disabled", rule.source),
                    ),
                    WatchAction::Enter => self.escalate(
                        now,
                        cfg,
                        format!("automatic Enter limit reached for {:?}", rule.source),
                    ),
                    WatchAction::Notify => {
                        self.escalate(now, cfg, format!("matched watch rule {:?}", rule.source))
                    }
                    WatchAction::Keys => {
                        self.escalate(now, cfg, format!("matched keys rule {:?}", rule.source))
                    }
                    WatchAction::Ignore => None,
                }
            }
            None if stable_for >= Duration::from_secs(cfg.stuck_after.as_secs()) => self.escalate(
                now,
                cfg,
                format!("screen unchanged for {}", cfg.stuck_after.display()),
            ),
            _ => None,
        }
    }

    fn escalate(&mut self, now: Duration, cfg: &WatchCfg, reason: String) -> Option<WatchDecision> {
        if self.escalated {
            return None;
        }
        if self.last_escalation.is_some_and(|last| {
            now.saturating_sub(last) < Duration::from_secs(cfg.cooldown.as_secs())
        }) {
            return None;
        }
        self.escalated = true;
        self.last_escalation = Some(now);
        Some(WatchDecision::Escalate { reason })
    }
}

/// Validate user rule syntax before a session is labeled as watched.
pub fn validate_config(cfg: &WatchCfg) -> Result<()> {
    if cfg.poll.as_secs() == 0 {
        bail!("watch.poll must be greater than 0");
    }
    WatchEngine::new(cfg).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WatchProcess {
    session: String,
    origin: String,
    pid: u32,
    token: String,
}

#[derive(Debug, Serialize)]
struct WatchListRow {
    session: String,
    pid: u32,
    status: &'static str,
}

struct PidGuard {
    path: PathBuf,
    stop_path: PathBuf,
    record: WatchProcess,
    lock: File,
}

impl PidGuard {
    fn acquire(root: &Path, session_name: &str, origin: &str) -> Result<Option<Self>> {
        create_private_dir_all(root)?;
        let path = pidfile_path(root, session_name);
        let stop_path = stopfile_path(root, session_name);
        loop {
            let mut lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .context("opening watcher pidfile")?;
            if !same_file(&lock, &path) {
                continue;
            }
            if !try_lock_exclusive(&lock)? {
                let active =
                    read_locked_process(&mut lock).context("reading active watcher pidfile")?;
                if active.origin == origin {
                    return Ok(None);
                }
                if same_file(&lock, &path) {
                    let _ = std::fs::remove_file(&path);
                }
                continue;
            }
            if !same_file(&lock, &path) {
                continue;
            }
            let record = WatchProcess {
                session: session_name.to_string(),
                origin: origin.to_string(),
                pid: std::process::id(),
                token: watch_token(),
            };
            lock.set_len(0)?;
            lock.seek(SeekFrom::Start(0))?;
            serde_json::to_writer(&mut lock, &record).context("writing watcher pidfile")?;
            writeln!(lock)?;
            lock.flush()?;
            let _ = std::fs::remove_file(&stop_path);
            return Ok(Some(Self {
                path,
                stop_path,
                record,
                lock,
            }));
        }
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        if same_file(&self.lock, &self.path)
            && read_process_file(&mut self.lock).as_ref() == Some(&self.record)
        {
            let _ = std::fs::remove_file(&self.path);
        }
        if std::fs::read_to_string(&self.stop_path).is_ok_and(|token| token == self.record.token) {
            let _ = std::fs::remove_file(&self.stop_path);
        }
    }
}

fn watch_root(ctx: &Ctx) -> PathBuf {
    let socket = ctx.tmux.store_socket();
    ctx.paths.socket_state_dir("watch", socket.as_deref())
}

fn watch_log(root: &Path) -> PathBuf {
    root.join("watch.log")
}

fn pidfile_path(root: &Path, session_name: &str) -> PathBuf {
    root.join(format!("{}.pid", encode_state_component(session_name)))
}

fn stopfile_path(root: &Path, session_name: &str) -> PathBuf {
    root.join(format!("{}.stop", encode_state_component(session_name)))
}

fn watch_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

fn same_file(file: &File, path: &Path) -> bool {
    let Ok(opened) = file.metadata() else {
        return false;
    };
    let Ok(current) = std::fs::metadata(path) else {
        return false;
    };
    opened.dev() == current.dev() && opened.ino() == current.ino()
}

fn try_lock_exclusive(file: &File) -> std::io::Result<bool> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EAGAIN) || err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(err)
    }
}

fn read_process_file(file: &mut File) -> Option<WatchProcess> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_locked_process(file: &mut File) -> Option<WatchProcess> {
    for _ in 0..20 {
        if let Some(process) = read_process_file(file) {
            return Some(process);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

fn active_process(path: &Path) -> Result<Option<WatchProcess>> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("opening watcher pidfile"),
    };
    if !same_file(&file, path) {
        return Ok(None);
    }
    if try_lock_exclusive(&file)? {
        if same_file(&file, path) {
            let _ = std::fs::remove_file(path);
        }
        return Ok(None);
    }
    read_locked_process(&mut file)
        .map(Some)
        .with_context(|| format!("reading active watcher pidfile {}", path.display()))
}

fn append_log(root: &Path, session_name: &str, message: &str) {
    if create_private_dir_all(root).is_err() {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(watch_log(root))
    {
        let _ = writeln!(
            file,
            "{} [{}] {message}",
            session::now_epoch(),
            session_name
        );
    }
}

/// Spawn the current tpp executable as a detached watcher for one session.
pub fn spawn_detached(ctx: &Ctx, session_name: &str) -> Result<()> {
    validate_config(&ctx.cfg.watch)?;
    let root = watch_root(ctx);
    create_private_dir_all(&root)?;
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(watch_log(&root))?;
    let stderr = stdout.try_clone()?;
    let mut command =
        Command::new(std::env::current_exe().context("resolving current tpp binary")?);
    if let Some(socket) = ctx.tmux.socket() {
        command.args(["-L", socket]);
    }
    command
        .arg("--config")
        .arg(&ctx.config_path)
        .args(["watch", "run", "-t", session_name])
        .env("TPP_CONFIG_DIR", &ctx.paths.config_dir)
        .env("TPP_STATE_DIR", &ctx.paths.state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .context("spawning detached session watcher")?;
    append_log(
        &root,
        session_name,
        &format!("spawned watcher pid {}", child.id()),
    );
    Ok(())
}

fn origin_is_alive(ctx: &Ctx, origin: &str) -> bool {
    ctx.tmux
        .run(["display-message", "-p", "-t", origin, "#{pane_dead}"])
        .is_ok_and(|dead| dead.trim() == "0")
}

fn screen_hash(screen: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    screen.hash(&mut hasher);
    hasher.finish()
}

fn sanitized_line(text: &str, limit: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .map(|ch| match ch {
            '$' => '＄',
            '`' => '｀',
            '\\' => '＼',
            '\'' => '’',
            '"' => '”',
            ';' => '；',
            '|' => '｜',
            '&' => '＆',
            '<' => '＜',
            '>' => '＞',
            '(' => '（',
            ')' => '）',
            '!' => '！',
            '#' => '＃',
            '*' => '＊',
            '?' => '？',
            '[' => '［',
            ']' => '］',
            '{' => '｛',
            '}' => '｝',
            '~' => '～',
            _ => ch,
        })
        .take(limit)
        .collect()
}

fn nudge_message(session_name: &str, reason: &str, tail: &str) -> String {
    let session_line = sanitized_line(session_name, 160);
    let reason_line = sanitized_line(reason, 160);
    let tail_line = sanitized_line(tail, 120);
    format!(
        "[tpp:{session_line}] ⚠️ stuck: {reason_line} — last: \"{tail_line}\" — check: tpp attach {session_line}"
    )
}

fn send_parent_nudge(ctx: &Ctx, parent: &str, message: &str) -> Result<()> {
    bracketed_paste(&ctx.tmux, parent, message)?;
    ctx.tmux.run(["send-keys", "-t", &tgt(parent), "Enter"])?;
    Ok(())
}

fn run_notify(
    ctx: &Ctx,
    session_name: &str,
    parent: Option<&str>,
    reason: &str,
    tail: &str,
) -> Result<()> {
    if ctx.cfg.watch.notify.trim().is_empty() {
        return Ok(());
    }
    let command = notify_command(&ctx.cfg.watch.notify);
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("TPP_SESSION", session_name)
        .env("TPP_SESSION_NAME", session_name)
        .env("TPP_REASON", reason)
        .env("TPP_TAIL", tail)
        .env(
            "TPP_DIR",
            session::session_dir(&ctx.tmux, session_name).unwrap_or_default(),
        )
        .env("TPP_PARENT_PANE", parent.unwrap_or(""))
        .status()
        .context("running watch notify command")?;
    if !status.success() {
        bail!("watch notify command exited {}", status.code().unwrap_or(1));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellQuote {
    Unquoted,
    Single,
    Double,
}

/// Render placeholders as quoted environment expansions without interpolating screen text.
fn notify_command(template: &str) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut quote = ShellQuote::Unquoted;
    let mut offset = 0;
    while offset < template.len() {
        let remaining = &template[offset..];
        let placeholder = if remaining.starts_with("{session}") {
            Some(("TPP_SESSION", "{session}".len()))
        } else if remaining.starts_with("{reason}") {
            Some(("TPP_REASON", "{reason}".len()))
        } else {
            None
        };
        if let Some((name, length)) = placeholder {
            match quote {
                ShellQuote::Unquoted => rendered.push_str(&format!("\"${{{name}}}\"")),
                ShellQuote::Single => rendered.push_str(&format!("'\"${{{name}}}\"'")),
                ShellQuote::Double => rendered.push_str(&format!("${{{name}}}")),
            }
            offset += length;
            continue;
        }

        let ch = remaining.chars().next().expect("offset is within template");
        rendered.push(ch);
        offset += ch.len_utf8();
        if ch == '\\' && quote != ShellQuote::Single {
            if let Some(next) = template[offset..].chars().next() {
                let escaped =
                    quote == ShellQuote::Unquoted || matches!(next, '$' | '`' | '"' | '\\' | '\n');
                if escaped {
                    rendered.push(next);
                    offset += next.len_utf8();
                    continue;
                }
            }
        }
        match (quote, ch) {
            (ShellQuote::Unquoted, '\'') => quote = ShellQuote::Single,
            (ShellQuote::Unquoted, '"') => quote = ShellQuote::Double,
            (ShellQuote::Single, '\'') => quote = ShellQuote::Unquoted,
            (ShellQuote::Double, '"') => quote = ShellQuote::Unquoted,
            _ => {}
        }
    }
    rendered
}

fn escalate(ctx: &Ctx, root: &Path, session_name: &str, reason: &str, screen: &str) {
    let parent = session::parent_pane(&ctx.tmux, session_name);
    let tail = last_lines(screen, 5);
    let reason_line = sanitized_line(reason, 160);
    append_log(root, session_name, &format!("escalated: {reason_line}"));

    if ctx.cfg.watch.nudge_parent {
        if let Some(parent) = parent.as_deref() {
            let message = nudge_message(session_name, &reason_line, &tail);
            if let Err(err) = send_parent_nudge(ctx, parent, &message) {
                append_log(root, session_name, &format!("parent nudge failed: {err}"));
            }
        }
    }
    if let Err(err) = run_notify(ctx, session_name, parent.as_deref(), &reason_line, &tail) {
        append_log(root, session_name, &format!("notify failed: {err}"));
    }
}

fn stop_requested(root: &Path, session_name: &str, token: &str) -> bool {
    std::fs::read_to_string(stopfile_path(root, session_name))
        .is_ok_and(|requested| requested == token)
}

fn wait_for_stop(root: &Path, session_name: &str, token: &str, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        if stop_requested(root, session_name, token) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(100)));
    }
}

fn watch_loop(ctx: &Ctx, root: &Path, session_name: &str, origin: &str, token: &str) -> Result<()> {
    let mut engine = WatchEngine::new(&ctx.cfg.watch)?;
    let started = Instant::now();
    let poll = Duration::from_secs(ctx.cfg.watch.poll.as_secs());
    loop {
        if stop_requested(root, session_name, token) {
            return Ok(());
        }
        if session::origin_pane(&ctx.tmux, session_name).as_deref() != Some(origin)
            || !origin_is_alive(ctx, origin)
        {
            return Ok(());
        }
        let captured = match capture(&ctx.tmux, origin, Some(30), true, false) {
            Ok(captured) => captured,
            Err(_) => return Ok(()),
        };
        let stripped = strip_ansi(&captured);
        let screen = last_lines(&trim_trailing_blank(&stripped), 30);
        match engine.observe(started.elapsed(), screen_hash(&screen), &screen) {
            Some(WatchDecision::Enter { pattern }) => {
                if ctx
                    .tmux
                    .run(["send-keys", "-t", &tgt(origin), "Enter"])
                    .is_err()
                {
                    return Ok(());
                }
                append_log(
                    root,
                    session_name,
                    &format!("automatic Enter for {pattern:?}"),
                );
            }
            Some(WatchDecision::Escalate { reason }) => {
                escalate(ctx, root, session_name, &reason, &screen);
            }
            None => {}
        }
        if wait_for_stop(root, session_name, token, poll) {
            return Ok(());
        }
    }
}

/// Run one watcher loop in the foreground until its origin pane disappears or dies.
pub fn run_foreground(ctx: &Ctx, args: WatchTargetArgs) -> Result<()> {
    validate_config(&ctx.cfg.watch)?;
    let session_name = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, &args.target);
    if !session::exists(&ctx.tmux, &session_name) {
        return Ok(());
    }
    let Some(origin) = session::origin_pane(&ctx.tmux, &session_name) else {
        session::set_watch_armed(&ctx.tmux, &session_name, false);
        return Ok(());
    };
    if !origin_is_alive(ctx, &origin) {
        session::set_watch_armed(&ctx.tmux, &origin, false);
        return Ok(());
    }
    let root = watch_root(ctx);
    let guard = match PidGuard::acquire(&root, &session_name, &origin) {
        Ok(guard) => guard,
        Err(err) => {
            session::set_watch_armed(&ctx.tmux, &origin, false);
            return Err(err);
        }
    };
    let Some(guard) = guard else {
        return Ok(());
    };
    session::set_watch_armed(&ctx.tmux, &session_name, true);
    append_log(&root, &session_name, "watcher started");
    let result = watch_loop(ctx, &root, &session_name, &origin, &guard.record.token);
    session::set_watch_armed(&ctx.tmux, &origin, false);
    append_log(&root, &session_name, "watcher stopped");
    result
}

fn active_watchers(root: &Path) -> Result<Vec<WatchProcess>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).context("reading watcher state"),
    };
    let mut active = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("pid") {
            continue;
        }
        if let Some(process) = active_process(&entry.path())? {
            active.push(process);
        }
    }
    active.sort_by(|left, right| left.session.cmp(&right.session));
    Ok(active)
}

/// List live watcher pidfiles on the selected tmux socket.
pub fn list_watchers(ctx: &Ctx) -> Result<()> {
    let active = active_watchers(&watch_root(ctx))?;
    let rows = active
        .into_iter()
        .map(|process| WatchListRow {
            session: process.session,
            pid: process.pid,
            status: "running",
        })
        .collect::<Vec<_>>();
    if ctx.json {
        return print_json(&rows);
    }
    if ctx.quiet {
        for row in rows {
            println!("{}", row.session);
        }
        return Ok(());
    }
    if rows.is_empty() {
        eprintln!("no active watchers");
        return Ok(());
    }
    println!("{:<36} {:<8} STATUS", "SESSION", "PID");
    for row in rows {
        println!("{:<36} {:<8} {}", row.session, row.pid, row.status);
    }
    Ok(())
}

/// Stop a live watcher without failing when the session has none.
pub fn stop_if_running(ctx: &Ctx, session_name: &str) -> Result<bool> {
    let root = watch_root(ctx);
    let path = pidfile_path(&root, session_name);
    let Some(process) = active_process(&path)? else {
        session::set_watch_armed(&ctx.tmux, session_name, false);
        return Ok(false);
    };
    let stop_path = stopfile_path(&root, session_name);
    std::fs::write(&stop_path, &process.token).context("requesting watcher stop")?;
    for _ in 0..100 {
        match active_process(&path)? {
            None => {
                clear_stop_request(&stop_path, &process.token);
                session::set_watch_armed(&ctx.tmux, &process.origin, false);
                append_log(&root, session_name, "watcher stopped by command");
                return Ok(true);
            }
            Some(current) if current.token != process.token => {
                clear_stop_request(&stop_path, &process.token);
                return Ok(true);
            }
            Some(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    bail!("watcher did not stop within 5s for session {session_name}")
}

/// Stop the watcher recorded for one session and clear its armed marker.
pub fn stop_watcher(ctx: &Ctx, args: WatchTargetArgs) -> Result<()> {
    let session_name = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, &args.target);
    if !stop_if_running(ctx, &session_name)? {
        die(
            code::NOT_FOUND,
            format!("No watcher for session {session_name}"),
        );
    }
    Ok(())
}

fn clear_stop_request(path: &Path, token: &str) {
    if std::fs::read_to_string(path).is_ok_and(|requested| requested == token) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active_process, notify_command, nudge_message, pidfile_path, stop_requested, PidGuard,
        RuleSet, WatchDecision, WatchState,
    };
    use crate::config::{WatchAction, WatchCfg, WatchRuleCfg};
    use std::process::Command;
    use std::time::Duration;

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn user_rules_precede_builtins_and_first_match_wins() {
        let rules = RuleSet::compile(&[
            WatchRuleCfg {
                pattern: "Press enter".to_string(),
                action: WatchAction::Ignore,
                keys: Vec::new(),
            },
            WatchRuleCfg {
                pattern: "continue".to_string(),
                action: WatchAction::Notify,
                keys: Vec::new(),
            },
        ])
        .unwrap();

        let matched = rules.matched("Press enter to continue").unwrap();
        assert_eq!(matched.source, "Press enter");
        assert_eq!(matched.action, WatchAction::Ignore);
    }

    #[test]
    fn slash_delimited_rules_are_regexes() {
        let rules = RuleSet::compile(&[WatchRuleCfg {
            pattern: "/OAuth [0-9]+/".to_string(),
            action: WatchAction::Notify,
            keys: Vec::new(),
        }])
        .unwrap();

        assert_eq!(
            rules.matched("OAuth 123").unwrap().action,
            WatchAction::Notify
        );
        assert!(RuleSet::compile(&[WatchRuleCfg {
            pattern: "/[unterminated/".to_string(),
            action: WatchAction::Notify,
            keys: Vec::new(),
        }])
        .is_err());
    }

    #[test]
    fn builtins_cover_frozen_prompts_and_claude_idle() {
        let rules = RuleSet::compile(&[]).unwrap();
        for screen in [
            "Press enter to continue",
            "Enter to confirm · Esc to cancel",
            "Do you trust the contents of this directory?",
            "Yes, I trust this folder",
        ] {
            assert_eq!(rules.matched(screen).unwrap().action, WatchAction::Enter);
        }
        assert_eq!(
            rules.matched("composer  ? for shortcuts").unwrap().action,
            WatchAction::Ignore
        );
        assert_eq!(
            rules
                .matched("Press enter to continue\n? for shortcuts")
                .unwrap()
                .action,
            WatchAction::Ignore
        );
    }

    #[test]
    fn notify_placeholders_expand_through_quoted_environment_references() {
        assert_eq!(
            notify_command("notify --title {session} --message \"{reason}\" --tag '{session}'"),
            "notify --title \"${TPP_SESSION}\" --message \"${TPP_REASON}\" --tag ''\"${TPP_SESSION}\"''"
        );

        let output = Command::new("sh")
            .args([
                "-c",
                &notify_command("printf '<%s>|<%s>|<%s>' {session} \"{reason}\" '{session}'"),
            ])
            .env("TPP_SESSION", "space * $(false)")
            .env("TPP_REASON", "reason ; echo injected")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "<space * $(false)>|<reason ; echo injected>|<space * $(false)>"
        );
    }

    #[test]
    fn pidfile_lock_enforces_single_watcher_ownership() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let first = PidGuard::acquire(root, "tpp/test", "%1").unwrap().unwrap();
        assert_eq!(
            active_process(&pidfile_path(root, "tpp/test")).unwrap(),
            Some(first.record.clone())
        );
        assert!(PidGuard::acquire(root, "tpp/test", "%1").unwrap().is_none());
        assert!(!stop_requested(root, "tpp/test", &first.record.token));

        let second = PidGuard::acquire(root, "tpp/test", "%2").unwrap().unwrap();
        assert_eq!(
            active_process(&pidfile_path(root, "tpp/test")).unwrap(),
            Some(second.record.clone())
        );
        drop(first);
        assert_eq!(
            active_process(&pidfile_path(root, "tpp/test")).unwrap(),
            Some(second.record.clone())
        );
        std::fs::write(&second.stop_path, &second.record.token).unwrap();
        assert!(stop_requested(root, "tpp/test", &second.record.token));
        drop(second);
        assert!(PidGuard::acquire(root, "tpp/test", "%3").unwrap().is_some());
    }

    #[test]
    fn parent_nudge_neutralizes_shell_active_screen_text() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("injected");
        let dangerous = format!(
            "$(touch {}) ; `touch {}`",
            marker.display(),
            marker.display()
        );
        let message = nudge_message("tpp/$(false)", &dangerous, &dangerous);

        let _ = Command::new("sh")
            .args(["-c", &message])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(!marker.exists());
        assert!(message.contains("＄（touch"));
        assert!(message.contains("；"));
    }

    #[test]
    fn matched_prompts_use_the_fast_threshold() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Press enter to continue").unwrap();
        let mut state = WatchState::default();

        assert_eq!(state.observe(seconds(0), 1, Some(&matched), &cfg), None);
        assert_eq!(state.observe(seconds(4), 1, Some(&matched), &cfg), None);
        assert_eq!(
            state.observe(seconds(5), 1, Some(&matched), &cfg),
            Some(WatchDecision::Enter {
                pattern: "Press enter to continue".to_string()
            })
        );
    }

    #[test]
    fn unknown_screens_use_the_stuck_threshold() {
        let cfg = WatchCfg::default();
        let mut state = WatchState::default();

        assert_eq!(state.observe(seconds(0), 1, None, &cfg), None);
        assert_eq!(state.observe(seconds(29), 1, None, &cfg), None);
        assert_eq!(
            state.observe(seconds(30), 1, None, &cfg),
            Some(WatchDecision::Escalate {
                reason: "screen unchanged for 30s".to_string()
            })
        );
        assert_eq!(state.observe(seconds(60), 1, None, &cfg), None);
    }

    #[test]
    fn screen_changes_reset_the_episode_and_enter_budget() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Press enter to continue").unwrap();
        let mut state = WatchState::default();

        state.observe(seconds(0), 1, Some(&matched), &cfg);
        assert!(matches!(
            state.observe(seconds(5), 1, Some(&matched), &cfg),
            Some(WatchDecision::Enter { .. })
        ));
        assert_eq!(state.observe(seconds(6), 2, None, &cfg), None);
        assert_eq!(state.observe(seconds(35), 2, None, &cfg), None);
        assert!(matches!(
            state.observe(seconds(36), 2, None, &cfg),
            Some(WatchDecision::Escalate { .. })
        ));
    }

    #[test]
    fn unchanged_screen_after_enter_escalates() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Enter to confirm").unwrap();
        let mut state = WatchState::default();

        state.observe(seconds(0), 9, Some(&matched), &cfg);
        assert!(matches!(
            state.observe(seconds(5), 9, Some(&matched), &cfg),
            Some(WatchDecision::Enter { .. })
        ));
        assert_eq!(
            state.observe(seconds(8), 9, Some(&matched), &cfg),
            Some(WatchDecision::Escalate {
                reason: "screen unchanged after automatic Enter".to_string()
            })
        );
    }

    #[test]
    fn disabled_or_exhausted_auto_enter_escalates() {
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("Enter to confirm").unwrap();
        let disabled = WatchCfg {
            auto_enter: false,
            ..WatchCfg::default()
        };
        let mut state = WatchState::default();
        state.observe(seconds(0), 1, Some(&matched), &disabled);
        assert!(matches!(
            state.observe(seconds(5), 1, Some(&matched), &disabled),
            Some(WatchDecision::Escalate { .. })
        ));

        let exhausted = WatchCfg {
            max_enters: 0,
            ..WatchCfg::default()
        };
        let mut state = WatchState::default();
        state.observe(seconds(0), 1, Some(&matched), &exhausted);
        assert!(matches!(
            state.observe(seconds(5), 1, Some(&matched), &exhausted),
            Some(WatchDecision::Escalate { .. })
        ));
    }

    #[test]
    fn ignore_rules_suppress_unknown_escalation() {
        let cfg = WatchCfg::default();
        let rules = RuleSet::compile(&[]).unwrap();
        let matched = rules.matched("? for shortcuts").unwrap();
        let mut state = WatchState::default();

        state.observe(seconds(0), 1, Some(&matched), &cfg);
        assert_eq!(state.observe(seconds(600), 1, Some(&matched), &cfg), None);
    }

    #[test]
    fn cooldown_spans_content_change_episodes() {
        let cfg = WatchCfg::default();
        let mut state = WatchState::default();

        state.observe(seconds(0), 1, None, &cfg);
        assert!(matches!(
            state.observe(seconds(30), 1, None, &cfg),
            Some(WatchDecision::Escalate { .. })
        ));
        state.observe(seconds(31), 2, None, &cfg);
        assert_eq!(state.observe(seconds(61), 2, None, &cfg), None);
        assert!(matches!(
            state.observe(seconds(630), 2, None, &cfg),
            Some(WatchDecision::Escalate { .. })
        ));
    }
}
