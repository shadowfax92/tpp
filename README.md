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
| `new` (`n`) | Create a detached session (your shell if no command). `-A` = ok if it already exists. |
| `ls` (`l`, `list`) | List all tpp sessions. `--json`, `-q` names-only, `--exited` includes recorded ones. |
| `attach` (`a`) | Attach, or `switch-client` if you're already inside tmux. |
| `rm` (`kill`) | Kill sessions. `--all` removes every tpp session, `--record` saves output first. |
| `exit` (`e`) | Record the current session's output, then kill it. Run it from inside the session. |
| `rename` | Rename a session. |
| `has` | Exit `0` if a session exists, else `1`. Exact match. |
| `cat` (`cap`) | Print session output. `-n N` trailing lines, `-S` full scrollback, `-e` keep colors, `--json`. Replays the saved transcript if the session has exited. |
| `tail` (`follow`) | Follow output, printing new lines as they appear. |
| `wait` | Block until `--text <s>` appears, output is `--idle`, or the pane will `--exit`. `--timeout` (exit `4`), `--json`. |
| `send` (`s`) | Send input: literal `TEXT`, `--file`/`--stdin`, or `--keys` (tmux key names). `-e`/`--enter` appends Enter. |
| `paste` | Bracketed paste + Enter, so multi-line prompts with slashes and newlines land literally. |

Also: `config`, `init`, `doctor`, `completions <shell>`, and hidden tmux-compat verbs
(`has-session`, `new-session`, `send-keys`, …) that forward straight to `tmux`, so existing tmux
scripts work unchanged.

## Built for agents

- **`run` prints only the session name** on stdout (hints go to stderr) → `s=$(tpp run -- cmd)`.
- **Stable exit codes:** `0` ok · `2` usage · `3` not found · `4` timeout · `1` other.
- **`--json`** on `ls`, `cat`, `wait`, and `run --wait`.
- **Bracketed paste** delivers a prompt with `/slash` commands and newlines to a TUI exactly as
  written.
- **Omitted session names** use the sole session, or an `fzf` picker when there are several.

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
```

## How it works

Every call is `tmux [-L socket] -u <subcommand>`. Sessions are tagged with tmux user-options
(`@tpp`, `@tpp_dir`, `@tpp_origin_pane`, …) and read back in one `list-sessions` call, so
`ls` shows every tpp session. Output commands read from the startup pane instead of whatever
pane is currently active. `remain-on-exit` keeps a finished command's last screen so
`cat`/`tail` still work; `exit` / `rm --record` snapshot it under
`~/.tpp/data/exited/<socket>/` before killing.

## Development

```sh
make build      # debug+release binary into ./bin
cargo test      # unit + CLI-surface tests
cargo clippy --all-targets -- -D warnings
make lint fmt
```

Licensed MIT OR Apache-2.0.
