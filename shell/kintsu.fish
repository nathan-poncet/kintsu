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
    set -g __kintsu_ghost_file "__KINTSU_STATE_DIR__/sessions/$fish_pid.ghost"
    set -g __kintsu_seq 0
    set -g __kintsu_pending_seq -1
    set -g __kintsu_prompt_shown 0

    function __kintsu_count_prompt --on-event fish_prompt
        set -g __kintsu_seq (math $__kintsu_seq + 1)
        set -g __kintsu_prompt_shown 1
    end

    function __kintsu_preexec --on-event fish_preexec
        set -g __kintsu_prompt_shown 0
    end

    function __kintsu_postexec --on-event fish_postexec
        set -l kintsu_status $status
        command rm -f -- "$__kintsu_ghost_file" 2>/dev/null   # a fix is for the failure just before
        set -q KINTSU_DISABLE; and return $kintsu_status
        if string match -q 'kintsu*' -- "$argv[1]"
            __kintsu_take_marker
            return $kintsu_status
        end
        command kintsu triage --status $kintsu_status --command "$argv[1]" --cwd "$PWD" \
            --session "$KINTSU_SESSION" --shell fish --duration-ms "$CMD_DURATION" --signal-pid $fish_pid
        __kintsu_take_marker
        return $kintsu_status
    end

    # kintsu leaves a marker when it printed an "asking…" line: the answer may
    # replace that line if the next prompt is still the current one.
    function __kintsu_take_marker
        set -l marker "__KINTSU_STATE_DIR__/sessions/$KINTSU_SESSION.asking"
        test -e "$marker"; or return 0
        command rm -f -- "$marker"
        set -g __kintsu_pending_seq (math $__kintsu_seq + 1)
    end

    # fish redraws the prompt where it believes it is, so the message must go
    # above it: climb to the prompt's first line, clear from there, print, then
    # leave the cursor where fish expects it before asking for a repaint.
    function __kintsu_on_message --on-signal SIGUSR1
        set -l text (command kintsu pending --session "$KINTSU_SESSION" 2>&1 | string collect)
        test -n "$text"; or return
        if test "$__kintsu_prompt_shown" = 0
            # A signal handled between a command and its prompt: the cursor is
            # right under what was printed, and fish draws the prompt after us.
            if test "$__kintsu_pending_seq" = (math $__kintsu_seq + 1)
                printf '\e[1A\r\e[J'
                set -g __kintsu_pending_seq -1
            end
            printf '%s\n' "$text"
            return
        end
        set -l prompt_lines (fish_prompt 2>/dev/null | string collect | string split \n | count)
        set -l buffer_lines (commandline | count)
        test $buffer_lines -lt 1; and set buffer_lines 1
        set -l replace 0
        test "$__kintsu_pending_seq" = "$__kintsu_seq"; and set replace 1
        set -g __kintsu_pending_seq -1
        set -l down (math "$prompt_lines + $buffer_lines - 2")
        set -l up (math "$down + $replace")
        test $up -gt 0; and printf '\e[%dA' $up
        printf '\r\e[J%s\n' "$text"
        for i in (seq $down); printf '\n'; end
        commandline -f repaint
    end

    # ^K expands the last bubble into the panel, drawn on the tty under the
    # prompt; what the user takes with ⏎ comes back on stdout and lands in
    # the command line. Nothing runs until they press Enter. The screen is
    # then put back the way fish expects before a repaint (see above).
    function __kintsu_panel
        set -l prompt_lines (fish_prompt 2>/dev/null | string collect | string split \n | count)
        set -l buffer_lines (commandline | count)
        test $buffer_lines -lt 1; and set buffer_lines 1
        printf '\n'
        set -l out (command kintsu panel | string collect)
        set -l up (math "$prompt_lines + $buffer_lines - 1")
        test $up -gt 0; and printf '\e[%dA' $up
        printf '\r\e[J'
        set -l down (math "$prompt_lines + $buffer_lines - 2")
        for i in (seq $down); printf '\n'; end
        if test -n "$out"
            commandline -r -- "$out"
            commandline -f end-of-line
        end
        commandline -f repaint
    end

    # Tab on an empty line takes the fix a rule left for the last failure;
    # otherwise Tab completes as usual. fish cannot draw ghost text, so the
    # bubble says "Tab to fix".
    function __kintsu_tab
        if test -z (commandline) -a -r "$__kintsu_ghost_file"
            commandline -r -- (cat "$__kintsu_ghost_file")
            command rm -f -- "$__kintsu_ghost_file"
            commandline -f end-of-line
        else
            commandline -f complete
        end
    end

    bind \ck __kintsu_panel
    bind -M insert \ck __kintsu_panel 2>/dev/null
    bind \t __kintsu_tab
    bind -M insert \t __kintsu_tab 2>/dev/null
end
