# tpp — design

`tpp` ("tmux++") is a thin, ergonomic wrapper around the real `tmux` binary. It is
**not** a multiplexer of its own — it shells out to `tmux`, so `tpp` sessions show up
in your normal tmux server and play nicely with `tmx`, `grove`, and friends.

## Why it exists

It replaces `rmux` in the `sf-auto-mux` dispatch flow: spin up a **detached** session
in a worktree, **paste** a prompt into the agent TUI verbatim (bracketed paste),
**capture** its output, and tear it down — for both humans and agents.

## The four core capabilities

1. **List sessions universally.** Every `tpp`-created session is tagged (via tmux
   session user-options), and `tpp ls` shows every tagged session on the selected tmux
   socket. Scope tags still exist for commands that need a scoped default target.
2. **Run a command.** `tpp run -- <cmd>` creates a detached session running `<cmd>` and
   prints its name (capture it: `s=$(tpp run -- npm test)`). `--wait` blocks until the
   command exits and streams/returns its output + exit status.
3. **Get the output.** `tpp cat` snapshots a session's screen/scrollback; `tpp tail`
   follows it; `tpp wait` blocks until text appears / output goes idle / the pane exits.
   Output from sessions that have already exited is replayed from a recorded log.
4. **Paste into it.** `tpp send`/`tpp paste` deliver input. Multi-line text and TUIs use
   **bracketed paste** (tmux `paste-buffer -p`) so prompts with slashes and newlines go
   in literally and aren't interpreted.

## Model

- **Scope** = `git rev-parse --show-toplevel` of the cwd, else the cwd. Override with
  `--scope <dir>` / `--scope none`, or change the default in config (`[scope] mode`).
  `ls` is universal; scoped target selection remains for commands where the target is omitted.
- **Tags** live on the tmux session as user-options: `@tpp=1`, `@tpp_scope`, `@tpp_dir`,
  `@tpp_cmd`, `@tpp_created`. No external index needed for discovery — tmux is the source
  of truth. `ls` reads them back with a single `list-sessions -F` call.
- **remain-on-exit** is set on every `tpp` session so a finished command leaves its output
  on screen (so `cat`/`tail` still work) instead of vanishing.
- **Exited records.** `tpp exit` / `tpp rm --record` capture the final scrollback to
  `~/.local/state/tpp/exited/<name>.{json,log}` before killing, so `cat` can replay a dead
  session and `clear` purges the records. Auto-pruned after `[exit] prune_hours`.

## Command surface

Ergonomic (primary): `run`(r) · `new`(n) · `ls`(l,list) · `attach`(a) · `send`(s) ·
`paste` · `cat`(cap,capture) · `tail`(follow) · `wait` · `rm`(kill,remove) · `exit`(e,quit) ·
`clear`(clr) · `has` · `rename` · `config` · `init` · `doctor` · `completions`.

tmux-compat (hidden; for drop-in replacement of `rmux` in scripts): `has-session` ·
`new-session` · `attach-session` · `kill-session` · `list-sessions` · `set-buffer` ·
`paste-buffer` · `send-keys` · `capture-pane` · `x` (raw passthrough). These map the few
flags the scripts use onto the same internals (or forward straight to `tmux`).

## Agent ergonomics

- `--json` on `ls`, `cat`, `wait`, `run --wait`.
- `run` prints **only** the session name to stdout; everything else goes to stderr.
- Stable exit codes: `0` ok · `2` usage · `3` not found · `4` timeout · `1` other.
- `-q/--quiet`, idempotent `new -A` (no-op/attach if exists), `has` is exit-code-only.

## Config

`~/.config/tpp/config.toml` (override dir with `$TPP_CONFIG_DIR`). State under
`~/.local/state/tpp/` (`$TPP_STATE_DIR`). `tpp init` writes a starter config; `tpp doctor`
checks tmux + prints resolved paths/scope. See `tpp config path|show|edit`.

## Non-goals (v1)

No standalone PTY/daemon (that's `rmux`'s job). No window/pane layout management (that's
`layouts`/`tmx`). `tpp` stays focused on session lifecycle + I/O for agents and humans.
