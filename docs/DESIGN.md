# tpp — design

`tpp` ("tmux++") is a thin, ergonomic wrapper around the real `tmux` binary. It is
**not** a multiplexer of its own — it shells out to `tmux`, so `tpp` sessions show up
in your normal tmux server and play nicely with `tmx`, `grove`, and friends.

## Why it exists

It replaces `rmux` in the `sf-auto-mux` dispatch flow: spin up a **detached** session
in a worktree, **paste** a prompt into the agent TUI verbatim (bracketed paste),
**capture** its output, and tear it down — for both humans and agents.

## The four core capabilities

1. **List sessions globally.** Every `tpp`-created session is tagged (via tmux
   session user-options), and `tpp ls` shows every tagged session on the selected tmux
   socket. Omitted-target commands use that same global set.
2. **Run a command.** `tpp run -- <cmd>` creates a detached session running `<cmd>` and
   prints its name (capture it: `s=$(tpp run -- npm test)`). `--wait` blocks until the
   command exits and streams/returns its output + exit status.
3. **Get the output.** `tpp cat` snapshots a session's screen/scrollback; `tpp tail`
   follows it; `tpp wait` blocks until text appears / output goes idle / the pane exits.
   Output from sessions that have already exited is replayed from a recorded log.
4. **Paste into it.** `tpp send`/`tpp paste` deliver input. Multi-line text and TUIs use
   **bracketed paste** (tmux `paste-buffer -p`) so prompts with slashes and newlines go
   in literally and aren't interpreted. Session targets resolve to the startup pane.
   `paste` verifies submission by default.
5. **Address panes directly.** `tpp bind` names an arbitrary tmux pane using pane
   user-options. `send`, `paste`, `cat`, and `wait` can target `pane:<name>`.

## Model

- **Global session set.** Every human-facing command operates on all `tpp` sessions in the
  selected tmux socket. If a target is omitted, tpp picks the sole session or invokes `fzf`.
- **Tags** live on the tmux session as user-options: `@tpp=1`, `@tpp_dir`,
  `@tpp_cmd`, `@tpp_created`, `@tpp_origin_pane`. No external index needed for discovery
  or pane targeting — tmux is the source of truth. `ls` reads session metadata back with
  a single `list-sessions -F` call.
- **remain-on-exit** is set on every `tpp` session so a finished command leaves its output
  on screen (so `cat`/`tail` still work) instead of vanishing.
- **Root-pane liveness** is the process state of `@tpp_origin_pane`, not session existence.
  `has --alive` and `ls --json` use tmux `pane_dead`, `pane_pid`, and `pane_dead_status` so
  dispatchers can distinguish a lingered dead pane from a running agent.
- **Reaping** is config-driven cleanup for stale detached sessions. Attached sessions are skipped.
  Dead root panes are stale immediately; live sessions are stale only when the startup pane's
  `window_activity` is older than `[reap] ttl` (default `6h`). Actual removals use the shared
  lifecycle path, so records and once-only hooks behave like `exit`/`rm`.
- **Pane targets** are server-wide names stored on panes as `@tpp_name` and `@tpp_role`.
  `targets` scans `list-panes -a`, so there is no registry to go stale. If duplicate names
  exist because someone edited pane options manually, v1 resolves the first scan result.
  Removed panes disappear; panes kept by `remain-on-exit` show `dead` via `pane_dead`.
- **Verified delivery** captures the delivery target after Enter and looks for Claude/Codex
  pasted-content markers (`[Pasted Content`, `[Pasted text`). If a marker remains, tpp sends
  a few extra Enters with short backoff, then exits `5` with the captured tail if still stuck.
- **On-exit hooks** are session-local lifecycle glue for external orchestrators. `new --on-exit`
  writes the opaque command to private tpp state, installs a root-pane `pane-died` hook plus a
  guarded global `session-closed` hook, and uses an atomic marker directory to make all paths
  exactly-once. Hooked sessions force `remain-on-exit` on even if the default config disables it.
- **Exited records.** `tpp exit` / `tpp rm --record` capture the final scrollback to
  `~/.tpp/data/exited/<socket>/` before killing, so `cat` can replay a dead session
  without crossing tmux sockets and `clear` purges the records. Auto-pruned after
  `[exit] prune_hours`.

## Command surface

Ergonomic (primary): `run`(r) · `new`(n) · `ls`(l,list) · `attach`(a) · `send`(s) ·
`paste` · `bind` · `targets` · `unbind` · `cat`(cap,capture) · `tail`(follow) · `wait` ·
`rm`(kill,remove) · `reap` · `exit`(e,quit) · `clear`(clr) · `has` · `rename` · `config` · `init` ·
`doctor` · `completions`.

tmux-compat (hidden; for drop-in replacement of `rmux` in scripts): `has-session` ·
`new-session` · `attach-session` · `kill-session` · `list-sessions` · `set-buffer` ·
`paste-buffer` · `send-keys` · `capture-pane` · `x` (raw passthrough). These map the few
flags the scripts use onto the same internals (or forward straight to `tmux`).

## Agent ergonomics

- `--json` on `ls`, `cat`, `wait`, `run --wait`.
- `run` prints **only** the session name to stdout; everything else goes to stderr.
- Stable exit codes: `0` ok · `2` usage · `3` not found · `4` timeout · `5` unsent paste ·
  `1` other; `has --alive` uses `1` for exists-but-dead.
- `-q/--quiet`, idempotent `new -A` (no-op/attach if exists), `has` is exit-code-only.
- Human-facing omitted-session commands select the sole global session automatically, or use
  external `fzf` when multiple sessions are available. `cat -a` includes every recorded
  transcript in that picker; `tail` and `rm` invoke `fzf --multi`.
  `pane:<name>` is explicit-only and never appears in the session picker.

## Config

`~/.config/tpp/config.toml` (override dir with `$TPP_CONFIG_DIR`). State under
`~/.tpp/data/` (`$TPP_STATE_DIR`). `tpp init` writes a starter config; `tpp doctor`
checks tmux + prints resolved paths. `[reap] ttl` accepts `s`, `m`, `h`, and `d` units; `0`
disables idle live-session reaping while still allowing dead root panes to be cleaned. See
`tpp config path|show|edit`.

## Non-goals (v1)

No standalone PTY/daemon (that's `rmux`'s job). No pane-target state files; deleted panes cannot
be reported after tmux forgets them. No lease/pool ownership; sfmux owns that state. No window/pane
layout management (that's `layouts`/`tmx`). `tpp` stays focused on session lifecycle + I/O for
agents and humans.
