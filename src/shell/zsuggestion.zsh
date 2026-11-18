# zsuggestion shell integration
if [[ -o interactive ]] && (( $+commands[zsuggestion] )); then
  typeset -g ZSUGGESTION_ZSH_LOADED=1
  typeset -g _ZSUGGESTION_COMMAND=""
  typeset -g _ZSUGGESTION_COMMAND_CWD=""
  typeset -g _ZSUGGESTION_MENU_ACTIVE=0
  typeset -g _ZSUGGESTION_MENU_INDEX=1
  typeset -g _ZSUGGESTION_MENU_START=1
  typeset -g _ZSUGGESTION_MENU_BUFFER=""
  typeset -g _ZSUGGESTION_MENU_OWNS_DISPLAY=0
  typeset -g _ZSUGGESTION_MENU_REQUEST_BUFFER=""
  typeset -g _ZSUGGESTION_MENU_REQUEST_CWD=""
  typeset -g _ZSUGGESTION_MENU_REQUEST_CURSOR=0
  typeset -g _ZSUGGESTION_MENU_REQUEST_FD=-1
  typeset -g _ZSUGGESTION_MENU_INFLIGHT_BUFFER=""
  typeset -g _ZSUGGESTION_MENU_INFLIGHT_CURSOR=0
  typeset -g _ZSUGGESTION_MENU_REQUEST_DIRTY=0
  typeset -g _ZSUGGESTION_MENU_REFRESH_TICKS=0
  typeset -g _ZSUGGESTION_MENU_GENERATION=0
  typeset -g _ZSUGGESTION_MENU_INFLIGHT_GENERATION=0
  typeset -g _ZSUGGESTION_MENU_KEYTIMEOUT=-1
  typeset -g _ZSUGGESTION_MENU_RESTORE_INDEX=1
  typeset -g _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
  typeset -g _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=0
  typeset -g _ZSUGGESTION_MENU_TICK_FD=-1
  typeset -g _ZSUGGESTION_HAS_ZSELECT=0
  typeset -g _ZSUGGESTION_NATIVE_REQUEST_FD=-1
  typeset -g _ZSUGGESTION_NATIVE_REQUEST_PID=-1
  typeset -g _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
  typeset -g _ZSUGGESTION_NATIVE_START_TICKS=0
  typeset -g _ZSUGGESTION_NATIVE_INFLIGHT_GENERATION=0
  typeset -g _ZSUGGESTION_NATIVE_REQUESTED=0
  typeset -g _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER=""
  typeset -g _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=0
  typeset -g _ZSUGGESTION_IN_NATIVE_COMPLETION=0
  typeset -g _ZSUGGESTION_IN_BRACKETED_PASTE=0
  typeset -g _ZSUGGESTION_FUZZY_ACTIVE=0
  typeset -g _ZSUGGESTION_FUZZY_BASE=""
  typeset -g _ZSUGGESTION_FUZZY_QUERY=""
  typeset -g _ZSUGGESTION_FUZZY_KEYTIMEOUT=-1
  typeset -g _ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP=""
  typeset -g _ZSUGGESTION_PREVIEW_FD=-1
  typeset -g _ZSUGGESTION_PREVIEW_PID=-1
  typeset -g _ZSUGGESTION_PREVIEW_TICKS=0
  typeset -g _ZSUGGESTION_PREVIEW_TARGET=""
  typeset -g _ZSUGGESTION_PREVIEW_PATH=""
  typeset -g _ZSUGGESTION_PREVIEW_COMMAND=""
  typeset -ga _ZSUGGESTION_PREVIEW_LINES=()
  typeset -ga _ZSUGGESTION_PREVIEW_STYLES=()
  typeset -g _ZSUGGESTION_UTF8_UI=0
  typeset -g _ZSUGGESTION_CHARMAP="${(U)$(command locale charmap 2>/dev/null)}"
  [[ "$_ZSUGGESTION_CHARMAP" == *UTF-8* || "$_ZSUGGESTION_CHARMAP" == *UTF8* ]] && _ZSUGGESTION_UTF8_UI=1
  unset _ZSUGGESTION_CHARMAP
  typeset -ga _ZSUGGESTION_MENU_ACCEPTS=()
  typeset -ga _ZSUGGESTION_MENU_DISPLAYS=()
  typeset -ga _ZSUGGESTION_MENU_DESCRIPTIONS=()
  typeset -ga _ZSUGGESTION_MENU_KINDS=()
  typeset -ga _ZSUGGESTION_MENU_SOURCES=()
  typeset -ga _ZSUGGESTION_FOREIGN_HIGHLIGHTS=()
  typeset -ga _ZSUGGESTION_RENDERED_HIGHLIGHTS=()
  typeset -ga _ZSUGGESTION_DAEMON_ACCEPTS=()
  typeset -ga _ZSUGGESTION_DAEMON_DISPLAYS=()
  typeset -ga _ZSUGGESTION_DAEMON_DESCRIPTIONS=()
  typeset -ga _ZSUGGESTION_DAEMON_KINDS=()
  typeset -ga _ZSUGGESTION_DAEMON_SOURCES=()
  typeset -ga _ZSUGGESTION_NATIVE_ACCEPTS=()
  typeset -ga _ZSUGGESTION_NATIVE_DISPLAYS=()
  typeset -ga _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
  typeset -ga _ZSUGGESTION_NATIVE_KINDS=()
  typeset -g _ZSUGGESTION_PROMPT_COLUMN=-1
  typeset -g _ZSUGGESTION_PROMPT_QUERY=1
  typeset -g _ZSUGGESTION_PROMPT_ANSWERED=0
  typeset -g _ZSUGGESTION_PROMPT_LATE=0
  typeset -g _ZSUGGESTION_PROMPT_TIMEOUT=""
  typeset -g _ZSUGGESTION_ANCHOR_HEAD=""

  _zsuggestion_preexec() {
    _ZSUGGESTION_COMMAND="$1"
    _ZSUGGESTION_COMMAND_CWD="$PWD"
    return 0
  }

  _zsuggestion_precmd() {
    local exit_code=$?
    if [[ -n "$_ZSUGGESTION_COMMAND" ]]; then
      local session_id="${HOST:-unknown}:${TMUX_PANE:-${SSH_TTY:-local}}:$$"
      print -rn -- "$_ZSUGGESTION_COMMAND" | command zsuggestion record \
        --stdin \
        --cwd "$_ZSUGGESTION_COMMAND_CWD" \
        --exit-code "$exit_code" \
        --session "$session_id" >/dev/null 2>&1 &!
      _ZSUGGESTION_COMMAND=""
      _ZSUGGESTION_COMMAND_CWD=""
    fi
    return 0
  }

  # Native flag, subcommand, and value candidates come from the Zsh completion
  # system, as with fzf-tab. Load it at the first prompt when .zshrc has not,
  # so a compinit call later in .zshrc still wins.
  _zsuggestion_ensure_compsys() {
    add-zsh-hook -d precmd _zsuggestion_ensure_compsys
    (( __ZSUGGESTION_AUTO_COMPINIT__ )) || return 0
    (( $+functions[_main_complete] || $+functions[compdef] )) && return 0
    autoload -Uz compinit &&
      compinit -i -d "${ZSH_COMPDUMP:-${ZDOTDIR:-$HOME}/.zcompdump}" 2>/dev/null
    return 0
  }

  _zsuggestion_menu_cancel_query() {
    local fd="$_ZSUGGESTION_MENU_REQUEST_FD"
    (( _ZSUGGESTION_MENU_GENERATION++ ))
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
    fi
    _ZSUGGESTION_MENU_REQUEST_FD=-1
    _ZSUGGESTION_MENU_INFLIGHT_BUFFER=""
    _ZSUGGESTION_MENU_INFLIGHT_CURSOR=0
  }

  _zsuggestion_native_cancel() {
    local fd="$_ZSUGGESTION_NATIVE_REQUEST_FD"
    (( _ZSUGGESTION_MENU_GENERATION++ ))
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
    fi
    if (( _ZSUGGESTION_NATIVE_REQUEST_PID > 0 )); then
      kill "$_ZSUGGESTION_NATIVE_REQUEST_PID" 2>/dev/null
    fi
    _ZSUGGESTION_NATIVE_REQUEST_FD=-1
    _ZSUGGESTION_NATIVE_REQUEST_PID=-1
    _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
    _ZSUGGESTION_NATIVE_START_TICKS=0
    _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER=""
    _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=0
  }

  _zsuggestion_preview_cancel() {
    local fd="$_ZSUGGESTION_PREVIEW_FD"
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
    fi
    (( _ZSUGGESTION_PREVIEW_PID > 0 )) && kill "$_ZSUGGESTION_PREVIEW_PID" 2>/dev/null
    _ZSUGGESTION_PREVIEW_FD=-1
    _ZSUGGESTION_PREVIEW_PID=-1
    _ZSUGGESTION_PREVIEW_TICKS=0
  }

  _zsuggestion_preview_clear() {
    _zsuggestion_preview_cancel
    _ZSUGGESTION_PREVIEW_TARGET=""
    _ZSUGGESTION_PREVIEW_PATH=""
    _ZSUGGESTION_PREVIEW_COMMAND=""
    _ZSUGGESTION_PREVIEW_LINES=()
    _ZSUGGESTION_PREVIEW_STYLES=()
  }

  _zsuggestion_preview_consider() {
    local display source target path command_name alias_command
    local -a display_words
    if (( ${COLUMNS:-80} < 100 || ! _ZSUGGESTION_MENU_ACTIVE )); then
      _zsuggestion_preview_clear
      return
    fi
    display="${_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}"
    source="${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]}"
    target="${source}:${display}"
    [[ "$target" == "$_ZSUGGESTION_PREVIEW_TARGET" ]] && return
    _zsuggestion_preview_clear
    _ZSUGGESTION_PREVIEW_TARGET="$target"
    command_name="${${display%%[[:space:]]*}:t}"
    alias_command="${aliases[ls]:-}"
    if [[ "$command_name" == ls &&
          ( "$alias_command" == eza || "$alias_command" == "eza "* ) ]]; then
      _ZSUGGESTION_PREVIEW_COMMAND="${alias_command}${display#$command_name}"
    elif [[ "$command_name" == ls || "$command_name" == gls || "$command_name" == eza ]]; then
      _ZSUGGESTION_PREVIEW_COMMAND="$display"
    elif [[ "$source" == native || "$source" == filesystem ]]; then
      display_words=("${(z)display}")
      path="${(Q)display_words[-1]}"
      [[ -n "$path" ]] || return
      [[ "$path" == "~/"* ]] && path="$HOME/${path#\~/}"
      _ZSUGGESTION_PREVIEW_PATH="$path"
      _zsuggestion_menu_start_ticker
    fi
  }

  _zsuggestion_preview_ready() {
    local fd="$1" line style response_target
    local styled=$(( ${#_ZSUGGESTION_PREVIEW_COMMAND} > 0 ))
    local -a lines styles
    # Stale fd: whoever replaced it already unregistered and closed it. zle can
    # still dispatch it in the same poll round, and the number may be reused.
    (( fd == _ZSUGGESTION_PREVIEW_FD )) || return 0
    zle -F "$fd"
    if ! IFS= read -r -u "$fd" -d '' response_target ||
       [[ "$response_target" != "$_ZSUGGESTION_PREVIEW_TARGET" ]]; then
      exec {fd}<&-
      _ZSUGGESTION_PREVIEW_FD=-1
      _ZSUGGESTION_PREVIEW_PID=-1
      _ZSUGGESTION_PREVIEW_TICKS=0
      return 0
    fi
    if (( styled )); then
      while IFS= read -r -u "$fd" -d '' line &&
            IFS= read -r -u "$fd" -d '' style; do
        lines+=("$line")
        styles+=("$style")
        (( ${#lines} >= 8 )) && break
      done
    else
      while IFS= read -r -u "$fd" -d '' line; do
        lines+=("$line")
        styles+=("")
        (( ${#lines} >= 8 )) && break
      done
    fi
    exec {fd}<&-
    _ZSUGGESTION_PREVIEW_FD=-1
    _ZSUGGESTION_PREVIEW_PID=-1
    _ZSUGGESTION_PREVIEW_TICKS=0
    _ZSUGGESTION_PREVIEW_PATH=""
    _ZSUGGESTION_PREVIEW_COMMAND=""
    if (( ${#lines} )); then
      _ZSUGGESTION_PREVIEW_LINES=("${lines[@]}")
      _ZSUGGESTION_PREVIEW_STYLES=("${styles[@]}")
    else
      _ZSUGGESTION_PREVIEW_LINES=("Preview unavailable")
      _ZSUGGESTION_PREVIEW_STYLES=("")
    fi
    _zsuggestion_menu_publish
  }

  _zsuggestion_menu_cancel_request() {
    _zsuggestion_menu_cancel_query
    _zsuggestion_native_cancel
    _ZSUGGESTION_MENU_REQUEST_BUFFER=""
    _ZSUGGESTION_MENU_REQUEST_CWD=""
    _ZSUGGESTION_MENU_REQUEST_CURSOR=0
    _ZSUGGESTION_MENU_REQUEST_DIRTY=0
    _ZSUGGESTION_MENU_REFRESH_TICKS=0
    _ZSUGGESTION_MENU_RESTORE_INDEX=1
    _ZSUGGESTION_NATIVE_REQUESTED=0
    _ZSUGGESTION_NATIVE_START_TICKS=0
  }

  _zsuggestion_fuzzy_reset() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE && _ZSUGGESTION_FUZZY_KEYTIMEOUT >= 0 )); then
      KEYTIMEOUT=$_ZSUGGESTION_FUZZY_KEYTIMEOUT
    fi
    if (( _ZSUGGESTION_FUZZY_ACTIVE )) && [[ -n "$_ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP" ]]; then
      zle -K "$_ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP" 2>/dev/null
    fi
    _ZSUGGESTION_FUZZY_ACTIVE=0
    _ZSUGGESTION_FUZZY_BASE=""
    _ZSUGGESTION_FUZZY_QUERY=""
    _ZSUGGESTION_FUZZY_KEYTIMEOUT=-1
    _ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP=""
  }

  _zsuggestion_menu_clear() {
    local preserve_buffer="${1:-0}"
    if (( _ZSUGGESTION_MENU_KEYTIMEOUT >= 0 )); then
      KEYTIMEOUT=$_ZSUGGESTION_MENU_KEYTIMEOUT
      _ZSUGGESTION_MENU_KEYTIMEOUT=-1
    fi
    _zsuggestion_menu_cancel_request
    _ZSUGGESTION_MENU_ACTIVE=0
    _ZSUGGESTION_MENU_INDEX=1
    _ZSUGGESTION_MENU_START=1
    _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
    _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=0
    _zsuggestion_fuzzy_reset
    _zsuggestion_preview_clear
    (( preserve_buffer )) || _ZSUGGESTION_MENU_BUFFER=""
    _ZSUGGESTION_MENU_ACCEPTS=()
    _ZSUGGESTION_MENU_DISPLAYS=()
    _ZSUGGESTION_MENU_DESCRIPTIONS=()
    _ZSUGGESTION_MENU_KINDS=()
    _ZSUGGESTION_MENU_SOURCES=()
    _ZSUGGESTION_DAEMON_ACCEPTS=()
    _ZSUGGESTION_DAEMON_DISPLAYS=()
    _ZSUGGESTION_DAEMON_DESCRIPTIONS=()
    _ZSUGGESTION_DAEMON_KINDS=()
    _ZSUGGESTION_DAEMON_SOURCES=()
    _ZSUGGESTION_NATIVE_ACCEPTS=()
    _ZSUGGESTION_NATIVE_DISPLAYS=()
    _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
    _ZSUGGESTION_NATIVE_KINDS=()
    if (( _ZSUGGESTION_MENU_OWNS_DISPLAY )); then
      POSTDISPLAY=""
      region_highlight=( "${(@)region_highlight:#*memo=zsuggestion*}" )
      _ZSUGGESTION_MENU_OWNS_DISPLAY=0
    fi
  }

  _zsuggestion_fuzzy_start() {
    (( CURSOR == ${#BUFFER} )) || return 1
    _ZSUGGESTION_FUZZY_KEYTIMEOUT=$KEYTIMEOUT
    _ZSUGGESTION_FUZZY_PREVIOUS_KEYMAP="$KEYMAP"
    KEYTIMEOUT=1
    _ZSUGGESTION_FUZZY_ACTIVE=1
    zle -K zsuggestion-fuzzy
    _ZSUGGESTION_FUZZY_BASE="$BUFFER"
    _ZSUGGESTION_FUZZY_QUERY=""
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_schedule
    _zsuggestion_menu_render
  }

  _zsuggestion_fuzzy_refresh() {
    BUFFER="${_ZSUGGESTION_FUZZY_BASE}${_ZSUGGESTION_FUZZY_QUERY}"
    CURSOR=${#BUFFER}
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_schedule
    _zsuggestion_menu_render
  }

  # Measure where the prompt ends by asking the terminal for the cursor
  # position (DSR), so the popup lines up with the edited word under any prompt
  # theme. Terminals that never answer fall back to ui.prompt_offset.
  _zsuggestion_measure_prompt() {
    (( __ZSUGGESTION_UI_MEASURE_PROMPT__ && _ZSUGGESTION_PROMPT_QUERY )) || return 0
    [[ -z "$BUFFER" && "${TERM:-dumb}" != dumb && -w /dev/tty ]] || return 0
    local reply="" char col
    if [[ -z "$_ZSUGGESTION_PROMPT_TIMEOUT" ]]; then
      _ZSUGGESTION_PROMPT_TIMEOUT=0.2
      [[ -n "${SSH_CONNECTION:-}" ]] && _ZSUGGESTION_PROMPT_TIMEOUT=0.5
    fi
    zle -R
    print -rn -- $'\e[6n' 2>/dev/null >/dev/tty || return 0
    while read -rs -k 1 -t "$_ZSUGGESTION_PROMPT_TIMEOUT" char; do
      reply+="$char"
      [[ "$char" == R && "$reply" == *$'\e['<->';'<->R ]] && break
    done
    if [[ "$reply" == *$'\e['<->';'<->R ]]; then
      col="${${reply##*;}%R}"
      _ZSUGGESTION_PROMPT_COLUMN=$(( col - 1 ))
      _ZSUGGESTION_PROMPT_ANSWERED=1
      reply="${reply%$'\e['*}"
    else
      # Never reuse a column measured under an earlier prompt. A reply that
      # arrives after the deadline reaches ZLE as typed text; the pre-redraw
      # hook strips it. Terminals that have never answered are not asked again.
      _ZSUGGESTION_PROMPT_COLUMN=-1
      _ZSUGGESTION_PROMPT_LATE=1
      (( _ZSUGGESTION_PROMPT_ANSWERED )) || _ZSUGGESTION_PROMPT_QUERY=0
    fi
    # Keys typed while waiting for the answer go back to the line editor.
    [[ -n "$reply" ]] && zle -U -- "$reply"
    return 0
  }

  # ZLE consumes the ESC of a late cursor-position reply and inserts the rest,
  # such as "[12;5R". Remove it, adopt its column, and allow a longer deadline.
  _zsuggestion_strip_late_prompt_reply() {
    (( _ZSUGGESTION_PROMPT_LATE )) || return 0
    [[ "$BUFFER" == *'['<->';'<->R* ]] || return 0
    local before="${BUFFER%%\[<->;<->R*}" rest late
    rest="${BUFFER[${#before}+1,-1]}"
    late="${(M)rest#\[<->;<->R}"
    [[ -n "$late" ]] || return 0
    BUFFER="${before}${rest[${#late}+1,-1]}"
    CURSOR=${#BUFFER}
    _ZSUGGESTION_PROMPT_COLUMN=$(( ${${late##*;}%R} - 1 ))
    _ZSUGGESTION_PROMPT_LATE=0
    _ZSUGGESTION_PROMPT_ANSWERED=1
    _ZSUGGESTION_PROMPT_QUERY=1
    (( _ZSUGGESTION_PROMPT_TIMEOUT < 2 )) &&
      _ZSUGGESTION_PROMPT_TIMEOUT=$(( _ZSUGGESTION_PROMPT_TIMEOUT * 2 ))
    return 0
  }

  # Sets REPLY to the screen column where the word being completed starts and
  # _ZSUGGESTION_ANCHOR_HEAD to the buffer text before that word.
  _zsuggestion_anchor() {
    local input="${1[1,$2]}" word head line prompt_column=$_ZSUGGESTION_PROMPT_COLUMN
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      head=""
    else
      word="${input##*[[:space:]]}"
      head="${input[1,$(( ${#input} - ${#word} ))]}"
    fi
    _ZSUGGESTION_ANCHOR_HEAD="$head"
    (( prompt_column < 0 )) && prompt_column=__ZSUGGESTION_UI_PROMPT_OFFSET__
    if [[ "$head" == *$'\n'* ]]; then
      line="${head##*$'\n'}"
      prompt_column=0
    else
      line="$head"
    fi
    REPLY=$(( (prompt_column + ${(m)#line}) % ${COLUMNS:-80} ))
  }

  _zsuggestion_menu_render() {
    local index display description kind kind_label icon marker row fill ghost=""
    local separator="·" ellipsis="…"
    local selected_marker="▌ " history_icon="↺" command_icon="❯" native_icon="⇥"
    local fuzzy_ghost_prefix="  → "
    local file_icon="·" directory_icon="▸" option_icon="-" subcommand_icon="›" value_icon="="
    if (( ! _ZSUGGESTION_UTF8_UI )); then
      separator="|"
      ellipsis="~"
      selected_marker="| "
      history_icon="H"
      command_icon="$"
      native_icon="T"
      file_icon="F"
      directory_icon="D"
      option_icon="O"
      subcommand_icon="S"
      value_icon="V"
      fuzzy_ghost_prefix="  -> "
    fi
    local input="${BUFFER:-$_ZSUGGESTION_MENU_REQUEST_BUFFER}"
    local cursor_position=$CURSOR
    if [[ -z "$BUFFER" && -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      cursor_position=$_ZSUGGESTION_MENU_REQUEST_CURSOR
    fi
    local total=${#_ZSUGGESTION_MENU_DISPLAYS}
    local selected="$_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]"
    if (( cursor_position == ${#input} )); then
      if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
        [[ -n "$selected" ]] && ghost="${fuzzy_ghost_prefix}${selected}"
      elif [[ -n "$input" && "$selected" == "$input"* ]]; then
        ghost="${selected[${#input}+1,-1]}"
      fi
    fi
    region_highlight=( "${(@)region_highlight:#*memo=zsuggestion*}" )
    POSTDISPLAY="$ghost"
    local buffer_end=${#input}
    if (( _ZSUGGESTION_FUZZY_ACTIVE && ${#_ZSUGGESTION_FUZZY_QUERY} )); then
      local query_start=${#_ZSUGGESTION_FUZZY_BASE}
      region_highlight+=("$query_start $buffer_end fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
    fi
    if [[ -n "$ghost" ]]; then
      region_highlight+=("$buffer_end $(( buffer_end + ${#ghost} )) fg=__ZSUGGESTION_UI_GHOST__ memo=zsuggestion")
    fi
    local box_width=$(( ${COLUMNS:-80} - 2 ))
    (( box_width > __ZSUGGESTION_UI_MENU_WIDTH__ )) && box_width=__ZSUGGESTION_UI_MENU_WIDTH__
    if (( box_width < 40 )); then
      _zsuggestion_preview_clear
      local compact_width=$(( ${COLUMNS:-80} - 1 ))
      if (( total > 0 && compact_width >= 8 )); then
        _zsuggestion_anchor "$input" "$cursor_position"
        local compact_head="$_ZSUGGESTION_ANCHOR_HEAD" compact_typed="$input"
        local compact_padding compact_display="$selected" compact_completion=""
        if [[ -n "$compact_head" && "$compact_display" == "$compact_head"* ]]; then
          compact_display="${compact_display[${#compact_head}+1,-1]}"
          compact_typed="${input[${#compact_head}+1,$cursor_position]}"
        fi
        # Start under the edited word; move left only as far as the row needs.
        local compact_indent=$(( REPLY - 4 ))
        local compact_max_indent=$(( ${COLUMNS:-80} - 1 - 4 - ${(m)#compact_display} ))
        (( compact_indent > compact_max_indent )) && compact_indent=$compact_max_indent
        (( compact_indent < 0 )) && compact_indent=0
        compact_width=$(( ${COLUMNS:-80} - 1 - compact_indent ))
        printf -v compact_padding '%*s' "$compact_indent" ""
        if (( ${#compact_display} > compact_width - 4 )); then
          if (( ! _ZSUGGESTION_FUZZY_ACTIVE )) && [[ "$compact_display" == "$compact_typed"* ]]; then
            compact_completion="${compact_display[${#compact_typed}+1,-1]}"
          fi
          if [[ -n "$compact_completion" ]] &&
             (( ${#compact_completion} <= compact_width - 6 )); then
            compact_display="${ellipsis} ${compact_completion}"
          else
            local compact_tail_width=$(( compact_width - 4 - ${#ellipsis} ))
            local compact_tail_start=$(( ${#compact_display} - compact_tail_width + 1 ))
            compact_display="${ellipsis}${compact_display[$compact_tail_start,-1]}"
          fi
        fi
        kind="$_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]"
        case "$kind" in
          history) icon="$history_icon" ;;
          command) icon="$command_icon" ;;
          native) icon="$native_icon" ;;
          file) icon="$file_icon" ;;
          directory) icon="$directory_icon" ;;
          option) icon="$option_icon" ;;
          subcommand) icon="$subcommand_icon" ;;
          value) icon="$value_icon" ;;
          *) icon="." ;;
        esac
        row="${selected_marker}${icon} ${compact_display}"
        local compact_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + compact_indent ))
        POSTDISPLAY+=$'\n'"${compact_padding}${row}"
        region_highlight+=("$compact_start $(( compact_start + ${#row} )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_TEXT__ memo=zsuggestion")
        region_highlight+=("$compact_start $(( compact_start + 2 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
      fi
      _ZSUGGESTION_MENU_OWNS_DISPLAY=1
      return 0
    fi

    if (( total == 0 )); then
      region_highlight=( "${(@)region_highlight:#*memo=zsuggestion*}" )
      POSTDISPLAY=""
      if (( _ZSUGGESTION_FUZZY_ACTIVE && ${#_ZSUGGESTION_FUZZY_QUERY} )); then
        local query_start=${#_ZSUGGESTION_FUZZY_BASE}
        region_highlight+=("$query_start ${#input} fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
      fi
      _ZSUGGESTION_MENU_OWNS_DISPLAY=1
      return 0
    fi
    local window_size=$total
    (( window_size > __ZSUGGESTION_UI_MAX_VISIBLE__ )) && window_size=__ZSUGGESTION_UI_MAX_VISIBLE__
    local max_start=$(( total - window_size + 1 ))
    if (( window_size > 2 )); then
      (( _ZSUGGESTION_MENU_INDEX <= _ZSUGGESTION_MENU_START && _ZSUGGESTION_MENU_START > 1 )) && \
        _ZSUGGESTION_MENU_START=$(( _ZSUGGESTION_MENU_INDEX - 1 ))
      (( _ZSUGGESTION_MENU_INDEX >= _ZSUGGESTION_MENU_START + window_size - 1 )) && \
        _ZSUGGESTION_MENU_START=$(( _ZSUGGESTION_MENU_INDEX - window_size + 2 ))
    else
      (( _ZSUGGESTION_MENU_INDEX < _ZSUGGESTION_MENU_START )) && _ZSUGGESTION_MENU_START=$_ZSUGGESTION_MENU_INDEX
      (( _ZSUGGESTION_MENU_INDEX >= _ZSUGGESTION_MENU_START + window_size )) && \
        _ZSUGGESTION_MENU_START=$(( _ZSUGGESTION_MENU_INDEX - window_size + 1 ))
    fi
    (( _ZSUGGESTION_MENU_START < 1 )) && _ZSUGGESTION_MENU_START=1
    (( _ZSUGGESTION_MENU_START > max_start )) && _ZSUGGESTION_MENU_START=$max_start
    local end=$(( _ZSUGGESTION_MENU_START + window_size - 1 ))

    # Like an IDE completion popup: rows show the word being completed, the
    # popup starts under that word, and its width fits the content.
    _zsuggestion_anchor "$input" "$cursor_position"
    local anchor_column=$REPLY anchor_head="$_ZSUGGESTION_ANCHOR_HEAD"
    local anchor_word="${input[${#anchor_head}+1,$cursor_position]}"
    local shown max_title=0 max_description=0
    for (( index = 1; index <= total; index++ )); do
      shown="$_ZSUGGESTION_MENU_DISPLAYS[$index]"
      [[ -n "$anchor_head" && "$shown" == "$anchor_head"* ]] && shown="${shown[${#anchor_head}+1,-1]}"
      (( ${(m)#shown} > max_title )) && max_title=${(m)#shown}
      shown="$_ZSUGGESTION_MENU_DESCRIPTIONS[$index]"
      (( ${(m)#shown} > max_description )) && max_description=${(m)#shown}
    done
    local kind_width=6
    local description_width=$max_description
    (( description_width > 32 )) && description_width=32
    (( description_width < 8 )) && description_width=8
    local title_width=$max_title
    (( title_width < 12 )) && title_width=12
    if (( title_width + description_width + kind_width + 8 > box_width )); then
      description_width=$(( box_width - title_width - kind_width - 8 ))
      (( description_width < 12 )) && description_width=12
      title_width=$(( box_width - description_width - kind_width - 8 ))
    fi
    box_width=$(( title_width + description_width + kind_width + 8 ))
    local thumb="█" track="│"
    if (( ! _ZSUGGESTION_UTF8_UI )); then
      thumb='#'
      track='|'
    fi
    local thumb_size=0 thumb_pos=0 scrollbar_char="$track"
    if (( total > window_size )); then
      thumb_size=$(( window_size * window_size / total ))
      (( thumb_size < 1 )) && thumb_size=1
      (( thumb_size > window_size )) && thumb_size=$window_size
      thumb_pos=$(( (_ZSUGGESTION_MENU_START - 1) * (window_size - thumb_size) / (total - window_size) ))
    fi

    # Rows start with a two-column marker and an icon, so shift left by four
    # columns to put candidate text exactly under the edited word.
    local indent=$(( anchor_column - 4 ))
    local max_indent=$(( ${COLUMNS:-80} - box_width - 1 ))
    (( indent > max_indent )) && indent=$max_indent
    (( indent < 0 )) && indent=0
    local padding
    printf -v padding '%*s' "$indent" ""

    _zsuggestion_preview_consider

    for (( index = _ZSUGGESTION_MENU_START; index <= end; index++ )); do
      display="$_ZSUGGESTION_MENU_DISPLAYS[$index]"
      local display_truncated=0 completion_tail="" typed_part="$input"
      if [[ -n "$anchor_head" && "$display" == "$anchor_head"* ]]; then
        display="${display[${#anchor_head}+1,-1]}"
        typed_part="$anchor_word"
      fi
      if (( ${#display} > title_width )); then
        display_truncated=1
        if (( ! _ZSUGGESTION_FUZZY_ACTIVE )) && [[ "$display" == "$typed_part"* ]]; then
          completion_tail="${display[${#typed_part}+1,-1]}"
        fi
        if [[ -n "$completion_tail" ]] && (( ${#completion_tail} <= title_width - 2 )); then
          display="${ellipsis} ${completion_tail}"
        else
          local tail_width=$(( title_width - ${#ellipsis} ))
          local tail_start=$(( ${#display} - tail_width + 1 ))
          display="${ellipsis}${display[$tail_start,-1]}"
        fi
      fi
      description="$_ZSUGGESTION_MENU_DESCRIPTIONS[$index]"
      if (( ${#description} > description_width )); then
        description="${description[1,$(( description_width - 1 ))]}${ellipsis}"
      fi
      kind="$_ZSUGGESTION_MENU_KINDS[$index]"
      case "$kind" in
        history) icon="$history_icon" ;;
        command) icon="$command_icon" ;;
        native) icon="$native_icon" ;;
        file) icon="$file_icon" ;;
        directory) icon="$directory_icon" ;;
        option) icon="$option_icon" ;;
        subcommand) icon="$subcommand_icon" ;;
        value) icon="$value_icon" ;;
        *) icon="." ;;
      esac
      case "$kind" in
        history) kind_label="Hist" ;;
        command) kind_label="Cmd" ;;
        native) kind_label="Native" ;;
        file) kind_label="File" ;;
        directory) kind_label="Dir" ;;
        option) kind_label="Opt" ;;
        subcommand) kind_label="Sub" ;;
        value) kind_label="Value" ;;
        *) kind_label="" ;;
      esac
      marker="  "
      (( index == _ZSUGGESTION_MENU_INDEX )) && marker="$selected_marker"
      scrollbar_char="$track"
      if (( thumb_size > 0 )); then
        local row_offset=$(( index - _ZSUGGESTION_MENU_START ))
        (( row_offset >= thumb_pos && row_offset < thumb_pos + thumb_size )) && scrollbar_char="$thumb"
      fi
      printf -v row '%s%s %-*s %-*s %*s %s' \
        "$marker" "$icon" "$title_width" "$display" \
        "$description_width" "$description" "$kind_width" "$kind_label" "$scrollbar_char"

      line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
      POSTDISPLAY+=$'\n'"${padding}${row}"
      if (( index == _ZSUGGESTION_MENU_INDEX )); then
        region_highlight+=("$(( line_start )) $(( line_start + ${#row} - 1 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_TEXT__ memo=zsuggestion")
        region_highlight+=("$(( line_start )) $(( line_start + 2 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
        region_highlight+=("$(( line_start + 2 )) $(( line_start + 4 )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__ memo=zsuggestion")
      else
        region_highlight+=("$(( line_start )) $(( line_start + ${#row} - 1 )) fg=__ZSUGGESTION_UI_TEXT__ memo=zsuggestion")
        region_highlight+=("$(( line_start )) $(( line_start + 4 )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
      fi
      local rail_attrs="fg=__ZSUGGESTION_UI_MUTED__"
      [[ "$scrollbar_char" == "$thumb" ]] && rail_attrs="fg=__ZSUGGESTION_UI_SELECTED_TEXT__"
      region_highlight+=("$(( line_start + ${#row} - 1 )) $(( line_start + ${#row} )) $rail_attrs memo=zsuggestion")

      local match_length=0
      (( ! _ZSUGGESTION_FUZZY_ACTIVE && ! display_truncated )) && match_length=${#typed_part}
      (( match_length > title_width )) && match_length=$title_width
      if (( match_length > 0 )); then
        if (( index == _ZSUGGESTION_MENU_INDEX )); then
          region_highlight+=("$(( line_start + 4 )) $(( line_start + 4 + match_length )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_TEXT__,bold memo=zsuggestion")
        else
          region_highlight+=("$(( line_start + 4 )) $(( line_start + 4 + match_length )) fg=__ZSUGGESTION_UI_SELECTED_TEXT__,bold memo=zsuggestion")
        fi
      fi
      local description_start=$(( line_start + 5 + title_width ))
      if (( index == _ZSUGGESTION_MENU_INDEX )); then
        region_highlight+=("$description_start $(( description_start + description_width )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_SELECTED_SOURCE__ memo=zsuggestion")
      else
        region_highlight+=("$description_start $(( description_start + description_width )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
      fi
      local kind_start=$(( description_start + description_width + 1 ))
      if (( index == _ZSUGGESTION_MENU_INDEX )); then
        region_highlight+=("$kind_start $(( kind_start + kind_width )) bg=__ZSUGGESTION_UI_SELECTED_BACKGROUND__,fg=__ZSUGGESTION_UI_ACCENT__,bold memo=zsuggestion")
      else
        region_highlight+=("$kind_start $(( kind_start + kind_width )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
      fi
    done

    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      local footer=" Esc exit ${separator} C-K up ${separator} C-N down ${separator} Tab choose ${separator} Enter run "
      line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
      POSTDISPLAY+=$'\n'"${padding}${footer}"
      region_highlight+=("$line_start $(( line_start + ${#footer} )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
    else
      local footer=" Up/Down select ${separator} Tab fill ${separator} __ZSUGGESTION_COMPLETION_KEY_LABEL__ full "
      line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
      POSTDISPLAY+=$'\n'"${padding}${footer}"
      region_highlight+=("$line_start $(( line_start + ${#footer} )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
    fi
    local preview_source="${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]}"
    local preview_description="${_ZSUGGESTION_MENU_DESCRIPTIONS[$_ZSUGGESTION_MENU_INDEX]}"
    local show_preview=0
    if [[ ${#_ZSUGGESTION_PREVIEW_LINES} -gt 0 &&
          "${_ZSUGGESTION_PREVIEW_LINES[1]}" != "Preview unavailable" &&
          "${_ZSUGGESTION_PREVIEW_LINES[1]}" != "Preview timed out" ]]; then
      show_preview=1
    elif [[ "$preview_source" == command && -n "$preview_description" &&
          "$preview_description" != "System command" &&
          "$preview_description" != "Executable installed by "* ]]; then
      show_preview=1
    fi
    if (( ${COLUMNS:-80} >= 100 && show_preview )); then
      local preview_width=$(( ${COLUMNS:-80} - indent - 1 ))
      (( preview_width > 96 )) && preview_width=96
      if (( preview_width >= 40 )); then
        local preview_label="Preview: ${selected}"
        local preview_style style_span style_start style_end style_value style_rest
        local preview_content_width=$(( preview_width - 2 ))
        local -a preview_lines
        if (( ${#_ZSUGGESTION_PREVIEW_LINES} )); then
          preview_lines=("${_ZSUGGESTION_PREVIEW_LINES[@]}")
        else
          preview_lines=("Source: ${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]} / ${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}")
          preview_lines+=("$preview_description")
        fi
        (( ${#preview_label} > preview_width )) && \
          preview_label="${preview_label[1,$(( preview_width - 1 ))]}${ellipsis}"
        local line_start
        line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
        POSTDISPLAY+=$'\n'"${padding}${preview_label}"
        region_highlight+=("$line_start $(( line_start + ${#preview_label} )) fg=__ZSUGGESTION_UI_MUTED__ memo=zsuggestion")
        local preview_index
        for (( preview_index = 1; preview_index <= ${#preview_lines} && preview_index <= 8; preview_index++ )); do
          preview_line="${preview_lines[$preview_index]}"
          (( ${#preview_line} > preview_content_width )) && \
            preview_line="${preview_line[1,$(( preview_content_width - 1 ))]}${ellipsis}"
          line_start=$(( ${#input} + ${#POSTDISPLAY} + 1 + indent ))
          POSTDISPLAY+=$'\n'"${padding}${preview_line}"
          region_highlight+=("$line_start $(( line_start + ${#preview_line} )) fg=__ZSUGGESTION_UI_TEXT__ memo=zsuggestion")
          preview_style="${_ZSUGGESTION_PREVIEW_STYLES[$preview_index]}"
          for style_span in ${(s:;:)preview_style}; do
            style_start="${style_span%%:*}"
            style_rest="${style_span#*:}"
            style_end="${style_rest%%:*}"
            style_value="${style_rest#*:}"
            [[ "$style_start" == <-> && "$style_end" == <-> && -n "$style_value" ]] || continue
            (( style_start >= ${#preview_line} )) && continue
            (( style_end > ${#preview_line} )) && style_end=${#preview_line}
            (( style_start < style_end )) && \
              region_highlight+=("$(( line_start + style_start )) $(( line_start + style_end )) ${style_value} memo=zsuggestion")
          done
        done
      fi
    fi
    _ZSUGGESTION_MENU_OWNS_DISPLAY=1
  }

  # Collects only the aliases a completion request needs, so the shared daemon
  # never stores shell state: names extending the command word while it is
  # being typed, or the leading alias chain once arguments begin.
  _zsuggestion_alias_args() {
    emulate -L zsh
    setopt extended_glob
    reply=()
    local buffer="$1" word name
    local -i depth=0
    local -A seen
    (( ${#aliases} )) || return 0
    if [[ "$buffer" != *[[:space:]]* ]]; then
      [[ "$buffer" == [[:alnum:]_+-]## ]] || return 0
      for name in ${${(ok)aliases[(I)${(b)buffer}*]}[1,32]}; do
        reply+=("--alias=${name}=${aliases[$name]}")
      done
      return 0
    fi
    word="${buffer%%[[:space:]]*}"
    while (( depth++ < 8 )) && [[ -n "$word" && -z "${seen[$word]}" ]] &&
          (( ${+aliases[$word]} )); do
      seen[$word]=1
      reply+=("--alias=${word}=${aliases[$word]}")
      word="${${aliases[$word]##[[:space:]]#}%%[[:space:]]*}"
    done
  }

  _zsuggestion_menu_query() {
    local LC_ALL=C cursor_byte accept display description kind source reply
    local -a alias_args
    _zsuggestion_menu_cancel_request
    cursor_byte=${#LBUFFER}
    _zsuggestion_alias_args "$BUFFER"
    alias_args=("${reply[@]}")
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _ZSUGGESTION_MENU_ACCEPTS=()
    _ZSUGGESTION_MENU_DISPLAYS=()
    _ZSUGGESTION_MENU_DESCRIPTIONS=()
    _ZSUGGESTION_MENU_KINDS=()
    _ZSUGGESTION_MENU_SOURCES=()
    while IFS= read -r -d '' accept &&
          IFS= read -r -d '' display &&
          IFS= read -r -d '' description &&
          IFS= read -r -d '' kind &&
          IFS= read -r -d '' source; do
      _ZSUGGESTION_MENU_ACCEPTS+=("$accept")
      _ZSUGGESTION_MENU_DISPLAYS+=("$display")
      _ZSUGGESTION_MENU_DESCRIPTIONS+=("$description")
      _ZSUGGESTION_MENU_KINDS+=("$kind")
      _ZSUGGESTION_MENU_SOURCES+=("$source")
    done < <(print -rn -- "$BUFFER" | command zsuggestion complete \
      --stdin \
      --cursor "$cursor_byte" \
      --cwd "$PWD" \
      "${alias_args[@]}" \
      --format zsh 2>/dev/null)
    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} == 0 )); then
      _zsuggestion_menu_clear 1
      return 1
    fi
    _ZSUGGESTION_MENU_ACTIVE=1
    _ZSUGGESTION_MENU_INDEX=1
    _ZSUGGESTION_MENU_START=1
    _zsuggestion_menu_render
    return 0
  }

  _zsuggestion_menu_accept() {
    local accept="${_ZSUGGESTION_MENU_ACCEPTS[$_ZSUGGESTION_MENU_INDEX]}"
    local display="${_ZSUGGESTION_MENU_DISPLAYS[$_ZSUGGESTION_MENU_INDEX]}"
    local source="${_ZSUGGESTION_MENU_SOURCES[$_ZSUGGESTION_MENU_INDEX]}"
    local kind="${_ZSUGGESTION_MENU_KINDS[$_ZSUGGESTION_MENU_INDEX]}"
    (( CURSOR == ${#BUFFER} )) || return 1
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      _zsuggestion_menu_clear
      BUFFER="$accept"
      [[ "$source" == command && "$BUFFER" != *[[:space:]]* ]] && BUFFER+=" "
      CURSOR=${#BUFFER}
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      _zsuggestion_menu_schedule
      return 0
    fi
    LBUFFER+="$accept"
    if [[ ( "$source" == native || "$kind" == file || "$kind" == option ||
            "$kind" == subcommand || "$kind" == value ) &&
          "$BUFFER" == "$display" &&
          "${BUFFER[-1]}" != [@/:=,[:space:]] ]]; then
      LBUFFER+=" "
    fi
    POSTDISPLAY=""
    _zsuggestion_menu_clear
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_schedule
  }

  _zsuggestion_call_native_completion() {
    local widget="$1"
    _ZSUGGESTION_IN_NATIVE_COMPLETION=1
    {
      zle "$widget"
    } always {
      _ZSUGGESTION_IN_NATIVE_COMPLETION=0
    }
  }

  _zsuggestion_menu_refresh() {
    local previous_buffer="$_ZSUGGESTION_MENU_BUFFER" delta="" accept
    local index retained_index=$_ZSUGGESTION_MENU_INDEX
    local -a accepts displays descriptions kinds sources
    if (( _ZSUGGESTION_MENU_ACTIVE )) && [[ "$BUFFER" == "$previous_buffer"* ]]; then
      delta="${BUFFER[${#previous_buffer}+1,-1]}"
      for (( index = 1; index <= ${#_ZSUGGESTION_MENU_DISPLAYS}; index++ )); do
        [[ "${_ZSUGGESTION_MENU_DISPLAYS[$index]}" == "$BUFFER"* ]] || continue
        accept="${_ZSUGGESTION_MENU_ACCEPTS[$index]}"
        if [[ -n "$delta" ]]; then
          [[ "$accept" == "$delta"* ]] || continue
          accept="${accept[${#delta}+1,-1]}"
        fi
        if [[ -z "$accept" ]]; then
          if [[ "${_ZSUGGESTION_MENU_KINDS[$index]}" == file &&
                "${_ZSUGGESTION_MENU_DISPLAYS[$index]}" == "$BUFFER" ]]; then
            accept=" "
          else
            continue
          fi
        fi
        accepts+=("$accept")
        displays+=("${_ZSUGGESTION_MENU_DISPLAYS[$index]}")
        descriptions+=("${_ZSUGGESTION_MENU_DESCRIPTIONS[$index]}")
        kinds+=("${_ZSUGGESTION_MENU_KINDS[$index]}")
        sources+=("${_ZSUGGESTION_MENU_SOURCES[$index]}")
      done
    fi
    if (( ${#displays} )); then
      _ZSUGGESTION_MENU_ACCEPTS=("${accepts[@]}")
      _ZSUGGESTION_MENU_DISPLAYS=("${displays[@]}")
      _ZSUGGESTION_MENU_DESCRIPTIONS=("${descriptions[@]}")
      _ZSUGGESTION_MENU_KINDS=("${kinds[@]}")
      _ZSUGGESTION_MENU_SOURCES=("${sources[@]}")
      (( retained_index > ${#_ZSUGGESTION_MENU_DISPLAYS} )) && retained_index=${#_ZSUGGESTION_MENU_DISPLAYS}
      (( retained_index < 1 )) && retained_index=1
      _ZSUGGESTION_MENU_INDEX=$retained_index
      # Keep the current viewport; render re-clamps it around the selection
      # so a refresh never makes the visible window jump.
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=1
      _zsuggestion_menu_render
    else
      _zsuggestion_menu_clear
    fi
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    if [[ -n "$BUFFER" ]] && (( CURSOR == ${#BUFFER} )); then
      _zsuggestion_menu_schedule
    fi
  }

  _zsuggestion_menu_schedule() {
    local LC_ALL=C
    _zsuggestion_menu_cancel_query
    _zsuggestion_native_cancel
    _ZSUGGESTION_MENU_REQUEST_BUFFER="$BUFFER"
    _ZSUGGESTION_MENU_REQUEST_CWD="$PWD"
    _ZSUGGESTION_MENU_REQUEST_CURSOR=${#LBUFFER}
    _ZSUGGESTION_MENU_REQUEST_DIRTY=1
    _ZSUGGESTION_NATIVE_REQUESTED=0
    _ZSUGGESTION_NATIVE_START_TICKS=0
    _zsuggestion_menu_start_ticker
    _ZSUGGESTION_DAEMON_ACCEPTS=()
    _ZSUGGESTION_DAEMON_DISPLAYS=()
    _ZSUGGESTION_DAEMON_DESCRIPTIONS=()
    _ZSUGGESTION_DAEMON_KINDS=()
    _ZSUGGESTION_DAEMON_SOURCES=()
    _ZSUGGESTION_NATIVE_ACCEPTS=()
    _ZSUGGESTION_NATIVE_DISPLAYS=()
    _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
    _ZSUGGESTION_NATIVE_KINDS=()
  }

  _zsuggestion_candidate_rank() {
    case "$1" in
      command) REPLY=0 ;;
      option | subcommand | value) REPLY=1 ;;
      directory) REPLY=2 ;;
      file) REPLY=3 ;;
      history) REPLY=5 ;;
      *) REPLY=4 ;;
    esac
  }

  _zsuggestion_menu_publish() {
    local index display
    local -a redraw_hooks apply_hooks
    local limit=$(( __ZSUGGESTION_COMPLETION_MAX_CANDIDATES__ * 2 + __ZSUGGESTION_NATIVE_LIMIT__ ))
    _ZSUGGESTION_MENU_ACCEPTS=()
    _ZSUGGESTION_MENU_DISPLAYS=()
    _ZSUGGESTION_MENU_DESCRIPTIONS=()
    _ZSUGGESTION_MENU_KINDS=()
    _ZSUGGESTION_MENU_SOURCES=()

    for (( index = 1; index <= ${#_ZSUGGESTION_DAEMON_ACCEPTS}; index++ )); do
      display="${_ZSUGGESTION_DAEMON_DISPLAYS[$index]}"
      [[ -n "$display" ]] && (( ! ${_ZSUGGESTION_MENU_DISPLAYS[(Ie)$display]} )) || continue
      _ZSUGGESTION_MENU_ACCEPTS+=("${_ZSUGGESTION_DAEMON_ACCEPTS[$index]}")
      _ZSUGGESTION_MENU_DISPLAYS+=("$display")
      _ZSUGGESTION_MENU_DESCRIPTIONS+=("${_ZSUGGESTION_DAEMON_DESCRIPTIONS[$index]}")
      _ZSUGGESTION_MENU_KINDS+=("${_ZSUGGESTION_DAEMON_KINDS[$index]}")
      _ZSUGGESTION_MENU_SOURCES+=("${_ZSUGGESTION_DAEMON_SOURCES[$index]}")
      (( ${#_ZSUGGESTION_MENU_ACCEPTS} >= limit )) && break
    done

    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} < limit )); then
      for (( index = 1; index <= ${#_ZSUGGESTION_NATIVE_ACCEPTS}; index++ )); do
        display="${_ZSUGGESTION_NATIVE_DISPLAYS[$index]}"
        [[ -n "$display" ]] && (( ! ${_ZSUGGESTION_MENU_DISPLAYS[(Ie)$display]} )) || continue
        _ZSUGGESTION_MENU_ACCEPTS+=("${_ZSUGGESTION_NATIVE_ACCEPTS[$index]}")
        _ZSUGGESTION_MENU_DISPLAYS+=("$display")
        _ZSUGGESTION_MENU_DESCRIPTIONS+=("${_ZSUGGESTION_NATIVE_DESCRIPTIONS[$index]}")
        _ZSUGGESTION_MENU_KINDS+=("${_ZSUGGESTION_NATIVE_KINDS[$index]:-native}")
        _ZSUGGESTION_MENU_SOURCES+=("native")
        (( ${#_ZSUGGESTION_MENU_ACCEPTS} >= limit )) && break
      done
    fi

    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} > 1 )); then
      local -a ranks perm sorted_accepts sorted_displays sorted_descriptions sorted_kinds sorted_sources
      integer rank_index inner_index moving moving_rank other other_rank
      ranks=()
      for (( rank_index = 1; rank_index <= ${#_ZSUGGESTION_MENU_KINDS}; rank_index++ )); do
        _zsuggestion_candidate_rank \
          "${_ZSUGGESTION_MENU_KINDS[$rank_index]}" \
          "${_ZSUGGESTION_MENU_SOURCES[$rank_index]}"
        ranks+=("$REPLY")
      done
      perm=()
      for (( rank_index = 1; rank_index <= ${#ranks}; rank_index++ )); do
        perm+=("$rank_index")
      done
      for (( rank_index = 2; rank_index <= ${#perm}; rank_index++ )); do
        moving="${perm[$rank_index]}"
        moving_rank="${ranks[$moving]}"
        for (( inner_index = rank_index - 1; inner_index >= 1; inner_index-- )); do
          other="${perm[$inner_index]}"
          other_rank="${ranks[$other]}"
          (( other_rank <= moving_rank )) && break
          perm[inner_index+1]="$other"
        done
        perm[inner_index+1]="$moving"
      done
      for (( rank_index = 1; rank_index <= ${#perm}; rank_index++ )); do
        other="${perm[$rank_index]}"
        sorted_accepts+=("${_ZSUGGESTION_MENU_ACCEPTS[$other]}")
        sorted_displays+=("${_ZSUGGESTION_MENU_DISPLAYS[$other]}")
        sorted_descriptions+=("${_ZSUGGESTION_MENU_DESCRIPTIONS[$other]}")
        sorted_kinds+=("${_ZSUGGESTION_MENU_KINDS[$other]}")
        sorted_sources+=("${_ZSUGGESTION_MENU_SOURCES[$other]}")
      done
      _ZSUGGESTION_MENU_ACCEPTS=("${sorted_accepts[@]}")
      _ZSUGGESTION_MENU_DISPLAYS=("${sorted_displays[@]}")
      _ZSUGGESTION_MENU_DESCRIPTIONS=("${sorted_descriptions[@]}")
      _ZSUGGESTION_MENU_KINDS=("${sorted_kinds[@]}")
      _ZSUGGESTION_MENU_SOURCES=("${sorted_sources[@]}")
    fi


    _ZSUGGESTION_MENU_RESTORE_INDEX=$_ZSUGGESTION_MENU_INDEX
    zstyle -a zle-line-pre-redraw widgets redraw_hooks
    apply_hooks=( "${(@)redraw_hooks:#*:_zsh_highlight__zle-line-pre-redraw}" )
    if (( ${#apply_hooks} != ${#redraw_hooks} )); then
      zstyle zle-line-pre-redraw widgets "${apply_hooks[@]}"
      {
        zle zsuggestion-menu-apply
      } always {
        zstyle zle-line-pre-redraw widgets "${redraw_hooks[@]}"
      }
    else
      zle zsuggestion-menu-apply
    fi
    region_highlight=( "${_ZSUGGESTION_RENDERED_HIGHLIGHTS[@]}" )
  }

  _zsuggestion_menu_request_ready() {
    local fd="$1" accept display description kind source pending response_pending
    local any_pending=0
    local -a accepts displays descriptions kinds sources
    # Stale fd: whoever replaced it already unregistered and closed it. zle can
    # still dispatch it in the same poll round, and the number may be reused.
    (( fd == _ZSUGGESTION_MENU_REQUEST_FD )) || return 0
    if [[ "$_ZSUGGESTION_MENU_BUFFER" != "$_ZSUGGESTION_MENU_INFLIGHT_BUFFER" ]] ||
       (( _ZSUGGESTION_MENU_INFLIGHT_GENERATION != _ZSUGGESTION_MENU_GENERATION )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_MENU_REQUEST_FD=-1
      _ZSUGGESTION_MENU_INFLIGHT_BUFFER=""
      _ZSUGGESTION_MENU_INFLIGHT_CURSOR=0
      return 0
    fi

    zle -F "$fd"
    BUFFER="$_ZSUGGESTION_MENU_INFLIGHT_BUFFER"
    CURSOR=$_ZSUGGESTION_MENU_INFLIGHT_CURSOR
    if IFS= read -r -u "$fd" -d '' response_pending && [[ "$response_pending" == true ]]; then
      any_pending=1
    fi
    while IFS= read -r -u "$fd" -d '' accept &&
          IFS= read -r -u "$fd" -d '' display &&
          IFS= read -r -u "$fd" -d '' description &&
          IFS= read -r -u "$fd" -d '' kind &&
          IFS= read -r -u "$fd" -d '' source &&
          IFS= read -r -u "$fd" -d '' pending; do
      accepts+=("$accept")
      displays+=("$display")
      descriptions+=("$description")
      kinds+=("$kind")
      sources+=("$source")
      [[ "$pending" == true ]] && any_pending=1
    done
    exec {fd}<&-

    _ZSUGGESTION_MENU_REQUEST_FD=-1
    _ZSUGGESTION_DAEMON_ACCEPTS=("${accepts[@]}")
    _ZSUGGESTION_DAEMON_DISPLAYS=("${displays[@]}")
    _ZSUGGESTION_DAEMON_DESCRIPTIONS=("${descriptions[@]}")
    _ZSUGGESTION_DAEMON_KINDS=("${kinds[@]}")
    _ZSUGGESTION_DAEMON_SOURCES=("${sources[@]}")
    (( any_pending )) && _ZSUGGESTION_MENU_REFRESH_TICKS=5 || _ZSUGGESTION_MENU_REFRESH_TICKS=0
    (( _ZSUGGESTION_MENU_REFRESH_TICKS > 0 )) && _zsuggestion_menu_start_ticker
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE && ${#accepts} == 0 &&
          ! _ZSUGGESTION_MENU_ACTIVE )); then
      _ZSUGGESTION_NATIVE_ACCEPTS=()
      _ZSUGGESTION_NATIVE_DISPLAYS=()
      _ZSUGGESTION_NATIVE_DESCRIPTIONS=()
      _ZSUGGESTION_NATIVE_KINDS=()
      return 0
    fi
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE && ${#accepts} == 0 && _ZSUGGESTION_MENU_ACTIVE &&
          $+functions[_main_complete] )) &&
       (( ! _ZSUGGESTION_NATIVE_REQUESTED || _ZSUGGESTION_NATIVE_REQUEST_FD >= 0 )); then
      return 0
    fi
    _zsuggestion_menu_publish
  }

  _zsuggestion_native_request_ready() {
    local fd="$1" accept display description kind
    local -a accepts displays descriptions kinds
    # Stale fd: whoever replaced it already unregistered and closed it. zle can
    # still dispatch it in the same poll round, and the number may be reused.
    (( fd == _ZSUGGESTION_NATIVE_REQUEST_FD )) || return 0
    if [[ "$_ZSUGGESTION_MENU_BUFFER" != "$_ZSUGGESTION_NATIVE_INFLIGHT_BUFFER" ]] ||
       (( _ZSUGGESTION_NATIVE_INFLIGHT_GENERATION != _ZSUGGESTION_MENU_GENERATION )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_NATIVE_REQUEST_FD=-1
      _ZSUGGESTION_NATIVE_REQUEST_PID=-1
      _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
      _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER=""
      _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=0
      return 0
    fi

    zle -F "$fd"
    BUFFER="$_ZSUGGESTION_NATIVE_INFLIGHT_BUFFER"
    CURSOR=$_ZSUGGESTION_NATIVE_INFLIGHT_CURSOR
    while IFS= read -r -u "$fd" -d '' accept &&
          IFS= read -r -u "$fd" -d '' display &&
          IFS= read -r -u "$fd" -d '' description &&
          IFS= read -r -u "$fd" -d '' kind; do
      accepts+=("$accept")
      displays+=("$display")
      descriptions+=("$description")
      kinds+=("$kind")
    done
    exec {fd}<&-
    _ZSUGGESTION_NATIVE_REQUEST_FD=-1
    _ZSUGGESTION_NATIVE_REQUEST_PID=-1
    _ZSUGGESTION_NATIVE_REQUEST_TICKS=0

    _ZSUGGESTION_NATIVE_ACCEPTS=("${accepts[@]}")
    _ZSUGGESTION_NATIVE_DISPLAYS=("${displays[@]}")
    _ZSUGGESTION_NATIVE_DESCRIPTIONS=("${descriptions[@]}")
    _ZSUGGESTION_NATIVE_KINDS=("${kinds[@]}")
    _zsuggestion_menu_publish
  }

  _zsuggestion_native_capture_widget() {
    local fd
    exec {fd}< <(
      local __zsuggestion_original_lbuffer="$LBUFFER" __zsuggestion_original_current=$CURRENT
      local -a __zsuggestion_accepts __zsuggestion_displays __zsuggestion_descriptions __zsuggestion_kinds

      compadd() {
        local __zsuggestion_argument __zsuggestion_capture_rc __zsuggestion_current_prefix="$PREFIX"
        local __zsuggestion_added_prefix="" __zsuggestion_hidden_prefix="" __zsuggestion_ignored_prefix=""
        local __zsuggestion_added_suffix="" __zsuggestion_hidden_suffix="" __zsuggestion_ignored_suffix=""
        local __zsuggestion_display_array="" __zsuggestion_passthrough=0 __zsuggestion_flags __zsuggestion_flag __zsuggestion_value
        local __zsuggestion_index=1 __zsuggestion_flag_index
        # Calls that only ask compadd for matches (-A, -O, -D) add nothing.
        for __zsuggestion_argument in "$@"; do
          [[ "$__zsuggestion_argument" == (-|--) ]] && break
          [[ "$__zsuggestion_argument" == (-A|-O|-D) ]] && __zsuggestion_passthrough=1
        done
        # Parse compadd options, including clustered forms such as "-ld name"
        # that _describe and _git use for their description arrays.
        while (( __zsuggestion_index <= $# )); do
          __zsuggestion_argument="${@[__zsuggestion_index]}"
          case "$__zsuggestion_argument" in
            --|-) break ;;
            -?*)
              __zsuggestion_flags="${__zsuggestion_argument#-}"
              for (( __zsuggestion_flag_index = 1; __zsuggestion_flag_index <= ${#__zsuggestion_flags}; __zsuggestion_flag_index++ )); do
                __zsuggestion_flag="${__zsuggestion_flags[__zsuggestion_flag_index]}"
                [[ "$__zsuggestion_flag" == [OAD] ]] && __zsuggestion_passthrough=1
                if [[ "$__zsuggestion_flag" == o ]]; then
                  # -o takes an optional order list, attached or separate.
                  [[ -z "${__zsuggestion_flags[__zsuggestion_flag_index+1,-1]}" &&
                     "${@[__zsuggestion_index+1]}" == (match|nosort|numeric|reverse)* ]] &&
                    (( __zsuggestion_index++ ))
                  break
                fi
                [[ "$__zsuggestion_flag" == [PpiSsIdJVXxrRMFEWOAD] ]] || continue
                __zsuggestion_value="${__zsuggestion_flags[__zsuggestion_flag_index+1,-1]}"
                if [[ -z "$__zsuggestion_value" ]]; then
                  (( __zsuggestion_index++ ))
                  __zsuggestion_value="${@[__zsuggestion_index]}"
                fi
                case "$__zsuggestion_flag" in
                  P) __zsuggestion_added_prefix="$__zsuggestion_value" ;;
                  p) __zsuggestion_hidden_prefix="$__zsuggestion_value" ;;
                  i) __zsuggestion_ignored_prefix="$__zsuggestion_value" ;;
                  S) __zsuggestion_added_suffix="$__zsuggestion_value" ;;
                  s) __zsuggestion_hidden_suffix="$__zsuggestion_value" ;;
                  I) __zsuggestion_ignored_suffix="$__zsuggestion_value" ;;
                  d) __zsuggestion_display_array="$__zsuggestion_value" ;;
                esac
                break
              done
              ;;
            *) break ;;
          esac
          (( __zsuggestion_index++ ))
        done
        if (( __zsuggestion_passthrough || ${#__zsuggestion_accepts} >= __ZSUGGESTION_NATIVE_LIMIT__ )); then
          builtin compadd "$@"
          return
        fi

        # Like fzf-tab: let compadd filter the words and, through -D, keep the
        # caller's display strings ("word  -- description") aligned with them.
        local -a __zsuggestion_hits __zsuggestion_hit_displays
        [[ -n "$__zsuggestion_display_array" ]] && __zsuggestion_hit_displays=( "${(@P)__zsuggestion_display_array}" )
        builtin compadd -A __zsuggestion_hits -D __zsuggestion_hit_displays "$@" 2>/dev/null

        if [[ -z "$IPREFIX$ISUFFIX$QIPREFIX$QISUFFIX$SUFFIX$__zsuggestion_added_prefix$__zsuggestion_hidden_prefix$__zsuggestion_ignored_prefix" ]]; then
          local __zsuggestion_base_length=$(( ${#__zsuggestion_original_lbuffer} - ${#__zsuggestion_current_prefix} ))
          local __zsuggestion_base="" __zsuggestion_match __zsuggestion_full __zsuggestion_accept __zsuggestion_description __zsuggestion_kind __zsuggestion_tag="${curtag:-}"
          local __zsuggestion_hit_index
          (( __zsuggestion_base_length > 0 )) && __zsuggestion_base="${__zsuggestion_original_lbuffer[1,$__zsuggestion_base_length]}"
          if (( __zsuggestion_base_length >= 0 )) &&
             [[ -z "$__zsuggestion_current_prefix" ||
                "${__zsuggestion_original_lbuffer[$(( __zsuggestion_base_length + 1 )),-1]}" == "$__zsuggestion_current_prefix" ]]; then
            for (( __zsuggestion_hit_index = 1; __zsuggestion_hit_index <= ${#__zsuggestion_hits}; __zsuggestion_hit_index++ )); do
              (( ${#__zsuggestion_accepts} < __ZSUGGESTION_NATIVE_LIMIT__ )) || break
              __zsuggestion_match="${__zsuggestion_hits[$__zsuggestion_hit_index]}"
              [[ -n "$__zsuggestion_match" && "${(q)__zsuggestion_match}" == "$__zsuggestion_match" ]] || continue
              [[ -z "$__zsuggestion_current_prefix" ||
                 "${__zsuggestion_match[1,${#__zsuggestion_current_prefix}]}" == "$__zsuggestion_current_prefix" ]] || continue
              __zsuggestion_full="${__zsuggestion_base}${__zsuggestion_match}${__zsuggestion_hidden_suffix}${__zsuggestion_added_suffix}${__zsuggestion_ignored_suffix}"
              [[ "$__zsuggestion_full" == "$__zsuggestion_original_lbuffer"* && "$__zsuggestion_full" != "$__zsuggestion_original_lbuffer" ]] || continue
              (( ${__zsuggestion_displays[(Ie)$__zsuggestion_full]} )) && continue
              __zsuggestion_accept="${__zsuggestion_full[${#__zsuggestion_original_lbuffer}+1,-1]}"
              [[ -n "$__zsuggestion_accept" ]] || continue

              __zsuggestion_description="${__zsuggestion_hit_displays[$__zsuggestion_hit_index]}"
              if [[ "$__zsuggestion_description" == *" -- "* ]]; then
                __zsuggestion_description="${${__zsuggestion_description#* -- }##[[:space:]]#}"
              else
                __zsuggestion_description=""
              fi
              case "$__zsuggestion_tag" in
                options|*-options|flags|switches) __zsuggestion_kind=option ;;
                directories|*-directories|directory-stack|named-directories) __zsuggestion_kind=directory ;;
                files|*-files|globbed-files|all-files|paths) __zsuggestion_kind=file ;;
                *)
                  # External command names come from _command_names, also
                  # after sudo or env; other "commands" tags are a tool's
                  # own subcommands (cargo test, docker kill).
                  if (( __zsuggestion_original_current <= 1 || ${funcstack[(I)_command_names]} )); then
                    __zsuggestion_kind=command
                  else
                    case "$__zsuggestion_tag" in
                      commands|*-commands|*-subcommands|subcommands|commands-*) __zsuggestion_kind=subcommand ;;
                      *) __zsuggestion_kind=value ;;
                    esac
                  fi
                  ;;
              esac
              [[ "$__zsuggestion_kind" == value && "$__zsuggestion_match" == -* ]] && __zsuggestion_kind=option
              [[ ( "$__zsuggestion_kind" == file || "$__zsuggestion_kind" == command ) && "$__zsuggestion_full" == */ ]] && __zsuggestion_kind=directory
              if [[ -z "$__zsuggestion_description" ]]; then
                __zsuggestion_description="${__zsuggestion_tag//-/ }"
                [[ -n "$__zsuggestion_description" ]] || __zsuggestion_description="Zsh completion"
              fi
              __zsuggestion_accepts+=("$__zsuggestion_accept")
              __zsuggestion_displays+=("$__zsuggestion_full")
              __zsuggestion_descriptions+=("$__zsuggestion_description")
              __zsuggestion_kinds+=("$__zsuggestion_kind")
            done
          fi
        fi

        builtin compadd "$@"
        __zsuggestion_capture_rc=$?
        return "$__zsuggestion_capture_rc"
      }

      _main_complete >/dev/null 2>&1
      local __zsuggestion_index
      for (( __zsuggestion_index = 1; __zsuggestion_index <= ${#__zsuggestion_accepts}; __zsuggestion_index++ )); do
        printf '%s\0%s\0%s\0%s\0' \
          "${__zsuggestion_accepts[$__zsuggestion_index]}" \
          "${__zsuggestion_displays[$__zsuggestion_index]}" \
          "${__zsuggestion_descriptions[$__zsuggestion_index]}" \
          "${__zsuggestion_kinds[$__zsuggestion_index]}"
      done
    )
    _ZSUGGESTION_NATIVE_REQUEST_FD="$fd"
    _ZSUGGESTION_NATIVE_REQUEST_PID=$!
    _ZSUGGESTION_NATIVE_REQUEST_TICKS=0
    zle -F "$fd" _zsuggestion_native_request_ready
    compstate[insert]=''
    compstate[list]=''
  }

  _zsuggestion_menu_apply_result() {
    local -a rendered_highlights
    if (( ${#_ZSUGGESTION_MENU_ACCEPTS} == 0 )); then
      _ZSUGGESTION_MENU_ACTIVE=0
      if (( _ZSUGGESTION_MENU_KEYTIMEOUT >= 0 )); then
        KEYTIMEOUT=$_ZSUGGESTION_MENU_KEYTIMEOUT
        _ZSUGGESTION_MENU_KEYTIMEOUT=-1
      fi
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
        _zsuggestion_menu_render
      else
        POSTDISPLAY=""
        zle -R
      fi
      rendered_highlights=( "${region_highlight[@]}" )
      _ZSUGGESTION_RENDERED_HIGHLIGHTS=( "${rendered_highlights[@]}" )
      _ZSUGGESTION_RESTORE_HIGHLIGHTS=1
      zle -R
      region_highlight=( "${rendered_highlights[@]}" )
      return 0
    fi
    _ZSUGGESTION_MENU_ACTIVE=1
    if (( _ZSUGGESTION_MENU_KEYTIMEOUT < 0 )); then
      _ZSUGGESTION_MENU_KEYTIMEOUT=$KEYTIMEOUT
    fi
    (( KEYTIMEOUT > 2 )) && KEYTIMEOUT=2
    _ZSUGGESTION_MENU_INDEX=$_ZSUGGESTION_MENU_RESTORE_INDEX
    (( _ZSUGGESTION_MENU_INDEX > ${#_ZSUGGESTION_MENU_DISPLAYS} )) && _ZSUGGESTION_MENU_INDEX=${#_ZSUGGESTION_MENU_DISPLAYS}
    (( _ZSUGGESTION_MENU_INDEX < 1 )) && _ZSUGGESTION_MENU_INDEX=1
    _ZSUGGESTION_MENU_RESTORE_INDEX=1
    # Async results can land mid-navigation: keep the viewport where the user
    # left it instead of snapping the selection to the top of the window.
    (( _ZSUGGESTION_MENU_INDEX == 1 )) && _ZSUGGESTION_MENU_START=1
    _ZSUGGESTION_MENU_BUFFER="$_ZSUGGESTION_MENU_REQUEST_BUFFER"
    region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
    _zsuggestion_menu_render
    rendered_highlights=( "${region_highlight[@]}" )
    _ZSUGGESTION_RENDERED_HIGHLIGHTS=( "${rendered_highlights[@]}" )
    _ZSUGGESTION_RESTORE_HIGHLIGHTS=1
    zle -R
    region_highlight=( "${rendered_highlights[@]}" )
  }

  _zsuggestion_menu_start_ticker() {
    (( _ZSUGGESTION_MENU_TICK_FD >= 0 )) && return 0
    local fd
    exec {fd}< <(
      if (( _ZSUGGESTION_HAS_ZSELECT )); then
        while true; do
          zselect -t 3
          print -r -- tick || exit
        done
      else
        while sleep 0.03; do
          print -r -- tick || exit
        done
      fi
    )
    _ZSUGGESTION_MENU_TICK_FD="$fd"
    zle -F "$fd" _zsuggestion_menu_tick
  }

  _zsuggestion_menu_stop_ticker() {
    local fd="$_ZSUGGESTION_MENU_TICK_FD"
    if (( fd >= 0 )); then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_MENU_TICK_FD=-1
    fi
  }

  _zsuggestion_menu_idle_stop() {
    (( _ZSUGGESTION_MENU_TICK_FD >= 0 )) || return 0
    [[ -n "$_ZSUGGESTION_PREVIEW_PATH" || -n "$_ZSUGGESTION_PREVIEW_COMMAND" ]] && return 0
    (( _ZSUGGESTION_NATIVE_REQUEST_FD >= 0 )) && return 0
    if (( ! _ZSUGGESTION_NATIVE_REQUESTED )) && [[ -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      return 0
    fi
    (( _ZSUGGESTION_MENU_REQUEST_DIRTY )) && return 0
    (( _ZSUGGESTION_MENU_REFRESH_TICKS > 0 )) && return 0
    _zsuggestion_menu_stop_ticker
  }

  _zsuggestion_menu_tick() {
    local fd="$1" tick query_fd buffer cwd cursor reply
    local -a alias_args
    (( fd == _ZSUGGESTION_MENU_TICK_FD )) || return 0
    if ! IFS= read -r -u "$fd" tick; then
      zle -F "$fd" 2>/dev/null
      exec {fd}<&-
      _ZSUGGESTION_MENU_TICK_FD=-1
      return 0
    fi
    (( _ZSUGGESTION_IN_NATIVE_COMPLETION )) && return 0
    if [[ -n "$_ZSUGGESTION_PREVIEW_PATH" || -n "$_ZSUGGESTION_PREVIEW_COMMAND" ]]; then
      (( _ZSUGGESTION_PREVIEW_TICKS++ ))
      if (( _ZSUGGESTION_PREVIEW_FD < 0 && _ZSUGGESTION_PREVIEW_TICKS >= 2 )); then
        local preview_fd preview_target="$_ZSUGGESTION_PREVIEW_TARGET"
        # Emit everything at once: the handler reads to EOF, so output that
        # trickled in would block ZLE until the preview command finished.
        exec {preview_fd}< <(
          local preview_output
          if [[ -n "$_ZSUGGESTION_PREVIEW_COMMAND" ]]; then
            preview_output="$(command zsuggestion preview-command \
              --line "$_ZSUGGESTION_PREVIEW_COMMAND" \
              --cwd "$PWD" 2>/dev/null)"
          else
            preview_output="$(command zsuggestion preview-file \
              --path "$_ZSUGGESTION_PREVIEW_PATH" \
              --cwd "$PWD" 2>/dev/null)"
          fi
          print -rn -- "$preview_target"$'\0'"$preview_output"
        )
        _ZSUGGESTION_PREVIEW_FD="$preview_fd"
        _ZSUGGESTION_PREVIEW_PID=$!
        _ZSUGGESTION_PREVIEW_TICKS=0
        zle -F "$preview_fd" _zsuggestion_preview_ready
      elif (( _ZSUGGESTION_PREVIEW_FD >= 0 && _ZSUGGESTION_PREVIEW_TICKS > 34 )); then
        _zsuggestion_preview_cancel
        _ZSUGGESTION_PREVIEW_PATH=""
        _ZSUGGESTION_PREVIEW_COMMAND=""
        _ZSUGGESTION_PREVIEW_LINES=("Preview timed out")
        _ZSUGGESTION_PREVIEW_STYLES=("")
        zle -R
      fi
    fi
    if (( _ZSUGGESTION_NATIVE_REQUEST_FD >= 0 )); then
      (( _ZSUGGESTION_NATIVE_REQUEST_TICKS++ ))
      if (( _ZSUGGESTION_NATIVE_REQUEST_TICKS > 50 )); then
        _zsuggestion_native_cancel
        _zsuggestion_menu_publish
      fi
    fi
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE && ! _ZSUGGESTION_NATIVE_REQUESTED )) &&
       [[ -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      (( _ZSUGGESTION_NATIVE_START_TICKS++ ))
      if (( _ZSUGGESTION_NATIVE_START_TICKS >= 2 )) && (( $+functions[_main_complete] )); then
        _ZSUGGESTION_NATIVE_REQUESTED=1
        _ZSUGGESTION_NATIVE_INFLIGHT_BUFFER="$_ZSUGGESTION_MENU_REQUEST_BUFFER"
        _ZSUGGESTION_NATIVE_INFLIGHT_CURSOR=$_ZSUGGESTION_MENU_REQUEST_CURSOR
        _ZSUGGESTION_NATIVE_INFLIGHT_GENERATION=$_ZSUGGESTION_MENU_GENERATION
        _zsuggestion_call_native_completion zsuggestion-native-capture
      fi
    fi
    if (( ! _ZSUGGESTION_MENU_REQUEST_DIRTY )); then
      if (( _ZSUGGESTION_MENU_REFRESH_TICKS > 0 )); then
        (( _ZSUGGESTION_MENU_REFRESH_TICKS-- ))
        (( _ZSUGGESTION_MENU_REFRESH_TICKS == 0 )) || return 0
        _ZSUGGESTION_MENU_REQUEST_DIRTY=1
      else
        _zsuggestion_menu_idle_stop
        return 0
      fi
    fi

    _ZSUGGESTION_MENU_REQUEST_DIRTY=0
    buffer="$_ZSUGGESTION_MENU_REQUEST_BUFFER"
    cwd="$_ZSUGGESTION_MENU_REQUEST_CWD"
    cursor="$_ZSUGGESTION_MENU_REQUEST_CURSOR"
    if [[ -z "$buffer" ]]; then
      return 0
    fi

    _zsuggestion_menu_cancel_query
    _ZSUGGESTION_MENU_INFLIGHT_BUFFER="$buffer"
    _ZSUGGESTION_MENU_INFLIGHT_CURSOR=$cursor
    _ZSUGGESTION_MENU_INFLIGHT_GENERATION=$_ZSUGGESTION_MENU_GENERATION
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      exec {query_fd}< <(command zsuggestion fuzzy \
        --query "$_ZSUGGESTION_FUZZY_QUERY" \
        --cwd "$cwd" \
        --format zsh-v3 2>/dev/null)
    else
      _zsuggestion_alias_args "$buffer"
      alias_args=("${reply[@]}")
      exec {query_fd}< <(print -rn -- "$buffer" | command zsuggestion complete \
        --stdin \
        --cursor "$cursor" \
        --cwd "$cwd" \
        "${alias_args[@]}" \
        --format zsh-v3 2>/dev/null)
    fi
    _ZSUGGESTION_MENU_REQUEST_FD="$query_fd"
    zle -F "$query_fd" _zsuggestion_menu_request_ready
    _zsuggestion_menu_idle_stop
    return 0
  }

  _zsuggestion_menu_line_init() {
    _zsuggestion_measure_prompt
    _ZSUGGESTION_FUZZY_ACTIVE=0
    _ZSUGGESTION_FUZZY_BASE=""
    _ZSUGGESTION_FUZZY_QUERY=""
    POSTDISPLAY=""
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    _zsuggestion_menu_start_ticker
  }

  _zsuggestion_menu_line_finish() {
    _ZSUGGESTION_PROMPT_LATE=0
    _zsuggestion_menu_clear
    _zsuggestion_menu_stop_ticker
  }

  _zsuggestion_self_insert() {
    if (( _ZSUGGESTION_IN_BRACKETED_PASTE )); then
      zle _zsuggestion-native-self-insert
      return
    fi
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      _ZSUGGESTION_FUZZY_QUERY+="$KEYS"
      _zsuggestion_fuzzy_refresh
      return
    fi
    zle _zsuggestion-native-self-insert
    _zsuggestion_menu_refresh
  }

  # zsh-autosuggestions style: Right, End, Ctrl-F, and Ctrl-E accept the ghost
  # text of the highlighted row; Alt-F and Ctrl-Right accept its next word.
  # Elsewhere each key keeps the widget it had before zsuggestion loaded.
  _zsuggestion_ghost_text() {
    REPLY=""
    (( _ZSUGGESTION_MENU_ACTIVE && ! _ZSUGGESTION_FUZZY_ACTIVE && CURSOR == ${#BUFFER} )) || return 1
    REPLY="${POSTDISPLAY%%$'\n'*}"
    [[ -n "$REPLY" ]]
  }

  _zsuggestion_accept_ghost() {
    if _zsuggestion_ghost_text; then
      _zsuggestion_menu_accept
      return
    fi
    _zsuggestion_accept_fallback .forward-char
  }

  _zsuggestion_accept_ghost_word() {
    local ghost word
    if _zsuggestion_ghost_text; then
      ghost="$REPLY"
      word="${ghost%%[^[:space:]]*}"
      ghost="${ghost[${#word}+1,-1]}"
      word+="${ghost%%[[:space:]]*}"
      if [[ -n "$word" ]]; then
        LBUFFER+="$word"
        _zsuggestion_menu_refresh
        return
      fi
    fi
    _zsuggestion_accept_fallback .forward-word
  }

  _zsuggestion_space() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      _ZSUGGESTION_FUZZY_QUERY+=" "
      _zsuggestion_fuzzy_refresh
      return
    fi
    if [[ "$LBUFFER" == *" " ]] && (( CURSOR == ${#BUFFER} )); then
      LBUFFER="${LBUFFER% }"
      _zsuggestion_fuzzy_start
      return
    fi
    zle _zsuggestion-native-space
    _zsuggestion_menu_refresh
  }

  _zsuggestion_backward_delete() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      if [[ -n "$_ZSUGGESTION_FUZZY_QUERY" ]]; then
        _ZSUGGESTION_FUZZY_QUERY="${_ZSUGGESTION_FUZZY_QUERY[1,-2]}"
        _zsuggestion_fuzzy_refresh
      else
        _zsuggestion_menu_clear
        zle _zsuggestion-native-backward-delete
        _zsuggestion_menu_refresh
      fi
      return
    fi
    zle _zsuggestion-native-backward-delete
    _zsuggestion_menu_refresh
  }

  _zsuggestion_bracketed_paste() {
    _zsuggestion_menu_clear 1
    POSTDISPLAY=""
    _ZSUGGESTION_IN_BRACKETED_PASTE=1
    {
      zle _zsuggestion-native-bracketed-paste
    } always {
      _ZSUGGESTION_IN_BRACKETED_PASTE=0
    }
    _zsuggestion_menu_refresh
  }

  _zsuggestion_interrupt() {
    _zsuggestion_menu_clear 1
    POSTDISPLAY=""
    _ZSUGGESTION_FUZZY_ACTIVE=0
    _ZSUGGESTION_FUZZY_BASE=""
    _ZSUGGESTION_FUZZY_QUERY=""
    _ZSUGGESTION_MENU_REQUEST_BUFFER=""
    zle _zsuggestion-native-interrupt
  }

  _zsuggestion_tab() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      if (( _ZSUGGESTION_MENU_ACTIVE )); then
        _zsuggestion_menu_accept
      else
        zle beep
      fi
      return
    fi
    if (( _ZSUGGESTION_MENU_ACTIVE && CURSOR == ${#BUFFER} )); then
      _zsuggestion_menu_accept
      return
    fi
    _zsuggestion_menu_clear
    POSTDISPLAY=""
    _zsuggestion_call_native_completion _zsuggestion-native-tab
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
  }

  _zsuggestion_shift_tab() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      (( _ZSUGGESTION_MENU_INDEX > 1 )) && (( _ZSUGGESTION_MENU_INDEX-- ))
      _zsuggestion_menu_render
      zle -R
      return
    fi
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      zle beep
      return
    fi
    _zsuggestion_menu_clear
    POSTDISPLAY=""
    _zsuggestion_call_native_completion _zsuggestion-native-shift-tab
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
  }

  _zsuggestion_complete() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE && ! _ZSUGGESTION_MENU_ACTIVE )); then
      zle beep
      return
    fi
    if (( _ZSUGGESTION_MENU_ACTIVE && CURSOR == ${#BUFFER} )); then
      _zsuggestion_menu_accept
    elif (( CURSOR == ${#BUFFER} )) && _zsuggestion_menu_query; then
      _zsuggestion_menu_accept
    else
      (( _ZSUGGESTION_MENU_ACTIVE )) && _zsuggestion_menu_clear 1
      _zsuggestion_call_native_completion _zsuggestion-native-trigger
    fi
  }

  _zsuggestion_fuzzy_execute() {
    if (( ! _ZSUGGESTION_FUZZY_ACTIVE || ! _ZSUGGESTION_MENU_ACTIVE )); then
      zle beep
      return
    fi
    _zsuggestion_menu_accept || return
    zle _zsuggestion-native-enter
  }

  _zsuggestion_menu_down() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      (( _ZSUGGESTION_MENU_INDEX < ${#_ZSUGGESTION_MENU_ACCEPTS} )) && (( _ZSUGGESTION_MENU_INDEX++ ))
      _zsuggestion_menu_render
      zle -R
    elif (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      zle beep
    else
      zle _zsuggestion-native-down
      _zsuggestion_menu_refresh
    fi
  }

  _zsuggestion_menu_up() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      (( _ZSUGGESTION_MENU_INDEX > 1 )) && (( _ZSUGGESTION_MENU_INDEX-- ))
      _zsuggestion_menu_render
      zle -R
    elif (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      zle beep
    else
      zle _zsuggestion-native-up
      _zsuggestion_menu_refresh
    fi
  }

  _zsuggestion_escape() {
    if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
      local base="$_ZSUGGESTION_FUZZY_BASE"
      _zsuggestion_menu_clear
      BUFFER="$base"
      CURSOR=${#BUFFER}
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      [[ "$BUFFER" == *[![:space:]]* ]] && _zsuggestion_menu_schedule
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      zle -R
      return
    fi
    if (( _ZSUGGESTION_MENU_ACTIVE || _ZSUGGESTION_MENU_OWNS_DISPLAY )); then
      _zsuggestion_menu_clear 1
      POSTDISPLAY=""
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      zle -R
      return
    fi
    zle _zsuggestion-native-escape
  }

  _zsuggestion_history_move() {
    local native_widget="$1"
    _zsuggestion_menu_clear 1
    POSTDISPLAY=""
    zle "$native_widget"
    _ZSUGGESTION_MENU_BUFFER="$BUFFER"
    zle -R
  }

  _zsuggestion_history_up() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-up
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-up
  }

  _zsuggestion_history_up_application() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-up
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-up-application
  }

  _zsuggestion_history_down() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-down
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-down
  }

  _zsuggestion_history_down_application() {
    if (( _ZSUGGESTION_MENU_ACTIVE )); then
      zle zsuggestion-menu-down
      return
    fi
    _zsuggestion_history_move _zsuggestion-native-history-down-application
  }

  _zsuggestion_menu_pre_redraw() {
    _zsuggestion_strip_late_prompt_reply
    if (( _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS )); then
      _ZSUGGESTION_FOREIGN_HIGHLIGHTS=( "${(@)region_highlight:#*memo=zsuggestion*}" )
      _ZSUGGESTION_CAPTURE_FOREIGN_HIGHLIGHTS=0
    elif (( _ZSUGGESTION_MENU_ACTIVE || _ZSUGGESTION_FUZZY_ACTIVE )) &&
         [[ "$BUFFER" == "$_ZSUGGESTION_MENU_BUFFER" ]]; then
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
    elif (( _ZSUGGESTION_RESTORE_HIGHLIGHTS )); then
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
      _ZSUGGESTION_RESTORE_HIGHLIGHTS=0
    elif [[ -n "$BUFFER" ]]; then
      _ZSUGGESTION_FOREIGN_HIGHLIGHTS=( "${(@)region_highlight:#*memo=zsuggestion*}" )
    elif [[ -n "$_ZSUGGESTION_MENU_REQUEST_BUFFER" ]]; then
      region_highlight=( "${_ZSUGGESTION_FOREIGN_HIGHLIGHTS[@]}" )
    else
      _ZSUGGESTION_FOREIGN_HIGHLIGHTS=()
    fi
    if [[ "$BUFFER" != "$_ZSUGGESTION_MENU_BUFFER" ]]; then
      _zsuggestion_menu_clear
      _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      [[ -n "$BUFFER" ]] && (( CURSOR == ${#BUFFER} )) && _zsuggestion_menu_schedule
    elif (( CURSOR != ${#BUFFER} )); then
      if (( _ZSUGGESTION_FUZZY_ACTIVE )); then
        _zsuggestion_menu_clear
        _ZSUGGESTION_MENU_BUFFER="$BUFFER"
      else
        _zsuggestion_menu_cancel_request
      fi
    fi
    (( _ZSUGGESTION_MENU_ACTIVE || _ZSUGGESTION_FUZZY_ACTIVE )) && _zsuggestion_menu_render
  }

  autoload -Uz add-zsh-hook
  autoload -Uz add-zle-hook-widget
  # add-zle-hook-widget silently does nothing until zsh/zle is loaded, which
  # otherwise only happens if something earlier in .zshrc defined a widget.
  zmodload zsh/zle 2>/dev/null
  zmodload zsh/zselect 2>/dev/null && _ZSUGGESTION_HAS_ZSELECT=1
  if (( $+functions[_zsh_autosuggest_bind_widgets] )); then
    typeset -g ZSH_AUTOSUGGEST_MANUAL_REBIND=1
    precmd_functions=(${precmd_functions:#_zsh_autosuggest_start})
  fi
  add-zsh-hook preexec _zsuggestion_preexec
  add-zsh-hook precmd _zsuggestion_precmd
  add-zsh-hook precmd _zsuggestion_ensure_compsys
  add-zle-hook-widget line-init _zsuggestion_menu_line_init
  add-zle-hook-widget line-finish _zsuggestion_menu_line_finish
  add-zle-hook-widget line-pre-redraw _zsuggestion_menu_pre_redraw
  preexec_functions=(_zsuggestion_preexec ${preexec_functions:#_zsuggestion_preexec})
  precmd_functions=(_zsuggestion_precmd ${precmd_functions:#_zsuggestion_precmd})

  if [[ -z "${widgets[_zsuggestion-native-self-insert]:-}" ]]; then
    typeset -g _ZSUGGESTION_PREVIOUS_TRIGGER="${$(bindkey '__ZSUGGESTION_COMPLETION_KEY__')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_TRIGGER" || "$_ZSUGGESTION_PREVIOUS_TRIGGER" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_TRIGGER=set-mark-command
    typeset -g _ZSUGGESTION_PREVIOUS_DOWN="${$(bindkey '^N')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_UP="${$(bindkey '^K')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_SHIFT_TAB="${$(bindkey '^[[Z')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_ESCAPE="${$(bindkey '^[')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_ENTER="${$(bindkey '^M')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_UP="${$(bindkey '^[[A')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION="${$(bindkey '^[OA')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_DOWN="${$(bindkey '^[[B')##* }"
    typeset -g _ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION="${$(bindkey '^[OB')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_DOWN" || "$_ZSUGGESTION_PREVIOUS_DOWN" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_DOWN=down-line-or-history
    [[ -z "$_ZSUGGESTION_PREVIOUS_UP" || "$_ZSUGGESTION_PREVIOUS_UP" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_UP=kill-line
    [[ -z "$_ZSUGGESTION_PREVIOUS_SHIFT_TAB" || "$_ZSUGGESTION_PREVIOUS_SHIFT_TAB" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_SHIFT_TAB=reverse-menu-complete
    [[ -z "$_ZSUGGESTION_PREVIOUS_ESCAPE" ]] && _ZSUGGESTION_PREVIOUS_ESCAPE=undefined-key
    [[ -z "$_ZSUGGESTION_PREVIOUS_ENTER" || "$_ZSUGGESTION_PREVIOUS_ENTER" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_ENTER=accept-line
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_UP" || "$_ZSUGGESTION_PREVIOUS_HISTORY_UP" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_UP=up-line-or-history
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION" ||
       "$_ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION=$_ZSUGGESTION_PREVIOUS_HISTORY_UP
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN" || "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_DOWN=down-line-or-history
    [[ -z "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION" ||
       "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION=$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN
    zle -A "$_ZSUGGESTION_PREVIOUS_TRIGGER" _zsuggestion-native-trigger
    zle -A "$_ZSUGGESTION_PREVIOUS_DOWN" _zsuggestion-native-down
    zle -A "$_ZSUGGESTION_PREVIOUS_UP" _zsuggestion-native-up
    zle -A "$_ZSUGGESTION_PREVIOUS_SHIFT_TAB" _zsuggestion-native-shift-tab
    zle -A "$_ZSUGGESTION_PREVIOUS_ESCAPE" _zsuggestion-native-escape
    zle -A "$_ZSUGGESTION_PREVIOUS_ENTER" _zsuggestion-native-enter
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_UP" _zsuggestion-native-history-up
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_UP_APPLICATION" _zsuggestion-native-history-up-application
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN" _zsuggestion-native-history-down
    zle -A "$_ZSUGGESTION_PREVIOUS_HISTORY_DOWN_APPLICATION" _zsuggestion-native-history-down-application
    zle -A self-insert _zsuggestion-native-self-insert
    typeset -g _ZSUGGESTION_PREVIOUS_SPACE="${$(bindkey ' ')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_SPACE" || "$_ZSUGGESTION_PREVIOUS_SPACE" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_SPACE=self-insert
    zle -A "$_ZSUGGESTION_PREVIOUS_SPACE" _zsuggestion-native-space
    zle -A backward-delete-char _zsuggestion-native-backward-delete
    zle -A bracketed-paste _zsuggestion-native-bracketed-paste
    typeset -g _ZSUGGESTION_PREVIOUS_INTERRUPT="${$(bindkey '^C')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_INTERRUPT" || "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_INTERRUPT=send-break
    zle -A "$_ZSUGGESTION_PREVIOUS_INTERRUPT" _zsuggestion-native-interrupt
    typeset -g _ZSUGGESTION_PREVIOUS_TAB="${$(bindkey '^I')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_TAB" || "$_ZSUGGESTION_PREVIOUS_TAB" == "undefined-key" ]] && \
      _ZSUGGESTION_PREVIOUS_TAB=expand-or-complete
    zle -A "$_ZSUGGESTION_PREVIOUS_TAB" _zsuggestion-native-tab
  fi
  if [[ -z "${widgets[_zsuggestion-native-space]:-}" ]]; then
    if [[ -z "${_ZSUGGESTION_PREVIOUS_SPACE:-}" || "$_ZSUGGESTION_PREVIOUS_SPACE" == zsuggestion-space ]]; then
      typeset -g _ZSUGGESTION_PREVIOUS_SPACE="${$(bindkey ' ')##* }"
      if [[ -z "$_ZSUGGESTION_PREVIOUS_SPACE" || "$_ZSUGGESTION_PREVIOUS_SPACE" == "undefined-key" ||
            "$_ZSUGGESTION_PREVIOUS_SPACE" == zsuggestion-space ]]; then
        if [[ -n "${widgets[magic-space]:-}" ]]; then
          _ZSUGGESTION_PREVIOUS_SPACE=magic-space
        else
          _ZSUGGESTION_PREVIOUS_SPACE=self-insert
        fi
      fi
    fi
    zle -A "$_ZSUGGESTION_PREVIOUS_SPACE" _zsuggestion-native-space
  fi
  if [[ -z "${widgets[_zsuggestion-native-escape]:-}" ]]; then
    if [[ -z "${_ZSUGGESTION_PREVIOUS_ESCAPE:-}" || "$_ZSUGGESTION_PREVIOUS_ESCAPE" == zsuggestion-escape ]]; then
      typeset -g _ZSUGGESTION_PREVIOUS_ESCAPE="${$(bindkey '^[')##* }"
      [[ -z "$_ZSUGGESTION_PREVIOUS_ESCAPE" || "$_ZSUGGESTION_PREVIOUS_ESCAPE" == zsuggestion-escape ]] && \
        _ZSUGGESTION_PREVIOUS_ESCAPE=undefined-key
    fi
    zle -A "$_ZSUGGESTION_PREVIOUS_ESCAPE" _zsuggestion-native-escape
  fi
  if [[ -z "${widgets[_zsuggestion-native-interrupt]:-}" ]]; then
    if [[ -z "${_ZSUGGESTION_PREVIOUS_INTERRUPT:-}" || "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == zsuggestion-interrupt ]]; then
      typeset -g _ZSUGGESTION_PREVIOUS_INTERRUPT="${$(bindkey '^C')##* }"
      [[ -z "$_ZSUGGESTION_PREVIOUS_INTERRUPT" || "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == "undefined-key" ||
            "$_ZSUGGESTION_PREVIOUS_INTERRUPT" == zsuggestion-interrupt ]] && \
        _ZSUGGESTION_PREVIOUS_INTERRUPT=send-break
    fi
    zle -A "$_ZSUGGESTION_PREVIOUS_INTERRUPT" _zsuggestion-native-interrupt
  fi
  if [[ -z "${widgets[_zsuggestion-native-enter]:-}" ]]; then
    typeset -g _ZSUGGESTION_PREVIOUS_ENTER="${$(bindkey '^M')##* }"
    [[ -z "$_ZSUGGESTION_PREVIOUS_ENTER" || "$_ZSUGGESTION_PREVIOUS_ENTER" == "undefined-key" ||
          "$_ZSUGGESTION_PREVIOUS_ENTER" == zsuggestion-fuzzy-execute ]] && \
      _ZSUGGESTION_PREVIOUS_ENTER=accept-line
    zle -A "$_ZSUGGESTION_PREVIOUS_ENTER" _zsuggestion-native-enter
  fi
  typeset -gA _ZSUGGESTION_ACCEPT_FALLBACK _ZSUGGESTION_ACCEPT_FALLBACK_STRING
  () {
    # Read the current bindings once. "bindkey -s" string bindings are
    # replayed as input, since they have no widget to call.
    local line key default
    local -a words
    local -A bound bound_string
    local -A defaults=(
      '^[[C' forward-char '^[OC' forward-char '^F' forward-char
      '^E' end-of-line '^[[F' end-of-line '^[OF' end-of-line
      '^[f' forward-word '^[[1;5C' forward-word
    )
    for line in "${(@f)$(bindkey -L 2>/dev/null)}"; do
      words=( "${(@Q)${(z)line}}" )
      [[ "${words[1]}" == bindkey ]] || continue
      if [[ "${words[2]}" == -s ]]; then
        (( ${+defaults[${words[3]}]} )) && bound_string[${words[3]}]="${words[4]}"
      elif [[ "${words[2]}" != -* ]]; then
        (( ${+defaults[${words[2]}]} )) && bound[${words[2]}]="${words[3]}"
      fi
    done
    for key default in "${(@kv)defaults}"; do
      [[ -n "${_ZSUGGESTION_ACCEPT_FALLBACK[${(g:c:)key}]:-}${_ZSUGGESTION_ACCEPT_FALLBACK_STRING[${(g:c:)key}]:-}" ]] &&
        continue
      if (( ${+bound_string[$key]} )); then
        _ZSUGGESTION_ACCEPT_FALLBACK_STRING[${(g:c:)key}]="${(g:c:)bound_string[$key]}"
        continue
      fi
      line="${bound[$key]:-}"
      [[ -z "$line" || "$line" == undefined-key || "$line" == zsuggestion-accept-ghost* ]] &&
        line="$default"
      _ZSUGGESTION_ACCEPT_FALLBACK[${(g:c:)key}]="$line"
    done
  }

  _zsuggestion_accept_fallback() {
    if (( ${+_ZSUGGESTION_ACCEPT_FALLBACK_STRING[$KEYS]} )); then
      zle -U -- "${_ZSUGGESTION_ACCEPT_FALLBACK_STRING[$KEYS]}"
    else
      zle "${_ZSUGGESTION_ACCEPT_FALLBACK[$KEYS]:-$1}"
    fi
  }
  zle -N zsuggestion-accept-ghost _zsuggestion_accept_ghost
  zle -N zsuggestion-accept-ghost-word _zsuggestion_accept_ghost_word
  bindkey '^[[C' zsuggestion-accept-ghost
  bindkey '^[OC' zsuggestion-accept-ghost
  bindkey '^F' zsuggestion-accept-ghost
  bindkey '^E' zsuggestion-accept-ghost
  bindkey '^[[F' zsuggestion-accept-ghost
  bindkey '^[OF' zsuggestion-accept-ghost
  bindkey '^[f' zsuggestion-accept-ghost-word
  bindkey '^[[1;5C' zsuggestion-accept-ghost-word
  zle -N zsuggestion-tab _zsuggestion_tab
  bindkey '^I' zsuggestion-tab
  zle -N zsuggestion-complete _zsuggestion_complete
  zle -N zsuggestion-fuzzy-execute _zsuggestion_fuzzy_execute
  zle -N zsuggestion-menu-down _zsuggestion_menu_down
  zle -N zsuggestion-menu-up _zsuggestion_menu_up
  zle -N zsuggestion-shift-tab _zsuggestion_shift_tab
  zle -N zsuggestion-escape _zsuggestion_escape
  zle -N zsuggestion-history-up _zsuggestion_history_up
  zle -N zsuggestion-history-up-application _zsuggestion_history_up_application
  zle -N zsuggestion-history-down _zsuggestion_history_down
  zle -N zsuggestion-history-down-application _zsuggestion_history_down_application
  zle -N zsuggestion-menu-ready _zsuggestion_menu_request_ready
  zle -N zsuggestion-menu-apply _zsuggestion_menu_apply_result
  zle -N zsuggestion-menu-tick _zsuggestion_menu_tick
  zle -C zsuggestion-native-capture .complete-word _zsuggestion_native_capture_widget
  zle -N self-insert _zsuggestion_self_insert
  zle -N zsuggestion-space _zsuggestion_space
  zle -N backward-delete-char _zsuggestion_backward_delete
  zle -N bracketed-paste _zsuggestion_bracketed_paste
  zle -N zsuggestion-interrupt _zsuggestion_interrupt
  bindkey '__ZSUGGESTION_COMPLETION_KEY__' zsuggestion-complete
  bindkey '^N' zsuggestion-menu-down
  bindkey '^K' zsuggestion-menu-up
  bindkey '^[[Z' zsuggestion-shift-tab
  bindkey '^[' zsuggestion-escape
  bindkey '^[[A' zsuggestion-history-up
  bindkey '^[OA' zsuggestion-history-up-application
  bindkey '^[[B' zsuggestion-history-down
  bindkey '^[OB' zsuggestion-history-down-application
  bindkey '^C' zsuggestion-interrupt
  bindkey ' ' zsuggestion-space
  bindkey -N zsuggestion-fuzzy
  bindkey -M zsuggestion-fuzzy -R ' '-'~' self-insert
  bindkey -M zsuggestion-fuzzy ' ' zsuggestion-space
  bindkey -M zsuggestion-fuzzy '^?' backward-delete-char
  bindkey -M zsuggestion-fuzzy '^H' backward-delete-char
  bindkey -M zsuggestion-fuzzy '^I' zsuggestion-tab
  bindkey -M zsuggestion-fuzzy '^N' zsuggestion-menu-down
  bindkey -M zsuggestion-fuzzy '^K' zsuggestion-menu-up
  bindkey -M zsuggestion-fuzzy '^[[A' zsuggestion-menu-up
  bindkey -M zsuggestion-fuzzy '^[OA' zsuggestion-menu-up
  bindkey -M zsuggestion-fuzzy '^[[B' zsuggestion-menu-down
  bindkey -M zsuggestion-fuzzy '^[OB' zsuggestion-menu-down
  bindkey -M zsuggestion-fuzzy '__ZSUGGESTION_COMPLETION_KEY__' zsuggestion-complete
  bindkey -M zsuggestion-fuzzy '^M' zsuggestion-fuzzy-execute
  bindkey -M zsuggestion-fuzzy '^C' zsuggestion-interrupt
  bindkey -M zsuggestion-fuzzy '^[' zsuggestion-escape

  if [[ -n "${HISTFILE:-}" && -r "$HISTFILE" ]]; then
    command zsuggestion import-history --file "${HISTFILE:A}" >/dev/null 2>&1 &!
  fi
fi
