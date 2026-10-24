# Development

Notes for people working on Manta itself: building, releasing, testing, where the app keeps
its data, and known limitations. For the contribution workflow see
[CONTRIBUTING.md](../CONTRIBUTING.md); for the self-updater and publishing releases see
[updates.md](updates.md); for plugins see [plugins.md](plugins.md).

## Building

Requirements: macOS with the Xcode Command Line Tools and Rust via [rustup](https://rustup.rs/).

```sh
cargo run -p manta              # debug build
cargo run -p manta --release    # optimised build
```

Some features need the app bundle and are not available from `cargo run`, for example
becoming the default file manager (that needs `Manta.app` in `/Applications` or
`~/Applications`) and installing updates.

## Crates

| Crate | Purpose |
|---|---|
| `crates/core` | `Vfs` trait, `VfsPath`, shared types. Every backend implements `Vfs`; the operations layer and the GUI only talk to `Arc<dyn Vfs>` |
| `crates/vfs` | Backends: `LocalFs`, SFTP (`ssh2`), FTP/FTPS (`suppaftp` with its own rustls connector), S3 (own SigV4 client on `ureq`), Google Drive (Drive API v3 on `ureq`, OAuth PKCE; only offered after the user enters an OAuth client of their own, see [cloud.md](cloud.md)), Docker / Kubernetes (`ContainerFs` over the CLIs), read-only `ArchiveFs` (zip, tar, gz, bz2, xz, zst); `cloud_local` (iCloud / File Provider folders and their sync state) |
| `crates/ops` | `TransferManager` (copy, move, delete, extract, compress), `compare_dirs`, `plan_sync` |
| `crates/plugin` | Plugin contract, `PluginHost`, external plugin processes, JSON-lines protocol, localisation helpers (`manta_plugin::i18n`) |
| `crates/plugin-protocol` | Message types and a small stdio loop for writing Manta plugins in Rust; no dependency on Manta's internal crates |
| `crates/plugin-git` | Built-in Git plugin |
| `crates/plugin-terminal` | Built-in Terminal plugin |
| `crates/plugin-foldersize` | Built-in Folder Sizes plugin (size walker, treemap, Disk Usage panel) |
| `crates/plugin-rename` | Built-in Batch Rename plugin (rule engine, preview, presets, sheet) |
| `crates/plugin-markdown` | Built-in Markdown Preview plugin (pulldown-cmark parser, egui renderer, syntect highlighting, live reload) |
| `crates/plugin-quickactions` | Built-in Quick Actions plugin (shell actions on the selection, `actions.json`; see [quick-actions.md](quick-actions.md)) |
| `crates/plugin-projectinfo` | Built-in Project Info plugin (project detection, scripts, Project panel; see [project-info.md](project-info.md)) |
| `crates/plugin-duplicates` | Built-in Duplicates plugin (scanner, keep rules, moves extras to the Trash) |
| `crates/plugin-imagetools` | Built-in Image Tools plugin (Image panel, badges, resize / convert / optimise) |
| `crates/plugin-sysstats` | Built-in System Load plugin (CPU / GPU / memory sampler, status bar items) |
| `crates/updater` | Self-update engine (signed feed, download, verification, bundle swap; no UI). The app's UI is `crates/app/src/updates.rs` and `update_window.rs` |
| `crates/update-upload` | `manta-update-upload`: uploads release files over FTPS with the `crates/vfs` FTP backend (used by `scripts/publish-update.sh`) |
| `crates/app` | egui/eframe GUI (binary `manta`) |

## Building a release

```sh
scripts/build-release.sh
```

produces `dist/Manta.app` and `dist/Manta-<version>.dmg` (the version comes from
`[workspace.package]` in `Cargo.toml`). Requirements: macOS with the Xcode Command Line Tools
(`codesign`, `lipo`, `hdiutil`, `plutil`), rustup and `python3`. The script installs a
missing Rust target with `rustup target add`.

Steps:

1. Checks that [third-party-licenses.md](third-party-licenses.md) matches `Cargo.lock`
   (and lists no copyleft crate); a stale file stops the build. Regenerate it with
   `scripts/third-party-licenses.py`, review and commit it.
2. Release build for each target (fat LTO), with local paths remapped; for a universal build
   the binaries are joined with `lipo`.
3. Bundle: `Info.plist` from `packaging/macos/Info.plist.in`, `AppIcon.icns`, `LICENSE`,
   the third-party licence notices, and the English and German privacy-prompt strings
   (`packaging/macos/*.lproj`).
4. Signing (see below), then `codesign --verify --deep --strict`.
5. DMG with `Manta.app`, an `/Applications` link and a background image. The window layout
   is written by [dmgbuild](https://pypi.org/project/dmgbuild/) (installed once into a venv
   under `target/`); if that fails, the script falls back to scripting Finder, which may ask
   once whether your terminal may control Finder. Without either, the DMG is created without
   the custom layout. Compressed as UDZO.
6. Optionally notarization and stapling.

| Variable | Default | Meaning |
|---|---|---|
| `MANTA_SIGN_IDENTITY` | unset | `codesign` identity, e.g. `Developer ID Application: Jane Doe (AB12CD34EF)`. Unset: the self-signed identity below if it is in the login keychain, else ad-hoc |
| `MANTA_SELF_IDENTITY` | `Manta Code Signing` | name of the self-signed identity. It keeps the signature stable across releases, so macOS keeps granted permissions after updates; ad-hoc builds are asked again |
| `MANTA_NOTARY_PROFILE` | unset → skip | `xcrun notarytool` keychain profile |
| `MANTA_BUNDLE_ID` | `com.manta-filemanager.manta` | `CFBundleIdentifier` |
| `MANTA_ARCHS` | `arm64` | `arm64` (Manta supports Apple silicon only), `universal` (adds Intel), `x86_64` or `native` |
| `MANTA_MIN_MACOS` | `11.0` | `LSMinimumSystemVersion` and deployment target |
| `MANTA_DMG_LAYOUT` | `1` | `0` skips the window layout (plain DMG) |
| `MANTA_DMGBUILD` | venv under `target/` | path to your own `dmgbuild`, or `0` to skip it and use Finder scripting |
| `MANTA_DIST_DIR` | `dist` | output folder |
| `CARGO_TARGET_DIR` | `target/dist` | kept apart from dev builds (different `RUSTFLAGS`) |
| `MANTA_UPDATE_ZIP` | `0` | `1` also writes `dist/Manta-<version>.zip` for the auto-updater |
| `MANTA_UPDATE_URL` | `https://manta-filemanager.com/updates/update.json` | update feed URL, written into `Info.plist` and compiled in (empty: no feed) |
| `MANTA_UPDATE_PUBKEY` | `packaging/update-public-key.txt` | base64 Ed25519 public key that verifies updates |
| `MANTA_DMG` | `1` | `0` skips the DMG (only with `MANTA_UPDATE_ZIP=1`) |
| `MANTA_VERSION` | `Cargo.toml` | override the bundle version (testing update flows) |

Builds that are not notarized are blocked by Gatekeeper on first launch; users open them via
System Settings → Privacy & Security → **Open Anyway** (see the manual,
[Getting Started](manual/en/01-erste-schritte.md#gatekeeper)).

### Signing and notarization with a Developer ID

This needs a paid Apple Developer Program membership.

1. In Xcode → Settings → Accounts → Manage Certificates, create a *Developer ID
   Application* certificate (or download it from developer.apple.com). Check it with
   `security find-identity -v -p codesigning`.
2. Create an app-specific password at account.apple.com, then store the notary
   credentials once:
   `xcrun notarytool store-credentials manta-notary --apple-id you@example.com --team-id AB12CD34EF`
3. Build:
   ```sh
   MANTA_SIGN_IDENTITY="Developer ID Application: Jane Doe (AB12CD34EF)" \
   MANTA_NOTARY_PROFILE=manta-notary scripts/build-release.sh
   ```
   The app is signed with the hardened runtime and a secure timestamp
   (`packaging/entitlements.plist`); the DMG is signed and notarized, and the ticket is
   stapled to the DMG and the app.

### What the hardening does, and doesn't, protect

The release profile (`[profile.release]` in `Cargo.toml`) strips all symbols, turns off
debug info and uses fat LTO with one codegen unit. The script remaps local paths (home
folder, cargo registry, toolchain, checkout) to neutral ones such as `/cargo/…` and
`/manta/…`. C dependencies (vendored OpenSSL) bake their build folder into the binary;
`scripts/scrub-build-paths.py` overwrites those strings, and the build fails if any local
path is still left. The binary therefore doesn't reveal who built it or where, and without
symbols reverse engineering is more work. Signing and notarization prove where the app came
from and that it hasn't been modified.

None of this is encryption: the machine code can still be disassembled, and string literals
(UI texts, error messages, URLs, embedded plugin sources and resources) stay readable with
`strings`. Never put secrets (API keys, passwords) into the binary. `panic = "unwind"` stays
on purpose: the plugin host catches plugin panics with `catch_unwind`.

### Publishing updates

Releases reach users through a static, Ed25519-signed update feed (`update.json` +
`update.json.sig` + one ZIP per release) and a download folder for the DMG. In short:

```sh
cargo run -p manta-updater --bin manta-update-keygen   # once: key pair (private key in ~/.config/manta/)
scripts/publish-update.sh --setup-help                 # prints the one-time setup steps
scripts/publish-update.sh --dry-run                    # build, sign and stage; no upload
scripts/publish-update.sh                              # build, sign, upload, check the live feed
```

The upload destinations are not in the repository: they come from the environment or the
untracked `~/.config/manta/publish.env` (`MANTA_UPDATE_DEST`, `MANTA_DOWNLOAD_DEST`;
see `scripts/lib/publish-env.sh`). The FTP password stays in the macOS Keychain.
The website (https://manta-filemanager.com) is not part of this repository; an optional
`MANTA_SITE_PUBLISH` (path of an external publisher) is run after a stable release. The public key is compiled in, so build the release after creating the key and
commit `packaging/update-public-key.txt`; builds without it say "Updates aren’t set up in
this build" and never check. The private key must never be committed (`.gitignore` blocks
`*signing-key*`).

The feed format, signing scheme, threat model, web space layout and the engine API are
in [updates.md](updates.md).

### App icon

`packaging/icon/make_icons.py` regenerates the icons from code (SVG sources rendered with
resvg through `packaging/icon/render`, a small standalone crate): `AppIcon.icns`, the window
icon `crates/app/assets/app-icon.png` and the DMG background. The Manta icon set lives in
`packaging/icon/manta/` and is built with `--set manta` (for example
`python3 packaging/icon/make_icons.py --set manta --icon a`); without `--set`, the script
renders the earlier first-generation concepts.

## Tests

```sh
scripts/test.sh          # cargo test --workspace --lib --bins: unit and UI tests, no external services
scripts/test.sh full     # cargo test --workspace: also the integration tests in crates/*/tests
MANTA_RENDER_DIR=target/renders scripts/test.sh full    # also writes the UI screenshots
MANTA_TEST_CONTAINERS=1 scripts/test.sh full            # also the Docker / Kubernetes tests
```

- **README screenshots** (`docs/images/readme-overview.png`, `docs/images/readme-full.png`,
  2560 × 1600) come from synthetic fixtures only (fake Google Drive, a scripted Docker runner
  and terminal session, a throwaway Git repo, a fake load sampler):
  `MANTA_README_IMAGES=1 cargo test -p manta render_readme_images`.
- **UI tests** drive the real egui app headlessly. Tests that render screenshots are opt-in:
  they return early unless `MANTA_RENDER_DIR` is set, and also when no GPU adapter is
  available.
- **Integration tests** skip themselves when their service is missing:
  - **No test or render runs `docker` / `kubectl` unless `MANTA_TEST_CONTAINERS=1` is set.**
    In the app's tests the CLIs are blocked (`block_cli_for_tests`) and container discovery
    answers with made-up containers and pods (`containers::synthetic`), so screenshots can
    never show the developer's own containers.
  - Docker (`crates/vfs/tests/docker.rs`, terminal `remote*.rs`): only touches a throwaway `alpine` container it
    creates itself (label `manta-test`) and removes it afterwards; it never starts, stops or
    removes any other container.
  - Kubernetes (`kubernetes.rs`): read-only, and only against a local context (kind, minikube, Docker Desktop).
  - S3 (`s3_minio.rs`): a local `minio` binary, or MinIO in Docker (only with `MANTA_TEST_CONTAINERS=1`); `MANTA_S3_TEST_REQUIRE=1`
    turns a skip into a failure.
  - SFTP (`sftp_local.rs`): a throwaway, unprivileged `sshd` on 127.0.0.1.
- **Live tests** against real accounts are ignored by default and never run in CI:
  `ftps_live.rs` (read-only; server from `MANTA_FTPS_LIVE_HOST`, credentials from the
  Keychain) and `gdrive_live.rs` (needs your own OAuth client); see the comments at the top
  of those files.
- **Translations**: the catalog tests check every language (keys, placeholders, plurals, no
  hard-coded UI strings). `python3 scripts/i18n/check.py` runs the same checks without cargo. The
  glossary [i18n/glossary.md](i18n/glossary.md) is regenerated with
  `MANTA_UPDATE_GLOSSARY=1 cargo test -p manta glossary`.

Self-tests in a real run: `MANTA_MENU_SELFTEST=1` checks the generated menu bar;
`MANTA_DV_SELFTEST=status|on|off` checks or switches the default file manager role.

## Where Manta keeps its data

| What | Where |
|---|---|
| Settings, favorites, session, plugin state and data | `~/Library/Application Support/Manta/` (`settings.json`, `favorites.json`, `session.json`, …) |
| Unfinished transfers (no passwords) | `~/Library/Application Support/Manta/transfers.json` |
| Diagnostics log | `~/Library/Logs/Manta/manta.log` (rotates at 5 MB, keeps one backup). `MANTA_LOG=debug` (env_logger syntax) logs more |
| Thumbnail cache | `~/Library/Caches/Manta/thumbs` |
| Server passwords, key passphrases, S3 secret keys | macOS Keychain, service `com.manta-filemanager.manta.server`, account `protocol://user@host:port` |
| SFTP host keys | `~/.ssh/known_hosts` (`MANTA_KNOWN_HOSTS` points to another file, for tests) |

## Known limitations

- External plugins aren't sandboxed (see the security note in [plugins.md](plugins.md)).
- Project Info works on local folders only and doesn't evaluate `Makefile` includes or
  variables in target names.
- Markdown preview: no raw HTML beyond `<img>` and line breaks, no SVG images, no math
  rendering; remote documents are re-read on refresh and every 10 s instead of being watched.
- Quick Actions work on local folders only; `terminal` mode actions can't refresh the pane or
  report their result (they run in your shell).
- Terminal in containers and on servers: busybox `sh` (alpine) and bash 3 can't keep
  half-typed input when the terminal follows the pane (bash ≥ 4 and zsh can); a shell inside
  a container that isn't bash or POSIX `sh` follows only one way; file actions are shown as a
  card on the terminal instead of being printed through the remote shell; SFTP connections
  using a key file that isn't in your ssh config or agent ask in the terminal.
- Archives are read-only: no adding, deleting or renaming inside them. Password-protected zip
  entries are listed but can't be opened or extracted. Symlinks are shown as links and
  followed only inside the archive; extracting copies a link's target file (links to folders
  are skipped). Compressed tars are decompressed once into a temporary file (up to 2 GB) for
  fast access; bigger ones are re-read from the start for every file. Tar sparse files and
  zip "split" archives aren't supported.
- Default file manager: macOS can't replace Finder completely. The Desktop, the Finder icon
  in the Dock, double-clicks inside Finder windows and apps that talk to Finder directly
  (AppleScript `tell application "Finder"`) stay with Finder. Not available from `cargo run`
  or a DMG.
- Views: the Columns view starts at the folder shown when it was switched on (← past the
  first column moves it up); the Icons view has no free arrangement or "clean up" (items
  follow the sort order); thumbnails of server files are images only (no PDFs or videos),
  and files inside archives show their icons.
- Quick Look covers local files only; remote and archive items open in the built-in preview.
  Errors that LaunchServices reports after `open` started (double-click, Enter) go to the
  log, not to a toast.
- Implicit FTPS (port 990) is not supported.
- Servers, S3 and containers have no Trash: deleting there is permanent (Manta asks first and
  says so). Moving to the Trash has no Undo in Manta; use Finder's "Put Back".
- "Remove download" for File Provider folders (Google Drive for desktop, OneDrive, …) works
  only where the provider allows it; otherwise use Finder's "Remove Download".
- S3: rename / move inside a bucket is copy + delete (not atomic; objects over 5 GB can't be
  renamed); modification times and permissions can't be set; uploads buffer up to 16 MB in
  memory; AWS SSO, `credential_process` and assume-role profiles aren't supported (only
  static keys); deleting the last file of a folder without a folder marker makes the folder
  vanish.
- Containers: anything but reading needs `sh` in the container (distroless / `scratch`
  images are read-only; Kubernetes pods need `sh`). Listing a folder of a *stopped*
  container reads its whole subtree as a tar, capped at 512 MB. Writes go through `cat`, so
  file ownership is the exec user's. Kubernetes uses the context, namespace and container
  stored with the connection; no port-forwarding, no ephemeral debug containers. Discovery in
  the sidebar runs `docker ps -a` and `kubectl get` every 5 s while the sidebar is visible.
  Manta never starts, stops or removes containers.
- The conflict policy is chosen per job, not asked per file.
