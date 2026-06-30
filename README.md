<div align="center">

# ➕ tpp

**tmux++ — share, run, capture, and paste into tmux sessions.**

*An ergonomic `tmux` wrapper for humans and agents: universal session listing, one-shot command runs, verbatim paste, and output capture — all on your normal tmux server.*

</div>

`tpp` is a thin, fast wrapper around the real `tmux` binary. It doesn't run its own
multiplexer — it shells out to `tmux`, so every `tpp` session is a real tmux session that
shows up in plain `tmux`, in [`tmx`](../tmx), and alongside [`grove`](../grove) worktrees.

It exists to make tmux pleasant to drive **from scripts and AI agents** (and from your own
fingers): spin up a detached session in a directory, paste a prompt into it verbatim, read its
output, and tear it down — with short commands, stable exit codes, and `--json` everywhere it
matters. It's a drop-in replacement for `rmux` in the `sf-auto-mux` agent-dispatch flow.

- 🗂️ **Global session listing** — `tpp ls` shows every `tpp` session on the selected tmux
  socket, and omitted-target commands use the same global session set.
- ▶️ **Run a command** — `tpp run -- <cmd>` starts a detached session and prints its name.
  `--wait` runs it to completion, streaming output and exiting with its status.
- 📥 **Get the output** — `tpp cat` snapshots, `tpp tail` follows, `tpp wait` blocks until text
  appears / output goes idle / the command exits. Output survives the session: a finished
  command stays on screen, and `exit`/`rm --record` save a transcript you can replay.
- 📋 **Paste into it** — `tpp send` / `tpp paste` deliver input. Multi-line text and TUIs use
  **bracketed paste**, so prompts with slashes and newlines land literally.

---

## Install

Requires `tmux` 3.3+ and Rust (stable). `fzf` is optional (powers omitted-session pickers).

```sh
cd tpp
make install        # builds release, copies to ~/bin/tpp, codesigns
make fish           # optional: install fish completions
```

`make install` drops the binary at `~/bin/tpp` (override with `PREFIX=/usr/local make install`).
Then, optionally:

```sh
tpp init            # write ~/.config/tpp/config.toml
tpp doctor          # check tmux, show resolved socket / paths
```

## Quickstart

```sh
# Humans
tpp                          # list all tpp sessions (defaults to `ls`)
tpp new -s api -- npm run dev # named detached session running a command
tpp attach api               # attach (switch-client if you're already in tmux)
tpp cat api                  # print its recent output
tpp tail api                 # follow it live
tpp send -t api --enter -- "rs"   # type "rs" + Enter into it
tpp rm api                   # kill it

# Agents
s=$(tpp run -- pytest -q)    # capture the session name (handle)
tpp wait -t "$s" --exit      # block until the command finishes
tpp cat "$s" --json          # read the output as JSON
tpp rm "$s"

# Run-and-collect in one shot (like running the command, but in a session)
tpp run --wait -- cargo test # streams output, exits with cargo's status
```

## Commands

Run `tpp <cmd> --help` for full flags. Aliases in parentheses.

**Sessions**
| Command | Does |
|---|---|
| `run` (`r`) | Run a command in a new detached session; prints its name. `--wait` streams to completion and exits with the command's status. |
| `new` (`n`) | Create a detached session (your shell if no command). `-A` = ok if it already exists. |
| `ls` (`l`, `list`) | List all tpp sessions. `-a` is accepted for compatibility, `--exited` include recorded, `--json`, `-q` names-only. |
| `attach` (`a`) | Attach (or `switch-client` if you're inside tmux). No arg → sole session, or an `fzf` picker. |
| `rm` (`kill`, `remove`) | Kill sessions. No args → sole session, or an `fzf --multi` picker. `--all` removes every tpp session, `--record` saves output first. |
| `exit` (`e`, `quit`) | Record the current session's output, then kill it. Run it from inside the session. |
| `rename` | Rename a session. `rename NEW` picks the old session; `rename OLD NEW` is explicit. |
| `has` | Exit `0` if a session exists, else `1`. Exact match — never prefix-matches. |
| `clear` (`clr`) | Delete recorded exited-session transcripts. |

**Output**
| Command | Does |
|---|---|
| `cat` (`cap`, `capture`) | Print session output. No args → sole session, or an `fzf --multi` picker. `-n N` trailing lines, `-S` full scrollback, `-e` keep colors, `--json`. Replays from the saved transcript if the session has exited. |
| `tail` (`follow`) | Follow output, printing new lines as they appear. No args → sole session, or an `fzf --multi` picker. Multiple sessions get `[name]` prefixes. |
| `wait` | Block until `--text <s>` appears, output is `--idle`, or the pane will `--exit`. `--timeout` (exit `4`), `--json`. |

**Input**
| Command | Does |
|---|---|
| `send` (`s`) | Send to a session: literal `TEXT`, `--file`/`--stdin`, or `--keys` (tmux key names like `Enter`, `C-c`). No `-t` → sole session, or an `fzf` picker. `--paste` forces bracketed paste; `--enter` appends Enter. |
| `paste` | Bracketed paste + Enter (sugar over `send --paste --enter`). No `-t` → sole session, or an `fzf` picker. `--no-enter` to skip submit. |

**Meta**: `config` (`path`/`show`/`edit`/`init`), `init`, `doctor`, `completions <shell>`.

**tmux-compat** (hidden): `has-session`, `new-session`, `attach-session`, `kill-session`,
`list-sessions`, `set-buffer`, `paste-buffer`, `send-keys`, `capture-pane`, and `x` (raw
passthrough). These forward to `tmux`, so a script written for `rmux` works after replacing the
word `rmux` with `tpp` — see [Replacing rmux](#replacing-rmux-in-sf-auto-mux).

## Global sessions

Every command operates on all `tpp` sessions in the selected tmux socket. If a command needs a
session and you omit the name, tpp uses the sole session when there is one; with multiple
sessions it opens the `fzf` picker when available, then falls back to printing the candidate
names. There is no directory grouping.

## Configuration

`~/.config/tpp/config.toml` (path via `tpp config path`; override dir with `$TPP_CONFIG_DIR`).
State (recorded transcripts) lives under `~/.local/state/tpp/` (`$TPP_STATE_DIR`). All settings
are optional; `tpp init` writes the annotated starter file. Highlights:

```toml
socket = ""              # tmux -L socket; "" = your normal tmux server (set a name to isolate)
session_prefix = "tpp/"  # prefix for tpp-created tmux sessions; "" disables prefixing

[ls]
show_exited_hours = 24   # also surface recently-exited sessions in `ls`

[send]
bracketed_paste = true   # multi-line text pastes verbatim
enter_delay_ms = 0       # pause after a paste before pressing Enter

[new]
remain_on_exit = true    # keep a finished command's output on screen for cat/tail
history_limit = 100000

[capture]
lines = 200              # default trailing lines for `cat`

[tail]
interval_ms = 1000

[exit]
record_lines = 2000      # transcript length saved on exit
prune_hours = 24         # forget transcripts after N hours

[wait]
stable_for_ms = 750      # "idle" = output unchanged this long
timeout_ms = 30000
```

## Agent ergonomics

- **`run` prints only the session name** on stdout (hints go to stderr) → `s=$(tpp run -- cmd)`.
- **`--json`** on `ls`, `cat`, `wait`, and `run --wait`-adjacent flows.
- **Stable exit codes:** `0` ok · `2` usage (clap) · `3` not found · `4` timeout · `1` other.
- **`has`** is exit-code-only; **`-q`** trims chatter; **`new -A`** is idempotent.
- **Omitted session names** use the sole global session, or `fzf` when multiple sessions are
  available. `tail` and `rm` use `fzf --multi`.
- **Bracketed paste** means a pasted prompt with `/slash` commands and newlines reaches a TUI
  exactly as written.

## Replacing rmux in sf-auto-mux

`tpp` covers everything `sf-auto-mux.sh` needs. Two ways to switch:

**1. Drop-in** — replace the word `rmux` with `tpp`. The compat verbs forward to tmux verbatim:

```sh
tpp has-session  -t "$SESSION"
tpp new-session  -d -s "$SESSION" -c "$WORKTREE" "$START"   # auto-tagged as a tpp session
tpp set-buffer   -- "$PROMPT"
tpp paste-buffer -t "$SESSION" -p
tpp send-keys    -t "$SESSION" Enter
tpp attach-session -t "$SESSION"
tpp kill-session -t "$SESSION"
```

**2. Ergonomic** — collapse the paste dance into one verbatim command:

| rmux | tpp (ergonomic) |
|---|---|
| `rmux has-session -t S` | `tpp has S` |
| `rmux new-session -d -s S -c D CMD` | `tpp new -s S -c D -- CMD` |
| `set-buffer …` + `paste-buffer -t S -p` + `send-keys -t S Enter` | `tpp paste -t S -f promptfile` |
| `rmux attach-session -t S` | `tpp attach S` |
| `rmux kill-session -t S` | `tpp rm S` |
| `rmx exit` (inside the agent session) | `tpp exit` |

See [`docs/sf-auto-mux.md`](docs/sf-auto-mux.md) for an annotated before/after.

## How it works

- **Wraps tmux.** Every call is `tmux [-L socket] -u <subcommand>`; targets that must be exact
  (existence checks) use tmux's `=name` form, the rest use the plain name (which exact-matches
  an existing session).
- **Tags as user-options.** `@tpp`, `@tpp_dir`, `@tpp_cmd`, `@tpp_created` live on
  the session and are read back in one `list-sessions -F` call. `ls` shows every `@tpp` session.
- **remain-on-exit** keeps a finished command's last screen so `cat`/`tail` still work; `exit`
  / `rm --record` snapshot it under `~/.local/state/tpp/exited/<socket>/` before killing.

## Related tools

- [`rmux`](https://github.com/shadowfax92) — a full standalone multiplexer (own daemon/PTYs).
  `tpp` is the opposite bet: lean wrapper over the tmux you already run.
- [`tmx`](../tmx) — getting around tmux (session tree, jump, scratch popups).
- [`grove`](../grove) — tmux workspaces from git worktrees.

## Development

```sh
make build      # debug+release binary into ./bin
cargo test      # unit + CLI-surface tests
cargo clippy --all-targets -- -D warnings
make lint fmt
```

Licensed MIT OR Apache-2.0.
