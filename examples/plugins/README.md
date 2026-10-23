# Plugin templates

Small, commented example plugins for [Manta](https://github.com/web-workers-com/manta).
Copy one to start your own; the protocol (JSON lines over stdin/stdout,
`api_version = 1`) is described in [`docs/plugins.md`](../../docs/plugins.md).

| Template | Language | Needs | Shows |
| --- | --- | --- | --- |
| [`hello-python`](hello-python) | Python 3, standard library | `python3` | badges, chips, palette and context-menu commands, a side panel |
| [`hello-node`](hello-node) | Node.js, no npm packages | `node` | chips, palette and context-menu commands |
| [`hello-shell`](hello-shell) | bash + `jq` | `jq` (macOS 15+ built in) | chips, palette and context-menu commands |
| [`hello-rust`](hello-rust) | Rust, [`manta-plugin-protocol`](../../crates/plugin-protocol) crate | `cargo build --release` | chips, palette and context-menu commands |

All of them follow Manta's language (English and German), declare
`license`, `repository`, `min_manta_version` and `permissions` in
`plugin.toml`, log to stderr only and exit on `shutdown` or when stdin
closes.

**Test** a plugin without Manta (from the repository root):

```sh
python3 scripts/plugin-test.py examples/plugins/hello-node
python3 scripts/plugin-test.py examples/plugins/hello-node --locale de
```

**Install** it: Settings › Plugins › „Install Plugin…“, then choose the
folder, or a `.zip` / `.mantaplugin` of it.

**Package** it: a `.mantaplugin` is a zip whose root contains
`plugin.toml`, or one folder containing it:

```sh
cd examples/plugins && zip -r hello-node.mantaplugin hello-node
```
