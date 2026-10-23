# Quick Actions

Your own commands for the selected files: resize images, zip, open in VS
Code, checksums, rsync to the other pane, and anything else a shell command
can do. Built into Manta (Settings → Plugins → Quick Actions, on by
default). It replaces the former bundled Python plugin `quick-actions`: an
installed copy of that is switched off at startup, and its `actions.json`
(with your edits) is taken over the first time the built-in starts.

## Use

- **Right-click** files: the **Quick Actions** submenu lists the actions
  that apply to the selection (e.g. "Resize Images to 50%" only for images).
- **Command palette** (⇧⌘P): the same actions, plus **Edit Quick Actions…**,
  **Reload Quick Actions** and **Show Quick Actions Panel**.
- **Quick Actions panel** (right sidebar; **Show Quick Actions Panel** in the
  palette): the applicable actions as a list, and **Last runs** with
  status, duration and the output of the selected run. Running and waiting
  jobs have a **Cancel** button.

Background actions run at most two at a time. One that takes longer than a
moment shows a toast with **Cancel**; when it ends you get a result toast,
with the error output if it failed. Terminal actions are typed into
Manta's terminal below the pane (it opens if needed) and run there
visibly, in your shell history.

Quick Actions work on local folders only.

## actions.json

The actions live in `actions.json` in the plugin's data folder
(`~/Library/Application Support/Manta/plugin-data/manta.quickactions/`). It is
created with commented defaults on first run. **Edit Quick Actions…** opens
it in your default text editor; when you save it, the plugin reloads it
within a second (and tells you if something is wrong).

The file is JSON with comments (`//`, `/* */`) and trailing commas allowed.
An entry `{"//": "…"}` in the list is a comment too.

```jsonc
{
  "actions": [
    {
      "id": "resize-50",                 // unique: letters, digits, . - _
      "title": "Resize Images to 50%",   // menu, palette and panel
      "icon": "image",                   // panel icon (optional)
      "applies_to": ["jpg", "jpeg", "png", "heic"],
      "command": "w=$(sips -g pixelWidth {file} | awk '/pixelWidth/ {print $2}') && sips --resampleWidth $((w / 2)) {file} --out {dir}/{stem}-50%.{ext}",
      "message": "Resized {count} image{s}",
      "refresh": true
    }
  ]
}
```

| field | meaning | default |
|---|---|---|
| `id` | unique id; letters, digits, `.`, `-`, `_` | required |
| `title` | shown in the context menu, palette and panel | required |
| `title_de` | German title, used when Manta is in German (Settings → General → Language) | `title` |
| `command` | shell command template, run with `/bin/sh -c` in the folder of the selection | required |
| `applies_to` | list of `"any"`, `"files"`, `"folders"` and/or extensions (`"jpg"`, `"tar.gz"`; case doesn't matter). Every selected item must match one entry | `["any"]` |
| `min`, `max` | how many items must be selected | `1`, no limit |
| `mode` | `"background"`: runs hidden, progress toast after a moment, result toast (stderr on failure). `"terminal"`: typed into Manta's terminal (`run_in_terminal`), runs visibly. `"silent"`: runs hidden, a toast only if it fails | `"background"` |
| `output` | `"none"`, `"toast"` (stdout as the result toast) or `"panel"` (opens the panel and shows stdout there) | `"none"` |
| `confirm` | question shown in a sheet before running, e.g. `"Delete {count} file{s}?"` | none |
| `message` | success toast text | "<title>: done" |
| `refresh` | list the pane again afterwards (and the other pane if the command uses `{dest}`). Not for `terminal` mode | `false` |
| `icon` | panel icon: `file`, `folder`, `image`, `archive`, `text`, `code`, `terminal`, `copy`, `link`, `eye`, `key`, `lock`, `clock`, `check`, `warning`, `info`, `download`, `open`, … | `file` |
| `shortcut` | global key, e.g. `"⌃⌥Z"` | none |
| `message_de`, `confirm_de` | German `message` / `confirm`, used when Manta is in German | `message` / `confirm` |

`message` and `confirm` understand `{count}` (number of items), `{s}` ("s"
unless one item) and `{title}`.

### Language

The plugin follows Manta's language (English or German) and switches
live when you change it. The default actions are translated by their `id`
as long as you haven't changed their English `title` (or `message` /
`confirm`); a title you wrote yourself is shown exactly as written. To give
your own action a German title, add `title_de` (and optionally
`message_de`, `confirm_de`):

```jsonc
{
  "id": "optimize",
  "title": "Optimize PNG",
  "title_de": "PNG optimieren",
  "applies_to": ["png"],
  "command": "optipng {file}",
  "message": "Optimized {count} image{s}",
  "message_de": "{count} Bild(er) optimiert"
}
```

### Placeholders

| placeholder | value |
|---|---|
| `{files}` | every selected item, absolute paths |
| `{names}` | every selected name (relative to `{dir}`, the working directory) |
| `{file}` | one item; **the command runs once per selected item** |
| `{name}`, `{stem}`, `{ext}` | that item's name, name without extension, extension without the dot (also per item) |
| `{dir}` | the folder the items are in |
| `{dest}` | the folder shown in the other pane (the action is unavailable if that pane isn't local) |
| `{count}` | the number of selected items |

Don't mix `{file}`-style and `{files}`-style placeholders in one command.
Other `{words}` (awk programs, shell `{ …; }` groups) are left alone.

**Quoting is done for you**, based on where the placeholder stands:

- bare (`cp {files} {dest}/`): each value becomes a properly quoted shell
  word, whatever characters the name contains (spaces, quotes, `$`, `*`,
  newlines). Concatenation works: `{dir}/{stem}.jpg`.
- inside `"double quotes"` (`echo "Created {name}.zip"`): the plain text,
  escaped for double quotes.
- inside `'single quotes'` (`sh -c 'ls {file}'`): quoted words for the inner
  shell.

Never wrap a bare placeholder in your own quotes "just in case": `'{file}'`
would hand the inner quotes to the command.

## Default actions

Resize Images to 50% (`sips`), Convert to JPEG / PNG (`sips -s format`),
Compress to ZIP (`ditto` for one item, `zip -r` into `Archive.zip` for
several), Extract Archive (`ditto -x -k` / `tar -xf`), Open in VS Code
(`code`, or the app; a clear error if neither exists), Open in Terminal.app,
Copy Path (`pbcopy`), Reveal in Finder (`open -R`), Make Executable
(`chmod +x`), Count Lines (`wc -l`, as a toast), Calculate SHA-256
(`shasum -a 256`, in the panel) and Copy to Other Pane (`rsync -a
--progress` in the terminal, after a confirmation).

To get the current defaults after an update, delete or rename your
`actions.json`; it is created again.
