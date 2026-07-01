//! CLI-surface regression tests: aliases resolve, trailing args are captured, and the
//! tmux-compat verbs forward their raw args.

use clap::CommandFactory;
use clap::Parser;
use std::process::{Command, Output, Stdio};
use tpp::cli::{Cli, Cmd};
use tpp::commands::select::{normalize_explicit, parse_fzf_output};

struct TmuxServer {
    socket: String,
}

impl TmuxServer {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self {
            socket: format!("tpp-test-{}-{nanos}", std::process::id()),
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

fn assert_success(out: &Output) {
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
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
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake_fzf = fake_bin.join("fzf");
    std::fs::write(&fake_fzf, "#!/bin/sh\ncat\n").unwrap();
    let mut perms = std::fs::metadata(&fake_fzf).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_fzf, perms).unwrap();

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let path = format!("{}:{}", fake_bin.display(), original_path.to_string_lossy());
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
