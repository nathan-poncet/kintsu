# kintsu — zsh hook. In ~/.zshrc:   eval "$(kintsu init zsh)"
#
# preexec remembers the command line and when it started; precmd reads the
# exit status and lets `kintsu triage` decide whether to say anything. Every
# command line is reported so the last failure has its context; an empty
# Enter never re-reports the previous one. ^K expands the last bubble into
# the panel, in the bubble's place; nothing runs until you press Enter. Messages
# that arrive later (a model's answer) are printed above the prompt by a
# subscriber the hook keeps alive. KINTSU_DISABLE=1 switches the hook off.

if [[ -o interactive ]]; then
  autoload -Uz add-zsh-hook
  zmodload zsh/datetime 2>/dev/null

  export KINTSU_SESSION="$$"
  typeset -g __kintsu_command="" __kintsu_started="" __kintsu_fd="" __kintsu_last_subscribe=0
  typeset -gi __kintsu_seq=0 __kintsu_pending_seq=-1
  typeset -g __kintsu_ghost_file="__KINTSU_STATE_DIR__/sessions/$$.ghost"
  typeset -g __kintsu_bubble_file="__KINTSU_STATE_DIR__/sessions/$$.bubble"

  # An "asking…" line still waiting when another command starts: once that
  # command has printed, its row can no longer be found, so it is blanked
  # now, while the cursor is one fresh line under the prompt, and the
  # answer will come below, naming its command.
  __kintsu_preexec() {
    __kintsu_command="$1"
    __kintsu_started="${EPOCHREALTIME:-}"
    # Whatever this command prints is what sits above the next prompt;
    # `kintsu why` and `kintsu fix` leave a marker of their own.
    command rm -f -- "$__kintsu_bubble_file"
    if (( __kintsu_pending_seq == __kintsu_seq )); then
      local rendered="${(%%)PROMPT}"
      local -a prompt_rows=("${(@f)rendered}") typed_rows=("${(@f)1}")
      local prompt_lines=${#prompt_rows} typed_lines=${#typed_rows}
      (( prompt_lines < 1 )) && prompt_lines=1
      (( typed_lines < 1 )) && typed_lines=1
      local up=$(( prompt_lines + typed_lines ))
      print -n -- $'\e['"$up"$'A\e[2K\e['"$up"'B'
      __kintsu_pending_seq=-1
    fi
  }

  __kintsu_precmd() {
    local __kintsu_status=$? __kintsu_pipe="${pipestatus[*]}"
    local cmdline="$__kintsu_command" started="$__kintsu_started" duration=""
    local -a timing
    (( __kintsu_seq++ ))
    command rm -f -- "$__kintsu_ghost_file" 2>/dev/null   # a fix is for the failure just before
    __kintsu_command=""
    __kintsu_started=""
    [[ -n "${KINTSU_DISABLE:-}" ]] && return $__kintsu_status
    [[ -z "$__kintsu_fd" ]] && __kintsu_subscribe
    # An empty Enter runs no preexec, yet puts a prompt between the bubble
    # and the new one: the bubble marker no longer describes the screen.
    if [[ -z "$cmdline" ]]; then
      command rm -f -- "$__kintsu_bubble_file"
      return $__kintsu_status
    fi
    if [[ "$cmdline" == kintsu* ]]; then
      __kintsu_take_marker
      return $__kintsu_status
    fi
    if [[ -n "$started" && -n "${EPOCHREALTIME:-}" ]]; then
      duration=$(( (EPOCHREALTIME - started) * 1000 ))
      timing=(--duration-ms "${duration%.*}")
    fi
    command kintsu triage --status "$__kintsu_status" --pipestatus "$__kintsu_pipe" \
      --command "$cmdline" --cwd "$PWD" --session "$KINTSU_SESSION" --shell zsh "${timing[@]}"
    __kintsu_take_marker
    return $__kintsu_status
  }

  # kintsu leaves a marker when it printed an "asking…" line: the answer may
  # replace that line if no other prompt is drawn before it arrives.
  __kintsu_take_marker() {
    local marker="__KINTSU_STATE_DIR__/sessions/$KINTSU_SESSION.asking"
    [[ -e "$marker" ]] || return 0
    command rm -f -- "$marker"
    __kintsu_pending_seq=$__kintsu_seq
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
    local replace=0
    (( __kintsu_pending_seq == __kintsu_seq )) && replace=1
    __kintsu_pending_seq=-1
    __kintsu_rows_above
    local up=$(( REPLY + replace ))
    (( up > 0 )) && print -n -- $'\e['"$up"'A'
    print -n -- $'\r\e[J'
    print -rn -- "$text"
  }

  # Rows from the line after the edited text up to the prompt's first line.
  __kintsu_rows_above() {
    local rendered="${(%%)PROMPT}"
    local -a prompt_rows=("${(@f)rendered}") buffer_rows=("${(@f)BUFFER}")
    local prompt_lines=${#prompt_rows} buffer_lines=${#buffer_rows}
    (( prompt_lines < 1 )) && prompt_lines=1
    (( buffer_lines < 1 )) && buffer_lines=1
    REPLY=$(( prompt_lines + buffer_lines - 1 ))
  }

  # ^K expands the last bubble into the panel, in the bubble's place: kintsu
  # climbs the rows we name plus the bubble's own, draws, and on close puts
  # the bubble back and leaves the cursor where the prompt's first line
  # goes, so zsh redraws the prompt there. What the user takes with ⏎ comes
  # back on stdout and lands in the line editor. Nothing runs until Enter.
  __kintsu_panel_widget() {
    zle -I
    local out above
    __kintsu_rows_above
    above=$REPLY
    (( __kintsu_pending_seq == __kintsu_seq )) && (( above++ ))   # an "asking…" line is on screen
    __kintsu_pending_seq=-1
    out="$(command kintsu panel --above "$above")"
    if [[ -n "$out" ]]; then
      BUFFER="$out"
      CURSOR=${#BUFFER}
    fi
  }

  # Ghost text: a safe fix waits in a file; the next empty prompt shows it
  # dim after the cursor. Tab or → accepts it, anything else discards it,
  # and only Enter runs it.
  __kintsu_line_init() {
    [[ -r "$__kintsu_ghost_file" && -z "$BUFFER" ]] || return 0
    local ghost
    ghost="$(<"$__kintsu_ghost_file")"
    command rm -f -- "$__kintsu_ghost_file"
    [[ -n "$ghost" ]] || return 0
    POSTDISPLAY="$ghost"
    region_highlight+=("${#BUFFER} $(( ${#BUFFER} + ${#ghost} )) fg=8")
  }
  __kintsu_line_pre_redraw() {
    [[ -n "$POSTDISPLAY" && -n "$BUFFER" ]] || return 0
    POSTDISPLAY=""
    region_highlight=()
  }
  __kintsu_accept_ghost() {
    if [[ -n "$POSTDISPLAY" && -z "$BUFFER" ]]; then
      BUFFER="$POSTDISPLAY"
      POSTDISPLAY=""
      region_highlight=()
      CURSOR=${#BUFFER}
      return 0
    fi
    local previous="${1:-}"
    [[ -n "$previous" && "$previous" != undefined-key ]] && zle "$previous"
  }
  __kintsu_tab() { __kintsu_accept_ghost "$__kintsu_previous_tab"; }
  __kintsu_right() { __kintsu_accept_ghost "$__kintsu_previous_right"; }
  typeset -g __kintsu_previous_tab="${${(z)$(bindkey '^I')}[2]}"
  typeset -g __kintsu_previous_right="${${(z)$(bindkey '^[[C')}[2]}"
  [[ "$__kintsu_previous_tab" == __kintsu_tab ]] && __kintsu_previous_tab=expand-or-complete
  [[ "$__kintsu_previous_right" == __kintsu_right ]] && __kintsu_previous_right=forward-char
  autoload -Uz add-zle-hook-widget
  add-zle-hook-widget line-init __kintsu_line_init
  add-zle-hook-widget line-pre-redraw __kintsu_line_pre_redraw
  zle -N __kintsu_tab
  zle -N __kintsu_right
  bindkey '^I' __kintsu_tab
  bindkey '^[[C' __kintsu_right

  add-zsh-hook preexec __kintsu_preexec
  add-zsh-hook precmd __kintsu_precmd
  zle -N __kintsu_panel_widget
  bindkey '^K' __kintsu_panel_widget
fi
