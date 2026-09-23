# kintsu — bash hook. In ~/.bashrc:   eval "$(kintsu init bash)"
#
# bash has no preexec, so the prompt command reads the last history entry
# and its exit status. The history number tells an empty Enter apart from a
# new command, so the same failure is never reported twice. (With
# HISTCONTROL=ignoredups, running the exact same failing line twice in a row
# is reported once.)

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
  __kintsu_read_history || return $__kintsu_status
  [[ "$__kintsu_history_number" == "$__kintsu_last_history_number" ]] && return $__kintsu_status
  __kintsu_last_history_number="$__kintsu_history_number"
  (( __kintsu_status == 0 )) && return 0
  [[ "$__kintsu_command" == kintsu* ]] && return $__kintsu_status
  command kintsu triage --status "$__kintsu_status" --command "$__kintsu_command"
  return $__kintsu_status
}

__kintsu_read_history && __kintsu_last_history_number="$__kintsu_history_number"
PROMPT_COMMAND="__kintsu_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
