//! Per-session stable-screen detection and watchdog process lifecycle.

use std::collections::hash_map::DefaultHasher;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WatchProcess {
    session: String,
    pid: u32,
    process_start: String,
}

#[derive(Debug, Serialize)]
struct WatchListRow {
    session: String,
    pid: u32,
    status: &'static str,
}

struct PidGuard {
    path: PathBuf,
    pid: u32,
    process_start: String,
}

impl PidGuard {
    fn acquire(root: &Path, session_name: &str) -> Result<Option<Self>> {
        create_private_dir_all(root)?;
        let path = pidfile_path(root, session_name);
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let pid = std::process::id();
                    let process_start =
                        process_start(pid).context("reading watcher process start time")?;
                    let record = WatchProcess {
                        session: session_name.to_string(),
                        pid,
                        process_start: process_start.clone(),
                    };
                    if let Err(err) = serde_json::to_writer(&mut file, &record) {
                        let _ = std::fs::remove_file(&path);
                        return Err(err).context("writing watcher pidfile");
                    }
                    writeln!(file)?;
                    return Ok(Some(Self {
                        path,
                        pid,
                        process_start,
                    }));
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if read_process(&path).is_some_and(|process| process_matches(&process)) {
                        return Ok(None);
                    }
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(remove_err) if remove_err.kind() == ErrorKind::NotFound => {}
                        Err(remove_err) => {
                            return Err(remove_err).context("removing stale pidfile")
                        }
                    }
                }
                Err(err) => return Err(err).context("creating watcher pidfile"),
            }
        }
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        if read_process(&self.path).is_some_and(|process| {
            process.pid == self.pid && process.process_start == self.process_start
        }) {
            let _ = std::fs::remove_file(&self.path);
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

fn read_process(path: &Path) -> Option<WatchProcess> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn process_start(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let started = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!started.is_empty()).then_some(started)
}

fn process_matches(process: &WatchProcess) -> bool {
    process_start(process.pid).is_some_and(|started| started == process.process_start)
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
        .replace('"', "'")
        .chars()
        .take(limit)
        .collect()
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

fn notify_command(template: &str) -> String {
    template
        .replace("{session}", "${TPP_SESSION}")
        .replace("{reason}", "${TPP_REASON}")
}

fn escalate(ctx: &Ctx, root: &Path, session_name: &str, reason: &str, screen: &str) {
    let parent = session::parent_pane(&ctx.tmux, session_name);
    let tail = last_lines(screen, 5);
    let reason_line = sanitized_line(reason, 160);
    let tail_line = sanitized_line(&tail, 120);
    append_log(root, session_name, &format!("escalated: {reason_line}"));

    if ctx.cfg.watch.nudge_parent {
        if let Some(parent) = parent.as_deref() {
            let message = format!(
                "[tpp:{session_name}] ⚠️ stuck: {reason_line} — last: \"{tail_line}\" — check: tpp attach {session_name}"
            );
            if let Err(err) = send_parent_nudge(ctx, parent, &message) {
                append_log(root, session_name, &format!("parent nudge failed: {err}"));
            }
        }
    }
    if let Err(err) = run_notify(ctx, session_name, parent.as_deref(), &reason_line, &tail) {
        append_log(root, session_name, &format!("notify failed: {err}"));
    }
}

fn watch_loop(ctx: &Ctx, root: &Path, session_name: &str, origin: &str) -> Result<()> {
    let mut engine = WatchEngine::new(&ctx.cfg.watch)?;
    let started = Instant::now();
    let poll = Duration::from_secs(ctx.cfg.watch.poll.as_secs());
    loop {
        if !session::exists(&ctx.tmux, session_name) || !origin_is_alive(ctx, origin) {
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
        std::thread::sleep(poll);
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
        session::set_watch_armed(&ctx.tmux, &session_name, false);
        return Ok(());
    }
    let root = watch_root(ctx);
    let guard = match PidGuard::acquire(&root, &session_name) {
        Ok(guard) => guard,
        Err(err) => {
            session::set_watch_armed(&ctx.tmux, &session_name, false);
            return Err(err);
        }
    };
    let Some(_guard) = guard else {
        return Ok(());
    };
    session::set_watch_armed(&ctx.tmux, &session_name, true);
    append_log(&root, &session_name, "watcher started");
    let result = watch_loop(ctx, &root, &session_name, &origin);
    session::set_watch_armed(&ctx.tmux, &session_name, false);
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
        match read_process(&entry.path()) {
            Some(process) if process_matches(&process) => active.push(process),
            _ => {
                let _ = std::fs::remove_file(entry.path());
            }
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

/// Stop the watcher recorded for one session and clear its armed marker.
pub fn stop_watcher(ctx: &Ctx, args: WatchTargetArgs) -> Result<()> {
    let session_name = session::resolve_existing_name(&ctx.tmux, &ctx.cfg, &args.target);
    let root = watch_root(ctx);
    let path = pidfile_path(&root, &session_name);
    let Some(process) = read_process(&path) else {
        die(
            code::NOT_FOUND,
            format!("No watcher for session {session_name}"),
        );
    };
    if process_matches(&process) {
        let status = Command::new("kill")
            .args(["-TERM", &process.pid.to_string()])
            .status()
            .context("stopping session watcher")?;
        if !status.success() {
            bail!("could not stop watcher pid {}", process.pid);
        }
        for _ in 0..40 {
            if !process_matches(&process) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    let _ = std::fs::remove_file(path);
    session::set_watch_armed(&ctx.tmux, &session_name, false);
    append_log(&root, &session_name, "watcher stopped by command");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        notify_command, process_matches, process_start, RuleSet, WatchDecision, WatchProcess,
        WatchState,
    };
    use crate::config::{WatchAction, WatchCfg, WatchRuleCfg};
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
            },
            WatchRuleCfg {
                pattern: "continue".to_string(),
                action: WatchAction::Notify,
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
        }])
        .unwrap();

        assert_eq!(
            rules.matched("OAuth 123").unwrap().action,
            WatchAction::Notify
        );
        assert!(RuleSet::compile(&[WatchRuleCfg {
            pattern: "/[unterminated/".to_string(),
            action: WatchAction::Notify,
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
            notify_command("notify --title {session} --message \"{reason}\""),
            "notify --title ${TPP_SESSION} --message \"${TPP_REASON}\""
        );
    }

    #[test]
    fn watcher_identity_requires_pid_and_process_start() {
        let pid = std::process::id();
        let started = process_start(pid).unwrap();
        let process = WatchProcess {
            session: "tpp/test".to_string(),
            pid,
            process_start: started.clone(),
        };

        assert!(process_matches(&process));
        assert!(!process_matches(&WatchProcess {
            process_start: format!("{started}-different"),
            ..process
        }));
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
