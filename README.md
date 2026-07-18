<div align="center">

# ➕ tpp

**tmux++ — run, watch, and paste into tmux sessions, from scripts and AI agents.**

</div>

`tpp` is a thin, fast wrapper around the real `tmux`. It doesn't run its own multiplexer — it
shells out to `tmux`, so every `tpp` session is a normal tmux session you can also list and
attach to with plain `tmux`.

## Why

It's built to automate background agent work. Each AI coding agent — or any long-running
command — runs in its own detached tmux session: you start it in a directory, paste a prompt
into it verbatim, read its output, and kill it when it's done, all from a script. That lets you
fire off many agents at once, let them work in parallel, and collect their results as they
finish.

Short commands, stable exit codes, and `--json` where it matters make it easy to drive from a
script or by hand.

## Install

Requires `tmux` 3.3+ and Rust (stable). `fzf` is optional (powers the session pickers).

```sh
cd tpp
make install        # builds release, copies to ~/bin/tpp, codesigns
make fish           # optional: fish completions
```

`make install` drops the binary at `~/bin/tpp` (override with `PREFIX=/usr/local make install`).
Then, optionally:

```sh
tpp init            # write ~/.config/tpp/config.toml
tpp doctor          # check tmux, show resolved socket / paths
```

## Usage

```sh
# By hand
tpp                           # list all tpp sessions (defaults to `ls`)
tpp new -s api -- npm run dev  # named detached session running a command
tpp attach api                 # attach (switch-client if you're already in tmux)
tpp cat api                    # print its recent output
tpp tail api                   # follow it live
tpp send -t api "rs" -e        # type "rs" + Enter into it
tpp bind mediator --pane api --role mediator
tpp paste -t pane:mediator --stdin
tpp has api --alive            # 0 only while the root pane process is running
tpp reap --dry-run             # preview stale detached sessions before cleanup
tpp rm api                     # kill it

# From a script / agent
s=$(tpp run -- pytest -q)      # start detached, capture the session name
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
| `run` (`r`) | Run a command in a new detached session; prints its name. `--wait` streams to completion and exits with the command's status. |
| `new` (`n`) | Create a detached session (your shell if no command). `--on-exit CMD` runs a shell hook once when the root command exits. `-A` = ok if it already exists. |
| `ls` (`l`, `list`) | List all tpp sessions. `--json` includes `state`, `pane_dead`, root `pid`, and `exit_status`; `-q` names-only; `--exited` includes recorded ones. |
| `attach` (`a`) | Attach, or `switch-client` if you're already inside tmux. |
| `rm` (`kill`) | Kill sessions. `--all` removes every tpp session, `--record` saves output first. |
| `reap` | Remove stale detached sessions. Dead root panes are stale immediately; live sessions require root-window activity older than `[reap] ttl` (default `6h`). `--dry-run` previews reasons; output is recorded before removal by default. |
| `exit` (`e`) | Record the current session's output, then kill it. Run it from inside the session. |
| `rename` | Rename a session. |
| `has` | Exit `0` if a session exists, else `1`. With `--alive`, exit `0` only while the root pane is running, `1` when it has exited, and `3` when missing. Exact match. |
| `cat` (`cap`) | Print session output. `-n N` trailing lines, `-S` full scrollback, `-e` keep colors, `-a` includes every recorded transcript in the picker, `--json`. Replays the saved transcript if the session has exited. |
| `tail` (`follow`) | Follow output, printing new lines as they appear. |
| `wait` | Block until `--text <s>` appears, output is `--idle`, or the pane will `--exit`. `--timeout` (exit `4`), `--json`. |
| `send` (`s`) | Send input: literal `TEXT`, `--file`/`--stdin`, or `--keys` (tmux key names). `-e`/`--enter` appends Enter; `--verify` confirms pasted-content markers disappeared after Enter. |
| `paste` | Bracketed paste + Enter, so multi-line prompts with slashes and newlines land literally. Verifies submission by default; use `--no-verify` to skip. |
| `bind` | Bind a name to a tmux pane: `tpp bind mediator --here --role mediator` or `--pane %5`. |
| `targets` | List named panes with role, pane id, `session:window.pane`, and `live`/`dead` status. Supports `--json`. |
| `unbind` | Remove a named pane binding. |

Also: `config`, `init`, `doctor`, `completions <shell>`, and hidden tmux-compat verbs
(`has-session`, `new-session`, `send-keys`, …) that forward straight to `tmux`, so existing tmux
scripts work unchanged. For `capture-pane`, `send-keys`, and `paste-buffer`, a bare session `-t`
is pinned to that session's startup pane; explicit window/pane targets keep normal tmux semantics.

## Built for agents

- **`run` prints only the session name** on stdout (hints go to stderr) → `s=$(tpp run -- cmd)`.
- **Stable exit codes:** `0` ok · `2` usage · `3` not found · `4` timeout · `5` pasted content appears unsent · `1` other. `has --alive` uses `1` for exists-but-dead.
- **`--json`** on `ls`, `cat`, `wait`, and `run --wait`.
- **Bracketed paste** delivers a prompt with `/slash` commands and newlines to a TUI exactly as
  written.
- **Pane targets** let scripts address `pane:<name>` for `send`, `paste`, `cat`, and `wait`.
  Plain session targets use the session's startup pane, even after attaches or new windows.
  If a stamped startup pane is gone, pane I/O exits `3` instead of following session focus;
  unstamped legacy sessions retain tmux's bare-session behavior.
- **Omitted session names** use the sole session, or an `fzf` picker when there are several.

### Agent lifecycle contracts

`tpp has NAME` is existence-only, including sessions kept on screen by `remain-on-exit`.
Use `tpp has NAME --alive` when a dispatcher needs process truth: it checks the session's
startup pane and exits `0` only when `pane_dead=0`.

`tpp new --on-exit 'CMD' -- <agent>` runs `CMD` once when the startup pane exits naturally or
crashes, and also when the session is torn down by `tpp exit`, `tpp rm`, or raw
`tmux kill-session`. The hook runs with `TPP_SESSION`, `TPP_SESSION_NAME`, and
`TPP_EXIT_STATUS` in the environment; `TPP_EXIT_STATUS` is empty when tmux does not know a
status, such as killing a still-running command. Hooked sessions force `remain-on-exit` on so
the root pane remains inspectable even if the default config disables it. tpp stores a private
once-marker under its state dir, so later teardown does not double-fire. Hook failures are appended to
`<state>/hooks/<socket>/on-exit.log` and do not change the tpp command's exit path. A tmux
server crash or `kill-server` cannot be covered because tmux cannot run hooks after it dies.

`tpp reap` is the conservative cleanup path for stale detached sessions. It never reaps attached
sessions, reaps dead root panes with an `exited` reason, and reaps live sessions only when the
startup pane's `window_activity` age exceeds the configured TTL. Actual removal uses the same lifecycle path as
`rm`/`exit`, so on-exit hooks still fire once and output is recorded before the session is killed
unless `[reap] record = false` or `--no-record` is passed.

For prompt delivery, the supported script pattern is:

```sh
tpp wait -t "$s" --idle --stable-for 1000 --timeout 30000
tpp paste -t "$s" -f "$PROMPT_FILE"
tpp cat "$s" | tail -40
```

`paste` verifies submission by default for Claude/Codex-style TUIs: after Enter, tpp captures the
target and looks for `[Pasted Content` or `[Pasted text` markers. If a marker remains, tpp sends a
few extra Enters with short backoff. If the marker is still visible, the command exits `5` and prints
the captured tail. `send --verify` uses the same check after `--enter`; `send --keys` skips it.
`paste --no-enter` also skips verification because it intentionally leaves text unsubmitted.

Named panes support mediator and ping flows without a registry:

```sh
tpp bind mediator --here --role mediator
echo "worker done" | tpp paste -t pane:mediator --stdin
tpp targets --json
tpp unbind mediator
```

Bindings live as tmux pane user-options (`@tpp_name`, `@tpp_role`). Names are server-wide by
convention; if duplicate pane options are created manually, `pane:<name>` resolves the first match.
A removed pane cannot be listed without external state, but panes left by `remain-on-exit` show
`dead` through tmux `pane_dead`.

## Configuration

`~/.config/tpp/config.toml` (path via `tpp config path`; override the dir with `$TPP_CONFIG_DIR`).
Recorded transcripts live under `~/.tpp/data/` (`$TPP_STATE_DIR`). All settings are optional; `tpp init`
writes an annotated starter file. Highlights:

```toml
socket = ""              # tmux -L socket; "" = your normal tmux server (set a name to isolate)
session_prefix = "tpp/"  # prefix for tpp-created sessions; "" disables prefixing

[send]
bracketed_paste = true   # multi-line text pastes verbatim

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
```

## How it works

Every call is `tmux [-L socket] -u <subcommand>`. Sessions are tagged with tmux user-options
(`@tpp`, `@tpp_dir`, `@tpp_origin_pane`, …) and read back in one `list-sessions` call, so
`ls` shows every tpp session. Named pane targets are pane user-options and are discovered with
`list-panes -a`; no state file mirrors them. High-level pane commands use the startup pane
instead of whatever pane is currently active. `remain-on-exit` keeps a finished command's last
screen so `cat`/`tail` still work; `exit` / `rm --record` snapshot it under
`~/.tpp/data/exited/<socket>/` before killing, and `reap` records by default. `--on-exit` hooks are stored under
`~/.tpp/data/hooks/<socket>/` and guarded with an atomic once-marker.

## Development

```sh
make build      # debug+release binary into ./bin
cargo test      # unit + CLI-surface tests
cargo clippy --all-targets -- -D warnings
make lint fmt
```

Licensed MIT OR Apache-2.0.
