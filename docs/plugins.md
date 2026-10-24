# Writing Manta plugins

Manta has two kinds of plugins. Both appear in **Settings → Plugins** (⌘,),
where you switch them on and off.

- **Built-in plugins** (Git, Terminal, Folder Sizes, Batch Rename, Markdown
  Preview, Quick Actions, Project Info, Duplicates, Image Tools, System
  Load) are Rust code compiled into the app. They implement the `Plugin`
  trait in `crates/plugin/src/lib.rs`.
- **External plugins** are folders anyone can write, publish and install.
  Each folder holds a `plugin.toml` and a program in any language. Manta
  starts the program and talks to it with **JSON lines over stdin/stdout**.

This guide covers external plugins. Contents:

1. [Write a plugin](#1-write-a-plugin): `plugin.toml`, the main loop, templates
2. [Install and switch it on](#2-install-and-switch-it-on): what users do
3. [Develop and debug](#develop-and-debug): dev mode, Reload Plugins, logs, `scripts/plugin-test.py`
4. [Lifecycle, compatibility and performance](#lifecycle-compatibility-and-performance)
5. [Protocol reference (api_version 1)](#3-protocol-reference-api_version-1): every message with an example
6. [Security](#security)
7. [Publishing a plugin](#publishing-a-plugin): packaging, the community directory, the review checklist

**Templates** (copy one to start; each has a README and passes
`scripts/plugin-test.py`), listed in [`examples/plugins`](../examples/plugins):

| Template | Language | Needs |
|---|---|---|
| [`hello-python`](../examples/plugins/hello-python) | Python 3, standard library only, commented line by line | `python3` |
| [`hello-node`](../examples/plugins/hello-node) | Node.js, no npm packages | `node` |
| [`hello-shell`](../examples/plugins/hello-shell) | bash + `jq` | `jq` (in macOS 15+, else `brew install jq`) |
| [`hello-rust`](../examples/plugins/hello-rust) | Rust with the [`manta-plugin-protocol`](../crates/plugin-protocol) crate | a Rust toolchain to build it |

**Rust SDK.** [`crates/plugin-protocol`](../crates/plugin-protocol)
(`manta-plugin-protocol`) has typed messages and a tiny stdin/stdout loop
and depends only on `serde` / `serde_json`. Use it from git:
`manta-plugin-protocol = { git = "https://github.com/web-workers-com/manta" }`.
Manta's tests check it against the host's own types and against every JSON
example in this document.

## 1. Write a plugin

A plugin is a folder:

```
my-plugin/
├── plugin.toml
└── main.py          (or any program: a shell script, a Node app, a binary…)
```

### plugin.toml

```toml
id = "my-plugin"              # lowercase letters, digits, . - _ (manta.* is reserved)
name = "My Plugin"
version = "0.1.0"
description = "What it does, in one sentence."
author = "Your Name"
api_version = 1               # the protocol version this plugin speaks
command = ["python3", "main.py"]   # started with the plugin folder as cwd
capabilities = ["decorations", "chips"]   # optional, informational

# Optional, shown before install and in the trust sheet (recommended):
permissions = ["read_files"]  # what the plugin does, see below
license = "MIT"               # SPDX id; the directory requires an OSI-approved licence
repository = "https://github.com/you/manta-my-plugin"   # https only
homepage = "https://example.org/my-plugin"              # https only
min_manta_version = "0.3.0"   # Manta refuses the plugin on older versions

# Optional: name and description in other languages (see "Language").
[localized.de]
name = "Mein Plugin"
description = "Was es tut, in einem Satz."

# Optional: commands and panels can be declared here or sent at runtime.
[[commands]]
id = "hello"                  # no ':' or spaces
title = "Say Hello"
context = "palette"           # or "file_context_menu" (palette + right-click menu)
shortcut = "⌃⌥H"              # optional: ⌃ ⌥ ⇧ ⌘ + one key (also "ctrl+alt+h")

[[panels]]
id = "info"
title = "File Info"
placement = "right_sidebar"   # or "pane_bottom" (one per pane, below the file list)
                              # or "modal" (a centred sheet over the dimmed window)
default_open = false          # ignored for "modal"
# size = [640, 480]           # optional, "modal" only: preferred width and height
```

Manta checks the manifest when it scans the plugins folder and when you
install a plugin. Problems such as a missing field, a bad id, a wrong
`api_version`, a non-https URL, a too-new `min_manta_version` or duplicate
command ids show up in Settings (or in the install error) with a clear
message. Unknown keys are ignored, so a manifest written for a newer Manta
still loads.

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | 2–64 chars: lowercase letters, digits, `.` `-` `_`, starting with a letter or digit. `manta` and `manta.*` are reserved. The installed folder is named after it. |
| `name`, `version` | yes | Shown in Settings. Use semver (`1.2.0`); updates compare versions that way. |
| `api_version` | yes | `1`. See [compatibility](#versioning-and-compatibility). |
| `command` | yes | Program and arguments (see below). |
| `description`, `author` | no | Shown in Settings and the install sheet. |
| `permissions` | no | What the plugin does with the user's Mac: `read_files`, `write_files`, `network`, `run_programs`, `terminal` (uses `run_in_terminal`). **Declarative only**: Manta shows them before install and when the plugin is switched on but can't enforce them. Be honest; the directory review checks them. |
| `license`, `repository`, `homepage` | no | Shown in the install sheet; `repository` and `homepage` must be `https://` URLs. |
| `min_manta_version` | no | The oldest Manta the plugin works with. |
| `capabilities` | no | Informational list (`decorations`, `chips`, `commands`, `panels`, …). |
| `[localized.<lang>]` | no | `name` / `description` per language: `de`, `zh` (or `zh-Hans`), `es`, `fr`, `it`, `ru`. |
| `[[commands]]`, `[[panels]]` | no | Declared up front; `register_commands` / `register_panels` replace them at runtime. |

`command[0]` is looked up on `PATH`. Manta adds `/opt/homebrew/bin` and
`/usr/local/bin`, which apps started from the Finder usually lack. A program
containing a `/`, such as `./run.sh` or `bin/plugin`, is resolved relative to
the plugin folder. The process gets these environment variables:
`MANTA_PLUGIN_ID`, `MANTA_PLUGIN_DATA_DIR`, `MANTA_API_VERSION` and
`PYTHONUNBUFFERED=1`.

### The main loop

```python
import json, sys

def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()            # important: Manta reads line by line

for line in sys.stdin:            # ends when Manta closes stdin
    msg = json.loads(line)
    if msg["type"] == "run_command" and msg["id"] == "hello":
        send({"type": "toast", "level": "success", "text": "Hello!"})
    elif msg["type"] == "shutdown":
        break
```

Rules:

- **stdout is only for protocol messages**: one JSON object per line. Log
  to **stderr**. It goes to `<data dir>/plugin.log`, and the last 50 lines
  show in Settings → Plugins → Show Log.
- Flush after every message.
- Ignore message types you don't know. Newer Manta versions may send more.
- Exit within 500 ms of `shutdown`, or when stdin closes. After that the
  process is killed.

## 2. Install and switch it on

1. Open **Settings** (⌘, or "Settings…" in the command palette ⇧⌘P) and go
   to **Plugins**.
2. Click **Install Plugin…** and choose **From a File (.mantaplugin, .zip)…**
   or **From a Folder…**.
   - A `.mantaplugin` file is a zip archive whose root holds `plugin.toml`,
     or one folder that does (what the Finder's Compress makes). Manta
     unpacks it into a hidden staging folder inside the plugins folder,
     refusing absolute paths, `..`, symbolic links, more than 10 000
     entries or more than 512 MB, and ignoring `__MACOSX`. Unix permissions
     (the executable bit) are kept.
   - A folder is copied as is, without `.git`, `__pycache__` and `.DS_Store`.
3. Manta validates `plugin.toml` and shows a **review sheet**: name,
   version, author, description, the declared `permissions`, the command it
   runs, where it comes from, source code and website links, licence, size,
   and for an update the installed version it replaces, with a warning that
   Manta doesn't check plugins. Nothing is installed before the user clicks
   **Install** (or **Update**); Cancel deletes the staging folder.
4. The plugin is copied to `plugins/<id>/` and listed under **Installed**,
   **switched off**. If `command[0]` is a file inside the plugin (like
   `./run.sh` or `bin/plugin`) that lost its executable bit, Manta sets it;
   nothing else is changed.
5. The user switches it on. The first time, Manta shows the **trust sheet**
   (author, version, permissions, command, folder) and asks to confirm.

**Updates and duplicates.** Installing a plugin whose `id` is already
installed is an update: the sheet says so and shows both versions; after
confirming, the old folder is replaced and the on/off state is kept (a
running plugin is stopped first and started again). If two folders in the
plugins folder have the same id (copied there by hand), the second is shown
as "Can't load" with the reason. **Uninstall** on the card stops the plugin
and deletes its folder; its data folder is kept.

**Downloaded plugins and quarantine.** macOS marks files downloaded from
the internet with the `com.apple.quarantine` attribute, and Gatekeeper
blocks quarantined native programs that aren't notarized. Manta keeps the
flag: copying a folder keeps it, and unpacking a quarantined archive gives
every unpacked file the archive's quarantine value (as Archive Utility
does). The review sheet says the plugin was downloaded. Only when the user
switches the plugin on and confirms the trust sheet, which says so, does
Manta remove the flag from the plugin's files. Manta never removes it
without that confirmation. Scripts started through an interpreter
(`python3 main.py`, `node main.js`) aren't affected by quarantine; native
binaries are. Users who prefer to do it themselves can run
`xattr -dr com.apple.quarantine <plugin folder>` after reading the code.

**Browse Plugins** (top right in Settings › Plugins, and "Browse Plugins…"
in the command palette) opens the [plugin directory](https://manta-filemanager.com/plugins/)
in the browser. Manta doesn't download from the directory itself (yet).

| What | Where (macOS) |
|---|---|
| Installed plugins | `~/Library/Application Support/Manta/plugins/<id>/` |
| On/off state, panel state, panel sizes | `~/Library/Application Support/Manta/plugins.json` |
| Plugin data dir (`data_dir`) and `plugin.log` | `~/Library/Application Support/Manta/plugin-data/<id>/` |

New external plugins start **switched off**. Built-in plugins start on.

## Develop and debug

- **Test without Manta**: `python3 scripts/plugin-test.py path/to/plugin`
  checks `plugin.toml` like Manta does, starts your program with the same
  environment, plays a session (`init`, `pane_opened`, `directory_changed`,
  `selection_changed`, `file_action`s including an unknown kind and an
  unknown message type, `run_command` for every command you declare,
  `panel_action` for your buttons, `locale_changed`, `shutdown`), checks
  every line you print against the protocol (types, required fields, enum
  values, limits, `dir` matching the pane), answers `list_dir`,
  `read_file` and `rename_many`, and checks you exit within 500 ms. Exit
  code 1 on errors. Options: `--locale de`, `--strict` (warnings fail too),
  `-v` (every line), `--manifest-only`, and `--session file.jsonl` for your
  own script:

  ```
  {"type":"directory_changed","pane_state":"$PANE_LEFT"}
  {"expect":"set_chips"}
  {"type":"run_command","id":"say-hello","pane":"$PANE_LEFT"}
  {"expect":"toast","text":"Hello","timeout":2}
  ```

  `$DIR` is a temporary test folder (with a few files), `$DATA` the data
  folder, `"$PANE_LEFT"` / `"$PANE_RIGHT"` become pane states. Python 3.9+
  standard library only; CI-friendly.
- **Development mode**: start Manta from Terminal with
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (repeatable, also `--plugin-dev=<dir>` or `MANTA_PLUGIN_DEV=dir1:dir2`).
  The plugin runs straight from that folder for this session: not copied,
  trusted and switched on without asking, marked **Development** in
  Settings, nothing saved. An installed plugin with the same id steps aside
  until the next launch. Development plugins can't be uninstalled from
  Settings.
- **Reload**: the command palette's **Reload Plugins** restarts every
  running external plugin (re-reading `plugin.toml`) and rescans the
  plugins folder; the **Reload** link on a card restarts one plugin; the
  **Reload** button at the top only rescans.
- **Logs**: everything you write to stderr, and every `log` message, goes
  to `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`
  (truncated at start beyond 512 KB) and the last 50 lines to **Show Log**
  on the plugin's card. Manta adds its own lines there: skipped invalid
  messages, exits, restarts. Manta's own log is
  `~/Library/Logs/Manta/manta.log`.

### When things go wrong

- If the program can't start (e.g. `python3` is missing), the card shows
  **Error** and why.
- If the plugin exits on its own, Manta restarts it, at most 3 times a
  minute. After that it stays in the error state with the exit status, and
  the log holds the stderr tail. Click **Reload** to try again.
- Messages that aren't JSON, lack a `type` or have bad fields are skipped
  with a line in the log. Lines over 1 MB are dropped.

## Lifecycle, compatibility and performance

### Lifecycle

1. **Start**: when the plugin is switched on (and trusted), at app launch
   if it was on, after Reload, and after an update. Manta runs `command`
   with the plugin folder as working directory and the environment
   described above. stdin, stdout and stderr are pipes.
2. **`init`** is always the first message, then `pane_opened` and
   `directory_changed` for every pane. Messages Manta sends while the
   process is still starting are queued (up to 256) and delivered in order.
3. **Events** follow as the user works. Your replies can come at any time
   and in any order; requests that need an answer (`list_dir`, `read_file`,
   `rename_many`) are answered asynchronously.
4. **`shutdown`** when the plugin is switched off, uninstalled, updated or
   the app quits. Exit within 500 ms; after that the process is killed.
   Closing stdin means the same: exit when you read end of file.
5. **Crashes**: if the process exits on its own, Manta starts it again
   (a fresh `init`), at most 3 times a minute; after that the card shows
   **Error** with the exit status until Reload. Everything the plugin had
   shown (decorations, chips, status items, panel content, commands
   registered at runtime) is dropped when it stops.

### Versioning and compatibility

- `api_version` is the protocol version. This Manta speaks `1` and loads
  only plugins that declare `api_version = 1`.
- **Within one api_version, changes are additive only**: new message types,
  new optional fields, new enum values where this document says they may
  appear (`protocol`, `kind`, icon names). So: ignore message types and
  fields you don't know, treat unknown `protocol` values as remote, and
  ignore unknown `file_action` kinds. Manta does the same with what plugins
  send: unknown types and fields are logged and skipped.
- A change that would break existing plugins gets a new `api_version`,
  announced in the release notes; the plan is that Manta then keeps loading
  `api_version = 1` plugins for a transition period, so plugin authors
  have time to move.
- `min_manta_version` lets a plugin rely on something added later within
  the same api_version (a new message type, say).
- The Rust crate `manta-plugin-protocol` follows the api_version with its
  major version (1.x = api_version 1).

### Performance rules

- Answer quickly and never block your read loop on slow work: do it on a
  thread or in the background and send the result when it is ready. Manta
  never waits for a plugin (the UI doesn't block), but events queue up.
- Don't send messages in a busy loop. Send decorations, chips and sizes when
  the folder or your data changed, not on every event; `set_sizes` is
  merged, so send only what changed while counting. Status items: at most
  every second or two, and only when a value changed.
- Keep messages small: lines over 1 MB are dropped; at most 20 000
  decorations or sizes per message, 6 chips, 6 status items, 400 panel
  blocks, 2 000 rows per list; long texts are cut.
- Walk big folders lazily and cache by modification time in your data
  folder (`MANTA_PLUGIN_DATA_DIR`). Remote folders (`local_path: null`) are
  slow: read them with `list_dir` / `read_file` only when needed.
- Start fast: `init` arrives right away; register commands before doing
  expensive setup.

## 3. Protocol reference (api_version 1)

Every message is a JSON object on one line with a `"type"`. Optional fields
can be left out.

### Shared objects

**PaneState**: what a plugin knows about a pane:

```json
{
  "pane": "left",
  "path": "/Users/anna/Projects",
  "local_path": "/Users/anna/Projects",
  "protocol": "Local",
  "backend_label": "Local",
  "selection": ["/Users/anna/Projects/README.md"],
  "active": true
}
```

- `pane` identifies the pane. Manta shows any number of panes side by side
  (at least one). The first two keep their old names: `"left"` is pane 0 and
  `"right"` is pane 1; every other pane is a plain number (`2`, `3` …).
  Requests may use either form (`"left"`, `0` and `"0"` all mean pane 0).
  Ids are **identities, not positions**: a pane keeps its id while it exists
  and closing a pane never renumbers the others, so pane `2` may sit left of
  `"right"` and `"right"` may be gone while `"left"` and `2` remain. A new
  pane gets the smallest free id, so with two panes they are always
  `"left"` and `"right"`. Watch `pane_opened` / `pane_closed` (below) and
  forget a pane's state when it closes.
- `path` is the folder inside the pane's backend.
- `local_path` is the real OS path, or `null` for SFTP/FTP/S3 and containers.
  Check it before touching files.
- `protocol` is `"Local"`, `"Sftp"`, `"Ftp"`, `"Ftps"`, `"S3"`, `"Docker"` or
  `"Kubernetes"` (new values may be added; treat unknown ones as remote).
- `selection` holds absolute paths in display order. With nothing selected
  it holds the entry under the keyboard cursor.
- `remote` (optional, may be missing or `null`) is the server, container or
  bucket the pane shows: `{"protocol":"Docker","user":"","host":"web","port":0}`.
  Docker: `host` is the container name. Kubernetes: `host` is
  `namespace/pod` (or `pod`), `user` the pod's container and `context` the
  kubeconfig context (omitted when it is the current one). SFTP/FTP:
  `user`, `host`, `port`. `null` for local folders and archives.

**Tone**: a semantic colour that Manta maps to the light or dark theme.
It is one of `"neutral"` (the default), `"accent"`, `"success"`, `"warning"`,
`"danger"`, `"info"` or `"muted"`.

**Icon names**, for chips and list rows: `"branch"`, `"terminal"`, `"info"`,
`"warning"`, `"check"`, `"cloud"`, `"plug"`, `"file"`, `"folder"`, `"code"`,
`"image"`, `"archive"`, `"text"`, `"copy"`, `"link"`, `"eye"`, `"key"`,
`"lock"`, `"clock"`, `"stop"`, `"error"`, `"package"`, `"search"`, `"gear"`,
`"pencil"`, `"download"`, `"open"`, `"play"`, `"database"`, `"cube"`, `"stack"`,
`"hammer"`, `"flask"`, `"rocket"`, `"lightning"`, `"list"`, `"wrench"`, `"bug"`, `"trash"`,
`"globe"`, `"refresh"`. Unknown names are ignored.

### Host → plugin (stdin)

| type | fields | when |
|---|---|---|
| `init` | `api_version`, `plugin_id`, `data_dir`, `panes`, `locale` | always first; `locale` is the app's UI language (`"en"`, `"de"`, BCP 47; missing in older hosts → treat as `"en"`) |
| `locale_changed` | `locale` | the user switched the app's language (Settings → General → Language) |
| `directory_changed` | `pane_state` | a pane shows another folder (navigation, tab switch, connect) |
| `listing_refreshed` | `pane_state` | the same folder was listed again (refresh, after copy/move/delete) |
| `selection_changed` | `pane_state` | the selection or cursor moved |
| `active_pane_changed` | `pane` | another pane got focus |
| `pane_opened` | `pane` | a pane appeared (the user split a pane); sent before its first `directory_changed`, and for every pane right after `init` |
| `pane_closed` | `pane` | a pane was closed: drop what you keep for it. Manta has already dropped its decorations, chips, sizes and `pane_bottom` panel instance; requests for it are ignored. Its id may come back with a later `pane_opened` |
| `file_action` | `action` (FileAction) | the user started a file action in the GUI, or it finished (see below) |
| `run_command` | `id`, `pane` (PaneState of the active pane) | the user ran one of your commands |
| `panel_action` | `panel`, `action`, `pane` | a row or button with an `action` was clicked |
| `rename_result` | `pane`, `dir`, `ok`, `failed` | answer to your `rename_many` |
| `dir_listing` | `pane`, `dir`, `entries`, `error` | answer to your `list_dir` |
| `file_content` | `pane`, `path`, `data` (base64), `size`, `truncated`, `modified`, `error` | answer to your `read_file` |
| `shutdown` | | switched off or app quits: exit within 500 ms |

```json
{"type":"init","api_version":1,"plugin_id":"hello-python","data_dir":"/Users/anna/Library/Application Support/Manta/plugin-data/hello-python","panes":[{"pane":"left","path":"/Users/anna","local_path":"/Users/anna","protocol":"Local","backend_label":"Local","selection":[],"active":true}],"locale":"de"}
{"type":"locale_changed","locale":"en"}
{"type":"directory_changed","pane_state":{"pane":"right","path":"/var/www","local_path":null,"protocol":"Sftp","backend_label":"deploy@web-01 (SFTP)","selection":[],"active":false}}
{"type":"active_pane_changed","pane":"right"}
{"type":"file_action","action":{"id":3,"pane":"left","kind":"copy","sources":["/Users/anna/a.txt"],"destination":"/var/www","src_local":true,"dst_local":false,"src_label":"Local","dst_label":"deploy@web-01 (SFTP)","phase":"started","dst_remote":{"protocol":"Sftp","user":"deploy","host":"web-01","port":22}}}
{"type":"file_action","action":{"id":3,"pane":"left","kind":"copy","sources":["/Users/anna/a.txt"],"destination":"/var/www","src_local":true,"dst_local":false,"src_label":"Local","dst_label":"deploy@web-01 (SFTP)","phase":{"finished":{"ok":true,"files":1,"bytes":5120,"errors":[],"cancelled":false,"elapsed_ms":420}},"dst_remote":{"protocol":"Sftp","user":"deploy","host":"web-01","port":22}}}
{"type":"run_command","id":"hello","pane":{"pane":"left","path":"/Users/anna","local_path":"/Users/anna","protocol":"Local","backend_label":"Local","selection":[],"active":true}}
{"type":"panel_action","panel":"info","action":"refresh","pane":{"pane":"left","path":"/Users/anna","local_path":"/Users/anna","protocol":"Local","backend_label":"Local","selection":[],"active":true}}
{"type":"shutdown"}
```

**FileAction** (in `file_action`): a file action the user did in the GUI.
Manta performs it itself; the message is informational (the built-in
Terminal shows it as the equivalent shell command). Every action is sent
twice with the same `id`: first with `"phase":"started"`, then with
`"phase":{"finished":{…}}`. A transfer that is paused, waits for the
network or is retried still finishes exactly once (when it is really done,
failed or cancelled); jobs resumed after a restart send no events.

| field | meaning |
|---|---|
| `id` | unique per app run; the same for start and finish |
| `pane` | the pane the action started in (drag & drop: the pane dragged from) |
| `kind` | `copy`, `move`, `delete` (permanent), `trash` (local items moved to the macOS Trash, ⌘⌫), `rename`, `new_folder`, `open`, `connect`, `disconnect`, `sync`, `compress`, `extract` |
| `sources` | absolute paths in the source backend (`rename`: the old path; `new_folder`: the new folder; `connect`: the folder opened; `sync`: the source folder) |
| `destination` | `copy`/`move`: the target folder; `rename`: the new path; `sync`: the target folder; else `null` |
| `src_local`, `dst_local` | the backend is the local file system (paths are OS paths) |
| `src_label`, `dst_label` | e.g. `"Local"` or `"deploy@web-01 (SFTP)"` |
| `phase` | `"started"` or `{"finished":{"ok","files","bytes","errors","cancelled","elapsed_ms"}}` |
| `src_remote`, `dst_remote` | optional `{"protocol","user","host","port"}` of a remote side. Docker: `host` is the container name. Kubernetes: `host` is `namespace/pod`, `user` the container, `context` the kubeconfig context (omitted when current) |
| `sync` | optional, for `sync`: `{"delete_extra","both_ways"}` |
| `archive` | optional, for `compress` and `extract`: `{"path","format","local","remote","level","exclude_junk"}` (see below) |

A batch rename (`rename_many`, e.g. from the built-in Batch Rename) is one
`rename` action: `sources` holds every old path, `destination` the first
new path, and the extra field `renames` lists every `[old path, new path]`
pair.

**Archives.** `compress` packs `sources` into a new archive in the folder
`destination`; `archive.path` is the archive's path, `archive.format` one of
`"zip"`, `"tar.gz"`, `"tar.xz"`, `"tar.zst"`, plus `level` (1–9) and
`exclude_junk` (`.DS_Store` / `__MACOSX` left out). `extract` copies
`sources` out of the archive `archive.path` into `destination`; here
`sources` are paths *inside* the archive (from its root `/`), and an empty
`sources` list means the whole archive. `archive.local` says whether the
archive file is on this Mac (`archive.remote` names the server otherwise);
`format` may also be `"tar"`, `"tar.bz2"`, `"gzip"`, `"bzip2"`, `"xz"` or
`"zstd"` for extraction.

```json
{"type":"file_action","action":{"id":7,"pane":"left","kind":"compress","sources":["/Users/anna/site"],"destination":"/Users/anna","src_local":true,"dst_local":true,"src_label":"Local","dst_label":"Local","phase":"started","archive":{"path":"/Users/anna/site.tar.gz","format":"tar.gz","local":true,"remote":null,"level":6,"exclude_junk":true}}}
```

**Archive panes.** An archive opened as a folder is a read-only backend:
its `pane_state` has `local_path: null`, `protocol: "Local"` and a
`backend_label` such as `"backup.zip (read-only archive)"`; `path` is the
path inside the archive. Treat it like a remote folder (read files with
`read_file`, list with `list_dir`); `rename_many` fails there.

In `finished`, `ok` is true only if nothing failed and nothing was
cancelled; `errors` lists per-item errors (or the one fatal error). For
`delete`, `files` counts entries including folders; for `trash`, the
items moved (a folder counts once). Unknown kinds may be added later:
ignore them.

### Plugin → host (stdout)

**`set_decorations`**: badges, name tints and dimming for entries of a
folder. The message replaces your earlier decorations for that pane. `dir`
must be the folder the pane shows; if the user has moved on, the message is
dropped. Decorations from several plugins are combined: badges sit side by
side. Manta keeps at most 20 000 entries per message, and badge text is
1–3 characters.
They show in every view: in the list after the name, in the Icons view at
the icon's corner, in the Gallery under the preview (the filmstrip shows the
first badge) and in the active column of the Columns view. In the Columns
view `dir` is the active (focused) column's folder; folder sizes from
`set_sizes` show in that column and in the footer of the column right of it.

```json
{"type":"set_decorations","pane":"left","dir":"/Users/anna/Projects",
 "entries":{
   "big.iso":   {"badge":{"text":"L","tone":"warning"},"tooltip":"Large file: 4.2 GB"},
   "notes.md":  {"badge":{"text":"M","tone":"warning"},"name_tone":"warning"},
   "target":    {"dimmed":true,"tooltip":"Ignored"}
 }}
```

**`set_sizes`**: sizes (usually of folders) for entries of a folder, by
file name. Manta shows them in the Size column, sorts folders by them and
adds them to the selection size. The message is *merged* into your earlier
sizes for that folder, so send only what changed; everything is dropped when
the pane shows another folder. `dir` must be the folder the pane shows. Only
`bytes` and `complete` are required.

| field | meaning |
|---|---|
| `bytes` | logical size in bytes; while counting, what was counted so far |
| `complete` | `false` while still counting: shown dimmed as "≥ 1.2 GB" with a spinner |
| `files` | files counted (tooltip) |
| `on_disk` | optional allocated size in bytes (tooltip) |
| `unreadable` | folders that couldn't be read; the value is then shown as "≥ …" |

```json
{"type":"set_sizes","pane":"left","dir":"/Users/anna/Projects",
 "sizes":{"node_modules":{"bytes":412000000,"complete":false,"files":38211},
          "src":{"bytes":184320,"complete":true,"files":42,"on_disk":200704}}}
```

**`set_chips`**: chips in the pane's path bar, right of the breadcrumb (up
to 6). A chip with `command` is clickable and runs that command of your
plugin. When the pane is narrow, chips shrink instead of disappearing: first
the longest texts are shortened with "…", then chips show only their icon
(from the left; a chip without an icon shows its first letter), with the
full text in the tooltip and still clickable. A chip is hidden only when not
even its icon fits; the current folder name always keeps its room. Give your
chip an `icon` so it stays recognisable.

```json
{"type":"set_chips","pane":"left","dir":"/Users/anna/Projects",
 "chips":[{"text":"main","icon":"branch","tone":"accent","tooltip":"2 ahead","command":"status"}]}
```

**`set_status_items`**: items in the window's status bar, on the right
before the "⇧⌘P Command Palette" hint (up to 6 per plugin). The message
replaces your earlier items; `"items":[]` removes them. They stay until
replaced or your plugin is switched off, independent of panes and folders,
so send them again only when something changed (the built-in System Load
plugin sends every 2 s, and only when a value changed). Only `id` and
`text` are required.

| field | meaning |
|---|---|
| `id` | unique within your plugin |
| `text` | e.g. `"CPU 12 %"`, up to 40 characters |
| `compact` | shorter text for narrow windows; `""` = icon and meter only; omitted = the item is hidden instead |
| `tooltip` | shown on hover (the full `text` is added when the item is shortened) |
| `icon` | a chip icon name, or `"cpu"`, `"gpu"`, `"memory"` |
| `level` | `"normal"` (default), `"warn"` (amber) or `"error"` (red): text, icon and meter |
| `meter` | a small bar after the text, 0.0–1.0 |
| `priority` | higher stays longer when space is short (default 0) |
| `action` | what a click does: `{"kind":"command","command":"…"}` runs that command of your plugin (`run_command`), `{"kind":"open_url","url":"https://…"}` opens a URL (a scheme is required), `{"kind":"open_app","bundle_id":"com.apple.ActivityMonitor"}` opens an app by bundle id. Invalid actions are dropped |

When the window gets narrow, the status bar gives way in this order: the
palette hint shrinks to "⇧⌘P", then goes; items without `compact` go,
lowest `priority` first; the rest switch to their compact form; then those
go, lowest `priority` first. Give an item a `compact` form and an `icon` if
it should stay visible.

```json
{"type":"set_status_items","items":[{"id":"cpu","text":"CPU 91 %","compact":"","tooltip":"10 cores","icon":"cpu","level":"warn","meter":0.91,"priority":30,"action":{"kind":"open_app","bundle_id":"com.apple.ActivityMonitor"}},{"id":"vpn","text":"VPN on","icon":"lock","action":{"kind":"command","command":"toggle-vpn"}}]}
```

**`toast`**: `level` is `"info"` (the default), `"success"`, `"warning"` or
`"error"`. With `button` and `command`, the toast gets a button that runs
that command of your plugin (you receive `run_command`).

```json
{"type":"toast","level":"success","text":"Uploaded 3 files"}
{"type":"toast","level":"success","text":"Renamed 12 items","button":"Undo","command":"undo"}
```

**`rename_many`**: rename entries of `dir` in a pane, local or remote, as
`[old name, new name]` pairs. Manta does it in the background in a safe
order: swaps and cycles (`a → b → a`) and case-only renames (`readme →
README` on a case-insensitive volume) go through temporary names. It never
overwrites: if a target is taken by an entry that isn't renamed away, if two
pairs share a target, or a name is invalid, the whole batch is refused. It
reports one `rename` file action, refreshes the pane and answers with
`rename_result` (`ok`: `[old, new]` pairs done; `failed`: `[old, error]`).
To undo, send `rename_many` again with the `ok` pairs reversed.

```json
{"type":"rename_many","pane":"left","dir":"/Users/anna/Photos","renames":[["IMG_0041.JPG","Iceland-41.jpg"],["IMG_0042.JPG","Iceland-42.jpg"]]}
{"type":"rename_result","pane":"left","dir":"/Users/anna/Photos","ok":[["IMG_0041.JPG","Iceland-41.jpg"]],"failed":[["IMG_0042.JPG","“Iceland-42.jpg” already exists"]]}
```

**`list_dir`**: the entries of `dir` as the pane lists them, answered with
`dir_listing`. `entries` holds `{name, is_dir, size, modified, shown}`
(`modified`: Unix seconds; `shown`: visible in the pane): shown entries
first in the pane's display order, then hidden or filtered-out ones.

```json
{"type":"list_dir","pane":"left","dir":"/Users/anna/Photos"}
{"type":"dir_listing","pane":"left","dir":"/Users/anna/Photos","entries":[{"name":"IMG_0041.JPG","is_dir":false,"size":2000000,"modified":1760950800,"shown":true}],"error":null}
```

**`read_file`**: the bytes of a file in the pane's backend, local or remote
(SFTP, FTP, …): Manta reads it in the background through the pane's
connection and answers with `file_content`. `max_bytes` defaults to 1 MiB
and is capped at 16 MiB; a longer file comes back cut, with
`"truncated":true` and its full `size`. `data` is base64 (standard
alphabet, padded); `modified` is Unix seconds. On failure `data` is empty
and `error` says why.

```json
{"type":"read_file","pane":"right","path":"/var/www/README.md","max_bytes":2097152}
{"type":"file_content","pane":"right","path":"/var/www/README.md","data":"IyBXZWxjb21lCg==","size":10,"truncated":false,"modified":1760950800,"error":null}
```

**`navigate`**: open a folder in a pane. Without `remote` it works only
for local panes. With `remote` (the pane's `remote` object from its
`pane_state`) it also works for servers and containers, but only while the
pane still shows exactly that backend, so a request that raced with the user
switching servers is dropped. It is ignored if the pane already shows that
folder.

```json
{"type":"navigate","pane":"left","path":"/Users/anna/Downloads"}
{"type":"navigate","pane":"right","path":"/etc/nginx","remote":{"protocol":"Docker","user":"","host":"web","port":0}}
```

**`run_in_terminal`**: run a shell command visibly in the built-in
Terminal below a pane, as if the user had typed it. Manta opens the
terminal panel if it is closed. Once the shell is idle at its prompt, it
`cd`s to `cwd` (optional), types `command` and presses Enter, so the command
runs in the user's shell, shows its output live and lands in the shell
history. If a program is running in the terminal, the command waits until it
finishes. Anything the user had half-typed at the prompt is kept and comes
back afterwards (zsh, bash 4+). Leave out `pane` for the active pane. If the
Terminal plugin is switched off, Manta shows a toast saying so and the
command doesn't run. `command` is shell text: quote paths yourself (e.g.
`shlex.quote` in Python). Use it for long-running or interactive commands
(`rsync --progress`, `npm install`, `ssh`).

```json
{"type":"run_in_terminal","pane":"left","command":"rsync -a --progress 'IMG 1.jpg' /Volumes/Backup/","cwd":"/Users/anna/Photos"}
```

**`refresh_pane`**: list a pane's folder again, e.g. after your plugin
created or changed files there. Leave out `pane` for every pane.

```json
{"type":"refresh_pane","pane":"left"}
```

**`select`**: select files in a pane and scroll to them, e.g. "show me
these duplicates". If the pane shows another folder, Manta opens the
folder of the first path first. Only paths in that folder are selected;
others are ignored. It works only for local panes. Leave out `pane` for
the active pane. Entries the pane doesn't show (hidden files, a filter)
can't be selected.

```json
{"type":"select","pane":"left","paths":["/Users/anna/Photos/IMG_0041.JPG","/Users/anna/Photos/IMG_0042.JPG"]}
```

**`register_commands`** / **`register_panels`**: replace what `plugin.toml`
declared, with the same fields as there.

```json
{"type":"register_commands","commands":[{"id":"hello","title":"Say Hello","context":"palette","shortcut":"⌃⌥H"}]}
{"type":"register_panels","panels":[{"id":"info","title":"File Info","placement":"right_sidebar","default_open":false}]}
{"type":"register_panels","panels":[{"id":"wizard","title":"Wizard","placement":"modal","size":[640,480]}]}
```

A `modal` panel is a sheet centred over the dimmed window, like Manta's
own dialogs. It opens with `set_panel_open` (never at startup) and closes
with Esc or `set_panel_open` with `"open":false`. Like a macOS sheet it has
no close button in its title bar, so give it a Cancel / Done button. While it
is open, the app's shortcuts are off.

In the palette, commands are listed under your plugin's name. Internally
they are namespaced as `<plugin id>:<command id>`, so ids only need to be
unique within your plugin. Shortcuts with ⌘ or ⌃ work everywhere, even while
the terminal has focus. Shortcuts without ⌘ or ⌃ work only while no text
field or panel has keyboard focus.

**`set_panel_content`**: the body of a panel, as blocks. `pane` picks the
instance of a `pane_bottom` panel; leave it out to set every instance.

```json
{"type":"set_panel_content","panel":"info","blocks":[
  {"type":"heading","text":"report.pdf"},
  {"type":"text","text":"Not backed up yet","tone":"warning"},
  {"type":"key_value","rows":[{"key":"Size","value":"1.8 MB"},{"key":"Kind","value":"PDF"}]},
  {"type":"list","rows":[{"icon":"file","text":"a.txt","detail":"2 KB","tone":"neutral","action":"open:a.txt"}]},
  {"type":"code","text":"$ make\nok"},
  {"type":"section","text":"Scripts","detail":"package.json"},
  {"type":"tags","tags":[{"text":"Laravel 11","tone":"danger","icon":"rocket"},{"text":"pnpm","tone":"success"}]},
  {"type":"separator"},
  {"type":"buttons","buttons":[{"label":"Refresh","action":"refresh","primary":true},{"label":"Close","action":"close"}]}
]}
```

| block | fields |
|---|---|
| `heading` | `text` |
| `text` | `text`, `tone` |
| `code` | `text` (monospace, keeps line breaks) |
| `key_value` | `rows`: `[{key, value, tone?}]` |
| `list` | `rows`: `[{icon?, text, detail?, tone?, action?, tooltip?}]`. A row with `action` is clickable; `tooltip` shows on hover |
| `buttons` | `buttons`: `[{label, action, primary?}]` |
| `separator` | |
| `section` | `text`, `detail?`: a small caption starting a group (e.g. above a list), `detail` right-aligned |
| `tags` | `tags`: `[{text, tone?, icon?}]`: small coloured pills in a wrapping row |
| `image` | `path`, `max_height?`, `caption?`: a local image file (see below) |

Clicking a row or button sends `panel_action` with its `action`.

**Image blocks** show a PNG, JPEG, GIF (first frame), WebP or BMP file from
this Mac, aspect-fit to the panel's width, at most `max_height` points tall
(default 200, capped at 800), with rounded corners and an optional
`caption` below. `path` must be an absolute local path: URLs (`file://`,
`http://`, `data:`), relative paths and `..` are refused and shown as a
note. Manta reads and decodes the file in the background, scaled down to
at most 1024 pixels on the long edge, turned by its EXIF orientation, and
reloads it when the file's modification time changes. For HEIC or TIFF,
convert a preview first (e.g. `sips -s format png -Z 1024 in.heic --out
preview.png` into your data folder) and show that.

```json
{"type":"set_panel_content","panel":"info","blocks":[{"type":"image","path":"/Users/anna/Sites/shop/public/hero.jpg","max_height":240,"caption":"1920×1080 · sRGB"},{"type":"key_value","rows":[{"key":"Size","value":"340 KB"}]}]}
```

**`set_panel_open`**: open or close a panel. `pane` applies to `pane_bottom`
panels; leave it out for every pane.

```json
{"type":"set_panel_open","panel":"info","open":true}
```

**`toggle_panel`**: open the panel if it is closed, close it if it is
open, e.g. from a chip's command. `pane` selects the instance of a
`pane_bottom` panel; leave it out for the active pane.

```json
{"type":"toggle_panel","panel":"project"}
```

**`log`**: a line for Settings → Show Log. `level` is `"debug"`, `"info"`,
`"warn"` or `"error"`.

```json
{"type":"log","level":"info","message":"scanned 120 files"}
```

## Bundled plugins

Plugin folders in the repository's top-level `plugins/` folder (any
sub-folder with a `plugin.toml`; at the moment there are none, the folder
holds the community directory `registry.json`) plus
`examples/plugins/hello-python` are embedded into the Manta binary at
build time by `crates/plugin/build.rs` (their `tests/` folders and caches are
left out). **Settings → Plugins → Available** lists the ones that aren't
installed, each with an **Install** button. Installing writes the files into
the plugins folder, so it works wherever the app lives; the plugin starts
switched off and asks for the usual confirmation when you switch it on. If
an installed copy has an older `version` than the bundled one (compared
semver-style: `1.10.0 > 1.9.2`, `1.0.0 > 1.0.0-beta.2`), its card shows the
newer version with an **Update** button; updating keeps the on/off state.

In Rust: `manta_plugin::bundled::embedded()` returns them,
`PluginHost::set_bundled`, `bundled_rows()` and `install_bundled(id)` drive
the Settings section.

To add a bundled plugin, create `plugins/<name>/plugin.toml` and its
program; a rebuild picks it up.

### Quick Actions and Project Info (now built in)

Quick Actions (`manta.quickactions`, [`docs/quick-actions.md`](quick-actions.md))
and Project Info (`manta.projectinfo`, [`docs/project-info.md`](project-info.md))
used to be bundled Python plugins and are built-in Rust plugins now
(`crates/plugin-quickactions`, `crates/plugin-projectinfo`). They still use
the declarative blocks (through `Plugin::panel_blocks`), so they are good
examples of filtering context-menu commands by the selection, `RunInTerminal`,
`RefreshPane`, toasts with a button (Cancel), `Modal` confirmation sheets,
clickable path-bar chips with `TogglePanel` and polling a project's files.

**Retired external plugins.** `manta_plugin::host::RETIRED_EXTERNAL` lists
(old external id, new built-in id) pairs. On startup (and on every rescan)
an installed external plugin with a retired id is switched off (its folder
is kept), a toast says once that it is built in now, Settings marks it
"Replaced by built-in" with an **Uninstall** button, and it can't be
switched on again (`PluginRow::replaced_by`, `PluginHost::replaced_by`).
When the built-in starts and its data dir is still empty, the files of
`plugin-data/<old id>/` (except `plugin.log`) are copied over.

### Image Tools and Duplicates (now built in)

Image Tools (`manta.imagetools`, `crates/plugin-imagetools`) and
Duplicates (`manta.duplicates`, `crates/plugin-duplicates`) used to be
bundled Python plugins (`image-tools`, `duplicates`) and are built-in Rust
plugins now, listed in `RETIRED_EXTERNAL`. Their panels and sheets are drawn
natively in egui (no blocks). Their data carries over: Image Tools'
`options.json` and preview cache, Duplicates' `settings.json`.

- **Image Tools** decorates images with "lo"/"XL" badges and tooltips
  (`SetDecorations`; header parsing in `headers.rs`, EXIF via
  `kamadak-exif`, `sips -g` for HEIC/TIFF/AVIF, an mtime cache, at most
  2000 images per folder), shows the **Image** side panel (preview through
  an egui image loader of its own, EXIF, GPS with Open in Maps) and offers
  resize, convert, rotate/flip, strip metadata, optimize for web, @2x/@1x
  and Copy as Data URI in an "Image Tools" context submenu, through a
  `Modal` sheet. Batches run on a worker thread with a progress
  `ActionToast` (Cancel), then `RefreshPane` and a result toast with the
  bytes before and after. The tools run as child processes with plain argv.
- **Duplicates** scans in three steps (size, both 64 KB ends, SHA-256 via
  `sha2`) on a few threads with cancellation, shows the **Duplicates** side
  panel, a `Modal` confirmation, `Select` for Select in Pane, and moves
  extras to the Trash with `trash`, the Finder (`osascript`) or the
  `trash_command` from `settings.json`.

### Several panes

Older plugins that only know `"left"` and `"right"` keep working: with two
panes nothing changes, and messages about other panes carry a number they
can ignore. To support more panes:

- keep per-pane state in a map keyed by the `pane` value, not in two slots;
- add a pane on `pane_opened` (or on its first `directory_changed`) and drop
  it on `pane_closed`;
- "the other pane" is ambiguous with three or more. Manta's copy target is
  the most recently active other pane: track `active_pane_changed` (most
  recent first) and pick the first one that isn't the active pane, as the
  built-in Quick Actions do for `{dest}`.

## Built-in plugins (Rust)

Built-in plugins implement `manta_plugin::Plugin` and are registered in
`builtin_plugins()` in `crates/app/src/app.rs` (Git, Terminal, Folder
Sizes, Batch Rename, Markdown Preview, Quick Actions, Project Info,
Duplicates, Image Tools, System Load). In Rust a pane is `manta_plugin::PaneId(u16)`, with
`PaneId::LEFT` (0) and `PaneId::RIGHT` (1) for the classic two (the old
enum spellings `PaneId::Left` / `PaneId::Right` still compile as aliases);
`PaneId::key()` gives `"left"`, `"right"` or the number. Panes come and go
with `PluginEvent::PaneOpened(PaneId)` and `PluginEvent::PaneClosed(PaneId)`
(the Terminal hangs up the closed pane's shells there); `PluginHost`
offers `close_pane`, `pane_ids` and `copy_pane_panels` for the app. Folder sizes are sent with `HostRequest::SetSizes { pane, dir,
sizes: Vec<(String, SizeInfo)> }`, the Rust side of `set_sizes`. The Rust
side of the other messages: `HostRequest::RenameMany { pane, dir, renames }`
→ `PluginEvent::RenameResult(RenameResult { pane, dir, ok, failed })`,
`HostRequest::ListDir { pane, dir }` → `PluginEvent::DirListing(..)`,
`HostRequest::ReadFile { pane, path, max_bytes }` → `PluginEvent::FileContent(FileContent
{ pane, path, bytes, size, truncated, modified, error })` (read through the pane's VFS on a
worker, so it works for remote panes; `max_bytes` is capped at `MAX_READ_BYTES`, 16 MiB) (all
delivered only to the asking plugin), `HostRequest::ActionToast { level,
text, button, command }`, `HostRequest::RefreshPane { pane }`, `HostRequest::Select { pane, paths }`
(→ `HostEvent::Select { pane, dir, names }` for the app; `pane: None` means the active pane),
`HostRequest::SetStatusItems { items: Vec<StatusItem> }` (the Rust side of `set_status_items`;
build items with `StatusItem::new(id, text)` and struct update syntax; the app reads them with
`PluginHost::status_items()`) and
`HostRequest::RunInTerminal { pane, command, cwd }` (`toggle_panel` maps to
`HostRequest::TogglePanel`). The host routes the
latter to the Terminal plugin as `PluginEvent::RunInTerminal { pane, command,
cwd }` (with `pane` resolved to the active pane if it was `None`), an event
only `manta.terminal` receives. A `PanelPlacement::Modal` panel takes its size from
`Plugin::panel_size(panel)` (a new trait method with a default, so existing
`PanelSpec` literals keep compiling). All trait methods run on the
UI thread and must return quickly. Do slow work on your own threads and
report back through the `PluginHandle`. Built-ins can draw their own egui UI
(`panel_ui`, `settings_ui`), or return declarative blocks from
`Plugin::panel_blocks(panel, pane)` (drawn by the same renderer as external
plugins' `set_panel_content`, clicks arrive in `Plugin::panel_action(panel,
action, pane)`; both are trait methods with defaults), and send
`HostRequest::TogglePanel { panel, pane }` to toggle a panel (`pane: None`
means the active pane). If a built-in panics, Manta catches the panic,
switches that plugin off for the session and shows the error in Settings.

### Language

Manta is localised in English, German, Simplified Chinese, Spanish,
French, Italian and Russian. Plugins should show their own
texts (command titles, panel titles and contents, chips, tooltips, toasts,
settings) in the app's language when they can, and fall back to English
otherwise. All bundled plugins do.

- External plugins get `locale` in `init` (`"en"`, `"de"`, `"zh-Hans"`,
  `"es"`, `"fr"`, `"it"`, `"ru"`) and `locale_changed` when the user
  switches the language. Re-send `register_commands` / `register_panels` /
  chips / panel content with translated titles after a change.
- Built-in plugins read `manta_plugin::locale()` or
  `PluginHandle::locale()` (`"en"`, `"de"`, `"zh-Hans"`, …; set before `enable` is called)
  and get `PluginEvent::LocaleChanged(code)` on a change. Return translated
  `CommandSpec` / `PanelSpec` titles from `commands()` / `panels()`; the host
  asks for them again after the event. Panels drawn every frame just use the
  new language on the next frame.
- Command ids, panel ids and actions never change with the language.
- Only translate your own UI. File names, command output, metadata values
  and anything the user typed stay as they are.

German style (same as the app): Finder terms ("Informationen",
"In den Papierkorb legen"), informal "du", „…“ quotes, a space before the
ellipsis ("Umbenennen …"), decimal comma ("12,4 MB").

**Built-in plugins (Rust)** keep a `(key, English, German)` table and use
`manta_plugin::i18n`:

```rust
// src/i18n.rs
use manta_plugin::i18n::{Catalog, Entries};

pub static CATALOG: Catalog = Catalog::new(ENTRIES);
const ENTRIES: Entries = &[
    ("cmd.show", "Show Disk Usage", "Speicherbelegung einblenden"),
    ("files.one", "{count} file", "{count} Datei"),
    ("files.other", "{count} files", "{count} Dateien"),
    ("error.read", "Couldn't read “{name}”", "„{name}“ konnte nicht gelesen werden"),
];

macro_rules! tr {
    ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
}
pub(crate) use tr;

// elsewhere
tr!("cmd.show")                  // &'static str in the active language
tr!("files", count = n)          // "files.one" / "files.other", {count} = 12.345
tr!("error.read", name = file)   // String
```

`manta_plugin::i18n` also has `format_count`, `format_decimal` and
`localize_decimal` for numbers, `with_lang` / `set_thread_lang` to force a
language in tests and renders, and `i18n::check` with the tests every
built-in runs: `catalog_problems` (German missing, a copy of English,
placeholders differ, unpaired plurals, wrong quotes or ellipsis),
`missing_keys` (every `tr!("…")` key exists) and `hard_coded_strings`
(no literal text passed to `ui.label`, `RichText::new`, `title:` …).

**External plugins** do the same with a dictionary. In Python:

```python
STRINGS = {
    "en": {"cmd.hello": "Say Hello", "files": "{n} files"},
    "de": {"cmd.hello": "Hallo sagen", "files": "{n} Dateien"},
}
LANG = "en"

def _(key, **kw):
    table = STRINGS.get(LANG, STRINGS["en"])
    return table.get(key, STRINGS["en"].get(key, key)).format(**kw)

def set_lang(code):
    global LANG
    LANG = "de" if (code or "en").lower().startswith("de") else "en"

for msg in messages():
    if msg["type"] == "init":
        set_lang(msg.get("locale"))
        send_commands()           # titles via _("cmd.hello")
    elif msg["type"] == "locale_changed":
        set_lang(msg["locale"])
        send_commands()           # and panels, chips, panel content
```

The titles in `plugin.toml` (`[[commands]]`, `[[panels]]`) are English;
send `register_commands` / `register_panels` again with translated titles
after `init` (when the locale isn't English) and after `locale_changed`.
The plugin's own name and description, shown in Settings and as the
context-menu submenu, come from an optional table in `plugin.toml`:

```toml
name = "Quick Actions"
description = "Your own commands for the selection …"

[localized.de]
name = "Schnellaktionen"
description = "Deine eigenen Befehle für die Auswahl …"
```

Texts the user writes stay as written. A plugin with its own config can
offer per-language fields instead, like Quick Actions' optional `title_de`
in `actions.json` (see `docs/quick-actions.md`).

See `examples/plugins/hello-python` for a complete example (all seven
languages in `plugin.toml`, English and German in the program); the other
templates do the same.

### Keyboard focus

- While any widget has keyboard focus, app shortcuts without ⌘ (Tab, arrows,
  Enter, ⌫, Space, Esc, F-keys) are not handled by the app.
- While a widget inside a plugin panel has focus, ⌘C, ⌘V, ⌘A, ⌘K, ⌘←, ⌘→,
  ⌘⌫ and ⌥⌘⌫ go to the panel. Other ⌘-shortcuts keep working.
- App shortcuts plugins shouldn't take: ⌘T (new tab), ⌥⌘L (Transfers),
  ⌘Y (Quick Look), ⌘⌫ / ⌥⌘⌫ (Trash / delete), ⌘D / ⇧⌘D (panes) and the
  rest of the table in the user manual (`docs/manual/en/18-tastenkuerzel.md`).
- Plugin shortcuts with ⌘ or ⌃ are global. A panel should therefore leave
  its own toggle key (e.g. ⌃`) to the host.
- Take focus with `request_focus` and lock it with
  `memory.set_focus_lock_filter` (tab, arrows, escape). Clicking elsewhere
  releases it.

## Security

**External plugins run as normal programs with your user's permissions.**
Manta doesn't sandbox them. A plugin process can do anything a program you
start in Terminal can: read, change and delete your files (also outside the
folders Manta shows), start other programs, use the network and read the
environment. Through the protocol it additionally sees what you do in Manta
(folders, selections, file actions including server names and user names
of remote panes, but never passwords or keys) and can ask Manta to read
files through a pane's connection (`read_file`, `list_dir`), rename
entries, navigate, select files and type commands into the built-in
Terminal (`run_in_terminal`, visible to you). The declared `permissions`
in `plugin.toml` describe what the author says the plugin does; Manta shows
them but can't enforce them.

What Manta does to keep you in control:

- nothing is installed before you have seen the review sheet (author,
  version, declared permissions, command, source, licence) and confirmed;
  archives are unpacked safely (no paths outside the staging folder, no
  symbolic links, size limits);
- new plugins start **switched off**;
- the first time you switch one on, the trust sheet shows what it will run
  and where, and asks you to confirm; only then does a downloaded plugin
  lose macOS's quarantine flag;
- what plugins send is validated and size-limited (bad messages are skipped
  and logged; image blocks only show absolute local paths; status item
  links need a scheme; `navigate` to a server only works while the pane
  still shows that server);
- a plugin that keeps crashing is stopped after 3 restarts a minute;
- Settings shows each plugin's log and lets you switch it off or uninstall
  it at any time; uninstalling deletes its folder (the data folder is kept);
- built-in plugins that panic are switched off for the session instead of
  taking the app down.

Install plugins only from people you trust, and read the code of plugins
from the internet before you switch them on. Entries in the community
directory have been looked at by a maintainer when they were added (see
below), which lowers the risk but is no guarantee.

## Publishing a plugin

### Package a release

1. Put the plugin in a **public repository** under an **OSI-approved
   licence** (MIT, Apache-2.0, BSD, GPL, MPL, …) and add `license`,
   `repository`, `permissions` and `min_manta_version` to `plugin.toml`.
2. Run `python3 scripts/plugin-test.py <folder> --strict` (from a clone of
   the Manta repository) until it passes.
3. Zip the folder: `zip -r my-plugin-1.2.0.mantaplugin my-plugin -x '*/.git/*' '*/target/*' '*/__pycache__/*'`
   (or Compress in the Finder and rename `.zip` to `.mantaplugin`). Keep
   the executable bit of scripts and binaries; a plain zip does.
4. Native programs: build for Apple silicon (arm64). Sign and notarize them
   if you can; otherwise users get them quarantined and Manta removes the
   flag only after they confirm the trust sheet.
5. Attach the file to a **GitHub release** (or any stable https URL) and
   note its SHA-256: `shasum -a 256 my-plugin-1.2.0.mantaplugin`.

Users install the file with **Install Plugin… → From a File**. A newer
release with the same `id` and a higher `version` installs as an update.

### The community directory

[`plugins/registry.json`](../plugins/registry.json) in this repository
lists community plugins; the directory is shown on
[manta-filemanager.com/plugins/](https://manta-filemanager.com/plugins/).
It is maintained through **pull requests** (welcome): add or update your
entry and open a PR. The format is described by
[`plugins/registry.schema.json`](../plugins/registry.schema.json):

```json
{
  "schema": 1,
  "plugins": [
    {
      "id": "cloud-sync",
      "name": { "en": "Cloud Sync", "de": "Cloud-Sync" },
      "description": { "en": "Sync state of cloud folders as badges.", "de": "Sync-Status von Cloud-Ordnern als Badges." },
      "author": "Anna Berger",
      "repository": "https://github.com/anna-berger/manta-cloud-sync",
      "license": "MIT",
      "latest": {
        "version": "1.2.0",
        "url": "https://github.com/anna-berger/manta-cloud-sync/releases/download/v1.2.0/cloud-sync-1.2.0.mantaplugin",
        "sha256": "<64 hex digits>"
      },
      "min_manta_version": "0.3.0",
      "platforms": ["any"],
      "runtime": "node",
      "permissions": ["read_files", "network"],
      "tags": ["cloud", "sync"]
    }
  ]
}
```

`name` and `description` need `en` and may have `de`, `zh-Hans`, `es`,
`fr`, `it`, `ru`. `platforms` is `["any"]` for scripts and
`["macos-arm64"]` for native binaries; `runtime` tells users what they need
(`python3`, `node`, `shell`, `native`, `other`). Check your entry with
`python3 scripts/plugin-registry.py` (add `--fetch` to download each
release, verify its SHA-256 and check its `plugin.toml` against the entry)
before you open the PR; Manta's test suite runs the same check (without
`--fetch`).

**Requirements for an entry** (the review checklist):

- [ ] Public source code at `repository`, under an OSI-approved licence
      (`license` is its SPDX id), and the release built from that source.
- [ ] `id`, `version`, `permissions` and `min_manta_version` in the
      registry match the release's `plugin.toml`; `sha256` matches the file
      at `latest.url` (https).
- [ ] `scripts/plugin-test.py --strict` passes on the release.
- [ ] `permissions` are honest: the reviewer reads the code for file
      writes, network access, started programs and `run_in_terminal`.
- [ ] **No telemetry, analytics or other network traffic the user doesn't
      expect** unless it is disclosed in the description and README and can
      be switched off. No ads, no crypto mining, no obfuscated or minified
      code without its source, no downloading and running code at runtime.
- [ ] It does nothing destructive without the user asking (deleting,
      overwriting, moving files) and logs only to stderr / its data folder.
- [ ] A README that says what it does, what it needs (`runtime`) and how to
      uninstall any extra files it creates.
- [ ] One plugin per entry; the name doesn't impersonate Manta or another
      plugin; `manta.*` ids are reserved.

Updates: change `latest` (version, URL, SHA-256) in a PR. Maintainers may
remove entries that break the rules or stop working; security problems in a
listed plugin can be reported to the maintainers without a public issue
(contact on [manta-filemanager.com](https://manta-filemanager.com/)).

Manta itself doesn't download from the directory yet: users download the
file and install it from Settings, where the review and trust sheets apply
as for any other plugin.
