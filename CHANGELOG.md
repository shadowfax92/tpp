# Changelog

All notable changes to `tpp` are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [0.1.0] — unreleased

First release. `tpp` (tmux++) is an ergonomic wrapper around the `tmux` binary for humans and
agents.

### Added
- **Sessions:** `run` (`r`), `new` (`n`), `name`, `ls` (`l`/`list`), `attach` (`a`), `rm`
  (`kill`/`remove`), `exit` (`e`/`quit`), `rename`, `has`, `clear` (`clr`).
- **Petname auto-naming:** unnamed `new` and `run` sessions use memorable
  `<adjective>-<animal>-<mmdd>` names; `name -n N` pre-mints unused, mutually unique names
  for scripts without creating sessions.
- **Output:** `cat` (`cap`/`capture`), `tail` (`follow`), `wait` — with `--json` and replay of
  recorded exited sessions.
- **Input:** `send` (`s`) and `paste` — literal text, `--file`/`--stdin`, `--keys`, and
  bracketed paste for verbatim multi-line content; `--enter` to submit. `paste` verifies
  Claude/Codex pasted-content submission by default, while `send --verify` opts in.
- **Global session model:** `ls` shows all `tpp` sessions on the selected tmux socket, and
  omitted-target commands use the same global set with sole-session or `fzf` selection.
- **Agent ergonomics:** `run` prints only the session name; stable exit codes (`3` not found,
  `4` timeout); `run --wait` streams to completion and exits with the command's status; `wait`
  on text / idle / pane-exit.
- **Sfmux lifecycle:** `has --alive` checks the root pane process instead of session existence;
  `ls --json` includes `state`, `pane_dead`, root `pid`, and `exit_status`; `new --on-exit`
  runs an exactly-once shell hook for pane exit and teardown paths.
- **Stuck-session watchdog:** command-bearing `new` sessions launch a detached watcher that
  auto-confirms known trust/continue prompts, ignores known idle screens, and escalates unknown
  stable output through the dispatching parent pane plus an optional notification command.
  Includes `new --no-watch`, `new --parent-pane`, `run --watch`, and `watch run|ls|stop`.
- **Sfmux pane targets:** `bind`, `targets`, and `unbind` name arbitrary tmux panes through pane
  user-options, and `send`, `paste`, `cat`, and `wait` accept `pane:<name>`.
- **Session family bridge:** the reserved `parent` target lets `send`, `paste`, `cat`, `tail`,
  and `wait` reach the pane that spawned the caller, while `children` lists sessions spawned
  from the current, explicit, or session-origin pane using existing tmux user-options only.
- **Durable mail:** `mail TARGET`, `mail send`, `mail ls`, `mail read`, and `reply` dual-write
  markdown message files to isolated socket/session mailboxes. One sanitized path-bearing
  pane ping acts as a best-effort doorbell; unread counts surface in `ls`, and mailboxes
  move, archive, and reset with session lifecycle.
- **tmux-compat verbs** (`has-session`, `new-session`, `attach-session`, `kill-session`,
  `list-sessions`, `set-buffer`, `paste-buffer`, `send-keys`, `capture-pane`, `x`) so `tpp` is a
  drop-in for `rmux` in `sf-auto-mux` after `s/rmux/tpp/`.
- **Config** at `~/.config/tpp/config.toml`; socket-namespaced recorded transcripts under
  `~/.tpp/data/`. `init`, `config`, `doctor`, `completions`.
- `remain-on-exit` on tpp sessions so finished commands keep their output for `cat`/`tail`.

### Fixed
- Pane captures (`cat`, `tail`, `wait`, verification) no longer include tmux's dead-pane
  chrome: a trailing "Pane is dead (status …)" overlay row is stripped so real output stays
  reachable behind the blank screen padding above it.
