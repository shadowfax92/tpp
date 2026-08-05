# tpp — design

`tpp` is a thin orchestration layer over a terminal multiplexer. It does not own PTYs or run a
daemon. The default backend shells out to tmux; `herdr-mode = true` instead maps the same
high-level session contract onto tabs in the default Herdr session.

## Why it exists

It replaces `rmux` in the `sf-auto-mux` dispatch flow: spin up a **detached** session
in a worktree, **paste** a prompt into the agent TUI verbatim (bracketed paste),
**capture** its output, and tear it down — for both humans and agents.

## Core capabilities

1. **List sessions globally.** `tpp ls` shows every tpp session in the selected backend
   namespace. Omitted-target commands use that same global set.
2. **Run a command.** `tpp run -- <cmd>` creates a detached session running `<cmd>` and
   prints its name (capture it: `s=$(tpp run -- npm test)`). `--wait` blocks until the
   command exits and streams/returns its output + exit status.
3. **Get the output.** `tpp cat` snapshots a session's screen/scrollback; `tpp tail`
   follows it; `tpp wait` blocks until text appears / output goes idle / the pane exits.
   Output from sessions that have already exited is replayed from a recorded log.
4. **Paste into it.** `tpp send`/`tpp paste` deliver input. Multi-line text and TUIs use
   the backend's literal/bracketed input path so prompts with slashes and newlines go
   in literally and aren't interpreted. Session targets resolve to the startup pane.
   `paste` verifies submission by default.
5. **Address panes directly.** `tpp bind` names an arbitrary backend pane. `send`, `paste`,
   `cat`, and `wait` can target `pane:<name>`.
6. **Recover blocked agent starts.** Command-bearing `new` sessions launch one detached
   watcher that clears known trust/continue prompts and escalates unknown stable screens.
   `run` opts in with `--watch`.

## Model

- **Backend boundary.** Commands depend on a semantic `Backend` interface covering lifecycle,
  discovery, process state, focus, capture, input, pane identity, parent links, watch markers,
  and named bindings. Backend-specific command syntax does not leak into high-level commands.
- **tmux backend.** Tags live on the tmux session as user-options: `@tpp=1`, `@tpp_dir`,
  `@tpp_cmd`, `@tpp_created`, `@tpp_origin_pane`. No external index needed for discovery
  or pane targeting — tmux is the source of truth. `ls` reads session metadata back with
  a single `list-sessions -F` call.
- **Herdr backend.** The default Herdr session contains one lazy workspace labeled `tpp`.
  The first tpp session owns its initial tab; later sessions create sibling tabs labeled with
  their full tpp names. A flock-protected JSON registry records workspace, tab, root-pane,
  binding, parent, watch, command, and lifecycle identities. Discovery reconciles manually
  closed tabs and panes against Herdr before returning results.
- **Family bridge.** `parent` and `children` operate on canonical raw pane ids through backend
  metadata, so the parent need not itself be a tpp session.
- **Mail uses a doorbell/mailbox split.** Full markdown messages are synchronously
  dual-written to socket-scoped sender `sent/` and recipient `inbox/` files with
  per-mailbox monotonic ids. Only one sanitized path-bearing notification line enters the
  recipient pane; notification failure never invalidates durable delivery.
- **Mailbox isolation is ergonomic, not a security boundary.** The current backend pane selects
  the caller's session mailbox or a pane-keyed fallback for ordinary human panes. `-t` is the
  explicit cross-mailbox mediator path, while `parent` can resolve to either kind.
- **Mail lifecycle follows session lifecycle.** A new generation clears ghost state,
  rename moves the live mailbox, and rm/exit/reap archive it beneath socket-scoped exited
  state for the same configured retention window.
- **Names** default to memorable `<adjective>-<animal>-<mmdd>` petnames for both `new` and
  `run`; command meaning stays in backend metadata. Random retries avoid occupied combinations
  before numeric `-N` suffixing. `name` pre-mints one or more unused names without creating
  sessions, and explicit `-s` names remain unchanged.
- **Retained exits.** A finished command leaves its terminal inspectable so `cat` and `tail`
  still work. tmux uses `remain-on-exit`; the Herdr runner atomically records the status and
  holds the pane without returning to an interactive prompt.
- **Root-pane liveness** is backend process state, not session existence. `has --alive` and
  `ls --json` distinguish a retained finished command from a running agent.
- **Reaping** is config-driven cleanup for stale detached sessions. Attached sessions are skipped.
  Dead root panes are stale immediately; live sessions are stale only when the startup pane's
  `window_activity` is older than `[reap] ttl` (default `6h`). Herdr has no equivalent activity
  timestamp, so it only reaps finished tabs. Actual removals use the shared lifecycle path.
- **Pane targets** are backend-wide names. tmux stores `@tpp_name` and `@tpp_role` on panes;
  Herdr stores bindings in its locked registry. Removed panes disappear during reconciliation.
- **Verified delivery** captures the delivery target after Enter and looks for Claude/Codex
  pasted-content markers or the pasted body's tail on a composer prompt within the last five
  non-empty lines. If either remains, tpp retries Enter with short backoff, then exits `5` with the
  captured tail if still stuck. Composer scoping excludes submitted echoes in scrollback.
- **On-exit hooks** are session-local lifecycle glue for external orchestrators. Both backends
  persist the opaque command and use an atomic marker directory for exactly-once execution.
  tmux installs pane/session hooks; the Herdr command runner fires natural exits directly and
  registry reconciliation covers manually closed tabs.
- **Watchers** are session-local detached `tpp watch run` processes. Each resolves the startup
  pane once, captures only that raw id, ANSI-strips and hashes its last 30 lines,
  and treats a hash change as activity. User rules precede built-ins; known blockers use the short
  prompt threshold, ignored idle screens remain silent, and unmatched screens use the longer stall
  threshold. Rules can send Enter or an ordered backend key sequence, notify, or ignore; they are
  config-defined with an optional built-in rule set and inspectable through `tpp watch rules`.
  Automated sends are pattern-gated and bounded; escalation is once per stable episode plus a
  session cooldown.
- **Parent escalation** uses backend parent metadata, normally resolved from the current pane at
  `new` or `run` time and overridable on `new` with `--parent-pane`. The watcher uses the internal
  bracketed-paste path against that raw pane id, neutralizes shell-active punctuation in dynamic
  fields, then sends Enter. An optional shell notifier gets captured tail text only through
  `TPP_TAIL`, not command-string substitution.
- **Watcher state** is namespaced under `~/.tpp/data/watch/<namespace>/`: one stale-checked
  pidfile per session plus an append-only `watch.log`. Backend metadata records whether a watcher
  is armed; the watcher exits when the session or origin pane is gone/dead.
- **Exited records.** `tpp exit` / `tpp rm --record` capture the final scrollback to
  `~/.tpp/data/exited/<socket>/` before killing, so `cat` can replay a dead session
  without crossing backend namespaces and `clear` purges the records. Auto-pruned after
  `[exit] prune_hours`.

## Command surface

Ergonomic (primary): `run`(r) · `new`(n) · `name` · `ls`(l,list) · `children` · `mail` ·
`reply` · `attach`(a) ·
`send`(s) ·
`paste` · `bind` · `targets` · `unbind` · `cat`(cap,capture) · `tail`(follow) · `wait` · `watch` ·
`rm`(kill,remove) · `reap` · `exit`(e,quit) · `clear`(clr) · `has` · `rename` · `config` · `init` ·
`doctor` · `completions`.

tmux-compat (hidden; for drop-in replacement of `rmux` in scripts): `has-session` ·
`new-session` · `attach-session` · `kill-session` · `list-sessions` · `set-buffer` ·
`paste-buffer` · `send-keys` · `capture-pane` · `x` (raw passthrough). These map the few
flags the scripts use onto the same internals (or forward straight to `tmux`).
They are available only with the tmux backend; high-level commands are the portable surface.

## Agent ergonomics

- `--json` on `ls`, `children`, `cat`, `wait`, `run --wait`.
- `run` and `name` print **only** session names to stdout; everything else goes to stderr.
- Stable exit codes: `0` ok · `2` usage · `3` not found · `4` timeout · `5` unsent paste ·
  `1` other; `has --alive` uses `1` for exists-but-dead.
- `-q/--quiet`, idempotent `new -A` (no-op/attach if exists), `has` is exit-code-only.
- Human-facing omitted-session commands select the sole global session automatically, or use
  external `fzf` when multiple sessions are available. `cat -a` includes every recorded
  transcript in that picker; `tail` and `rm` invoke `fzf --multi`.
  `pane:<name>` and `parent` are explicit-only and never appear in the session picker.

## Config

`~/.config/tpp/config.toml` (override dir with `$TPP_CONFIG_DIR`). State under
`~/.tpp/data/` (`$TPP_STATE_DIR`). `tpp init` writes a starter config; `tpp doctor`
checks the selected backend and prints resolved paths. `[reap] ttl` and `[watch]` durations accept `s`, `m`, `h`,
and `d` units. Watch rules use substring matching unless wrapped in `/.../` for regex; actions are
`enter`, `notify`, and `ignore`. See `tpp config path|show|edit`.

## Non-goals (v1)

No standalone PTY or global daemon (the watchdog is intentionally per-session). No lease/pool
ownership; sfmux owns that state. No general window/pane layout management. The Herdr backend
owns only one workspace and one root pane per tpp tab; `tpp` stays focused on lifecycle and I/O.
