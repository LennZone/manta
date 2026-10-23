#!/usr/bin/env python3
"""Hello Python: an example Manta plugin (standard library only).

How a plugin works
------------------
Manta starts this program and talks to it with JSON lines:

* Manta -> plugin: one JSON object per line on **stdin**
  (`init`, `directory_changed`, `run_command`, ... see docs/plugins.md).
* plugin -> Manta: one JSON object per line on **stdout**
  (`set_decorations`, `set_chips`, `toast`, ...).
* **stderr** is your log. It ends up in Settings -> Plugins -> "Show log"
  and in `<data_dir>/plugin.log`. Never print anything else to stdout!

What this plugin does
---------------------
* a warning badge "L" on files larger than 10 MB,
* a chip in the path bar with the number of items in the folder,
* "Show File Info" in the file list's context menu: opens a side panel
  with details about the selected file,
* "Say Hello" in the command palette (or press Ctrl+Option+H): a toast,
* everything in English or German, following Manta's language.

Copy this folder to start your own plugin.
"""

import json
import os
import stat
import sys
import time

LARGE = 10 * 1024 * 1024  # 10 MB

# --------------------------------------------------------------- language
#
# Manta tells plugins its UI language: `locale` in `init` ("en", "de")
# and `locale_changed` when the user switches it. Keep your texts in a
# table per language and look them up with `_("key", name=value)`. After a
# change, send everything with text again (commands, panels, chips, panel
# content): Manta shows what you sent last.

STRINGS = {
    "en": {
        "cmd.file_info": "Show File Info",
        "cmd.say_hello": "Say Hello",
        "panel.file_info": "File Info",
        "large_file": "Large file: {size}",
        "items.one": "{count} item",
        "items.other": "{count} items",
        "items.tooltip": "Items in this folder, counted by Hello Python",
        "no_selection": "No selection",
        "select_hint": "Select a file to see its details.",
        "local_only": "File info only works for local folders.",
        "kind.folder": "Folder",
        "kind.alias": "Alias",
        "kind.file": "File",
        "key.kind": "Kind",
        "key.size": "Size",
        "key.modified": "Modified",
        "key.permissions": "Permissions",
        "key.path": "Path",
        "key.note": "Note",
        "larger_than": "Larger than 10 MB",
        "more_selected": "and {count} more selected",
        "cant_read": "Can't read it: {error}",
        "refresh": "Refresh",
        "close": "Close",
        "hello": "Hello from Python! You're in “{folder}”.",
        "bytes": "{count} bytes",
    },
    "de": {
        "cmd.file_info": "Dateiinformationen zeigen",
        "cmd.say_hello": "Hallo sagen",
        "panel.file_info": "Dateiinformationen",
        "large_file": "Große Datei: {size}",
        "items.one": "{count} Objekt",
        "items.other": "{count} Objekte",
        "items.tooltip": "Objekte in diesem Ordner, gezählt von Hello Python",
        "no_selection": "Keine Auswahl",
        "select_hint": "Wähle eine Datei aus, um ihre Details zu sehen.",
        "local_only": "Dateiinformationen gibt es nur für lokale Ordner.",
        "kind.folder": "Ordner",
        "kind.alias": "Alias",
        "kind.file": "Datei",
        "key.kind": "Art",
        "key.size": "Größe",
        "key.modified": "Geändert",
        "key.permissions": "Zugriffsrechte",
        "key.path": "Pfad",
        "key.note": "Hinweis",
        "larger_than": "Größer als 10 MB",
        "more_selected": "und {count} weitere ausgewählt",
        "cant_read": "Kann nicht gelesen werden: {error}",
        "refresh": "Aktualisieren",
        "close": "Schließen",
        "hello": "Hallo aus Python! Du bist in „{folder}“.",
        "bytes": "{count} Byte",
    },
}
# German words that are the same in English.
SAME_IN_GERMAN = {"kind.alias"}

lang = "en"


def set_locale(code):
    """Use the best match for a language tag ("de-DE" -> "de"), else English."""
    global lang
    code = str(code or "en").replace("_", "-").split("-")[0].lower()
    lang = code if code in STRINGS else "en"


def _(key, **values):
    """The text for `key` in the current language, `{name}`s filled in."""
    text = STRINGS[lang].get(key) or STRINGS["en"][key]
    for name, value in values.items():
        text = text.replace("{" + name + "}", str(value))
    return text


def plural(key, count):
    """`key.one` for 1, else `key.other`."""
    return _(key + (".one" if count == 1 else ".other"), count=count)


# What we know about the two panes ("left" / "right"), updated from events.
panes = {}
# The folder Manta gave us for settings and state.
data_dir = None
# Whether we showed file info at least once (then we keep it up to date).
info_shown = False


# --------------------------------------------------------------- talking


def send(message):
    """Write one message to Manta: a single JSON line on stdout."""
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()  # Manta reads line by line; don't let it wait


def log(text):
    """Log to stderr (shown in Settings -> Plugins -> Show log)."""
    print(text, file=sys.stderr, flush=True)


# --------------------------------------------------------------- helpers


def human_size(n):
    for unit in ("bytes", "KB", "MB", "GB", "TB"):
        if n < 1000 or unit == "TB":
            if unit == "bytes":
                return _("bytes", count=n)
            text = f"{n:.1f} {unit}"
            return text.replace(".", ",") if lang == "de" else text  # 12,4 MB
        n /= 1000
    return str(n)


def register():
    """Commands and panels with titles in the current language (these
    replace the ones in plugin.toml)."""
    send({"type": "register_commands", "commands": [
        {"id": "file-info", "title": _("cmd.file_info"), "context": "file_context_menu"},
        {"id": "say-hello", "title": _("cmd.say_hello"), "context": "palette", "shortcut": "⌃⌥H"},
    ]})
    send({"type": "register_panels", "panels": [
        {"id": "file-info", "title": _("panel.file_info"), "placement": "right_sidebar", "default_open": False},
    ]})


def update_folder(pane_state):
    """Badges for large files and the item-count chip for one pane."""
    pane = pane_state["pane"]
    folder = pane_state["path"]  # the folder as Manta names it
    local = pane_state.get("local_path")  # real path, or None for SFTP/FTP

    decorations = {}
    chips = []
    if local:
        try:
            entries = list(os.scandir(local))
        except OSError as e:
            log(f"can't list {local}: {e}")
            entries = []
        for entry in entries:
            try:
                size = entry.stat(follow_symlinks=False).st_size
            except OSError:
                continue
            if entry.is_file(follow_symlinks=False) and size > LARGE:
                # Keys are file names in that folder. A decoration can have a
                # badge (1-3 characters), a name tint, `dimmed` and a tooltip.
                decorations[entry.name] = {
                    "badge": {"text": "L", "tone": "warning"},
                    "tooltip": _("large_file", size=human_size(size)),
                }
        n = len(entries)
        chips.append(
            {
                "text": plural("items", n),
                "icon": "info",
                "tone": "neutral",
                "tooltip": _("items.tooltip"),
            }
        )

    # Sending an empty set clears what we showed before. `dir` tells
    # Manta which folder this is for; if the user has moved on in the
    # meantime, Manta drops the message.
    send({"type": "set_decorations", "pane": pane, "dir": folder, "entries": decorations})
    send({"type": "set_chips", "pane": pane, "dir": folder, "chips": chips})


def show_file_info(pane_state):
    """Fill the "file-info" panel with the selected file's details."""
    global info_shown
    info_shown = True
    selection = pane_state.get("selection") or []
    local = pane_state.get("local_path")

    if not selection:
        blocks = [
            {"type": "heading", "text": _("no_selection")},
            {"type": "text", "text": _("select_hint"), "tone": "muted"},
        ]
    elif not local:
        blocks = [
            {"type": "heading", "text": os.path.basename(selection[0])},
            {
                "type": "text",
                "text": _("local_only"),
                "tone": "warning",
            },
        ]
    else:
        name = os.path.basename(selection[0])
        path = os.path.join(local, name)
        try:
            st = os.lstat(path)
            kind = (
                _("kind.folder")
                if stat.S_ISDIR(st.st_mode)
                else _("kind.alias") if stat.S_ISLNK(st.st_mode) else _("kind.file")
            )
            date_format = "%d.%m.%Y %H:%M" if lang == "de" else "%Y-%m-%d %H:%M"
            rows = [
                {"key": _("key.kind"), "value": kind},
                {"key": _("key.size"), "value": human_size(st.st_size)},
                {
                    "key": _("key.modified"),
                    "value": time.strftime(date_format, time.localtime(st.st_mtime)),
                },
                {"key": _("key.permissions"), "value": stat.filemode(st.st_mode)},
                {"key": _("key.path"), "value": path},
            ]
            if st.st_size > LARGE:
                rows.append({"key": _("key.note"), "value": _("larger_than"), "tone": "warning"})
            blocks = [{"type": "heading", "text": name}, {"type": "key_value", "rows": rows}]
            if len(selection) > 1:
                blocks.append(
                    {
                        "type": "text",
                        "text": _("more_selected", count=len(selection) - 1),
                        "tone": "muted",
                    }
                )
        except OSError as e:
            blocks = [
                {"type": "heading", "text": name},
                {"type": "text", "text": _("cant_read", error=e.strerror), "tone": "danger"},
            ]

    # Buttons send a `panel_action` with their `action` back to us.
    blocks.append(
        {
            "type": "buttons",
            "buttons": [
                {"label": _("refresh"), "action": "refresh", "primary": True},
                {"label": _("close"), "action": "close"},
            ],
        }
    )
    send({"type": "set_panel_content", "panel": "file-info", "blocks": blocks})


# --------------------------------------------------------------- events


def handle(message):
    global data_dir
    kind = message.get("type")

    if kind == "init":
        # Always the first message. `panes` holds both panes' state.
        data_dir = message["data_dir"]
        set_locale(message.get("locale"))  # missing in older Manta: English
        log(f"hello-python started, data dir {data_dir}, language {lang}")
        register()
        for p in message.get("panes", []):
            panes[p["pane"]] = p
            update_folder(p)

    elif kind == "locale_changed":
        # The user switched Manta's language: send every text again.
        set_locale(message.get("locale"))
        register()
        for p in panes.values():
            update_folder(p)
            if info_shown and p.get("active"):
                show_file_info(p)

    elif kind in ("directory_changed", "listing_refreshed"):
        # A pane shows another folder, or the same folder was re-listed
        # (after a refresh or a file operation).
        p = message["pane_state"]
        panes[p["pane"]] = p
        update_folder(p)

    elif kind == "selection_changed":
        p = message["pane_state"]
        panes[p["pane"]] = p
        if info_shown and p.get("active"):
            show_file_info(p)

    elif kind == "active_pane_changed":
        for name, p in panes.items():
            p["active"] = name == message["pane"]

    elif kind == "run_command":
        # `pane` is the state of the active pane when the command ran.
        if message["id"] == "say-hello":
            folder = os.path.basename(message["pane"]["path"]) or "/"
            send({"type": "toast", "level": "success", "text": _("hello", folder=folder)})
        elif message["id"] == "file-info":
            show_file_info(message["pane"])
            send({"type": "set_panel_open", "panel": "file-info", "open": True})

    elif kind == "panel_action":
        if message["action"] == "refresh":
            show_file_info(message["pane"])
        elif message["action"] == "close":
            send({"type": "set_panel_open", "panel": "file-info", "open": False})

    elif kind == "shutdown":
        # Switched off or the app quits: exit quickly (within 500 ms).
        sys.exit(0)

    # Unknown message types: ignore them. Newer Manta versions may send
    # messages this plugin doesn't know yet.


def main():
    for line in sys.stdin:  # ends when Manta closes stdin
        line = line.strip()
        if not line:
            continue
        try:
            handle(json.loads(line))
        except Exception as e:  # a bug in one handler shouldn't kill the plugin
            log(f"error handling {line[:200]}: {e!r}")


if __name__ == "__main__":
    main()
