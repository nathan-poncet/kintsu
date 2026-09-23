# kintsu — zsh hook. In ~/.zshrc:   eval "$(kintsu init zsh)"
#
# preexec remembers the command line as typed; precmd reads its exit status
# and lets `kintsu triage` decide whether to say anything. Nothing runs on
# success, and an empty Enter never re-reports the previous failure.

autoload -Uz add-zsh-hook

typeset -g __kintsu_command=""

__kintsu_preexec() {
  __kintsu_command="$1"
}

__kintsu_precmd() {
  local __kintsu_status=$?
  local cmdline="$__kintsu_command"
  __kintsu_command=""
  [[ -z "$cmdline" || "$cmdline" == kintsu* ]] && return $__kintsu_status
  (( __kintsu_status == 0 )) && return 0
  command kintsu triage --status "$__kintsu_status" --command "$cmdline"
  return $__kintsu_status
}

add-zsh-hook preexec __kintsu_preexec
add-zsh-hook precmd __kintsu_precmd
