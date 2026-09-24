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

  __kintsu_fix() {
    local fix
    fix="$(command kintsu fix --raw 2>/dev/null)" || return 0
    READLINE_LINE="$fix"
    READLINE_POINT=${#READLINE_LINE}
  }

  __kintsu_read_history && __kintsu_last_history_number="$__kintsu_history_number"
  PROMPT_COMMAND="__kintsu_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
  bind -x '"\C-k": __kintsu_fix' 2>/dev/null
fi
