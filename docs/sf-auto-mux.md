# Using tpp in sf-auto-mux

`sf-auto-mux.sh` dispatches a task to a grove worktree + a detached agent session, pastes the
request in **verbatim**, and returns. It drives the multiplexer with these calls:

```sh
rmux has-session  -t "$SESSION"
rmux new-session  -d -s "$SESSION" -c "$WORKTREE" "$START"
rmux set-buffer   -- "$PROMPT"
rmux paste-buffer -t "$SESSION" -p     # -p = bracketed paste (verbatim)
rmux send-keys    -t "$SESSION" Enter
rmux attach-session -t "$SESSION"      # (printed as the attach hint)
```

and, inside the spawned agent session, tears down with `rmx exit`.

`tpp` supports all of this. Pick one of the two migrations below.

## Option A — drop-in (smallest diff)

Replace the word `rmux` with `tpp`. tpp's hidden tmux-compat verbs forward straight to `tmux`,
so behavior is identical — and `new-session` additionally tags the session so it shows up in
`tpp ls`.

```diff
-  command -v rmux >/dev/null 2>&1 || die "missing dependency: rmux" 127
+  command -v tpp  >/dev/null 2>&1 || die "missing dependency: tpp" 127
...
-elif rmux has-session -t "$SESSION" 2>/dev/null; then
+elif tpp has-session -t "$SESSION" 2>/dev/null; then
...
-emit rmux new-session -d -s "$SESSION" -c "$WORKTREE" "$START"
+emit tpp  new-session -d -s "$SESSION" -c "$WORKTREE" "$START"
...
-emit rmux set-buffer -- "$PROMPT"
-emit rmux paste-buffer -t "$SESSION" -p
-emit rmux send-keys -t "$SESSION" Enter
+emit tpp  set-buffer -- "$PROMPT"
+emit tpp  paste-buffer -t "$SESSION" -p
+emit tpp  send-keys -t "$SESSION" Enter
...
-printf 'attach:   rmux attach-session -t %s\n' "$SESSION"
+printf 'attach:   tpp attach %s\n' "$SESSION"
```

And the worker-directive teardown: `rmx exit` → `tpp exit`.

## Option B — ergonomic (fewer calls)

The three-call paste dance becomes one verbatim command. Write the prompt to a file (as the
skill already does) and `tpp paste` it:

```sh
# create the detached agent session
tpp new -s "$SESSION" -c "$WORKTREE" -- "$START"

# wait for the TUI, then paste the prompt verbatim and submit (bracketed paste + Enter)
sleep "${SF_MUX_READY_DELAY:-30}"
tpp paste -t "$SESSION" -f "$PROMPT_FILE"      # one call replaces set-buffer+paste-buffer+send-keys

# hints
echo "attach:   tpp attach $SESSION"
```

Idempotence check: `tpp has "$SESSION"` (exact match — never prefix-matches a longer name).
Teardown inside the worker: `tpp exit`.

## Why bracketed paste matters here

`sf-auto-mux` pastes the user's request into the Claude/Codex TUI **word-for-word**, including
`/slash` triggers and multi-line code blocks. `tpp paste` (and `tpp send --paste`) wrap the text
in bracketed-paste markers via `tmux paste-buffer -p`, so the TUI receives it as pasted content
rather than interpreting it keystroke-by-keystroke. Content is staged through a tmux buffer from
stdin, so there's no shell-arg escaping to mangle quotes, backticks, or `$`.

## Optional: isolate agent sessions on their own socket

By default tpp shares your normal tmux server (so the sessions appear in `tmx`). To keep
dispatched agent sessions on a separate server, set a socket — in `~/.config/tpp/config.toml`:

```toml
socket = "agents"
```

or per-call with `tpp -L agents …`. `tpp attach` prints a hint that already includes the socket.
