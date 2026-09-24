# kintsu — fish hook. In ~/.config/fish/config.fish:   kintsu init fish | source
#
# fish_postexec fires after every command line with its text in $argv and
# the exit status in $status; empty lines never fire it. $CMD_DURATION is
# the last command's duration in milliseconds. ^K inserts the fix for the
# last failure in the command line; nothing runs until you press Enter.
# Messages that arrive later reach the shell as SIGUSR1: the handler prints
# them and repaints the prompt. KINTSU_DISABLE=1 switches the hook off.

if status is-interactive
    set -gx KINTSU_SESSION $fish_pid

    function __kintsu_postexec --on-event fish_postexec
        set -l kintsu_status $status
        set -q KINTSU_DISABLE; and return $kintsu_status
        string match -q 'kintsu*' -- "$argv[1]"; and return $kintsu_status
        command kintsu triage --status $kintsu_status --command "$argv[1]" --cwd "$PWD" \
            --session "$KINTSU_SESSION" --shell fish --duration-ms "$CMD_DURATION" --signal-pid $fish_pid
        return $kintsu_status
    end

    function __kintsu_on_message --on-signal SIGUSR1
        command kintsu pending --session "$KINTSU_SESSION"
        commandline -f repaint
    end

    function __kintsu_fix
        set -l fix (command kintsu fix --raw 2>/dev/null); or return
        commandline -r -- "$fix"
        commandline -f end-of-line
    end

    bind \ck __kintsu_fix
    bind -M insert \ck __kintsu_fix 2>/dev/null
end
