# kintsu — bash hook. In ~/.bashrc:   eval "$(kintsu init bash)"
#
# bash has no preexec, so the prompt command reads the last history entry
# and its exit status. The history number tells an empty Enter apart from a
# new command, so nothing is reported twice. (With HISTCONTROL=ignoredups,
# the exact same line twice in a row is reported once.) ^K inserts the fix
# for the last failure in the line; nothing runs until you press Enter.
# KINTSU_DISABLE=1 switches the hook off in this shell.

if [[ $- == *i* ]]; then
  export KINTSU_SESSION="$$"
  __kintsu_last_history_number=""

  __kintsu_read_history() {
    local entry
    entry=$(HISTTIMEFORMAT='' builtin history 1) || return 1
    [[ -n "$entry" ]] || return 1
    read -r __kintsu_history_number __kintsu_command <<<"$entry"
  }

  __kintsu_prompt_command() {
    local __kintsu_status=$?
    local __kintsu_history_number __kintsu_command
    [[ -n "${KINTSU_DISABLE:-}" ]] && return $__kintsu_status
    __kintsu_read_history || return $__kintsu_status
    [[ "$__kintsu_history_number" == "$__kintsu_last_history_number" ]] && return $__kintsu_status
    __kintsu_last_history_number="$__kintsu_history_number"
    [[ "$__kintsu_command" == kintsu* ]] && return $__kintsu_status
    command kintsu triage --status "$__kintsu_status" --command "$__kintsu_command" --cwd "$PWD" \
      --session "$KINTSU_SESSION" --shell bash
    return $__kintsu_status
  }

  # ^K expands the last bubble into the panel, drawn on the tty; what the
  # user takes with ⏎ comes back on stdout and lands in the line editor.
  # bash leaves the cursor on the prompt and, afterwards, redraws only the
  # prompt's last line where the cursor is: so the panel gets a fresh line,
  # its rows are cleared, and the prompt's leading lines are printed again.
  __kintsu_panel() {
    local out rendered="" prompt_lines=1
    printf '\n'
    out="$(command kintsu panel)"
    rendered="${PS1@P}" 2>/dev/null
    local -a prompt_rows=()
    if [[ -n "$rendered" ]]; then
      mapfile -t prompt_rows <<< "$rendered"
      prompt_lines=${#prompt_rows[@]}
    fi
    local buffer_lines
    buffer_lines=$(printf '%s\n' "$READLINE_LINE" | wc -l | tr -d ' ')
    local up=$(( prompt_lines + buffer_lines - 1 ))
    (( up > 0 )) && printf '\e[%dA' "$up"
    printf '\r\e[J'
    local i
    for (( i = 0; i < prompt_lines - 1; i++ )); do
      printf '%s\n' "${prompt_rows[i]}"
    done
    if [[ -n "$out" ]]; then
      READLINE_LINE="$out"
      READLINE_POINT=${#READLINE_LINE}
    fi
  }

  __kintsu_read_history && __kintsu_last_history_number="$__kintsu_history_number"
  PROMPT_COMMAND="__kintsu_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
  bind -x '"\C-k": __kintsu_panel' 2>/dev/null
fi
