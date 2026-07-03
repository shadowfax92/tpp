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

# wait for the TUI to settle, then paste the prompt verbatim and submit
tpp wait -t "$SESSION" --idle --stable-for "${SF_MUX_STABLE_FOR_MS:-1000}" --timeout "${SF_MUX_READY_TIMEOUT_MS:-30000}"
tpp paste -t "$SESSION" -f "$PROMPT_FILE"      # verifies Claude/Codex did not leave a paste marker
tpp cat "$SESSION" | tail -40                  # optional debug receipt for logs

# hints
echo "attach:   tpp attach $SESSION"
```

Existence check: `tpp has "$SESSION"` (exact match — never prefix-matches a longer name).
Liveness check: `tpp has "$SESSION" --alive` (`0` running, `1` exists-but-dead, `3` missing).
Lease cleanup: `tpp new --on-exit 'sfmux pool on-session-exit ...' ...` so the hook fires once
on natural exit, crash, `tpp exit`, `tpp rm`, or raw `tmux kill-session`. Teardown inside the
worker can still call `tpp exit`; the once-marker prevents a double release.
Delivery check: `tpp paste` exits `5` if `[Pasted Content` or `[Pasted text` remains visible after
retrying Enter, which means the agent TUI appears to have kept the payload in the composer.

## Why bracketed paste matters here

`sf-auto-mux` pastes the user's request into the Claude/Codex TUI **word-for-word**, including
`/slash` triggers and multi-line code blocks. `tpp paste` (and `tpp send --paste`) wrap the text
in bracketed-paste markers via `tmux paste-buffer -p`, so the TUI receives it as pasted content
rather than interpreting it keystroke-by-keystroke. Content is staged through a tmux buffer from
stdin, so there's no shell-arg escaping to mangle quotes, backticks, or `$`.

## Mediator pane pings

For navigator/worker orchestration, bind the navigator or human pane once and send pings to the
pane name:

```sh
tpp bind mediator --here --role mediator
tpp paste -t pane:mediator --stdin
tpp targets --json
```

Pane bindings live in tmux as `@tpp_name` and `@tpp_role`, not in sfmux or tpp state files.

## Lifecycle hooks

`tpp new --on-exit CMD` is intentionally opaque to tpp: sfmux should bake every lease/worktree
identifier into `CMD`. tpp also exports `TPP_SESSION`, `TPP_SESSION_NAME`, and
`TPP_EXIT_STATUS` for convenience. `TPP_EXIT_STATUS` is empty when the command was killed before
tmux knew a status.

tmux server death and `tmux kill-server` cannot run hooks because the hook runner dies with the
server. For normal pane exit/crash, `tpp exit`, `tpp rm`, and raw `tmux kill-session`, the hook
is guarded by a private once-marker under tpp state.

## Optional: isolate agent sessions on their own socket

By default tpp shares your normal tmux server (so the sessions appear in `tmx`). To keep
dispatched agent sessions on a separate server, set a socket — in `~/.config/tpp/config.toml`:

```toml
socket = "agents"
```

or per-call with `tpp -L agents …`. `tpp attach` prints a hint that already includes the socket.
