#!/bin/bash
# Hello Shell: a minimal Manta plugin as a bash script, using jq for JSON.
#
# How it talks to Manta (see docs/plugins.md):
#   * Manta -> plugin: one JSON object per line on stdin
#     (`init`, `directory_changed`, `run_command`, `shutdown`, ...).
#   * plugin -> Manta: one JSON object per line on stdout
#     (`register_commands`, `set_chips`, `toast`, ...).
#   * stderr is the log (Settings › Plugins › "Show log").
#     NEVER echo anything but protocol JSON to stdout: send logs with log().
#
# What it does:
#   * "Say Hello" in the command palette: a toast.
#   * "Count Selection" in the file list's context menu: a toast with the
#     number of selected items.
#   * a chip in the path bar with the number of items in local folders.
#   * texts in English or German, following Manta's language.
#
# Needs jq (/usr/bin/jq on macOS 15+, else `brew install jq`). Written for
# the bash 3.2 that macOS ships: no associative arrays.

# ------------------------------------------------------------ language

LANG_CODE="en"

set_locale() {
  # "de-DE" / "de_DE" -> "de"; anything we don't translate -> English.
  case "$1" in
    de | de-* | de_*) LANG_CODE="de" ;;
    *) LANG_CODE="en" ;;
  esac
}

# text KEY [NUMBER-OR-NAME]: the text for KEY in the current language.
text() {
  case "$LANG_CODE:$1" in
    de:say_hello) echo "Hallo sagen" ;;
    de:count_selection) echo "Auswahl zählen" ;;
    de:hello) echo "Hallo aus der Shell! Du bist in „$2“." ;;
    de:selected) if [ "$2" = 1 ]; then echo "1 Objekt ausgewählt"; else echo "$2 Objekte ausgewählt"; fi ;;
    de:items) if [ "$2" = 1 ]; then echo "1 Objekt"; else echo "$2 Objekte"; fi ;;
    de:items_tooltip) echo "Objekte in diesem Ordner, gezählt von Hello Shell" ;;
    *:say_hello) echo "Say Hello" ;;
    *:count_selection) echo "Count Selection" ;;
    *:hello) echo "Hello from the shell! You're in “$2”." ;;
    *:selected) if [ "$2" = 1 ]; then echo "1 item selected"; else echo "$2 items selected"; fi ;;
    *:items) if [ "$2" = 1 ]; then echo "1 item"; else echo "$2 items"; fi ;;
    *:items_tooltip) echo "Items in this folder, counted by Hello Shell" ;;
  esac
}

# ------------------------------------------------------------ talking

# Every message is built by `jq -nc`: one compact line, and values passed
# with --arg/--argjson are quoted correctly whatever they contain. jq
# writes the line and exits, so each message reaches Manta immediately.

log() { echo "$*" >&2; } # stderr: Settings › Plugins › Show log

# The last state of every pane, as one JSON object {"left": {...}, ...},
# so we can re-send the chips in the new language after `locale_changed`.
PANES='{}'

register_commands() {
  # "file_context_menu": the palette and the file list's right-click menu.
  jq -nc --arg hello "$(text say_hello)" --arg count "$(text count_selection)" \
    '{type: "register_commands", commands: [
        {id: "say-hello", title: $hello, context: "palette"},
        {id: "count-selection", title: $count, context: "file_context_menu"}]}'
}

# update_chips PANE_STATE_JSON: the item-count chip for one pane.
update_chips() {
  local state="$1" local_path n
  PANES=$(jq -c --argjson p "$state" '.[$p.pane | tostring] = $p' <<<"$PANES")
  # `local_path` is null for SFTP/FTP/S3/... folders: we can't read those.
  local_path=$(jq -r '.local_path // empty' <<<"$state")
  [ -n "$local_path" ] || return 0
  if [ ! -r "$local_path" ]; then
    log "can't list $local_path"
    return 0
  fi
  # Count entries with a glob (dotglob: hidden ones too; nullglob: an
  # empty folder gives 0). Safe for any file name, even with newlines.
  n=$(shopt -s nullglob dotglob; set -- "$local_path"/*; echo $#)
  # `dir` must be the folder the pane shows (`path`); `pane` is "left",
  # "right" or a number, so pass it through unchanged with --argjson.
  jq -nc --argjson s "$state" --arg text "$(text items "$n")" --arg tip "$(text items_tooltip)" \
    '{type: "set_chips", pane: $s.pane, dir: $s.path,
      chips: [{text: $text, icon: "folder", tone: "neutral", tooltip: $tip}]}'
}

toast() { # toast LEVEL TEXT
  jq -nc --arg level "$1" --arg text "$2" '{type: "toast", level: $level, text: $text}'
}

# ------------------------------------------------------------ events

handle() {
  local msg="$1" type id pane state
  type=$(jq -r '.type // empty' <<<"$msg") || return 0
  case "$type" in
    init) # always the first message
      set_locale "$(jq -r '.locale // "en"' <<<"$msg")"
      log "hello-shell started, data dir $(jq -r .data_dir <<<"$msg"), language $LANG_CODE"
      register_commands
      while IFS= read -r state; do update_chips "$state"; done < <(jq -c '.panes[]?' <<<"$msg")
      ;;
    locale_changed) # re-send everything that has text
      set_locale "$(jq -r .locale <<<"$msg")"
      register_commands
      while IFS= read -r state; do update_chips "$state"; done < <(jq -c '.[]' <<<"$PANES")
      ;;
    directory_changed | listing_refreshed)
      update_chips "$(jq -c .pane_state <<<"$msg")"
      ;;
    pane_closed) # forget it, so we don't send chips for it again
      PANES=$(jq -c --argjson m "$msg" 'del(.[$m.pane | tostring])' <<<"$PANES")
      ;;
    run_command) # `pane` is the active pane's state when the command ran
      id=$(jq -r .id <<<"$msg")
      case "$id" in
        say-hello)
          # The folder's name: last part of the path ("/" for the root).
          pane=$(jq -r '.pane.path | split("/") | map(select(. != "")) | last // "/"' <<<"$msg")
          toast success "$(text hello "$pane")"
          ;;
        count-selection)
          toast info "$(text selected "$(jq '.pane.selection | length' <<<"$msg")")"
          ;;
      esac
      ;;
    shutdown) # exit within 500 ms
      exit 0
      ;;
    # Unknown types: ignore them. Newer Manta versions send more.
  esac
}

if ! command -v jq >/dev/null 2>&1; then
  log "hello-shell needs jq: brew install jq"
  exit 1
fi

# One message per line until Manta closes stdin (EOF).
while IFS= read -r line || [ -n "$line" ]; do
  [ -n "$line" ] && handle "$line"
done
exit 0
