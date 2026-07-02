//! CLI-surface regression tests: aliases resolve, trailing args are captured, and the
//! tmux-compat verbs forward their raw args.

use clap::CommandFactory;
use clap::Parser;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tpp::cli::{Cli, Cmd};
use tpp::commands::select::{normalize_explicit, parse_fzf_output};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct TmuxServer {
    socket: String,
}

impl TmuxServer {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self {
            socket: format!("tpp-test-{}-{nanos}-{seq}", std::process::id()),
        }
    }
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn parse(args: &[&str]) -> Cli {
    Cli::parse_from(std::iter::once("tpp").chain(args.iter().copied()))
}

fn try_parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("tpp").chain(args.iter().copied()))
}

fn run_tpp(server: &TmuxServer, root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tpp"))
        .arg("-L")
        .arg(&server.socket)
        .args(args)
        .env("TPP_CONFIG_DIR", root.join("config"))
        .env("TPP_STATE_DIR", root.join("state"))
        .output()
        .expect("run tpp")
}

fn fake_fzf_path(fake_bin: &std::path::Path, script: &str) -> String {
    std::fs::create_dir_all(fake_bin).unwrap();
    let fake_fzf = fake_bin.join("fzf");
    std::fs::write(&fake_fzf, script).unwrap();
    let mut perms = std::fs::metadata(&fake_fzf).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_fzf, perms).unwrap();

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    format!("{}:{}", fake_bin.display(), original_path.to_string_lossy())
}

fn run_tmux(server: &TmuxServer, args: &[&str]) -> Output {
    Command::new("tmux")
        .arg("-L")
        .arg(&server.socket)
        .args(args)
        .output()
        .expect("run tmux")
}

fn wait_for_raw_capture(server: &TmuxServer, target: &str, needle: &str) -> String {
    let mut last = String::new();
    for _ in 0..50 {
        let out = run_tmux(server, &["capture-pane", "-p", "-t", target]);
        assert_success(&out);
        last = String::from_utf8_lossy(&out.stdout).to_string();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("raw tmux capture for {target} did not contain {needle:?}:\n{last}");
}

fn wait_for_file_lines(path: &std::path::Path, count: usize) -> String {
    let mut last = String::new();
    for _ in 0..50 {
        last = std::fs::read_to_string(path).unwrap_or_default();
        if last.lines().count() >= count {
            return last;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "{} did not reach {count} lines; last contents:\n{last}",
        path.display()
    );
}

fn assert_success(out: &Output) {
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn assert_not_found(out: &Output, session: &str) {
    assert_eq!(
        out.status.code(),
        Some(3),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(&format!("No such session {session}")),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn assert_exit_code(out: &Output, code: i32) {
    assert_eq!(
        out.status.code(),
        Some(code),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn bare_invocation_has_no_subcommand() {
    assert!(parse(&[]).cmd.is_none());
}

#[test]
fn ls_aliases() {
    for a in ["ls", "l", "list"] {
        assert!(matches!(parse(&[a]).cmd, Some(Cmd::Ls(_))), "alias {a}");
    }
}

#[test]
fn exit_aliases() {
    for a in ["exit", "e", "quit"] {
        assert!(matches!(parse(&[a]).cmd, Some(Cmd::Exit(_))), "alias {a}");
    }
}

#[test]
fn send_short_alias_and_target() {
    match parse(&["s", "-t", "x", "hello"]).cmd {
        Some(Cmd::Send(a)) => {
            assert_eq!(a.target.as_deref(), Some("x"));
            assert_eq!(a.text, vec!["hello"]);
        }
        other => panic!("expected Send, got {other:?}"),
    }
}

#[test]
fn send_text_allows_enter_after_value() {
    match parse(&["send", "-e", "yo"]).cmd {
        Some(Cmd::Send(a)) => {
            assert_eq!(a.text, vec!["yo"]);
            assert!(a.enter);
        }
        other => panic!("expected Send, got {other:?}"),
    }

    match parse(&["send", "yo", "-e"]).cmd {
        Some(Cmd::Send(a)) => {
            assert_eq!(a.text, vec!["yo"]);
            assert!(a.enter);
        }
        other => panic!("expected Send, got {other:?}"),
    }

    match parse(&["send", "yo", "--enter"]).cmd {
        Some(Cmd::Send(a)) => {
            assert_eq!(a.text, vec!["yo"]);
            assert!(a.enter);
        }
        other => panic!("expected Send, got {other:?}"),
    }

    match parse(&["send", "--", "yo", "-e"]).cmd {
        Some(Cmd::Send(a)) => {
            assert_eq!(a.text, vec!["yo", "-e"]);
            assert!(!a.enter);
        }
        other => panic!("expected Send, got {other:?}"),
    }
}

#[test]
fn paste_text_allows_no_enter_after_value() {
    match parse(&["paste", "yo", "--no-enter"]).cmd {
        Some(Cmd::Paste(a)) => {
            assert_eq!(a.text, vec!["yo"]);
            assert!(a.no_enter);
        }
        other => panic!("expected Paste, got {other:?}"),
    }
}

#[test]
fn run_captures_trailing_command() {
    match parse(&["run", "--", "npm", "test"]).cmd {
        Some(Cmd::Run(a)) => assert_eq!(a.command, vec!["npm", "test"]),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn compat_new_session_forwards_raw_args() {
    match parse(&["new-session", "-d", "-s", "x", "-c", "/tmp", "cmd"]).cmd {
        Some(Cmd::NewSession(r)) => {
            assert_eq!(r.args, vec!["-d", "-s", "x", "-c", "/tmp", "cmd"]);
        }
        other => panic!("expected NewSession, got {other:?}"),
    }
}

#[test]
fn compat_paste_buffer_forwards_flags() {
    match parse(&["paste-buffer", "-t", "x", "-p"]).cmd {
        Some(Cmd::PasteBuffer(r)) => assert_eq!(r.args, vec!["-t", "x", "-p"]),
        other => panic!("expected PasteBuffer, got {other:?}"),
    }
}

#[test]
fn global_socket_flag_parses_before_subcommand() {
    let cli = parse(&["-L", "mysock", "ls"]);
    assert_eq!(cli.socket.as_deref(), Some("mysock"));
}

#[test]
fn scope_flag_is_not_a_cli_option() {
    assert!(try_parse(&["--scope", "none", "ls"]).is_err());
}

#[test]
fn help_does_not_mention_scope() {
    let mut help = Vec::new();
    Cli::command().write_long_help(&mut help).unwrap();
    let help = String::from_utf8(help).unwrap();

    assert!(!help.contains("--scope"));
    assert!(!help.contains("scope"));
}

#[test]
fn tail_help_uses_global_default_wording() {
    let help = Cli::try_parse_from(["tpp", "tail", "--help"])
        .unwrap_err()
        .to_string();

    assert!(!help.contains("--scope"));
    assert!(!help.contains("scope"));
    assert!(help.contains("default: the sole session, or a picker"));
}

#[test]
fn rm_without_names_uses_global_picker_candidates() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    let fake_bin = tmp.path().join("bin");
    let path = fake_fzf_path(&fake_bin, "#!/bin/sh\ncat\n");
    let dir_a = tmp.path().join("a");
    let dir_b = tmp.path().join("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["new", "-s", "one", "-c", dir_a.to_str().unwrap()],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["new", "-s", "two", "-c", dir_b.to_str().unwrap()],
    ));

    let before = run_tpp(&server, tmp.path(), &["-q", "ls"]);
    assert_success(&before);
    let before = String::from_utf8_lossy(&before.stdout);
    assert!(before.contains("tpp/one"));
    assert!(before.contains("tpp/two"));

    let rm = Command::new(env!("CARGO_BIN_EXE_tpp"))
        .arg("-L")
        .arg(&server.socket)
        .args(["-q", "rm"])
        .env("TPP_CONFIG_DIR", tmp.path().join("config"))
        .env("TPP_STATE_DIR", tmp.path().join("state"))
        .env("PATH", path)
        .output()
        .expect("run tpp rm");
    assert_success(&rm);

    let after = run_tpp(&server, tmp.path(), &["-q", "ls"]);
    assert_success(&after);
    assert!(String::from_utf8_lossy(&after.stdout).trim().is_empty());
}

#[test]
fn cat_resolves_unprefixed_live_session_name() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "run",
            "-s",
            "codex/live",
            "--",
            "sh",
            "-c",
            "printf live-output; sleep 2",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["wait", "-t", "codex/live", "--text", "live-output"],
    ));

    let cat = run_tpp(&server, tmp.path(), &["cat", "codex/live"]);
    assert_success(&cat);
    assert!(String::from_utf8_lossy(&cat.stdout).contains("live-output"));
}

#[test]
fn cat_resolves_unprefixed_recorded_session_name() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "run",
            "-s",
            "codex/recorded",
            "--wait",
            "--record",
            "--",
            "sh",
            "-c",
            "printf recorded-output",
        ],
    ));

    let cat = run_tpp(&server, tmp.path(), &["cat", "codex/recorded"]);
    assert_success(&cat);
    assert!(String::from_utf8_lossy(&cat.stdout).contains("recorded-output"));
}

#[test]
fn cat_without_name_prints_recent_recorded_session() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "run",
            "-s",
            "codex/auto-recorded",
            "--wait",
            "--record",
            "--",
            "sh",
            "-c",
            "printf auto-recorded-output",
        ],
    ));

    let cat = run_tpp(&server, tmp.path(), &["cat", "-S"]);
    assert_success(&cat);
    assert!(String::from_utf8_lossy(&cat.stdout).contains("auto-recorded-output"));
}

#[test]
fn cat_all_offers_recorded_sessions_to_fzf() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    for (name, output) in [
        ("codex/pick-one", "pick-one-output"),
        ("codex/pick-two", "pick-two-output"),
    ] {
        assert_success(&run_tpp(
            &server,
            tmp.path(),
            &[
                "run",
                "-s",
                name,
                "--wait",
                "--record",
                "--",
                "sh",
                "-c",
                "printf $0",
                output,
            ],
        ));
    }
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[ls]\nshow_exited_hours = 0\n",
    )
    .unwrap();

    let fake_bin = tmp.path().join("bin");
    let path = fake_fzf_path(
        &fake_bin,
        "#!/bin/sh\ncat > \"$FZF_CAPTURE\"\nprintf '%s\\n' \"$FZF_PICK\"\n",
    );
    let capture_path = tmp.path().join("fzf-candidates");
    let cat = Command::new(env!("CARGO_BIN_EXE_tpp"))
        .arg("-L")
        .arg(&server.socket)
        .args(["cat", "-a", "-S"])
        .env("TPP_CONFIG_DIR", tmp.path().join("config"))
        .env("TPP_STATE_DIR", tmp.path().join("state"))
        .env("PATH", path)
        .env("FZF_CAPTURE", &capture_path)
        .env("FZF_PICK", "tpp/codex/pick-two")
        .output()
        .expect("run tpp cat");

    assert_success(&cat);
    assert!(String::from_utf8_lossy(&cat.stdout).contains("pick-two-output"));

    let candidates = std::fs::read_to_string(capture_path).unwrap();
    assert!(candidates.contains("tpp/codex/pick-one"));
    assert!(candidates.contains("tpp/codex/pick-two"));
}

#[test]
fn exit_records_output_until_clear() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "run",
            "-s",
            "codex/exit-record",
            "--",
            "sh",
            "-c",
            "i=1; while [ \"$i\" -le 1105 ]; do echo line-$i; i=$((i+1)); done; sleep 30",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "wait",
            "-t",
            "codex/exit-record",
            "--text",
            "line-1105",
            "--timeout",
            "5000",
        ],
    ));

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["exit", "codex/exit-record"],
    ));

    let exited_dir = tmp.path().join("state").join("exited");
    let logs: Vec<_> = std::fs::read_dir(&exited_dir)
        .unwrap()
        .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("log"))
        .collect();
    assert_eq!(logs.len(), 1, "logs under {}", exited_dir.display());
    assert!(logs[0]
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("socket-")));

    let cat = run_tpp(&server, tmp.path(), &["cat", "-S", "codex/exit-record"]);
    assert_success(&cat);
    let stdout = String::from_utf8_lossy(&cat.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1000);
    assert_eq!(lines.first(), Some(&"line-106"));
    assert_eq!(lines.last(), Some(&"line-1105"));

    let clear = run_tpp(&server, tmp.path(), &["clear"]);
    assert_success(&clear);
    assert!(String::from_utf8_lossy(&clear.stdout).contains("cleared 1"));

    let missing = run_tpp(&server, tmp.path(), &["cat", "-S", "codex/exit-record"]);
    assert_not_found(&missing, "tpp/codex/exit-record");
}

#[test]
fn cat_uses_original_pane_after_active_window_changes() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "run",
            "-s",
            "codex/original",
            "--",
            "sh",
            "-c",
            "printf original-output; sleep 5",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["wait", "-t", "codex/original", "--text", "original-output"],
    ));

    assert_success(&run_tmux(
        &server,
        &[
            "new-window",
            "-t",
            "tpp/codex/original",
            "printf other-output; sleep 5",
        ],
    ));
    let raw = wait_for_raw_capture(&server, "tpp/codex/original", "other-output");
    assert!(raw.contains("other-output"));

    let cat = run_tpp(&server, tmp.path(), &["cat", "codex/original"]);
    assert_success(&cat);
    let stdout = String::from_utf8_lossy(&cat.stdout);
    assert!(stdout.contains("original-output"), "{stdout}");
    assert!(!stdout.contains("other-output"), "{stdout}");
}

#[test]
fn alive_check_distinguishes_lingering_dead_sessions() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/alive-live",
            "--",
            "sh",
            "-c",
            "printf alive-ready; sleep 5",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["wait", "-t", "codex/alive-live", "--text", "alive-ready"],
    ));

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["has", "codex/alive-live", "--alive"],
    ));

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["new", "-s", "codex/alive-dead", "--", "sh", "-c", "exit 0"],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "wait",
            "-t",
            "codex/alive-dead",
            "--exit",
            "--timeout",
            "5000",
        ],
    ));

    assert_success(&run_tpp(&server, tmp.path(), &["has", "codex/alive-dead"]));
    assert_exit_code(
        &run_tpp(&server, tmp.path(), &["has", "codex/alive-dead", "--alive"]),
        1,
    );
    assert_exit_code(
        &run_tpp(
            &server,
            tmp.path(),
            &["has", "codex/alive-missing", "--alive"],
        ),
        3,
    );
}

#[test]
fn ls_json_reports_alive_state_fields() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/json-live",
            "--",
            "sh",
            "-c",
            "printf json-live-ready; sleep 5",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["wait", "-t", "codex/json-live", "--text", "json-live-ready"],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["new", "-s", "codex/json-dead", "--", "sh", "-c", "exit 0"],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "wait",
            "-t",
            "codex/json-dead",
            "--exit",
            "--timeout",
            "5000",
        ],
    ));

    let ls = run_tpp(&server, tmp.path(), &["--json", "ls"]);
    assert_success(&ls);
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&ls.stdout).unwrap();
    let live = rows
        .iter()
        .find(|row| row["name"] == "tpp/codex/json-live")
        .unwrap();
    let dead = rows
        .iter()
        .find(|row| row["name"] == "tpp/codex/json-dead")
        .unwrap();

    assert_eq!(live["state"], "running");
    assert_eq!(live["pane_dead"], false);
    assert!(live["pid"].as_u64().is_some());
    assert!(live["exit_status"].is_null());
    assert_eq!(dead["state"], "exited");
    assert_eq!(dead["pane_dead"], true);
    assert_eq!(dead["exit_status"], 0);
}

#[test]
fn alive_check_treats_missing_origin_pane_as_not_alive() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/missing-origin",
            "--",
            "sh",
            "-c",
            "printf root-ready; sleep 30",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["wait", "-t", "codex/missing-origin", "--text", "root-ready"],
    ));
    let origin = run_tmux(
        &server,
        &[
            "show-option",
            "-qv",
            "-t",
            "tpp/codex/missing-origin",
            "@tpp_origin_pane",
        ],
    );
    assert_success(&origin);
    let origin = String::from_utf8_lossy(&origin.stdout).trim().to_string();
    assert!(!origin.is_empty());

    assert_success(&run_tmux(
        &server,
        &[
            "split-window",
            "-t",
            "tpp/codex/missing-origin",
            "sh",
            "-c",
            "sleep 30",
        ],
    ));
    assert_success(&run_tmux(&server, &["kill-pane", "-t", &origin]));

    assert_exit_code(
        &run_tpp(
            &server,
            tmp.path(),
            &["has", "codex/missing-origin", "--alive"],
        ),
        1,
    );

    let ls = run_tpp(&server, tmp.path(), &["--json", "ls"]);
    assert_success(&ls);
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&ls.stdout).unwrap();
    let row = rows
        .iter()
        .find(|row| row["name"] == "tpp/codex/missing-origin")
        .unwrap();
    assert_eq!(row["state"], "exited");
    assert_eq!(row["pane_dead"], true);
}

#[test]
fn on_exit_hook_fires_once_for_natural_exit() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    let hook_file = tmp.path().join("hook-natural");
    let hook = format!(
        "printf '%s:%s\\n' \"$TPP_SESSION_NAME\" \"$TPP_EXIT_STATUS\" >> {}",
        shell_quote(&hook_file.to_string_lossy())
    );

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/on-exit-natural",
            "--on-exit",
            &hook,
            "--",
            "sh",
            "-c",
            "exit 7",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "wait",
            "-t",
            "codex/on-exit-natural",
            "--exit",
            "--timeout",
            "5000",
        ],
    ));

    let lines = wait_for_file_lines(&hook_file, 1);
    assert_eq!(
        lines.lines().collect::<Vec<_>>(),
        ["tpp/codex/on-exit-natural:7"]
    );

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["rm", "codex/on-exit-natural"],
    ));
    std::thread::sleep(Duration::from_millis(200));
    let lines = std::fs::read_to_string(&hook_file).unwrap();
    assert_eq!(lines.lines().count(), 1);
}

#[test]
fn on_exit_hook_fires_for_tpp_rm() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    let hook_file = tmp.path().join("hook-rm");
    let pid_file = tmp.path().join("hook-rm.pid");
    let hook = format!(
        "if ps -p \"$(cat {})\" >/dev/null 2>&1; then state=alive; else state=dead; fi; printf '%s:%s\\n' \"$TPP_SESSION_NAME\" \"$state\" >> {}",
        shell_quote(&pid_file.to_string_lossy()),
        shell_quote(&hook_file.to_string_lossy())
    );

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/on-exit-rm",
            "--on-exit",
            &hook,
            "--",
            "sh",
            "-c",
            "sleep 30",
        ],
    ));
    let origin = run_tmux(
        &server,
        &[
            "show-option",
            "-qv",
            "-t",
            "tpp/codex/on-exit-rm",
            "@tpp_origin_pane",
        ],
    );
    assert_success(&origin);
    let origin = String::from_utf8_lossy(&origin.stdout).trim().to_string();
    let pid = run_tmux(
        &server,
        &["display-message", "-p", "-t", &origin, "#{pane_pid}"],
    );
    assert_success(&pid);
    std::fs::write(
        &pid_file,
        String::from_utf8_lossy(&pid.stdout).trim().as_bytes(),
    )
    .unwrap();
    assert_success(&run_tpp(&server, tmp.path(), &["rm", "codex/on-exit-rm"]));

    let lines = wait_for_file_lines(&hook_file, 1);
    assert_eq!(
        lines.lines().collect::<Vec<_>>(),
        ["tpp/codex/on-exit-rm:dead"]
    );
}

#[test]
fn on_exit_hook_fires_for_tpp_exit() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    let hook_file = tmp.path().join("hook-exit");
    let pid_file = tmp.path().join("hook-exit.pid");
    let hook = format!(
        "if ps -p \"$(cat {})\" >/dev/null 2>&1; then state=alive; else state=dead; fi; printf '%s:%s\\n' \"$TPP_SESSION_NAME\" \"$state\" >> {}",
        shell_quote(&pid_file.to_string_lossy()),
        shell_quote(&hook_file.to_string_lossy())
    );

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/on-exit-exit",
            "--on-exit",
            &hook,
            "--",
            "sh",
            "-c",
            "sleep 30",
        ],
    ));
    let origin = run_tmux(
        &server,
        &[
            "show-option",
            "-qv",
            "-t",
            "tpp/codex/on-exit-exit",
            "@tpp_origin_pane",
        ],
    );
    assert_success(&origin);
    let origin = String::from_utf8_lossy(&origin.stdout).trim().to_string();
    let pid = run_tmux(
        &server,
        &["display-message", "-p", "-t", &origin, "#{pane_pid}"],
    );
    assert_success(&pid);
    std::fs::write(
        &pid_file,
        String::from_utf8_lossy(&pid.stdout).trim().as_bytes(),
    )
    .unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["exit", "codex/on-exit-exit"],
    ));

    let lines = wait_for_file_lines(&hook_file, 1);
    assert_eq!(
        lines.lines().collect::<Vec<_>>(),
        ["tpp/codex/on-exit-exit:dead"]
    );
}

#[test]
fn on_exit_hook_forces_remain_on_exit_for_natural_exit() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[new]\nremain_on_exit = false\n",
    )
    .unwrap();
    let hook_file = tmp.path().join("hook-no-remain");
    let hook = format!(
        "printf '%s:%s\\n' \"$TPP_SESSION_NAME\" \"$TPP_EXIT_STATUS\" >> {}",
        shell_quote(&hook_file.to_string_lossy())
    );

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/on-exit-no-remain",
            "--on-exit",
            &hook,
            "--",
            "sh",
            "-c",
            "exit 0",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "wait",
            "-t",
            "codex/on-exit-no-remain",
            "--exit",
            "--timeout",
            "5000",
        ],
    ));

    let lines = wait_for_file_lines(&hook_file, 1);
    assert_eq!(
        lines.lines().collect::<Vec<_>>(),
        ["tpp/codex/on-exit-no-remain:0"]
    );
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &["has", "codex/on-exit-no-remain"],
    ));
}

#[test]
fn on_exit_hook_fires_for_raw_tmux_kill_session() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    let hook_file = tmp.path().join("hook-raw");
    let hook = format!(
        "printf '%s:%s\\n' \"$TPP_SESSION_NAME\" \"$TPP_EXIT_STATUS\" >> {}",
        shell_quote(&hook_file.to_string_lossy())
    );

    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new",
            "-s",
            "codex/on-exit-raw",
            "--on-exit",
            &hook,
            "--",
            "sh",
            "-c",
            "sleep 30",
        ],
    ));
    assert_success(&run_tmux(
        &server,
        &["kill-session", "-t", "tpp/codex/on-exit-raw"],
    ));

    let lines = wait_for_file_lines(&hook_file, 1);
    assert_eq!(
        lines.lines().collect::<Vec<_>>(),
        ["tpp/codex/on-exit-raw:"]
    );
}

#[test]
fn compat_new_session_cat_uses_original_pane_after_active_window_changes() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "new-session",
            "-d",
            "-s",
            "codex/compat",
            "printf compat-original-output; sleep 5",
        ],
    ));
    assert_success(&run_tpp(
        &server,
        tmp.path(),
        &[
            "wait",
            "-t",
            "codex/compat",
            "--text",
            "compat-original-output",
        ],
    ));

    assert_success(&run_tmux(
        &server,
        &[
            "new-window",
            "-t",
            "tpp/codex/compat",
            "printf compat-other-output; sleep 5",
        ],
    ));
    let raw = wait_for_raw_capture(&server, "tpp/codex/compat", "compat-other-output");
    assert!(raw.contains("compat-other-output"));

    let cat = run_tpp(&server, tmp.path(), &["cat", "codex/compat"]);
    assert_success(&cat);
    let stdout = String::from_utf8_lossy(&cat.stdout);
    assert!(stdout.contains("compat-original-output"), "{stdout}");
    assert!(!stdout.contains("compat-other-output"), "{stdout}");
}

#[test]
fn missing_explicit_targets_report_not_found() {
    if !tmux_available() {
        return;
    }

    let server = TmuxServer::new();
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        &["cat", "codex/missing"][..],
        &["tail", "codex/missing"][..],
        &["exit", "codex/missing"][..],
    ] {
        let out = run_tpp(&server, tmp.path(), args);
        assert_not_found(&out, "tpp/codex/missing");
    }
}

#[test]
fn rename_accepts_new_name_without_session() {
    match try_parse(&["rename", "api2"])
        .expect("rename with picker target")
        .cmd
    {
        Some(Cmd::Rename(a)) => assert_eq!(a.names, vec!["api2"]),
        other => panic!("expected Rename, got {other:?}"),
    }
}

#[test]
fn rename_keeps_explicit_old_and_new_names() {
    match parse(&["rename", "api", "api2"]).cmd {
        Some(Cmd::Rename(a)) => assert_eq!(a.names, vec!["api", "api2"]),
        other => panic!("expected Rename, got {other:?}"),
    }
}

#[test]
fn selector_helpers_normalize_targets_and_fzf_output() {
    assert_eq!(normalize_explicit("=api"), "api");
    assert_eq!(
        parse_fzf_output("api\n worker \n\n"),
        vec!["api".to_string(), "worker".to_string()]
    );
}
