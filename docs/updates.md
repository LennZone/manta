# Auto-updates

Manta updates itself from a **static feed on your own web space**: no server-side code,
no database, no third-party service. Anything that serves files over HTTPS will do
(shared hosting, a VPS with nginx/Apache, S3 + CloudFront, GitHub Pages, …).

The engine is the `manta-updater` crate (`crates/updater`, no UI). It is a small custom
updater, not Sparkle: Ed25519 signatures (`ed25519-dalek`), SHA-256, `ureq` for HTTP, and
macOS tools (`ditto`, `codesign`, `plutil`) for bundles.

```
check ──► download ──► prepare ──► install ──► relaunch
signed     streaming,    ditto -x,    atomic swap   helper waits for the
feed,      resumable,    bundle ID,   (+ one admin  old process to exit,
semver,    size+SHA-256  version,     prompt if     then `open`s the
channel,   +Ed25519      codesign,    needed),      new version
min_macos                Team ID      old → Trash
```

## Quick start

```sh
# 1. once: create the signing key pair
cargo run -p manta-updater --bin manta-update-keygen
#    → ~/.config/manta/update-signing-key (secret, mode 0600) — BACK IT UP
#    → packaging/update-public-key.txt       (public, commit it; compiled into the app)

# 2. once: the FTP password into the macOS Keychain (prompts for it; never in a file)
security add-generic-password -s manta-update-ftp -a <ftp-user> -w

#    and the upload destinations into an untracked local file (no passwords there)
mkdir -p ~/.config/manta && cat > ~/.config/manta/publish.env <<'EOF'
MANTA_UPDATE_DEST=ftp://ftp.example.com/updates/
MANTA_DOWNLOAD_DEST=ftp://ftp.example.com/download/
EOF
chmod 600 ~/.config/manta/publish.env

# 3. every release: bump the version in Cargo.toml ([workspace.package] version), then
scripts/publish-update.sh            # first run creates release-notes/<version>.{en,de}.md and stops
$EDITOR release-notes/0.2.0.en.md release-notes/0.2.0.de.md
scripts/publish-update.sh --dry-run  # optional: look at dist/update-staging/ first
scripts/publish-update.sh            # build, sign, upload, check the live feed
```

Add `MANTA_SIGN_IDENTITY` / `MANTA_NOTARY_PROFILE` as for any release ([development.md](development.md#building-a-release)); the publish script passes everything on to `scripts/build-release.sh`.
`scripts/publish-update.sh --setup-help` prints the one-time steps.

## The official setup

| | |
|---|---|
| Feed URL (compiled into every build) | `https://manta-filemanager.com/updates/update.json` (`MANTA_UPDATE_URL` overrides). |
| Downloads | `https://manta-filemanager.com/download/`: `Manta-<version>.dmg`, `Manta.dmg` (latest stable, the website's button) and `latest.json` (`MANTA_DOWNLOAD_URL` / `MANTA_DOWNLOAD_DEST` override). |
| Website | `https://manta-filemanager.com/` (maintained outside this repository; `MANTA_SITE_PUBLISH` can name its publisher, run after every stable release, `MANTA_PUBLISH_SITE=0` skips it). |
| Upload | FTPS (explicit TLS) to an FTPS web space whose FTP account is rooted in the web root, so its `/` is `https://manta-filemanager.com/`. The destinations are **not in the repository**: `MANTA_UPDATE_DEST` and `MANTA_DOWNLOAD_DEST` come from the environment or from the untracked `~/.config/manta/publish.env` (`MANTA_PUBLISH_ENV` names another file; the environment wins; see `scripts/lib/publish-env.sh`). The scripts stop with an example if they are missing. The upload runs `manta-update-upload` (`crates/update-upload`), i.e. Manta's own FTP backend. The login is always encrypted; if the network resets encrypted data connections, it continues with a clear data channel (the files are public and Ed25519-signed). `MANTA_FTP_DATA_TLS=require` forbids that. |
| FTP host | The name the FTP server's TLS certificate is issued for. On shared hosting that is often the provider's server name rather than your own domain; TLS is always verified, so only the certificate's name works. |
| Credentials | macOS Keychain, generic password, service `manta-update-ftp`, account = FTP user. The script reads the account from the item and the password with `security find-generic-password -s manta-update-ftp -a <user> -w`. |
| Signing key | `~/.config/manta/update-signing-key` (`MANTA_UPDATE_KEY` overrides) |
| Bundle id | `com.manta-filemanager.manta` (`MANTA_BUNDLE_ID` overrides) |

The password never lands in a file, in a command line (`ps`) or in the output:
`security … -w` writes it into a shell variable and a shell builtin (`printf`) pipes it to
the uploader's stdin.

**Why Rust, and what the uploader does.** `manta-update-upload` uses the same FTPS code as
an FTPS pane in the app (`crates/vfs/src/ftp/`), so every fix for the release server also
fixes browsing it in Manta:

* TLS via rustls with the macOS certificate check. Every data connection resumes the TLS
  session of its control connection (ProFTPD `RequireSessionReuse`, vsftpd
  `require_ssl_reuse`).
* Passive mode always connects to the control host (the address in the PASV reply is
  ignored; curl's "Bad PASV/EPSV response: 150" came from that and EPSV), and falls back
  between `PASV` and `EPSV` when the server refuses one.
* Data channel policy (`--data-channel` / `MANTA_FTP_DATA_TLS`): `auto` (default) tries an
  encrypted data connection and continues with `PROT C` (clear data, encrypted login) if
  the handshake is reset or times out, and prints a note; `require` never does; `off`
  starts clear. Some company networks reset every encrypted FTP data connection (observed
  with curl, Python `ftplib` with session reuse, TLS 1.2 and 1.3; a raw TCP connection to
  the PASV port works), so on such a network `auto` is the only setting that uploads.
* Each file goes to `NAME.uploading`, then is renamed (if the server refuses to rename over
  an existing file, the target is deleted first); sizes are checked after the upload and
  after the rename. Order: ZIP(s), `update.json.sig`, `update.json`.
* Missing remote folders are created (MKD; an existing folder is fine, also when the
  server only answers a vague `550`). Nothing is ever deleted except a file that is being
  replaced by its new version.

By hand (the scripts do exactly this; the password only ever travels on stdin):

```sh
cargo build --release -p manta-update-upload
U=target/release/manta-update-upload
security find-generic-password -s manta-update-ftp -w | $U ftp://ftp.example.com/updates/ FILE...
security find-generic-password -s manta-update-ftp -w | $U --list ftp://ftp.example.com/
security find-generic-password -s manta-update-ftp -w | $U --mkdir ftp://ftp.example.com/updates/
$U --tree website --exclude README.md --exclude .DS_Store --dry-run ftp://ftp.example.com/
security find-generic-password -s manta-update-ftp -w \
  | $U --tree website --exclude README.md --exclude .DS_Store ftp://ftp.example.com/
```

`--list DIR` lists a folder (read only). `--mkdir DIR` creates it and its parents.
`--tree LOCAL` uploads a folder's contents recursively (subfolders and assets first,
`.html` pages last); `--exclude` takes a name (any depth) or a relative path with a `/`;
`--dry-run` prints the plan without logging in. Symbolic links are refused.

The user comes from `--user`, the URL (`ftp://user@host/`), `MANTA_FTP_USER` or the
Keychain item's account. The remote folder is absolute as the FTP account sees it (for this
chrooted account `/` is the web folder).

## What you need to provide

| What | Example | Notes |
|---|---|---|
| Feed URL | `https://manta-filemanager.com/updates/update.json` | **HTTPS** with a valid certificate (Let's Encrypt is fine). Compiled into every build, so choose a URL you can keep. |
| Upload access | `ftp://ftp.example.com/updates/` (FTPS) or `you@example.com:/var/www/example.com/manta/` (SSH) | FTP must support TLS. For SSH, key login recommended (`ssh-copy-id`). |
| The private key, backed up | `~/.config/manta/update-signing-key` | Password manager or an encrypted USB stick. Lost key = installed copies can't be updated any more. |

## Web space layout

```
https://manta-filemanager.com/          (= the FTP account's /)
├── index.html, assets/, …    the website (not part of this repository)
├── updates/
│   ├── update.json           the feed (small, never cached)
│   ├── update.json.sig       detached Ed25519 signature of update.json
│   ├── Manta-0.2.0.zip       one ZIP per version (immutable once uploaded)
│   ├── Manta-0.1.3.zip       (mirrored from the old feed when the new one was seeded)
│   └── .htaccess             (Apache) MIME types and caching, see below
└── download/
    ├── Manta-0.2.0.dmg       every published DMG
    ├── Manta.dmg             copy of the latest stable DMG (the website's button)
    └── latest.json           {version, dmg, url, latest_url, size, sha256, published}
```

New entries carry an **absolute** URL (`https://manta-filemanager.com/updates/Manta-<v>.zip`).
Older entries may be relative (`Manta-0.1.3.zip`); the app resolves those against the feed
URL it asked for (not the one a redirect ended at), so they work both from the new feed
and through the old folder's redirect. Old ZIPs can be deleted once they drop out of
`update.json` (the publish script keeps the newest 10 entries; `--keep N`).
`latest.json` and `Manta.dmg` change only for stable releases.

`.htaccess` for Apache (shared hosting); nginx equivalent below:

```apache
# Manta update feed
<IfModule mod_mime.c>
    AddType application/json .json
    AddType text/plain .sig
    AddType application/zip .zip
</IfModule>

# The feed must never be cached, or clients see updates late (or a manifest
# that doesn't match its signature).
<FilesMatch "^update\.json(\.sig)?$">
    <IfModule mod_headers.c>
        Header set Cache-Control "no-cache, no-store, must-revalidate"
        Header set Pragma "no-cache"
        Header unset ETag
    </IfModule>
    FileETag None
</FilesMatch>

# ZIPs never change after upload: cache them for a long time.
<FilesMatch "\.zip$">
    <IfModule mod_headers.c>
        Header set Cache-Control "public, max-age=31536000, immutable"
    </IfModule>
</FilesMatch>

Options -Indexes
```

```nginx
location /updates/ {
    types { application/json json; text/plain sig; application/zip zip; }
    location ~ ^/updates/update\.json(\.sig)?$ { add_header Cache-Control "no-cache, no-store, must-revalidate"; }
    location ~ \.zip$ { add_header Cache-Control "public, max-age=31536000, immutable"; }
}
```

The server should support HTTP `Range` requests (all common servers do); interrupted
downloads then resume instead of starting over.

## Feed format (`update.json`, schema 1)

```json
{
  "schema": 1,
  "app_id": "com.manta-filemanager.manta",
  "channel": "stable",
  "generated": "2026-10-21T10:00:00+00:00",
  "releases": [
    {
      "version": "0.2.0",
      "channel": "stable",
      "published": "2026-10-21T10:00:00+00:00",
      "min_macos": "11.0",
      "notes": { "en": "- New: …", "de": "- Neu: …" },
      "url": "Manta-0.2.0.zip",
      "size": 21734521,
      "sha256": "9f2c…(64 lowercase hex)",
      "signature": "base64 Ed25519 signature (64 bytes)"
    }
  ]
}
```

* `schema`: a client refuses a feed with a higher schema and tells the user to download the
  new version by hand. Unknown fields are ignored (forward compatible).
* `version`: semantic version, compared with semver. Pre-releases (`0.3.0-beta.1`) sort
  before the final version.
* `channel` per release: `stable` or `beta`. Missing: `beta` for pre-release versions,
  else `stable`. Stable clients see stable releases only; beta clients see both. The
  top-level `channel` is informational.
* `min_macos`: releases that need a newer macOS than the running one are skipped; the
  newest compatible release is offered instead.
* `notes`: Markdown, English required, German optional (falls back to English).
* `url`: relative to `update.json`, or an absolute `https://` URL (e.g. a CDN).

The client offers the **newest** release that is newer than the running version, on an
accepted channel, compatible with the OS, and newer than a version the user skipped
("Skip This Version" skips that version and everything older). It never downgrades.

## Signing scheme

One Ed25519 key pair. The public key is compiled into the app; the private key only ever
lives on the publisher's Mac.

**Per release.** `signature` signs this exact text (UTF-8, `\n` line ends, trailing `\n`):

```
manta-update-release-v1
app_id=com.manta-filemanager.manta
version=0.2.0
channel=stable
min_macos=11.0
size=21734521
sha256=9f2c…
```

The context line separates it from manifest signatures; `app_id`, `version`, `channel`,
`min_macos`, `size` and `sha256` bind the signature to exactly one ZIP of one version of one
app, so it can't be replayed for another version, another app or a relabelled channel.
Field values are validated (semver, digits-and-dots OS version, 64 hex digits) so the text
can't be made ambiguous. The URL is deliberately not signed (mirrors are fine: the hash
pins the content).

**Whole feed.** `update.json.sig` (detached, one base64 line) signs
`"manta-update-manifest-v1\n" + <exact bytes of update.json>`. Signing the raw bytes
avoids JSON canonicalisation. The client verifies it before parsing anything, and verifies
the chosen release's own signature again before downloading and after (size + SHA-256).
If the pair doesn't match (e.g. an upload is half done), the client fetches both once more
after 1.5 s before failing.

`manta-update-sign manifest` refuses to merge into a live `update.json` whose signature
doesn't verify with your key, so an entry injected on the server is never re-signed.

## Threat model

Protected:

* **Tampered or replaced files on the web space or in transit** (hacked hosting account,
  rogue CDN, MITM despite TLS): the client installs nothing that isn't signed with the
  private key. A changed ZIP fails SHA-256/size, a changed or forged entry fails the
  release signature, a changed feed (notes, URLs, versions) fails the manifest signature.
* **Signature replay**: an old release's signature can't be attached to another version,
  app or channel (they are in the signed text).
* **Downgrades**: only versions newer than the running one are offered, so an attacker
  can't push an old, vulnerable signed version.
* **Wrong app in a valid ZIP**: the unpacked bundle must have the expected bundle ID and
  exactly the promised version, and `codesign --verify --deep --strict` must pass.
* **Team ID pinning**: if the running app is signed with a Developer ID, the update must be
  signed by the same team (`codesign -dv`). Ad-hoc builds accept ad-hoc (or team-signed)
  updates and rely on Ed25519.
* **Plain HTTP**: refused (also after redirects); `http://` is only allowed for
  `127.0.0.1`/`localhost` (tests).
* **Half-installed app**: the swap is atomic (`renamex_np(RENAME_SWAP)` on APFS, or two
  renames with rollback), the result is checked, and on failure the old bundle is restored.

Not protected:

* **A stolen private key.** Whoever has it can ship updates to every user. Keep it off
  servers and out of git (`.gitignore` blocks `*signing-key*`), back it up offline. There
  is no key rotation yet: a new key needs a release signed with the old key that embeds
  the new one.
* **A lost private key.** Installed copies can't be updated any more; users have to
  download a new version manually.
* **Freeze attacks.** Someone controlling the web space can serve an *old, validly
  signed* feed and so withhold new updates (they can't install anything old or forged).
  `generated` is in the feed for a future "feed is older than the last one seen" check.
* **Availability.** A deleted or unreachable feed simply means "no update".
* **Your build machine.** If it's compromised, the attacker signs with your key.
* **Privacy.** The web server sees IP addresses and the `Manta-Updater/<version>`
  user agent of checking clients.

## Building with update support

| Setting | Where | Meaning |
|---|---|---|
| `packaging/update-public-key.txt` | file | Public key compiled in (`crates/updater/build.rs`). |
| `MANTA_UPDATE_PUBKEY` | env at build time | base64 public key; wins over the file (CI). |
| `MANTA_UPDATE_URL` | env at build time | Feed URL: written into `Info.plist` as `MantaUpdateFeedURL` **and** compiled in. |
| `MANTA_UPDATE_ZIP=1` | `scripts/build-release.sh` | Also writes `dist/Manta-<version>.zip` (`ditto -c -k --keepParent` of the signed, stapled app). |
| `MANTA_DMG=0` | `scripts/build-release.sh` | Skip the DMG (only with `MANTA_UPDATE_ZIP=1`). |
| `MANTA_VERSION` | `scripts/build-release.sh`, `publish-update.sh` | Override the version in `Info.plist` and file names (testing update flows). Normally bump `Cargo.toml`. |

Feed URL precedence at run time (`manta_updater::configured_feed_url()`):

1. `MANTA_UPDATE_URL` in the environment of the running app (testing only),
2. `MantaUpdateFeedURL` in the running bundle's `Info.plist`,
3. `MANTA_UPDATE_URL` compiled in.

The plist key lets you point an already built app at another URL without recompiling
(changing `Info.plist` breaks the code signature, so re-sign afterwards). Without a public
key or a feed URL the updater reports **"Updates are disabled: …"** with the reason, and
the app works as before.

The app's current version comes from its bundle's `CFBundleShortVersionString`
(`manta_updater::current_version(env!("CARGO_PKG_VERSION"))`).

## Keys

```sh
cargo run -p manta-updater --bin manta-update-keygen               # new key pair
cargo run -p manta-updater --bin manta-update-keygen -- --show-public  # rewrite public key file from the private key
cargo run -p manta-updater --bin manta-update-keygen -- --force    # replace the key (breaks updates for existing installs!)
```

Options: `--private-key PATH`, `--public-key PATH`. The private key file is
`# comment` lines plus the base64 32-byte Ed25519 seed, mode 0600 in a 0700 folder; it is
never overwritten without `--force`.

## Publishing

`scripts/publish-update.sh [--dry-run] [--skip-build] [--no-dmg] [--setup-help] [--channel stable|beta] [--keep N]`

1. Checks the key (exists, mode 0600) and the public key the app will embed.
2. Release notes from `release-notes/<version>.en.md` and `.de.md`. If missing, it
   writes templates and stops; it also stops while the template's `TODO` line is there.
3. `MANTA_UPDATE_ZIP=1 scripts/build-release.sh` (skipped with `--skip-build`).
4. Fetches the live `update.json` + `.sig`. 404 → the **seed feed**
   (`MANTA_UPDATE_SEED_URL`, default empty = none) is used as the starting point if it can be fetched, else a new feed.
   Any other error stops (so a network hiccup can't wipe the history);
   `MANTA_UPDATE_NEW_FEED=1` overrides. If HTTPS fails because of the certificate (e.g.
   before Let's Encrypt is enabled), the script warns and asks over `http://` instead;
   that is safe for this purpose because step 5 only accepts a feed whose signature
   verifies with your key. The app itself never uses `http://`.
5. `manta-update-sign manifest`: verifies the live (or seed) feed and all its entries with
   your key, adds or replaces this version with the absolute URL
   `<feed folder>/Manta-<version>.zip` (size, SHA-256, signature), keeps the newest N,
   signs `update.json`.
6. `manta-update-sign verify` with the **public key file** (catches a key mismatch).
   After seeding, the ZIPs of the kept entries with relative URLs are downloaded from the
   seed feed's folder and checked against their signed size and SHA-256 (they are uploaded
   in step 7). The DMG (`dist/Manta-<version>.dmg`, required unless `--no-dmg`) is staged
   in `download/`, for stable releases with `Manta.dmg` and `latest.json`.
7. Uploads the ZIP(s) first, then `update.json.sig` + `update.json`, so a client never
   sees a feed pointing at a missing ZIP; then the download files to
   `MANTA_DOWNLOAD_DEST` (`Manta-<version>.dmg`, `Manta.dmg`, `latest.json`, FTPS only):
   * **FTPS** (default, `MANTA_UPDATE_DEST=ftp://host/path/`; the path as the FTP account
     sees it): `manta-update-upload` (explicit TLS; plain FTP is refused), uploads to
     `*.uploading`, renames and verifies the sizes. The account comes from the Keychain
     item (`MANTA_FTP_KEYCHAIN_SERVICE`, default `manta-update-ftp`) or `MANTA_FTP_USER`;
     the password from the Keychain (asked for interactively if the item has none, never
     stored). `MANTA_FTP_DATA_TLS=auto|require|off` sets the data channel policy. See
     "The official setup".
   * **SSH**: `MANTA_UPDATE_DEST=user@host:/path/manta/`. `rsync -e ssh` (`scp` if rsync
     is missing). The feed pair is sent with `--delay-updates`, so both files switch at
     the same moment.
8. Downloads the live feed again and verifies it; checks that the ZIP(s) and download
   files answer HTTP 200 (warnings only; over `http://` while HTTPS is broken, with a
   warning that the app can't update until the certificate works).

`--dry-run` stops after step 6 and leaves everything in `dist/update-staging/`
(`MANTA_UPDATE_STAGING`): copy that folder anywhere to publish by hand. It also checks
that the Keychain item exists (without reading the password or logging in) and prints the
exact upload command a real run would execute.

The pre-rename `RUSTLIFT_*` variable names still work, with a deprecation warning.

Beta releases: give the version a pre-release suffix (`0.3.0-beta.1`, channel `beta`
automatically) or pass `--channel beta`.

## Website

The website `https://manta-filemanager.com/` is not part of this repository. After a real
stable publish, `scripts/publish-update.sh` runs the executable named by
`MANTA_SITE_PUBLISH` (environment or `~/.config/manta/publish.env`) so the site can follow
the release; unset, it prints a note and skips it, and `MANTA_PUBLISH_SITE=0` skips it as
well. A failure there only warns: the update is already live. The site must never write to
`/updates/` or `/download/`.

The download button links `https://manta-filemanager.com/download/Manta.dmg`; the page
reads `download/latest.json` (same origin) and shows "Version x.y.z · size" when it is
there.

## In the app

The app wires the engine in through `crates/app/src/updates.rs` (controller, no UI;
talks to the engine through a small `Backend` trait so tests use a fake) and
`crates/app/src/update_window.rs` (window, Settings section, status bar hint).

| | |
|---|---|
| Launch check | Every launch, in the background, when "Check for updates at launch" is on (default). There is no 24 h throttle (`interval_hours` is 0 = every launch; internal, no UI). A failure (offline, 404, bad signature) is only logged, never shown. |
| Update found at launch | A toast "Update available: 0.2.0" with **Show**, and a small "Update 0.2.0" hint in the status bar. No window pops up. A skipped version stays silent (the engine leaves it out). |
| Manual check | **Manta → Check for Updates…** (after "About Manta"), the palette command, or Settings → **Check Now**: opens the "Software Update" window, which shows "Checking…", then "Manta is up to date", the error with **Try Again**, or the update. |
| Update window | Native window: app icon, "Manta 0.2.0 is available — you have 0.1.0", size and date, the release notes (Markdown in the UI language, rendered with the Markdown plugin's renderer; web links open in the browser), **Install and Relaunch** / **Later** / **Skip This Version**. |
| Install | After confirmation only: download with a progress bar (MB done / total) and **Cancel**, then install (one macOS password prompt if the app folder isn't writable) and quit; the helper opens the new version. While file transfers run, it waits for them. Closing the window during a download keeps it going; when it's done, the status bar says "Update 0.2.0 ready". Errors show in the window with **Try Again**. |
| Can't replace itself | From a DMG, an App Translocation copy or a development build, the window explains what to do ("Drag Manta to the Applications folder first …") and **Install** is off. |
| Settings → General → Updates | Check at launch (switch), channel Stable / Beta, current version, last check, **Check Now**, a skipped version with **Reset**. |
| Updates not set up | Builds without the public key (or feed URL) never check; the menu item reads "Updates Not Set Up" and opens Settings, which says why in plain words. |
| State | `~/Library/Application Support/Manta/update-state.json` (`UpdateState`). |
| Log | `~/Library/Logs/Manta/manta.log`, lines starting with `updates:` (version, bundle path, feed URL at start-up; every status). |

`MANTA_UPDATE_SELFTEST=1` (testing): the app clicks through the whole flow by itself
(launch check → **Install and Relaunch** → download → install → quit; the helper relaunches
the new version), logging `update selftest: …` lines; it gives up after 3 minutes or on the
first error and quits.

## API for the UI

```rust
use manta_updater::{Channel, UpdateState, UpdateStatus, Updater, UpdaterConfig};

// At start-up (cheap, no I/O except reading Info.plist):
let updater = match UpdaterConfig::for_running_app(state.channel, env!("CARGO_PKG_VERSION")) {
    Ok(cfg) => Some(Updater::new(cfg)),
    Err(e) => { /* show e.to_string() as "Updates are disabled: …" in Settings */ None }
};
let ctx2 = ctx.clone();
updater.set_waker(move || ctx2.request_repaint());

// Automatic check (every launch by default, see `interval_hours`):
if state.check_due(chrono::Utc::now()) { updater.check(state.skipped_version.clone()); }

// Every frame:
for s in updater.poll() {
    match s {
        UpdateStatus::UpToDate | UpdateStatus::Available(_) => { state.last_check = Some(chrono::Utc::now()); /* save */ }
        UpdateStatus::ReadyToInstall(_) => { /* remember updater.downloaded_zip() in state.downloaded */ }
        UpdateStatus::Installed { .. } => { /* save everything, then quit: the helper relaunches */ }
        _ => {}
    }
}
```

`Updater` (cheap to clone, all methods return immediately; work runs on one background
thread at a time, a second call while busy returns `false`):

| Method | Effect / resulting states |
|---|---|
| `check(skipped: Option<Version>)` | `Checking` → `UpToDate` / `Available(Release)` / `Failed(msg)` |
| `download()` | needs `Available`; `Downloading{done,total}` (≤ 10/s) → `ReadyToInstall(Release)` |
| `download_release(release)` | same for a specific release |
| `restore_download(release, zip)` | re-verifies a ZIP from `UpdateState::downloaded` → `ReadyToInstall` |
| `cancel()` | stops a download → `Idle` (the `.part` file stays and is resumed next time) |
| `install_and_relaunch()` | needs `ReadyToInstall`; `Installing` → `Installed{relaunch}` → **quit the app** |
| `install_blocker()` | `Some(reason)` when the app runs from a DMG, App Translocation or `cargo run` (show it before offering "Install") |
| `location_problem()` | the same as a `LocationProblem` value (to explain it in the UI's language) |
| `last_error()` | the `UpdateError` behind the latest `Failed` (to localise the message by kind) |
| `status()`, `poll()`, `is_busy()`, `set_channel()`, `config()`, `set_waker(f)` | |

States: `Idle, Checking, UpToDate, Available(Release), Downloading{done,total},
ReadyToInstall(Release), Installing, Installed{relaunch}, Failed(String)`. `Failed`
messages are user-facing ("Could not reach the update server: cannot resolve example.com
(offline?)", "Signature check failed: …", "Manta can't update itself here: …").
`Release::notes.for_language("de")` gives the Markdown notes for the UI language.

`UpdateState` (serde, `#[serde(default)]`, store it as JSON with the settings):
`auto_check` (true), `interval_hours` (0 = every launch), `channel`, `last_check`, `skipped_version`,
`downloaded: Option<{release, zip}>`. Helpers: `check_due(now)`,
`forget_stale_download(current_version)` (call at start-up: drops the remembered ZIP once
that version is installed), and the free function `should_check(last, now, interval)`.

Lower-level functions (used by the facade, handy for tools): `check`, `check_with`,
`download`, `download_with`, `verify_file`, `prepare`, `prepare_with`, `install` →
`InstallPlan::apply`, `relaunch_after_exit(pid, app)`.

## Installing: what happens on disk

* Refused, with a reason: running from a read-only volume (DMG), from an App
  Translocation path, or not from a bundle (`cargo run`).
* **Writable folder** (the usual drag-installed `/Applications/Manta.app`, owned by
  the user): the new app is moved next to the old one as `.Manta.app.update-…`, then
  both are exchanged atomically with `renamex_np(RENAME_SWAP)` (two renames with rollback
  on file systems without it). The bundle at the original path is checked
  (`CFBundleShortVersionString`); if that fails the swap is undone. The old version goes
  to the Trash (`NSFileManager trashItemAtURL`, Finder as a fallback).
* **Not writable** (root-owned app, e.g. installed by a `.pkg`): one macOS password
  prompt runs this script as root, with every path single-quoted (see
  `install::admin_script`):
  ```sh
  set -e; /usr/bin/ditto '<new>' '<stage>' || { /bin/rm -rf '<stage>'; exit 1; };
  /usr/sbin/chown -R "$(/usr/bin/stat -f %u:%g '<app>')" '<stage>';
  /bin/mv '<app>' '<old>';
  if /bin/mv '<stage>' '<app>'; then /bin/rm -rf '<old>'; else /bin/mv '<old>' '<app>'; /bin/rm -rf '<stage>'; exit 1; fi
  ```
* Relaunch: `/bin/sh -c 'while kill -0 <pid>; do sleep 0.2; done; open <app>'` in its own
  process group (gives up waiting after 60 s). The app quits itself after `Installed`.
* The downloaded ZIP is deleted after a successful install; ZIPs live in
  `~/Library/Caches/Manta/updates`.

## Tests

* `cargo test -p manta-updater`: unit tests (manifest parsing and validation,
  canonical messages and signatures incl. tampered hash, version replay, wrong app ID, wrong
  key, semver/channel/`min_macos`/skipped selection, HTTPS-only rule, install location
  checks, admin-script quoting, codesign parsing and Team ID rule, scheduling, state
  serde) and integration tests against a local HTTP server on 127.0.0.1 with a fake,
  ad-hoc-signed bundle (check → download with progress → prepare → install into a temp
  "Applications" folder, backup handling, tampered ZIP, tampered/foreign-key/missing
  manifest signature, wrong bundle ID/version, broken code signature, cancel mid-download
  and resume with `Range`, the `Updater` facade end to end, an old feed URL behind a
  folder redirect for feed, signature and ZIP).
* `cargo test -p manta-update-upload`: uploads via `.uploading` + rename, size checks,
  servers that refuse to rename over a file or answer vague `550`s, folder creation,
  recursive tree upload (excludes, pages last, other remote files untouched), listing.
* `manta-update-selftest --feed URL --app PATH [--channel beta] [--pubkey FILE]
  [--backup keep|delete|trash] [--relaunch]`: runs the real pipeline on a real bundle
  (used for the end-to-end run with two release builds, a `--dry-run` feed and
  `python3 -m http.server`).
* `cargo test -p manta updates`: the app's update controller against a fake engine
  (launch check once and silent on failure, manual check results, install / Later / Skip /
  Cancel / Retry, waiting for transfers, re-verifying an earlier download, the blocker
  texts, the disabled build, settings persistence) and `update_ux` (toast and status bar
  hint open the window, the window's buttons, the menu command in a build without key,
  Settings → Updates) plus renders of every window state (`$MANTA_RENDER_DIR`).
* End to end with the real app: build 0.1.0 and 0.1.1-test with a throwaway key
  (`MANTA_UPDATE_PUBKEY`, `MANTA_UPDATE_URL=http://127.0.0.1:<port>/update.json`,
  `MANTA_ARCHS=native MANTA_DMG=0 MANTA_UPDATE_ZIP=1`, the second with
  `MANTA_VERSION=0.1.1-test`), stage the feed with `publish-update.sh --dry-run --skip-build
  --channel stable` (`MANTA_UPDATE_KEY`, `MANTA_UPDATE_STAGING`, `MANTA_RELEASE_NOTES_DIR`),
  serve the staging folder with `python3 -m http.server --bind 127.0.0.1`, copy the 0.1.0 app
  into a temporary "Applications" folder and start its binary with `MANTA_UPDATE_SELFTEST=1`.
  The relaunched app logs `updates: Manta 0.1.1-test …`.

## Before the first real release

1. `cargo run -p manta-updater --bin manta-update-keygen` → back up
   `~/.config/manta/update-signing-key`, commit `packaging/update-public-key.txt`.
2. Build a release **after** that (`scripts/build-release.sh` or `scripts/publish-update.sh`),
   so the key is compiled in. Only builds with the key can update themselves; copies
   already handed out without it have to be replaced by hand once.
3. Keychain item for the FTP password, release notes, `scripts/publish-update.sh --dry-run`,
   then `scripts/publish-update.sh`.
