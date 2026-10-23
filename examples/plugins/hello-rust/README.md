# Hello Rust

A minimal [Manta](https://github.com/web-workers-com/manta) plugin in Rust,
using the [`manta-plugin-protocol`](../../../crates/plugin-protocol) crate
(message types and the stdin/stdout loop). Copy this folder to start your
own plugin.

It shows the basics of the plugin protocol (JSON lines over stdin/stdout,
see [`docs/plugins.md`](../../../docs/plugins.md)):

- **Say Hello** in the command palette (⇧⌘P): shows a toast.
- **Count Selection** in the file list's context menu: a toast with the
  number of selected items.
- A chip in the path bar with the number of items in the folder (local
  folders only: `local_path` is `None` for SFTP, FTP, S3 …).
- English and German texts, following Manta's language (`init` /
  `locale_changed`).

Files: `plugin.toml` (the manifest), `Cargo.toml` and `src/main.rs` (the
plugin, commented).

## Build it

```sh
cd examples/plugins/hello-rust
cargo build --release
```

`plugin.toml` runs `./target/release/hello-rust`, so build before testing
or installing. `Cargo.toml` has its own empty `[workspace]` table: the
template isn't part of the Manta workspace and builds into its own
`target/` (ignored by `.gitignore`).

After copying the template out of the Manta repository, replace the `path`
dependency in `Cargo.toml` with the git one:

```toml
manta-plugin-protocol = { git = "https://github.com/web-workers-com/manta" }
```

## Test it without Manta

From the repository root (after `cargo build --release`):

```sh
python3 scripts/plugin-test.py examples/plugins/hello-rust
python3 scripts/plugin-test.py examples/plugins/hello-rust --locale de -v
```

It plays a session like Manta would and checks every line the plugin
prints. It must end with `OK: 0 error(s)`.

## Install it in Manta

Settings › Plugins › „Install Plugin…“, then choose this folder (or a
`.zip` / `.mantaplugin` of it). Manta shows the name, author, license and
`permissions` from `plugin.toml` and asks you to trust the plugin.

## Distribute it

Users shouldn't need a Rust toolchain. Ship the compiled binary instead of
`target/`:

```sh
mkdir -p bin && cp target/release/hello-rust bin/
```

and point `command` at it in `plugin.toml`:

```toml
command = ["./bin/hello-rust"]
```

For both Apple Silicon and Intel Macs, build a universal binary
(`cargo build --release --target aarch64-apple-darwin` and
`--target x86_64-apple-darwin`, then `lipo -create -output bin/hello-rust
…`). Then package the folder without `target/`. A `.mantaplugin` is a zip
whose root contains `plugin.toml`, or one folder containing it:

```sh
cd examples/plugins && zip -r hello-rust.mantaplugin hello-rust -x 'hello-rust/target/*'
```

## Make it yours

1. Change `id`, `name`, `description`, `author`, `repository` and
   `[localized.*]` in `plugin.toml`, and `name` in `Cargo.toml` (the binary
   name in `command` follows it). Keep `permissions` honest.
2. Edit `src/main.rs`. Log with `eprintln!` (shown in Settings › Plugins ›
   Show log); never `println!`, stdout is only for protocol messages.
