# Hello Shell

A minimal [Manta](https://github.com/web-workers-com/manta) plugin as a
bash script, using [`jq`](https://jqlang.org) for JSON. Copy this folder to
start your own plugin.

It shows the basics of the plugin protocol (JSON lines over stdin/stdout,
see [`docs/plugins.md`](../../../docs/plugins.md)):

- **Say Hello** in the command palette (⇧⌘P): shows a toast.
- **Count Selection** in the file list's context menu: a toast with the
  number of selected items.
- A chip in the path bar with the number of items in the folder (local
  folders only: `local_path` is `null` for SFTP, FTP, S3 …).
- English and German texts, following Manta's language (`init` /
  `locale_changed`).

Files: `plugin.toml` (the manifest) and `main.sh` (the plugin, commented).

## Requirements

- `jq`: part of macOS 15 and newer (`/usr/bin/jq`). On older macOS:
  `brew install jq`.
- `main.sh` must be executable, because `command = ["./main.sh"]` runs it
  directly (relative to the plugin folder):

  ```sh
  chmod +x main.sh
  ```

  Git keeps the mode; when you zip the folder, `zip` keeps it too.

## Test it without Manta

From the repository root:

```sh
python3 scripts/plugin-test.py examples/plugins/hello-shell
python3 scripts/plugin-test.py examples/plugins/hello-shell --locale de -v
```

It plays a session like Manta would and checks every line the plugin
prints. It must end with `OK: 0 error(s)`.

## Install it in Manta

Settings › Plugins › „Install Plugin…“, then choose this folder (or a
`.zip` / `.mantaplugin` of it). Manta shows the name, author, license and
`permissions` from `plugin.toml` and asks you to trust the plugin.

## Package it

A `.mantaplugin` is a zip whose root contains `plugin.toml`, or one folder
containing it:

```sh
cd examples/plugins && zip -r hello-shell.mantaplugin hello-shell
```

## Make it yours

1. Change `id`, `name`, `description`, `author`, `repository` and
   `[localized.*]` in `plugin.toml`. Keep `permissions` honest.
2. Edit `main.sh`. Rules that keep it working:
   - Only JSON goes to stdout. Log with `log …` (stderr); send `echo` and
     `printf` output there too (`>&2`).
   - Build messages with `jq -nc --arg name "$value" '{…}'`, never by
     pasting strings together: file names can contain quotes, backslashes
     and newlines.
   - The script handles one message at a time. Slow work (a big `find`,
     a network call) blocks everything after it, so keep handlers quick.
