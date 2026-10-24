#!/usr/bin/env python3
"""Test a Manta plugin without Manta.

    python3 scripts/plugin-test.py PLUGIN_DIR                 run the standard session
    python3 scripts/plugin-test.py PLUGIN_DIR --locale de     … with the German UI language
    python3 scripts/plugin-test.py PLUGIN_DIR --session s.jsonl   run your own session
    python3 scripts/plugin-test.py PLUGIN_DIR --manifest-only     only check plugin.toml
    python3 scripts/plugin-test.py --list-types               the message types this checker knows

What it does:

1. Reads and checks PLUGIN_DIR/plugin.toml the way Manta does (required
   fields, id rules, api_version, commands, panels, permissions …).
2. Starts the plugin's `command` with the plugin folder as working
   directory and the same environment variables Manta sets
   (MANTA_PLUGIN_ID, MANTA_PLUGIN_DATA_DIR, MANTA_API_VERSION,
   PYTHONUNBUFFERED=1, PATH with /opt/homebrew/bin and /usr/local/bin).
3. Plays a session against it, like Manta would: `init` with two panes
   showing a temporary test folder, `pane_opened`, `directory_changed`,
   `selection_changed`, a `file_action`, `run_command` for every command
   the plugin declares, `panel_action` for buttons it shows,
   `locale_changed`, and finally `shutdown`.
4. Checks every line the plugin prints against the protocol (api_version 1):
   valid JSON, a known `type`, required fields and their types, enum
   values, limits. It answers `list_dir`, `read_file` and `rename_many`
   like Manta (rename_many is answered but nothing is renamed).
5. Checks that the plugin exits within 500 ms of `shutdown`.

Exit code 0: no errors (warnings are allowed); 1: errors; 2: bad usage.

A custom session is a JSON lines file. Each line is either a host message
(sent as is) or a directive:

    {"type":"run_command","id":"hello","pane":"$PANE_LEFT"}
    {"expect":"toast","timeout":2}        wait for a message of this type (error if none)
    {"expect":"toast","text":"Hello"}     … whose fields contain these values
    {"sleep":0.2}

In strings, $DIR is the test folder, $DATA the plugin's data folder, and
"$PANE_LEFT" / "$PANE_RIGHT" (the whole string) become the pane states.
`init` is sent automatically before the session; `shutdown` after it.

Python 3.9+ standard library only. Keep in sync with
crates/plugin/src/protocol.rs (the Rust tests compare --list-types with it).
"""

import argparse
import base64
import json
import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

API_VERSION = 1
SHUTDOWN_GRACE = 0.5
MAX_LINE = 1024 * 1024
RESERVED = ("manta",)

TONES = {"neutral", "accent", "success", "warning", "danger", "info", "muted"}
TOAST_LEVELS = {"info", "success", "warning", "error"}
LOG_LEVELS = {"debug", "info", "warn", "error"}
STATUS_LEVELS = {"normal", "warn", "error"}
CONTEXTS = {"palette", "file_context_menu"}
PLACEMENTS = {"pane_bottom", "right_sidebar", "modal"}
PERMISSIONS = {"read_files", "write_files", "network", "run_programs", "terminal"}
ICONS = {
    "branch", "terminal", "info", "warning", "check", "cloud", "plug", "file", "folder", "code",
    "image", "archive", "text", "copy", "link", "eye", "key", "lock", "clock", "stop", "error",
    "package", "search", "gear", "pencil", "download", "open", "play", "database", "cube", "stack",
    "hammer", "flask", "rocket", "lightning", "list", "wrench", "bug", "trash", "globe", "refresh",
}
STATUS_ICONS = ICONS | {"cpu", "gpu", "memory"}

HOST_TYPES = [
    "init", "locale_changed", "directory_changed", "listing_refreshed", "selection_changed",
    "active_pane_changed", "pane_opened", "pane_closed", "file_action", "run_command",
    "panel_action", "shutdown", "rename_result", "dir_listing", "file_content",
]
PLUGIN_TYPES = [
    "set_decorations", "set_sizes", "set_chips", "toast", "navigate", "register_commands",
    "register_panels", "set_panel_content", "set_panel_open", "toggle_panel", "rename_many",
    "list_dir", "read_file", "run_in_terminal", "refresh_pane", "select", "set_status_items", "log",
]

# ------------------------------------------------------------------ output

USE_COLOR = sys.stdout.isatty() and not os.environ.get("NO_COLOR")


def color(code, s):
    return f"\033[{code}m{s}\033[0m" if USE_COLOR else s


class Report:
    def __init__(self, verbose):
        self.errors = []
        self.warnings = []
        self.verbose = verbose

    def error(self, msg):
        # The same problem on every message is reported once.
        if msg not in self.errors:
            print(color("31", "  error: ") + msg)
        self.errors.append(msg)

    def warn(self, msg):
        if msg not in self.warnings:
            print(color("33", "  warning: ") + msg)
        self.warnings.append(msg)

    def info(self, msg):
        print("  " + msg)

    def debug(self, msg):
        if self.verbose:
            print(color("2", "  " + msg))


# ------------------------------------------------------------------ TOML

def load_toml(text):
    """tomllib on Python 3.11+, else a small parser for what plugin.toml uses."""
    try:
        import tomllib  # type: ignore
        return tomllib.loads(text)
    except ImportError:
        return _mini_toml(text)


def _mini_toml(text):
    root = {}
    cur = root

    def value(s):
        s = s.strip()
        if s.startswith('"'):
            return json.loads(_take_string(s)[0])
        if s.startswith("'"):
            return s[1:s.index("'", 1)]
        if s.startswith("["):
            items, rest = [], s[1:]
            while True:
                rest = rest.lstrip()
                if rest.startswith("]"):
                    return items
                if rest.startswith('"'):
                    lit, rest = _take_string(rest)
                    items.append(json.loads(lit))
                else:
                    m = re.match(r"[^,\]]+", rest)
                    items.append(value(m.group(0)))
                    rest = rest[m.end():]
                rest = rest.lstrip()
                if rest.startswith(","):
                    rest = rest[1:]
        if s in ("true", "false"):
            return s == "true"
        try:
            return int(s)
        except ValueError:
            return float(s)

    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = _strip_comment(lines[i]).strip()
        i += 1
        if not line:
            continue
        if line.startswith("[["):
            path = line[2:line.index("]]")].strip().split(".")
            t = root
            for p in path[:-1]:
                t = t.setdefault(p, {})
            cur = {}
            t.setdefault(path[-1], []).append(cur)
            continue
        if line.startswith("["):
            path = line[1:line.index("]")].strip().split(".")
            cur = root
            for p in path:
                cur = cur.setdefault(p.strip().strip('"'), {})
            continue
        key, _, rest = line.partition("=")
        # Multi-line arrays.
        while rest.count("[") > rest.count("]") and i < len(lines):
            rest += " " + _strip_comment(lines[i]).strip()
            i += 1
        cur[key.strip().strip('"')] = value(rest)
    return root


def _take_string(s):
    i = 1
    while i < len(s):
        if s[i] == "\\":
            i += 2
            continue
        if s[i] == '"':
            return s[: i + 1], s[i + 1:]
        i += 1
    raise ValueError("unterminated string")


def _strip_comment(line):
    out, in_str, i = [], None, 0
    while i < len(line):
        c = line[i]
        if in_str:
            out.append(c)
            if c == "\\" and in_str == '"' and i + 1 < len(line):
                out.append(line[i + 1])
                i += 1
            elif c == in_str:
                in_str = None
        elif c in "\"'":
            in_str = c
            out.append(c)
        elif c == "#":
            break
        else:
            out.append(c)
        i += 1
    return "".join(out)


# ------------------------------------------------------------------ manifest

ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{1,63}$")
VERSION_RE = re.compile(r"^\d+\.\d+(\.\d+)?([-+].*)?$")


def check_manifest(dir_, r):
    path = os.path.join(dir_, "plugin.toml")
    if not os.path.isfile(path):
        r.error(f"no plugin.toml in {dir_}")
        return None
    try:
        m = load_toml(open(path, encoding="utf-8").read())
    except Exception as e:  # noqa: BLE001
        r.error(f"plugin.toml: {e}")
        return None
    for key in ("id", "name", "version", "api_version", "command"):
        if key not in m:
            r.error(f'plugin.toml: "{key}" is missing')
    if r.errors:
        return None
    pid = m["id"]
    if not isinstance(pid, str) or not ID_RE.match(pid):
        r.error(f'plugin.toml: id "{pid}" must be 2–64 lowercase letters, digits, ".", "-", "_"')
    elif any(pid == p or pid.startswith(p + ".") for p in RESERVED):
        r.error(f'plugin.toml: id "{pid}" is reserved for built-in plugins')
    if not str(m.get("name", "")).strip():
        r.error('plugin.toml: "name" must not be empty')
    if not str(m.get("version", "")).strip():
        r.error('plugin.toml: "version" must not be empty')
    elif not VERSION_RE.match(str(m["version"])):
        r.warn(f'plugin.toml: version "{m["version"]}" isn\'t semver-like (1.2.3); updates compare versions that way')
    if m.get("api_version") != API_VERSION:
        r.error(f"plugin.toml: api_version must be {API_VERSION} (this checker and Manta speak {API_VERSION})")
    cmd = m.get("command")
    if not isinstance(cmd, list) or not cmd or not all(isinstance(c, str) for c in cmd) or not cmd[0].strip():
        r.error('plugin.toml: "command" must be a list like ["python3", "main.py"]')
    for key in ("description", "author"):
        if not str(m.get(key, "")).strip():
            r.warn(f'plugin.toml: no "{key}" (shown in Settings and in the install dialog)')
    for key in ("homepage", "repository"):
        v = m.get(key)
        if v is not None and not (isinstance(v, str) and v.startswith("https://")):
            r.error(f'plugin.toml: "{key}" must be an https:// URL')
    if "license" in m and not isinstance(m["license"], str):
        r.error('plugin.toml: "license" must be a string (an SPDX id like "MIT")')
    mmv = m.get("min_manta_version")
    if mmv is not None and not (isinstance(mmv, str) and re.match(r"^\d+\.\d+(\.\d+)?$", mmv)):
        r.error('plugin.toml: "min_manta_version" must look like "0.4.0"')
    perms = m.get("permissions")
    if perms is not None:
        if not isinstance(perms, list):
            r.error('plugin.toml: "permissions" must be a list')
        else:
            for p in perms:
                if p not in PERMISSIONS:
                    r.error(f'plugin.toml: unknown permission "{p}" (known: {", ".join(sorted(PERMISSIONS))})')
    else:
        r.warn('plugin.toml: no "permissions": the install dialog can\'t tell users what the plugin does with their files')
    for lang, t in (m.get("localized") or {}).items():
        if not isinstance(t, dict):
            r.error(f"plugin.toml: [localized.{lang}] must be a table")
    check_commands(m.get("commands", []), r, "plugin.toml")
    check_panels(m.get("panels", []), r, "plugin.toml")
    known = {"id", "name", "version", "description", "author", "api_version", "command", "capabilities",
             "commands", "panels", "localized", "homepage", "repository", "license", "min_manta_version",
             "permissions"}
    for k in m:
        if k not in known:
            r.warn(f'plugin.toml: unknown key "{k}" (ignored by Manta)')
    return m


def check_commands(cmds, r, where):
    if not isinstance(cmds, list):
        r.error(f"{where}: commands must be a list")
        return
    seen = set()
    for c in cmds:
        if not isinstance(c, dict):
            r.error(f"{where}: a command must be an object")
            continue
        cid = c.get("id")
        if not isinstance(cid, str) or not cid.strip() or ":" in cid or re.search(r"\s", cid):
            r.error(f'{where}: command id {cid!r} must be non-empty without ":" or spaces')
        if cid in seen:
            r.error(f'{where}: command id "{cid}" is used twice')
        seen.add(cid)
        if not isinstance(c.get("title"), str) or not c["title"].strip():
            r.error(f'{where}: command "{cid}" needs a title')
        if "context" in c and c["context"] not in CONTEXTS:
            r.error(f'{where}: command "{cid}": context must be one of {sorted(CONTEXTS)}')
        if "shortcut" in c and c["shortcut"] is not None and not isinstance(c["shortcut"], str):
            r.error(f'{where}: command "{cid}": shortcut must be a string like "⌃⌥H"')


def check_panels(panels, r, where):
    if not isinstance(panels, list):
        r.error(f"{where}: panels must be a list")
        return
    seen = set()
    for p in panels:
        if not isinstance(p, dict):
            r.error(f"{where}: a panel must be an object")
            continue
        pid = p.get("id")
        if not isinstance(pid, str) or not pid.strip() or ":" in pid or re.search(r"\s", pid):
            r.error(f'{where}: panel id {pid!r} must be non-empty without ":" or spaces')
        if pid in seen:
            r.error(f'{where}: panel id "{pid}" is used twice')
        seen.add(pid)
        if not isinstance(p.get("title"), str):
            r.error(f'{where}: panel "{pid}" needs a title')
        if "placement" in p and p["placement"] not in PLACEMENTS:
            r.error(f'{where}: panel "{pid}": placement must be one of {sorted(PLACEMENTS)}')
        if "size" in p and not (isinstance(p["size"], list) and len(p["size"]) == 2 and all(isinstance(x, (int, float)) for x in p["size"])):
            r.error(f'{where}: panel "{pid}": size must be [width, height]')


# ------------------------------------------------------------------ message checks

class Bad(Exception):
    pass


def is_pane(v):
    if isinstance(v, bool):
        return False
    if isinstance(v, int):
        return 0 <= v <= 65535
    return isinstance(v, str) and (v in ("left", "right") or v.isdigit())


def need(msg, key, kind, optional=False):
    if key not in msg or (optional and msg[key] is None):
        if optional:
            return None
        raise Bad(f'"{key}" is missing')
    v = msg[key]
    ok = {
        "str": lambda x: isinstance(x, str),
        "bool": lambda x: isinstance(x, bool),
        "int": lambda x: isinstance(x, int) and not isinstance(x, bool) and x >= 0,
        "num": lambda x: isinstance(x, (int, float)) and not isinstance(x, bool),
        "list": lambda x: isinstance(x, list),
        "obj": lambda x: isinstance(x, dict),
        "pane": is_pane,
    }[kind](v)
    if not ok:
        raise Bad(f'"{key}" must be {kind}, got {json.dumps(v)[:60]}')
    return v


def enum(msg, key, values, optional=True):
    v = need(msg, key, "str", optional)
    if v is not None and v not in values:
        raise Bad(f'"{key}" must be one of {sorted(values)}, got "{v}"')
    return v


def only(msg, keys, what, warn):
    extra = [k for k in msg if k not in keys]
    if extra:
        warn(f"{what}: unknown field(s) {extra} are ignored by Manta")


def check_block(b, warn):
    if not isinstance(b, dict):
        raise Bad("a block must be an object")
    t = b.get("type")
    if t == "heading" or t == "code":
        need(b, "text", "str")
    elif t == "text":
        need(b, "text", "str")
        enum(b, "tone", TONES)
    elif t == "key_value":
        for row in need(b, "rows", "list"):
            need(row, "key", "str")
            need(row, "value", "str")
            enum(row, "tone", TONES)
    elif t == "list":
        for row in need(b, "rows", "list"):
            need(row, "text", "str")
            for k in ("icon", "detail", "action", "tooltip"):
                need(row, k, "str", True)
            enum(row, "tone", TONES)
            if row.get("icon") and row["icon"] not in ICONS:
                warn(f'list row icon "{row["icon"]}" is unknown (shown without icon)')
    elif t == "buttons":
        for btn in need(b, "buttons", "list"):
            need(btn, "label", "str")
            need(btn, "action", "str")
            need(btn, "primary", "bool", True)
    elif t == "separator":
        pass
    elif t == "section":
        need(b, "text", "str")
        need(b, "detail", "str", True)
    elif t == "tags":
        for tag in need(b, "tags", "list"):
            need(tag, "text", "str")
            enum(tag, "tone", TONES)
            need(tag, "icon", "str", True)
    elif t == "image":
        p = need(b, "path", "str")
        if not p.startswith("/") or "/../" in p + "/" or "://" in p:
            warn(f'image block path "{p}" isn\'t an absolute local path: Manta shows a note instead')
        need(b, "max_height", "num", True)
        need(b, "caption", "str", True)
    else:
        raise Bad(f"unknown block type {t!r}")


def check_message(msg, st, warn):
    """Raise Bad for an invalid message; call warn() for questionable ones."""
    t = msg["type"]
    if t in ("set_decorations", "set_sizes", "set_chips"):
        pane = need(msg, "pane", "pane")
        d = need(msg, "dir", "str")
        shown = st.pane_dirs.get(str(pane))
        if shown is not None and d != shown:
            warn(f'{t}: "dir" {d!r} isn\'t the folder pane {pane} shows ({shown!r}): Manta drops it')
    if t == "set_decorations":
        entries = need(msg, "entries", "obj", True) or {}
        if len(entries) > 20000:
            warn("set_decorations: more than 20 000 entries, Manta keeps the first 20 000")
        for name, d in entries.items():
            if not isinstance(d, dict):
                raise Bad(f"entry {name!r} must be an object")
            if d.get("badge") is not None:
                badge = need(d, "badge", "obj")
                text = need(badge, "text", "str")
                if not 1 <= len(text) <= 3:
                    warn(f"badge text {text!r} should be 1–3 characters (cut to 3, empty is dropped)")
                enum(badge, "tone", TONES, optional=False)
            enum(d, "name_tone", TONES)
            need(d, "dimmed", "bool", True)
            need(d, "tooltip", "str", True)
    elif t == "set_sizes":
        for name, s in (need(msg, "sizes", "obj", True) or {}).items():
            if not isinstance(s, dict):
                raise Bad(f"size {name!r} must be an object")
            need(s, "bytes", "int")
            need(s, "complete", "bool")
            for k in ("files", "on_disk", "unreadable"):
                need(s, k, "int", True)
    elif t == "set_chips":
        chips = need(msg, "chips", "list", True) or []
        if len(chips) > 6:
            warn("set_chips: more than 6 chips, Manta shows the first 6")
        for c in chips:
            need(c, "text", "str")
            enum(c, "tone", TONES)
            for k in ("icon", "tooltip", "command"):
                need(c, k, "str", True)
            if c.get("icon") and c["icon"] not in ICONS:
                warn(f'chip icon "{c["icon"]}" is unknown (ignored)')
            if c.get("command") and c["command"] not in st.commands:
                warn(f'chip command "{c["command"]}" isn\'t one of your commands')
    elif t == "toast":
        enum(msg, "level", TOAST_LEVELS)
        need(msg, "text", "str")
        b = need(msg, "button", "str", True)
        c = need(msg, "command", "str", True)
        if bool(b) != bool(c):
            warn("toast: a button needs both \"button\" and \"command\"")
        if c and c not in st.commands:
            warn(f'toast command "{c}" isn\'t one of your commands')
    elif t == "navigate":
        need(msg, "pane", "pane")
        need(msg, "path", "str")
        need(msg, "remote", "obj", True)
    elif t == "register_commands":
        cmds = need(msg, "commands", "list")
        errs = Report(False)
        _quiet(lambda: check_commands(cmds, errs, "register_commands"))
        if errs.errors:
            raise Bad("; ".join(errs.errors))
        st.commands = {c["id"] for c in cmds}
    elif t == "register_panels":
        panels = need(msg, "panels", "list")
        errs = Report(False)
        _quiet(lambda: check_panels(panels, errs, "register_panels"))
        if errs.errors:
            raise Bad("; ".join(errs.errors))
        st.panels = {p["id"] for p in panels}
    elif t == "set_panel_content":
        panel = need(msg, "panel", "str")
        need(msg, "pane", "pane", True)
        blocks = need(msg, "blocks", "list", True) or []
        if panel not in st.panels:
            warn(f'set_panel_content: panel "{panel}" isn\'t declared')
        if len(blocks) > 400:
            warn("set_panel_content: more than 400 blocks, Manta keeps the first 400")
        for b in blocks:
            check_block(b, warn)
            for btn in (b.get("buttons") or []):
                st.actions.add((panel, btn["action"]))
    elif t in ("set_panel_open", "toggle_panel"):
        panel = need(msg, "panel", "str")
        need(msg, "pane", "pane", True)
        if t == "set_panel_open":
            need(msg, "open", "bool")
        if panel not in st.panels:
            warn(f'{t}: panel "{panel}" isn\'t declared')
    elif t == "rename_many":
        need(msg, "pane", "pane")
        need(msg, "dir", "str")
        for pair in need(msg, "renames", "list"):
            if not (isinstance(pair, list) and len(pair) == 2 and all(isinstance(x, str) for x in pair)):
                raise Bad("renames must be [old name, new name] pairs")
    elif t == "list_dir":
        need(msg, "pane", "pane")
        need(msg, "dir", "str")
    elif t == "read_file":
        need(msg, "pane", "pane")
        need(msg, "path", "str")
        need(msg, "max_bytes", "int", True)
    elif t == "run_in_terminal":
        need(msg, "pane", "pane", True)
        need(msg, "command", "str")
        need(msg, "cwd", "str", True)
    elif t == "refresh_pane":
        need(msg, "pane", "pane", True)
    elif t == "select":
        need(msg, "pane", "pane", True)
        for p in need(msg, "paths", "list"):
            if not isinstance(p, str):
                raise Bad("paths must be strings")
    elif t == "set_status_items":
        items = need(msg, "items", "list", True) or []
        if len(items) > 6:
            warn("set_status_items: more than 6 items, Manta shows the first 6")
        for it in items:
            need(it, "id", "str")
            text = need(it, "text", "str")
            if len(text) > 40:
                warn(f"status item text {text!r} is longer than 40 characters (cut)")
            for k in ("compact", "tooltip", "icon"):
                need(it, k, "str", True)
            if it.get("icon") and it["icon"] not in STATUS_ICONS:
                warn(f'status item icon "{it["icon"]}" is unknown')
            enum(it, "level", STATUS_LEVELS)
            m = need(it, "meter", "num", True)
            if m is not None and not 0 <= m <= 1:
                warn("status item meter must be 0.0–1.0 (clamped)")
            if it.get("priority") is not None and not isinstance(it["priority"], int):
                raise Bad('"priority" must be an integer')
            a = need(it, "action", "obj", True)
            if a is not None:
                kind = a.get("kind")
                if kind == "command":
                    need(a, "command", "str")
                elif kind == "open_url":
                    if ":" not in need(a, "url", "str"):
                        warn("status item open_url needs a scheme (https:…): dropped")
                elif kind == "open_app":
                    need(a, "bundle_id", "str")
                else:
                    raise Bad(f"status item action kind {kind!r} is unknown")
    elif t == "log":
        enum(msg, "level", LOG_LEVELS)
        need(msg, "message", "str")
    fields = {
        "set_decorations": {"pane", "dir", "entries"}, "set_sizes": {"pane", "dir", "sizes"},
        "set_chips": {"pane", "dir", "chips"}, "toast": {"level", "text", "button", "command"},
        "navigate": {"pane", "path", "remote"}, "register_commands": {"commands"},
        "register_panels": {"panels"}, "set_panel_content": {"panel", "pane", "blocks"},
        "set_panel_open": {"panel", "pane", "open"}, "toggle_panel": {"panel", "pane"},
        "rename_many": {"pane", "dir", "renames"}, "list_dir": {"pane", "dir"},
        "read_file": {"pane", "path", "max_bytes"}, "run_in_terminal": {"pane", "command", "cwd"},
        "refresh_pane": {"pane"}, "select": {"pane", "paths"}, "set_status_items": {"items"},
        "log": {"level", "message"},
    }[t] | {"type"}
    only(msg, fields, t, warn)


def _quiet(f):
    saved = sys.stdout
    sys.stdout = open(os.devnull, "w")
    try:
        f()
    finally:
        sys.stdout.close()
        sys.stdout = saved


# ------------------------------------------------------------------ session

class State:
    def __init__(self, manifest):
        self.commands = {c["id"] for c in manifest.get("commands", []) if isinstance(c, dict)}
        self.panels = {p["id"] for p in manifest.get("panels", []) if isinstance(p, dict)}
        self.actions = set()
        self.pane_dirs = {}
        self.received = []  # (time, message)
        # Index into `received` when the last host message was sent: an
        # `expect` step also accepts replies that came in since then.
        self.mark = 0
        self.counts = {}


def make_fixture(root):
    d = os.path.join(root, "Test Folder")
    os.makedirs(os.path.join(d, "Sub folder"))
    with open(os.path.join(d, "notes.md"), "w") as f:
        f.write("# Notes\n\nHello from the plugin test.\n")
    with open(os.path.join(d, "photo.jpg"), "wb") as f:
        f.write(b"\xff\xd8\xff\xe0" + b"\0" * 2048)
    with open(os.path.join(d, "big.iso"), "wb") as f:
        f.truncate(12 * 1024 * 1024)  # sparse: takes no disk space
    with open(os.path.join(d, ".hidden"), "w") as f:
        f.write("x")
    return d


def pane_state(pane, path, selection=(), active=False):
    return {
        "pane": pane, "path": path, "local_path": path, "protocol": "Local",
        "backend_label": "Local", "selection": list(selection), "active": active,
    }


def child_env(manifest, data_dir):
    env = dict(os.environ)
    parts = env.get("PATH", "").split(":")
    for extra in ("/opt/homebrew/bin", "/usr/local/bin"):
        if extra not in parts:
            parts.append(extra)
    env["PATH"] = ":".join(p for p in parts if p)
    env["MANTA_PLUGIN_ID"] = manifest["id"]
    env["MANTA_PLUGIN_DATA_DIR"] = data_dir
    env["MANTA_API_VERSION"] = str(API_VERSION)
    env["PYTHONUNBUFFERED"] = "1"
    return env


def resolve_program(dir_, prog, env):
    if "/" in prog:
        p = prog if os.path.isabs(prog) else os.path.join(dir_, prog)
        return p if os.path.isfile(p) else None
    return shutil.which(prog, path=env["PATH"])


class Plugin:
    def __init__(self, dir_, manifest, data_dir, r):
        self.r = r
        env = child_env(manifest, data_dir)
        cmd = list(manifest["command"])
        prog = resolve_program(dir_, cmd[0], env)
        if prog is None:
            raise RuntimeError(f'program "{cmd[0]}" not found (PATH, or relative to the plugin folder)')
        if not os.access(prog, os.X_OK):
            raise RuntimeError(f'"{prog}" is not executable: chmod +x it (and commit the mode)')
        cmd[0] = prog
        self.proc = subprocess.Popen(
            cmd, cwd=dir_, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, bufsize=0,
        )
        self.lines = queue.Queue()
        self.stderr = []
        threading.Thread(target=self._read_out, daemon=True).start()
        threading.Thread(target=self._read_err, daemon=True).start()

    def _read_out(self):
        for raw in iter(self.proc.stdout.readline, b""):
            self.lines.put(raw)
        self.lines.put(None)

    def _read_err(self):
        for raw in iter(self.proc.stderr.readline, b""):
            line = raw.decode("utf-8", "replace").rstrip("\n")
            self.stderr.append(line)
            self.r.debug("stderr: " + line)

    def send(self, msg):
        line = json.dumps(msg, ensure_ascii=False) + "\n"
        self.r.debug("→ " + line.rstrip()[:200])
        try:
            self.proc.stdin.write(line.encode("utf-8"))
            self.proc.stdin.flush()
            return True
        except (BrokenPipeError, OSError):
            return False


def handle_line(raw, st, r, plugin):
    if len(raw) > MAX_LINE:
        r.error(f"a line of {len(raw)} bytes: Manta drops lines over 1 MB")
        return None
    text = raw.decode("utf-8", "replace").strip()
    if not text:
        r.warn("an empty line on stdout (skipped by Manta)")
        return None
    r.debug("← " + text[:200])
    try:
        msg = json.loads(text)
    except ValueError as e:
        r.error(f"not JSON on stdout (log to stderr instead!): {text[:120]!r} ({e})")
        return None
    if not isinstance(msg, dict) or not isinstance(msg.get("type"), str):
        r.error(f'a message without a "type": {text[:120]}')
        return None
    t = msg["type"]
    if t not in PLUGIN_TYPES:
        r.error(f'unknown message type "{t}" (Manta ignores it; typo?)')
        return None
    seen_warn = set()

    def warn(w):
        if w not in seen_warn:
            seen_warn.add(w)
            r.warn(w)

    try:
        check_message(msg, st, warn)
    except Bad as e:
        r.error(f"{t}: {e}  ← {text[:160]}")
        return None
    st.counts[t] = st.counts.get(t, 0) + 1
    st.received.append((time.monotonic(), msg))
    answer(msg, plugin, r)
    return msg


def answer(msg, plugin, r):
    t = msg["type"]
    if t == "list_dir":
        d = msg["dir"]
        entries, err = [], None
        try:
            for name in sorted(os.listdir(d), key=str.lower):
                p = os.path.join(d, name)
                stt = os.lstat(p)
                entries.append({"name": name, "is_dir": os.path.isdir(p), "size": stt.st_size,
                                "modified": int(stt.st_mtime), "shown": not name.startswith(".")})
            entries.sort(key=lambda e: not e["shown"])
        except OSError as e:
            err = str(e)
        plugin.send({"type": "dir_listing", "pane": msg["pane"], "dir": d, "entries": entries, "error": err})
    elif t == "read_file":
        path, cap = msg["path"], min(msg.get("max_bytes") or 1024 * 1024, 16 * 1024 * 1024)
        try:
            with open(path, "rb") as f:
                data = f.read(cap + 1)
            size = os.path.getsize(path)
            plugin.send({"type": "file_content", "pane": msg["pane"], "path": path,
                         "data": base64.b64encode(data[:cap]).decode(), "size": size,
                         "truncated": len(data) > cap, "modified": int(os.path.getmtime(path)), "error": None})
        except OSError as e:
            plugin.send({"type": "file_content", "pane": msg["pane"], "path": path, "data": "",
                         "size": None, "truncated": False, "modified": None, "error": str(e)})
    elif t == "rename_many":
        plugin.send({"type": "rename_result", "pane": msg["pane"], "dir": msg["dir"],
                     "ok": msg["renames"], "failed": []})


def pump(plugin, st, r, seconds):
    """Read plugin output for `seconds` (or until it exits)."""
    end = time.monotonic() + seconds
    while True:
        left = end - time.monotonic()
        if left <= 0:
            return True
        try:
            raw = plugin.lines.get(timeout=left)
        except queue.Empty:
            return True
        if raw is None:
            return False
        handle_line(raw, st, r, plugin)


def expect(plugin, st, r, want, timeout):
    t = want.get("expect")
    fields = {k: v for k, v in want.items() if k not in ("expect", "timeout")}
    start = st.mark
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        for _, m in st.received[start:]:
            if m["type"] == t and all(_contains(m.get(k), v) for k, v in fields.items()):
                r.info(color("32", "✓ ") + f"got {t}" + (f" {fields}" if fields else ""))
                return
        if not pump(plugin, st, r, 0.05):
            break
    r.error(f"expected a {t} message{(' with ' + json.dumps(fields, ensure_ascii=False)) if fields else ''} within {timeout} s")


def _contains(have, want):
    if isinstance(want, str) and isinstance(have, str):
        return want in have
    return have == want


def substitute(obj, env):
    if isinstance(obj, str):
        if obj in env and not isinstance(env[obj], str):
            return env[obj]
        for k, v in env.items():
            if isinstance(v, str):
                obj = obj.replace(k, v)
        return obj
    if isinstance(obj, list):
        return [substitute(x, env) for x in obj]
    if isinstance(obj, dict):
        return {k: substitute(v, env) for k, v in obj.items()}
    return obj


def standard_session(fixture, manifest):
    left = pane_state("left", fixture, active=True)
    sel = pane_state("left", fixture, [os.path.join(fixture, "notes.md")], active=True)
    steps = [
        {"type": "directory_changed", "pane_state": left},
        {"sleep": 0.3},
        {"type": "selection_changed", "pane_state": sel},
        {"type": "active_pane_changed", "pane": "left"},
        {"type": "listing_refreshed", "pane_state": sel},
        {"type": "file_action", "action": {
            "id": 1, "pane": "left", "kind": "new_folder", "sources": [os.path.join(fixture, "New Folder")],
            "destination": None, "src_local": True, "dst_local": True, "src_label": "Local",
            "dst_label": "Local", "phase": "started"}},
        {"type": "file_action", "action": {
            "id": 1, "pane": "left", "kind": "new_folder", "sources": [os.path.join(fixture, "New Folder")],
            "destination": None, "src_local": True, "dst_local": True, "src_label": "Local",
            "dst_label": "Local", "phase": {"finished": {"ok": True, "files": 1, "bytes": 0, "errors": [],
                                                       "cancelled": False, "elapsed_ms": 3}}}},
        {"type": "file_action", "action": {
            "id": 2, "pane": "left", "kind": "teleport", "sources": [], "destination": None,
            "src_local": True, "dst_local": True, "src_label": "Local", "dst_label": None,
            "phase": "started", "a_field_from_the_future": 1}},
        {"type": "message_from_the_future", "x": 1},
        {"sleep": 0.3},
        {"run_commands": True},
        {"panel_actions": True},
        {"type": "pane_opened", "pane": 2},
        {"type": "directory_changed", "pane_state": pane_state(2, fixture)},
        {"type": "pane_closed", "pane": 2},
        {"type": "locale_changed", "locale": "de"},
        {"sleep": 0.3},
    ]
    return steps


def run_session(dir_, manifest, args, r):
    tmp = tempfile.mkdtemp(prefix="manta-plugin-test-")
    try:
        fixture = make_fixture(tmp)
        data_dir = os.path.join(tmp, "plugin-data", manifest["id"])
        os.makedirs(data_dir)
        st = State(manifest)
        st.pane_dirs = {"left": fixture, "right": tmp, "0": fixture, "1": tmp}
        try:
            plugin = Plugin(dir_, manifest, data_dir, r)
        except (RuntimeError, OSError) as e:
            r.error(f"can't start the plugin: {e}")
            return
        left = pane_state("left", fixture, active=True)
        right = pane_state("right", tmp)
        env = {"$DIR": fixture, "$DATA": data_dir, "$PANE_LEFT": left, "$PANE_RIGHT": right}
        r.info(f"started: {' '.join(manifest['command'])}  (pid {plugin.proc.pid})")
        plugin.send({"type": "init", "api_version": API_VERSION, "plugin_id": manifest["id"],
                     "data_dir": data_dir, "panes": [left, right], "locale": args.locale})
        plugin.send({"type": "pane_opened", "pane": "left"})
        plugin.send({"type": "pane_opened", "pane": "right"})
        t0 = time.monotonic()
        alive = pump(plugin, st, r, 0.5)
        if st.received:
            r.info(f"first message after {int((st.received[0][0] - t0) * 1000)} ms")
        if args.session:
            steps = []
            for n, line in enumerate(open(args.session, encoding="utf-8"), 1):
                line = line.strip()
                if not line or line.startswith("#") or line.startswith("//"):
                    continue
                try:
                    steps.append(json.loads(line))
                except ValueError as e:
                    r.error(f"{args.session}:{n}: {e}")
                    return
        else:
            steps = standard_session(fixture, manifest)
        for step in steps:
            if not alive or plugin.proc.poll() is not None:
                break
            step = substitute(step, env)
            if "sleep" in step:
                alive = pump(plugin, st, r, float(step["sleep"]))
            elif "expect" in step:
                expect(plugin, st, r, step, float(step.get("timeout", 2)))
            elif step.get("run_commands"):
                for cid in sorted(st.commands):
                    sel_state = pane_state("left", fixture, [os.path.join(fixture, "notes.md")], active=True)
                    r.info(f"run_command {cid}")
                    before = len(st.received)
                    plugin.send({"type": "run_command", "id": cid, "pane": sel_state})
                    alive = pump(plugin, st, r, args.command_wait)
                    if len(st.received) == before:
                        r.info(color("2", f"  (no reply to {cid} within {args.command_wait} s)"))
                    if not alive:
                        break
            elif step.get("panel_actions"):
                for panel, action in sorted(st.actions)[:8]:
                    r.info(f"panel_action {panel} / {action}")
                    plugin.send({"type": "panel_action", "panel": panel, "action": action, "pane": left})
                    alive = pump(plugin, st, r, 0.3)
            else:
                if step.get("type") == "directory_changed":
                    ps = step.get("pane_state", {})
                    st.pane_dirs[str(ps.get("pane"))] = ps.get("path")
                st.mark = len(st.received)
                alive = plugin.send(step) and pump(plugin, st, r, 0.1)
        exited_early = plugin.proc.poll() is not None
        if exited_early:
            r.error(f"the plugin exited during the session (status {plugin.proc.returncode}); Manta restarts it at most 3 times a minute")
        else:
            plugin.send({"type": "shutdown"})
            t1 = time.monotonic()
            while plugin.proc.poll() is None and time.monotonic() - t1 < 3:
                pump(plugin, st, r, 0.02)
            took = time.monotonic() - t1
            if plugin.proc.poll() is None:
                plugin.proc.kill()
                r.error("the plugin didn't exit after shutdown (Manta kills it after 500 ms)")
            elif took > SHUTDOWN_GRACE:
                r.error(f"the plugin needed {int(took * 1000)} ms to exit after shutdown (Manta kills it after 500 ms)")
            else:
                r.info(f"exited {int(took * 1000)} ms after shutdown (status {plugin.proc.returncode})")
        while True:
            try:
                raw = plugin.lines.get_nowait()
            except queue.Empty:
                break
            if raw is None:
                break
            handle_line(raw, st, r, plugin)
        time.sleep(0.05)
        if plugin.stderr:
            r.info(f"stderr (your log, {len(plugin.stderr)} lines):")
            for line in plugin.stderr[-12:]:
                r.info(color("2", "  | " + line))
        if st.counts:
            r.info("messages: " + ", ".join(f"{k} ×{v}" for k, v in sorted(st.counts.items())))
        else:
            r.warn("the plugin sent no messages at all")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description="Test a Manta plugin without Manta.")
    ap.add_argument("plugin", nargs="?", help="the plugin folder (with plugin.toml)")
    ap.add_argument("--session", help="a JSON lines session file (see the docstring)")
    ap.add_argument("--locale", default="en", help="locale sent in init (default en)")
    ap.add_argument("--manifest-only", action="store_true", help="only check plugin.toml")
    ap.add_argument("--command-wait", type=float, default=0.6, help="seconds to wait after each run_command")
    ap.add_argument("--strict", action="store_true", help="treat warnings as errors")
    ap.add_argument("-v", "--verbose", action="store_true", help="show every line sent and received")
    ap.add_argument("--list-types", action="store_true", help="print the known message types as JSON")
    args = ap.parse_args()
    if args.list_types:
        print(json.dumps({"api_version": API_VERSION, "host": HOST_TYPES, "plugin": PLUGIN_TYPES}))
        return 0
    if not args.plugin:
        ap.print_usage()
        return 2
    dir_ = os.path.abspath(args.plugin)
    r = Report(args.verbose)
    print(color("1", f"plugin.toml  {dir_}"))
    manifest = check_manifest(dir_, r)
    if manifest and not args.manifest_only and not r.errors:
        print(color("1", f"session      {manifest['id']} {manifest['version']}"))
        run_session(dir_, manifest, args, r)
    problems = len(r.errors) + (len(r.warnings) if args.strict else 0)
    summary = f"{len(set(r.errors))} error(s), {len(set(r.warnings))} warning(s)"
    print(color("31" if problems else "32", ("FAILED: " if problems else "OK: ") + summary))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
