# Hello Node

A minimal [Manta](https://github.com/web-workers-com/manta) plugin in plain
Node.js, no npm packages. Copy this folder to start your own plugin.

It shows the basics of the plugin protocol (JSON lines over stdin/stdout,
see [`docs/plugins.md`](../../../docs/plugins.md)):

- **Say Hello** in the command palette (⇧⌘P): shows a toast.
- **Count Selection** in the file list's context menu: a toast with the
  number of selected items.
- A chip in the path bar with the number of items in the folder (local
  folders only: `local_path` is `null` for SFTP, FTP, S3 …).
- English and German texts, following Manta's language (`init` /
  `locale_changed`).

Files: `plugin.toml` (the manifest) and `main.js` (the plugin, commented).

## Requirements

Node.js 18 or newer on the `PATH` (Manta also looks in `/opt/homebrew/bin`
and `/usr/local/bin`): `brew install node`.

## Test it without Manta

From the repository root:

```sh
python3 scripts/plugin-test.py examples/plugins/hello-node
python3 scripts/plugin-test.py examples/plugins/hello-node --locale de -v
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
cd examples/plugins && zip -r hello-node.mantaplugin hello-node
```

## Make it yours

1. Change `id`, `name`, `description`, `author`, `repository` and
   `[localized.*]` in `plugin.toml`. Keep `permissions` honest.
2. Edit `main.js`. Log with `process.stderr.write` (shown in Settings ›
   Plugins › Show log); never `console.log`, stdout is only for protocol
   messages.

### TypeScript

Manta only runs what `command` names, so there is no build step here. To
write the plugin in TypeScript, keep the source in `main.ts`, compile it
to `main.js` (for example `npx tsc --target es2022 --module commonjs
main.ts`) and ship the compiled `main.js`. `command` stays `["node",
"main.js"]`.
