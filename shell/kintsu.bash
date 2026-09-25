# kintsu — bash hook. In ~/.bashrc:   eval "$(kintsu init bash)"
#
# bash has no preexec, so the prompt command reads the last history entry
# and its exit status. The history number tells an empty Enter apart from a
# new command, so nothing is reported twice. (With HISTCONTROL=ignoredups,
# the exact same line twice in a row is reported once.) ^K expands the last
# bubble into the panel, in its place; nothing runs until you press Enter.
# KINTSU_DISABLE=1 switches the hook off in this shell.

if [[ $- == *i* ]]; then
  export KINTSU_SESSION="$$"
  __kintsu_last_history_number=""
  __kintsu_bubble_file="__KINTSU_STATE_DIR__/sessions/$$.bubble"
  __kintsu_asking_file="__KINTSU_STATE_DIR__/sessions/$$.asking"

  # The bubble marker says what kintsu printed right above the prompt, the
  # asking marker that an "asking…" line waits under it. Another command's
  # output, or an empty Enter, puts something else above the prompt.
  __kintsu_forget_bubble() {
    command rm -f -- "$__kintsu_bubble_file" "$__kintsu_asking_file"
  }

  __kintsu_read_history() {
    local entry
    entry=$(HISTTIMEFORMAT='' builtin history 1) || return 1
    [[ -n "$entry" ]] || return 1
    read -r __kintsu_history_number __kintsu_command <<<"$entry"
  }

  __kintsu_prompt_command() {
    local __kintsu_status=$? __kintsu_pipe="${PIPESTATUS[*]}"
    local __kintsu_history_number __kintsu_command
    [[ -n "${KINTSU_DISABLE:-}" ]] && return $__kintsu_status
    __kintsu_read_history || return $__kintsu_status
    if [[ "$__kintsu_history_number" == "$__kintsu_last_history_number" ]]; then
      __kintsu_forget_bubble   # an empty Enter: a prompt sits above the new one
      return $__kintsu_status
    fi
    __kintsu_last_history_number="$__kintsu_history_number"
    if [[ "$__kintsu_command" == kintsu* ]]; then
      # kintsu why and kintsu fix write their own bubble marker
      [[ "$__kintsu_command" == kintsu\ why* || "$__kintsu_command" == kintsu\ fix* ]] \
        || __kintsu_forget_bubble
      return $__kintsu_status
    fi
    __kintsu_forget_bubble   # this command's output is above the prompt now
    command kintsu triage --status "$__kintsu_status" --pipestatus "$__kintsu_pipe" \
      --command "$__kintsu_command" --cwd "$PWD" --session "$KINTSU_SESSION" --shell bash
    return $__kintsu_status
  }

  # ^K expands the last bubble into the panel, in the bubble's place: from a
  # fresh line, kintsu climbs the rows we name plus the bubble's own, draws,
  # and on close puts the bubble back and leaves the cursor where the
  # prompt's first line goes. bash then redraws only the prompt's last line
  # where the cursor is, so the leading lines are printed again here. What
  # the user takes with ⏎ comes back on stdout and lands in the line editor.
  __kintsu_panel() {
    local out rendered="" prompt_lines=1
    rendered="${PS1@P}" 2>/dev/null
    local -a prompt_rows=()
    if [[ -n "$rendered" ]]; then
      mapfile -t prompt_rows <<< "$rendered"
      prompt_lines=${#prompt_rows[@]}
    fi
    local buffer_lines
    buffer_lines=$(printf '%s\n' "$READLINE_LINE" | wc -l | tr -d ' ')
    local above=$(( prompt_lines + buffer_lines - 1 ))
    if [[ -e "$__kintsu_asking_file" ]]; then   # an "asking…" line is on screen under the bubble
      (( above++ ))
      command rm -f -- "$__kintsu_asking_file"
    fi
    printf '\n'
    out="$(command kintsu panel --above "$above")"
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
