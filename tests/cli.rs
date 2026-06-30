//! CLI-surface regression tests: aliases resolve, trailing args are captured, and the
//! tmux-compat verbs forward their raw args.

use clap::Parser;
use tpp::cli::{Cli, Cmd};
use tpp::commands::select::{normalize_explicit, parse_fzf_output};

fn parse(args: &[&str]) -> Cli {
    Cli::parse_from(std::iter::once("tpp").chain(args.iter().copied()))
}

fn try_parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("tpp").chain(args.iter().copied()))
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
