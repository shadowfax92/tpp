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
