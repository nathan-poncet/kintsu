# kintsu — zsh hook. In ~/.zshrc:   eval "$(kintsu init zsh)"
#
# preexec remembers the command line and when it started; precmd reads the
# exit status and lets `kintsu triage` decide whether to say anything. Every
# command line is reported so the last failure has its context; an empty
# Enter never re-reports the previous one. ^K inserts the fix for the last
# failure in the line editor; nothing runs until you press Enter. Messages
# that arrive later (a model's answer) are printed above the prompt by a
# subscriber the hook keeps alive. KINTSU_DISABLE=1 switches the hook off.

if [[ -o interactive ]]; then
  autoload -Uz add-zsh-hook
  zmodload zsh/datetime 2>/dev/null

  export KINTSU_SESSION="$$"
  typeset -g __kintsu_command="" __kintsu_started="" __kintsu_fd="" __kintsu_last_subscribe=0
  typeset -gi __kintsu_seq=0 __kintsu_pending_seq=-1

  __kintsu_preexec() {
    __kintsu_command="$1"
    __kintsu_started="${EPOCHREALTIME:-}"
  }

  __kintsu_precmd() {
    local __kintsu_status=$?
    local cmdline="$__kintsu_command" started="$__kintsu_started" duration=""
    local -a timing
    (( __kintsu_seq++ ))
    __kintsu_command=""
    __kintsu_started=""
    [[ -n "${KINTSU_DISABLE:-}" ]] && return $__kintsu_status
    [[ -z "$__kintsu_fd" ]] && __kintsu_subscribe
    [[ -z "$cmdline" || "$cmdline" == kintsu* ]] && return $__kintsu_status
    if [[ -n "$started" && -n "${EPOCHREALTIME:-}" ]]; then
      duration=$(( (EPOCHREALTIME - started) * 1000 ))
      timing=(--duration-ms "${duration%.*}")
    fi
    command kintsu triage --status "$__kintsu_status" --command "$cmdline" --cwd "$PWD" \
      --session "$KINTSU_SESSION" --shell zsh "${timing[@]}"
    # 3: a model is being asked and an "asking…" line was printed; the answer
    # may replace it if no other prompt is drawn before it arrives.
    (( $? == 3 )) && __kintsu_pending_seq=$__kintsu_seq
    return $__kintsu_status
  }

  # A background `kintsu subscribe` whose output zle watches: a message is
  # printed above the line being edited, which is then redrawn intact.
  __kintsu_subscribe() {
    (( EPOCHSECONDS - __kintsu_last_subscribe < 30 )) && return
    __kintsu_last_subscribe=$EPOCHSECONDS
    exec {__kintsu_fd}< <(command kintsu subscribe --session "$KINTSU_SESSION" 2>/dev/null)
    zle -F "$__kintsu_fd" __kintsu_deliver
  }

  # `zle -I` parks the cursor on the line after the edited text and lets zsh
  # redraw the prompt where the cursor is once we return. Climbing back to
  # the prompt's first line and clearing from there puts the message above
  # the prompt instead of leaving a stale copy of it behind.
  __kintsu_deliver() {
    local fd=$1 line text=""
    if ! IFS= read -r -u "$fd" line; then
      zle -F "$fd"
      exec {fd}<&-
      __kintsu_fd=""
      return
    fi
    text="$line"$'\n'
    while IFS= read -r -t 0.05 -u "$fd" line; do text+="$line"$'\n'; done
    zle -I
    local rendered="${(%%)PROMPT}"
    local -a prompt_rows=("${(@f)rendered}") buffer_rows=("${(@f)BUFFER}")
    local prompt_lines=${#prompt_rows} buffer_lines=${#buffer_rows} replace=0
    (( prompt_lines < 1 )) && prompt_lines=1
    (( buffer_lines < 1 )) && buffer_lines=1
    (( __kintsu_pending_seq == __kintsu_seq )) && replace=1
    __kintsu_pending_seq=-1
    local up=$(( prompt_lines + buffer_lines - 1 + replace ))
    (( up > 0 )) && print -n -- $'\e['"$up"'A'
    print -n -- $'\r\e[J'
    print -rn -- "$text"
  }

  __kintsu_fix_widget() {
    local fix
    fix="$(command kintsu fix --raw 2>/dev/null)" || { zle -M "kintsu: no fix for the last failure"; return 1; }
    BUFFER="$fix"
    CURSOR=${#BUFFER}
    zle redisplay
  }

  add-zsh-hook preexec __kintsu_preexec
  add-zsh-hook precmd __kintsu_precmd
  zle -N __kintsu_fix_widget
  bindkey '^K' __kintsu_fix_widget
fi
