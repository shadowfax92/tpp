# Changelog

All notable changes to `tpp` are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [0.1.0] — unreleased

First release. `tpp` (tmux++) is an ergonomic wrapper around the `tmux` binary for humans and
agents.

### Added
- **Sessions:** `run` (`r`), `new` (`n`), `ls` (`l`/`list`), `attach` (`a`), `rm`
  (`kill`/`remove`), `exit` (`e`/`quit`), `rename`, `has`, `clear` (`clr`).
- **Output:** `cat` (`cap`/`capture`), `tail` (`follow`), `wait` — with `--json` and replay of
  recorded exited sessions.
- **Input:** `send` (`s`) and `paste` — literal text, `--file`/`--stdin`, `--keys`, and
  bracketed paste for verbatim multi-line content; `--enter` to submit.
- **Universal listing with scope tags:** sessions are tagged (tmux session user-options) with
  the directory they were created in (git root by default); `ls` shows all `tpp` sessions while
  omitted-target commands can still use scope. `[scope] mode = git|cwd|none`, `--scope`.
- **Agent ergonomics:** `run` prints only the session name; stable exit codes (`3` not found,
  `4` timeout); `run --wait` streams to completion and exits with the command's status; `wait`
  on text / idle / pane-exit.
- **tmux-compat verbs** (`has-session`, `new-session`, `attach-session`, `kill-session`,
  `list-sessions`, `set-buffer`, `paste-buffer`, `send-keys`, `capture-pane`, `x`) so `tpp` is a
  drop-in for `rmux` in `sf-auto-mux` after `s/rmux/tpp/`.
- **Config** at `~/.config/tpp/config.toml`; socket-scoped recorded transcripts under
  `~/.local/state/tpp/`. `init`, `config`, `doctor`, `completions`.
- `remain-on-exit` on tpp sessions so finished commands keep their output for `cat`/`tail`.
