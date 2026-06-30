# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_tpp_global_optspecs
	string join \n L/socket= json q/quiet config= h/help V/version
end

function __fish_tpp_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_tpp_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_tpp_using_subcommand
	set -l cmd (__fish_tpp_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c tpp -n "__fish_tpp_needs_command" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_needs_command" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_needs_command" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_needs_command" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c tpp -n "__fish_tpp_needs_command" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "run" -d 'Run a command in a new detached session (prints the session name)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "r" -d 'Run a command in a new detached session (prints the session name)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "new" -d 'Create a session (detached; runs your shell if no command is given)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "n" -d 'Create a session (detached; runs your shell if no command is given)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "ls" -d 'List all tpp sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "l" -d 'List all tpp sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "list" -d 'List all tpp sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "attach" -d 'Attach to a session (interactive)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "a" -d 'Attach to a session (interactive)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "send" -d 'Send text or keys to a session'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "s" -d 'Send text or keys to a session'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "paste" -d 'Paste text into a session verbatim (bracketed) and press Enter'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "cat" -d 'Print a session\'s output (live, or replayed if it has already exited)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "cap" -d 'Print a session\'s output (live, or replayed if it has already exited)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "capture" -d 'Print a session\'s output (live, or replayed if it has already exited)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "tail" -d 'Follow a session\'s output as it changes'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "follow" -d 'Follow a session\'s output as it changes'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "wait" -d 'Block until text appears, output goes idle, or the pane exits'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "rm" -d 'Remove (kill) sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "kill" -d 'Remove (kill) sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "remove" -d 'Remove (kill) sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "exit" -d 'Exit the current session: record its output, then kill it'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "e" -d 'Exit the current session: record its output, then kill it'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "quit" -d 'Exit the current session: record its output, then kill it'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "clear" -d 'Clear recorded exited sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "clr" -d 'Clear recorded exited sessions'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "has" -d 'Exit 0 if a session exists, non-zero otherwise (script-friendly)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "rename" -d 'Rename a session'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "config" -d 'Show, edit, or initialize configuration'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "init" -d 'Write a starter config (and optionally install fish completions)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "doctor" -d 'Check tmux availability and print resolved paths'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "completions" -d 'Generate shell completions (bash, zsh, fish, …)'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "has-session" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "new-session" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "attach-session" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "kill-session" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "list-sessions" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "set-buffer" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "paste-buffer" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "send-keys" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "capture-pane" -d 'Catch-all positional bucket for hidden tmux-compat verbs — forwarded to tmux verbatim'
complete -c tpp -n "__fish_tpp_needs_command" -f -a "x" -d 'Raw passthrough to tmux (using tpp\'s socket)'
complete -c tpp -n "__fish_tpp_using_subcommand run" -s s -l name -d 'Session name (auto-generated from the command if omitted)' -r
complete -c tpp -n "__fish_tpp_using_subcommand run" -s c -l dir -d 'Working directory for the session' -r
complete -c tpp -n "__fish_tpp_using_subcommand run" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand run" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand run" -s w -l wait -d 'Wait for the command to finish, stream its output, then exit with its status'
complete -c tpp -n "__fish_tpp_using_subcommand run" -l record -d 'With --wait: also record the output as an exited session'
complete -c tpp -n "__fish_tpp_using_subcommand run" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand run" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand run" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand run" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand r" -s s -l name -d 'Session name (auto-generated from the command if omitted)' -r
complete -c tpp -n "__fish_tpp_using_subcommand r" -s c -l dir -d 'Working directory for the session' -r
complete -c tpp -n "__fish_tpp_using_subcommand r" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand r" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand r" -s w -l wait -d 'Wait for the command to finish, stream its output, then exit with its status'
complete -c tpp -n "__fish_tpp_using_subcommand r" -l record -d 'With --wait: also record the output as an exited session'
complete -c tpp -n "__fish_tpp_using_subcommand r" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand r" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand r" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand r" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand new" -s s -l name -d 'Session name (auto-generated from the directory if omitted)' -r
complete -c tpp -n "__fish_tpp_using_subcommand new" -s c -l dir -d 'Working directory for the session' -r
complete -c tpp -n "__fish_tpp_using_subcommand new" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand new" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand new" -s A -l attach -d 'OK if it already exists (no-op, exit 0) instead of erroring'
complete -c tpp -n "__fish_tpp_using_subcommand new" -s d -l detached -d 'Accepted for tmux symmetry; `new` is always detached'
complete -c tpp -n "__fish_tpp_using_subcommand new" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand new" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand new" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand new" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand n" -s s -l name -d 'Session name (auto-generated from the directory if omitted)' -r
complete -c tpp -n "__fish_tpp_using_subcommand n" -s c -l dir -d 'Working directory for the session' -r
complete -c tpp -n "__fish_tpp_using_subcommand n" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand n" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand n" -s A -l attach -d 'OK if it already exists (no-op, exit 0) instead of erroring'
complete -c tpp -n "__fish_tpp_using_subcommand n" -s d -l detached -d 'Accepted for tmux symmetry; `new` is always detached'
complete -c tpp -n "__fish_tpp_using_subcommand n" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand n" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand n" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand n" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand ls" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand ls" -s a -l all -d 'Accepted for compatibility; `ls` already shows all tpp sessions'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -l exited -d 'Include recently exited sessions'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -l no-exited -d 'Hide recently exited sessions'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand ls" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand l" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand l" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand l" -s a -l all -d 'Accepted for compatibility; `ls` already shows all tpp sessions'
complete -c tpp -n "__fish_tpp_using_subcommand l" -l exited -d 'Include recently exited sessions'
complete -c tpp -n "__fish_tpp_using_subcommand l" -l no-exited -d 'Hide recently exited sessions'
complete -c tpp -n "__fish_tpp_using_subcommand l" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand l" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand l" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand l" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand list" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand list" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand list" -s a -l all -d 'Accepted for compatibility; `ls` already shows all tpp sessions'
complete -c tpp -n "__fish_tpp_using_subcommand list" -l exited -d 'Include recently exited sessions'
complete -c tpp -n "__fish_tpp_using_subcommand list" -l no-exited -d 'Hide recently exited sessions'
complete -c tpp -n "__fish_tpp_using_subcommand list" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand list" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand list" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand list" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand attach" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand attach" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand attach" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand attach" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand attach" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand attach" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand a" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand a" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand a" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand a" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand a" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand a" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s t -l target -d 'Target session (default: the sole session, or a picker)' -r
complete -c tpp -n "__fish_tpp_using_subcommand send" -s f -l file -d 'Read text from a file' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand send" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand send" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand send" -l stdin -d 'Read text from stdin'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s k -l keys -d 'Interpret args as tmux key names (Enter, C-c, Escape) instead of literal text'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s p -l paste -d 'Use bracketed paste (verbatim multi-line; good for TUIs)'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s e -l enter -d 'Press Enter after sending typed text'
complete -c tpp -n "__fish_tpp_using_subcommand send" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand send" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s t -l target -d 'Target session (default: the sole session, or a picker)' -r
complete -c tpp -n "__fish_tpp_using_subcommand s" -s f -l file -d 'Read text from a file' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand s" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand s" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand s" -l stdin -d 'Read text from stdin'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s k -l keys -d 'Interpret args as tmux key names (Enter, C-c, Escape) instead of literal text'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s p -l paste -d 'Use bracketed paste (verbatim multi-line; good for TUIs)'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s e -l enter -d 'Press Enter after sending typed text'
complete -c tpp -n "__fish_tpp_using_subcommand s" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand s" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand paste" -s t -l target -d 'Target session (default: the sole session, or a picker)' -r
complete -c tpp -n "__fish_tpp_using_subcommand paste" -s f -l file -d 'Read text from a file' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand paste" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand paste" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand paste" -l stdin -d 'Read text from stdin'
complete -c tpp -n "__fish_tpp_using_subcommand paste" -l no-enter -d 'Leave pasted text unsubmitted'
complete -c tpp -n "__fish_tpp_using_subcommand paste" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand paste" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand paste" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand paste" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s n -l lines -d 'Trailing lines to print (0 = visible screen only; default from config)' -r
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand cat" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s e -l escape -d 'Include escape sequences (colors)'
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s S -l all-history -d 'Print the entire scrollback'
complete -c tpp -n "__fish_tpp_using_subcommand cat" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand cat" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s n -l lines -d 'Trailing lines to print (0 = visible screen only; default from config)' -r
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand cap" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s e -l escape -d 'Include escape sequences (colors)'
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s S -l all-history -d 'Print the entire scrollback'
complete -c tpp -n "__fish_tpp_using_subcommand cap" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand cap" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s n -l lines -d 'Trailing lines to print (0 = visible screen only; default from config)' -r
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand capture" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s e -l escape -d 'Include escape sequences (colors)'
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s S -l all-history -d 'Print the entire scrollback'
complete -c tpp -n "__fish_tpp_using_subcommand capture" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand capture" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand tail" -s i -l interval -d 'Poll interval in ms (default from config)' -r
complete -c tpp -n "__fish_tpp_using_subcommand tail" -s n -l lines -d 'Print this many trailing lines before following' -r
complete -c tpp -n "__fish_tpp_using_subcommand tail" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand tail" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand tail" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand tail" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand tail" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand tail" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand follow" -s i -l interval -d 'Poll interval in ms (default from config)' -r
complete -c tpp -n "__fish_tpp_using_subcommand follow" -s n -l lines -d 'Print this many trailing lines before following' -r
complete -c tpp -n "__fish_tpp_using_subcommand follow" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand follow" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand follow" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand follow" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand follow" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand follow" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand wait" -s t -l target -d 'Target session (default: the sole session, or a picker)' -r
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l text -d 'Wait until this text appears in the pane' -r
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l stable-for -d 'Idle threshold in ms (default from config)' -r
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l timeout -d 'Timeout in ms (default from config; 0 = no timeout)' -r
complete -c tpp -n "__fish_tpp_using_subcommand wait" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l idle -d 'Wait until output is unchanged for the idle threshold'
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l exit -d 'Wait until the pane\'s command exits'
complete -c tpp -n "__fish_tpp_using_subcommand wait" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand wait" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand wait" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand wait" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand rm" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand rm" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand rm" -l all -d 'Remove every tpp session'
complete -c tpp -n "__fish_tpp_using_subcommand rm" -l record -d 'Record output before killing'
complete -c tpp -n "__fish_tpp_using_subcommand rm" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand rm" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand rm" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand rm" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand kill" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand kill" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand kill" -l all -d 'Remove every tpp session'
complete -c tpp -n "__fish_tpp_using_subcommand kill" -l record -d 'Record output before killing'
complete -c tpp -n "__fish_tpp_using_subcommand kill" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand kill" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand kill" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand kill" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand remove" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand remove" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand remove" -l all -d 'Remove every tpp session'
complete -c tpp -n "__fish_tpp_using_subcommand remove" -l record -d 'Record output before killing'
complete -c tpp -n "__fish_tpp_using_subcommand remove" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand remove" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand remove" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand remove" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand exit" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand exit" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand exit" -l no-record -d 'Don\'t record output before killing'
complete -c tpp -n "__fish_tpp_using_subcommand exit" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand exit" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand exit" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand exit" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand e" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand e" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand e" -l no-record -d 'Don\'t record output before killing'
complete -c tpp -n "__fish_tpp_using_subcommand e" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand e" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand e" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand e" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand quit" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand quit" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand quit" -l no-record -d 'Don\'t record output before killing'
complete -c tpp -n "__fish_tpp_using_subcommand quit" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand quit" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand quit" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand quit" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand clear" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand clear" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand clear" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand clear" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand clear" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand clear" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand clr" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand clr" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand clr" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand clr" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand clr" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand clr" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand has" -s t -l target -d 'Session name (tmux-style flag form)' -r
complete -c tpp -n "__fish_tpp_using_subcommand has" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand has" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand has" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand has" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand has" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand has" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand rename" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand rename" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand rename" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand rename" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand rename" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand rename" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -f -a "path" -d 'Print the config file path'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -f -a "show" -d 'Print the effective config'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -f -a "edit" -d 'Open the config in $EDITOR'
complete -c tpp -n "__fish_tpp_using_subcommand config; and not __fish_seen_subcommand_from path show edit init" -f -a "init" -d 'Write a starter config'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from path" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from path" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from path" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from path" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from path" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from path" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from show" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from show" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from show" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from show" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from show" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from edit" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from edit" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from edit" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from edit" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from edit" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from edit" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -l force -d 'Overwrite an existing config'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand config; and __fish_seen_subcommand_from init" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand init" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand init" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand init" -l force -d 'Overwrite an existing config'
complete -c tpp -n "__fish_tpp_using_subcommand init" -l fish -d 'Also install fish completions to ~/.config/fish/completions'
complete -c tpp -n "__fish_tpp_using_subcommand init" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand init" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand init" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand init" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand doctor" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand doctor" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand doctor" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand doctor" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand doctor" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand completions" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand completions" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand completions" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand completions" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand completions" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand completions" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand has-session" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand has-session" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand has-session" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand has-session" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand has-session" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand has-session" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand new-session" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand new-session" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand new-session" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand new-session" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand new-session" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand new-session" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand attach-session" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand attach-session" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand attach-session" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand attach-session" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand attach-session" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand attach-session" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand kill-session" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand kill-session" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand kill-session" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand kill-session" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand kill-session" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand kill-session" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand list-sessions" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand list-sessions" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand list-sessions" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand list-sessions" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand list-sessions" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand list-sessions" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand set-buffer" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand set-buffer" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand set-buffer" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand set-buffer" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand set-buffer" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand set-buffer" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand paste-buffer" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand paste-buffer" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand paste-buffer" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand paste-buffer" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand paste-buffer" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand paste-buffer" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand send-keys" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand send-keys" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand send-keys" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand send-keys" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand send-keys" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand send-keys" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand capture-pane" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand capture-pane" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand capture-pane" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand capture-pane" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand capture-pane" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand capture-pane" -s V -l version -d 'Print version'
complete -c tpp -n "__fish_tpp_using_subcommand x" -s L -l socket -d 'tmux socket name (`tmux -L`). Default: from config, else the shared tmux server' -r
complete -c tpp -n "__fish_tpp_using_subcommand x" -l config -d 'Config file path (default: ~/.config/tpp/config.toml)' -r -F
complete -c tpp -n "__fish_tpp_using_subcommand x" -l json -d 'Machine-readable JSON output (where supported)'
complete -c tpp -n "__fish_tpp_using_subcommand x" -s q -l quiet -d 'Suppress non-essential output (with `ls`, print only names)'
complete -c tpp -n "__fish_tpp_using_subcommand x" -s h -l help -d 'Print help'
complete -c tpp -n "__fish_tpp_using_subcommand x" -s V -l version -d 'Print version'
