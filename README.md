<div align="center">

# ➕ tpp

**Run, watch, and paste into terminal sessions, from scripts and AI agents.**

</div>

`tpp` is a thin orchestration layer over a terminal multiplexer. tmux remains the default
backend. With `herdr-mode = true`, one lazy `tpp` workspace is created in the default Herdr
session and every tpp session becomes a tab whose label omits the configured storage prefix. The
command surface is the same on both backends.

## Why

It's built to automate background agent work. Each AI coding agent — or any long-running
command — runs in its own isolated terminal: you start it in a directory, paste a prompt
into it verbatim, read its output, and kill it when it's done, all from a script. That lets you
fire off many agents at once, let them work in parallel, and collect their results as they
finish.

Short commands, stable exit codes, and `--json` where it matters make it easy to drive from a
script or by hand.

## Install

Requires Rust (stable), plus either `tmux` 3.3+ or Herdr 0.8+. `fzf` is optional (powers the
session pickers).

```sh
cd tpp
make install        # builds release, copies to ~/bin/tpp, codesigns
make fish           # optional: fish completions
```

`make install` drops the binary at `~/bin/tpp` (override with `PREFIX=/usr/local make install`).
Then, optionally:

```sh
tpp init            # write ~/.config/tpp/config.toml
tpp doctor          # check the selected backend and resolved paths
```

## Usage

```sh
# By hand
tpp                           # list all tpp sessions (defaults to `ls`)
tpp new -s api -- npm run dev  # detached command + stuck-screen watcher
tpp attach api                 # attach or focus its tab
tpp cat api                    # print its recent output
tpp tail api                   # follow it live
tpp send -t api "rs" -e        # type "rs" + Enter into it
tpp bind mediator --pane api --role mediator
tpp paste -t pane:mediator --stdin
tpp paste -t parent "done"      # reach the pane that spawned this session
tpp children -q                 # sessions spawned from this pane
tpp has api --alive            # 0 only while the root pane process is running
tpp reap --dry-run             # preview stale detached sessions before cleanup
tpp watch ls                   # list active per-session watchers
tpp watch rules                # show effective config + built-in rule order
tpp rm api                     # kill it

# From a script / agent
s=$(tpp run -- pytest -q)      # start detached, capture the session name
tpp name -n 3                  # pre-mint three unused names without creating sessions
tpp wait -t "$s" --exit        # block until the command finishes
tpp cat "$s" --json            # read the output as JSON
tpp rm "$s"

# Run and collect in one shot
tpp run --wait -- cargo test   # streams output, exits with cargo's status
```

### Commands

Run `tpp <cmd> --help` for full flags. Aliases in parentheses.

| Command | Does |
|---|---|
| `run` (`r`) | Run a command in a new detached session; prints its name. Without `-s`, uses a dated petname such as `snazzy-otter-0730`. `--wait` streams to completion and exits with the command's status. `--watch` opts into stuck-screen detection. |
| `new` (`n`) | Create a detached session (your shell if no command). Without `-s`, uses a dated petname. Command sessions get a watcher by default; `--no-watch` disables it and `--parent-pane` overrides the escalation target. `--on-exit CMD` runs a shell hook once when the root command exits. `-A` = ok if it already exists. |
| `name` | Print a fresh dated petname without creating a session. `-n N` prints N mutually unique names, one per line. |
| `watch` | Control per-session watchers: `run -t NAME` (foreground/internal), `ls`, and `stop -t NAME`. |
| `ls` (`l`, `list`) | List all tpp sessions. Sessions with unread mail show `mail:N`; `--json` includes numeric `mail_unread` plus `state`, `pane_dead`, root `pid`, and `exit_status`; `-q` names-only; `--exited` includes recorded ones. |
| `children` | List sessions spawned from the current pane. `--pane %N` queries a pane, `-t SESSION` queries a session's startup pane, `--json` is machine-readable, and `-q` prints names only. |
| `mail` | Send durable mail with `mail TARGET` or `mail send TARGET`; `mail ls` lists inbox and sent copies, while `mail read ID` prints and marks an inbox message read. Supports `-m`, `--file`, `--stdin`, `--subject`, and `--no-ping`. |
| `reply` | Reply to an id in the caller's inbox, preserving the thread with `In-Reply-To`. |
| `attach` (`a`) | Attach or focus the selected session/tab. |
| `rm` (`kill`) | Kill sessions. `--all` removes every tpp session, `--record` saves output first. |
| `reap` | Remove stale detached sessions. Dead root panes are stale immediately; live sessions require root-window activity older than `[reap] ttl` (default `6h`). `--dry-run` previews reasons; output is recorded before removal by default. |
| `exit` (`e`) | Record the current session's output, then kill it. Run it from inside the session. |
| `rename` | Rename a session. |
| `has` | Exit `0` if a session exists, else `1`. With `--alive`, exit `0` only while the root pane is running, `1` when it has exited, and `3` when missing. Exact match. |
| `cat` (`cap`) | Print session output. `-n N` trailing lines, `-S` full scrollback, `-e` keep colors, `-a` includes every recorded transcript in the picker, `--json`. Replays the saved transcript if the session has exited. |
| `tail` (`follow`) | Follow output, printing new lines as they appear. |
| `wait` | Block until `--text <s>` appears, output is `--idle`, or the pane will `--exit`. `--timeout` (exit `4`), `--json`. |
| `send` (`s`) | Send input: literal `TEXT`, `--file`/`--stdin`, or `--keys` (backend key names). `-e`/`--enter` appends Enter; `--verify` confirms pasted-content markers disappeared after Enter. |
| `paste` | Bracketed paste + Enter, so multi-line prompts with slashes and newlines land literally. Verifies submission by default; use `--no-verify` to skip. |
| `bind` | Bind a name to a pane: `tpp bind mediator --here --role mediator` or `--pane ID`. |
| `targets` | List named panes with role, pane id, `session:window.pane`, and `live`/`dead` status. Supports `--json`. |
| `unbind` | Remove a named pane binding. |

Also: `config`, `init`, `doctor`, `completions <shell>`, and hidden tmux-compat verbs
(`has-session`, `new-session`, `send-keys`, …) that forward straight to `tmux`, so existing tmux
scripts work unchanged. For `capture-pane`, `send-keys`, and `paste-buffer`, a bare session `-t`
is pinned to that session's startup pane; explicit window/pane targets keep normal tmux semantics.
These compatibility verbs intentionally exit with a usage error in Herdr mode; high-level tpp
commands are the portable interface.

## Built for agents

- **`run` and `name` print only session names** on stdout (hints go to stderr) →
  `s=$(tpp run -- cmd)` or `s=$(tpp name)`.
- **Automatic names are memorable petnames**: `<adjective>-<animal>-<mmdd>`, with the
  configured `session_prefix` applied as usual. Explicit `-s` names are unchanged.
- **Stable exit codes:** `0` ok · `2` usage · `3` not found · `4` timeout · `5` pasted content appears unsent · `1` other. `has --alive` uses `1` for exists-but-dead.
- **`--json`** on `ls`, `children`, `cat`, `wait`, and `run --wait`.
- **Bracketed paste** delivers a prompt with `/slash` commands and newlines to a TUI exactly as
  written.
- **Pane targets** let scripts address `pane:<name>` or the reserved `parent` keyword for
  `send`, `paste`, `cat`, `tail`, and `wait`. `parent` resolves through the caller's session
  to its recorded spawning pane; a session literally named `parent` remains addressable as
  `tpp/parent`.
  Plain session targets use the session's startup pane, even after attaches or new windows.
  If a stamped startup pane is gone, pane I/O exits `3` instead of following session focus;
  Unstamped legacy tmux sessions retain tmux's bare-session behavior.
- **Omitted session targets** use the sole session, or an `fzf` picker when there are several.

### Agent lifecycle contracts

`tpp has NAME` is existence-only, including sessions kept on screen by `remain-on-exit`.
Use `tpp has NAME --alive` when a dispatcher needs process truth: it checks the session's
startup pane and exits `0` only when `pane_dead=0`.

`tpp new --on-exit 'CMD' -- <agent>` runs `CMD` once when the startup command exits and when
the session is torn down by `tpp exit` or `tpp rm`. The tmux backend also covers raw
`tmux kill-session`; the Herdr runner records natural exits directly, while a manually closed
Herdr tab is reconciled the next time tpp reads its registry. The hook receives `TPP_SESSION`,
`TPP_SESSION_NAME`, and `TPP_EXIT_STATUS`; the status is empty when a still-running command is
removed. A private atomic marker prevents double firing.

Command-bearing `tpp new` sessions also arm a detached watcher by default. Bare-shell sessions
are not watched; use `new --no-watch` or `[watch] enabled = false` to opt out, and use
`tpp run --watch -- <cmd>` to opt a `run` session in. Each watcher captures only the stored
startup pane, ANSI-strips and hashes the last 30 lines, and resets whenever the screen
changes. That screen-change rule keeps animated TUIs from being mistaken for stalls.

User rules run before built-ins, first match wins, and can `enter`, send arbitrary backend `keys`,
`notify`, or `ignore`. A stable known prompt is handled after `prompt_stable` (default `5s`).
The safety-checks menu selects "Keep waiting" with `Down` then `Enter`; other built-ins press
Enter only for `Press enter to continue`, `Enter to confirm`, `Do you trust`, and
`trust this folder`. The Claude idle marker `? for shortcuts` is ignored. Set
`builtin_rules = false` to use config rules only, and inspect the effective order with
`tpp watch rules` (`--json` and `-q` are supported). Unmatched stable output escalates after
`stuck_after` (default `30s`) without sending input. `auto_enter` gates every automated key send,
and `max_enters` caps sends per unchanged episode; a send must produce a changed capture or the
watcher escalates. Escalation fires once per unchanged episode and respects the per-session
`cooldown`.

When `new` or `run` is called inside the active backend it stores the caller's raw pane id;
`new --parent-pane PANE` overrides it. Escalation bracket-pastes
one message plus Enter into that pane and can also run `[watch] notify`. Notify commands receive `TPP_SESSION`,
`TPP_SESSION_NAME`, `TPP_REASON`, `TPP_TAIL` (last five lines), `TPP_DIR`, and
`TPP_PARENT_PANE`; only `{session}` and `{reason}` are substituted in the command string, so
captured screen text stays out of shell templates. Shell-active punctuation in dynamic parent-nudge
fields is rendered inert before paste. Watcher pidfiles and action logs live under
`<state>/watch/<namespace>/`; backend metadata marks an armed session.

`tpp reap` is the conservative cleanup path for stale detached sessions. It never reaps attached
sessions, reaps dead root panes with an `exited` reason, and reaps live sessions only when the
startup pane's `window_activity` age exceeds the configured TTL. Actual removal uses the same lifecycle path as
`rm`/`exit`, so on-exit hooks still fire once and output is recorded before the session is killed
unless `[reap] record = false` or `--no-record` is passed. Herdr currently exposes no equivalent
activity timestamp, so its reap path removes exited tabs but conservatively leaves live tabs alone.

For prompt delivery, the supported script pattern is:

```sh
tpp wait -t "$s" --idle --stable-for 1000 --timeout 30000
tpp paste -t "$s" -f "$PROMPT_FILE"
tpp cat "$s" | tail -40
```

`paste` verifies submission by default for Claude/Codex-style TUIs: after Enter, tpp captures the
target and checks for `[Pasted Content` / `[Pasted text` markers or the pasted body's tail still on a
composer prompt in the last five non-empty lines. If either remains, tpp sends a few extra Enters
with short backoff, then exits `5` with the captured tail if still stuck. Limiting literal-body
checks to the composer avoids mistaking submitted scrollback echoes for unsent input. `send
--verify` uses the same check after `--enter`; `send --keys` skips it. `paste --no-enter` also skips
verification because it intentionally leaves text unsubmitted.

Named panes and automatic parent links support mediator and ping flows without a registry:

```sh
tpp bind mediator --here --role mediator
echo "worker done" | tpp paste -t pane:mediator --stdin
tpp targets --json
tpp unbind mediator

# From a child: reach the raw pane that created this session.
tpp paste -t parent "worker done"

# From a parent: enumerate children created from this pane.
tpp children
```

Bindings live as tmux pane user-options (`@tpp_name`, `@tpp_role`) or in the locked Herdr
registry. Names are backend-wide. Removed panes disappear during discovery; retained finished
command panes show `dead`.

The parent/child bridge uses the same backend session metadata, so it needs no pane binding.

## Mail

Mail separates the durable message from its notification. The mailbox is the data plane:
the full markdown body is written synchronously to private, backend-scoped files under
`~/.tpp/data/mail/<namespace>/`. Each send gets a monotonic id in both the sender's `sent/`
and the recipient's `inbox/`, and `mail read` records a small `.read` marker. The doorbell
is only one sanitized bracketed-paste line containing the recipient id, a short excerpt,
and the absolute inbox path. If that paste fails, the send still exits `0` and warns on
stderr because the message remains discoverable with `mail ls`.

Mailboxes default from the current tmux or Herdr pane: a tpp pane uses its session mailbox and an
ordinary human pane uses a pane-keyed fallback box. `parent` follows the recorded family link;
`mail ls -t SESSION` and `mail read ID -t SESSION` are the explicit mediator escape hatch.
Outside the selected multiplexer, sending uses the `local` identity, while listing or replying without `-t`
exits `2` with a hint.

The script-friendly pattern is to put long content in a file and capture the recipient
path printed on stdout:

```sh
inbox_path=$(tpp mail "$worker" --file request.md --subject "Review request")
printf 'delivered at %s\n' "$inbox_path"

# In the recipient session:
tpp mail ls --unread
tpp mail read m1
tpp reply m1 --file response.md
```

`--no-ping` makes delivery entirely file-only. Session mailboxes move on `rename`, are
cleared when a recycled name is created, and are archived with exited state on
`rm`/`exit`/`reap`.

## Configuration

`~/.config/tpp/config.toml` (path via `tpp config path`; override the dir with `$TPP_CONFIG_DIR`).
Recorded transcripts live under `~/.tpp/data/` (`$TPP_STATE_DIR`). All settings are optional; `tpp init`
writes an annotated starter file. Highlights:

```toml
herdr-mode = false       # true = one `tpp` workspace with a named tab per session in Herdr
socket = ""              # tmux -L socket; "" = your normal tmux server (set a name to isolate)
session_prefix = "tpp/"  # prefix for tpp-created sessions; "" disables prefixing

[send]
bracketed_paste = true   # multi-line text pastes verbatim
enter_delay_ms = 0       # delay Enter after literal text or logical keys

[new]
remain_on_exit = true    # keep a finished command's output on screen for cat/tail

[exit]
record_lines = 1000      # transcript length saved on exit
prune_hours = 24         # forget transcripts after N hours

[wait]
stable_for_ms = 750      # "idle" = output unchanged this long
timeout_ms = 30000

[reap]
ttl = "6h"               # idle threshold for detached live sessions; "0" disables that
record = true            # save scrollback before killing a reaped session

[watch]
enabled = true
poll = "3s"
prompt_stable = "5s"
stuck_after = "30s"
auto_enter = true
max_enters = 2
builtin_rules = true
nudge_parent = true
notify = ""
# notify = "mac-notify send --blocker \"tpp {session}: {reason}\""
cooldown = "10m"

# [[watch.rules]]
# pattern = "Retry with a faster model"  # plain text = substring; /.../ = regex
# action = "keys"                        # enter | keys | notify | ignore
# keys = ["Down", "Enter"]               # backend key names, sent in one call

# [[watch.rules]]
# pattern = "/Sign in to continue/"
# action = "notify"
```

Herdr uses its atomic `pane run` operation for bracketed pastes. That operation submits text and
Enter together and has no delayed-Enter variant, so `enter_delay_ms` applies only to literal text
and logical-key submissions in Herdr mode.

## How it works

The command layer uses one semantic backend interface for lifecycle, process state, focus,
capture, input, pane identity, and named bindings. The tmux backend stores discovery metadata in
tmux user-options (`@tpp`, `@tpp_dir`, `@tpp_origin_pane`, …). The Herdr backend targets the
default Herdr session, lazily creates one workspace labeled `tpp`, and maps each tpp session to a
root pane in a named tab. A private, locked registry under
`~/.tpp/data/herdr/herdr%3Adefault/` records those tab/pane identities and reconciles manually
closed tabs on subsequent commands.

High-level pane commands always use the session's startup pane. Finished Herdr commands write
their exit status before holding the terminal in an inspectable state, matching tmux
`remain-on-exit`; `has --alive` still becomes false while `cat` remains available. `exit` /
`rm --record` snapshot output under
`~/.tpp/data/exited/<socket>/` before killing, and `reap` records by default. `--on-exit` hooks are stored under
`~/.tpp/data/hooks/<socket>/` and guarded with an atomic once-marker.
Per-session watcher pidfiles and `watch.log` use `~/.tpp/data/watch/<socket>/`; the detached
watcher is the current `tpp` executable running `watch run` on the same backend namespace.
Durable mailboxes use `~/.tpp/data/mail/<socket>/`; session names and pane ids are
percent-encoded as single filesystem components.

## Development

```sh
make build      # debug+release binary into ./bin
cargo test      # unit + CLI-surface tests
cargo clippy --all-targets -- -D warnings
make lint fmt
```

Licensed MIT OR Apache-2.0.
