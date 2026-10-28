# Cloud storage

Manta reaches cloud storage in two ways:

1. **Google Drive, natively** (`manta_vfs::gdrive`): a `Vfs` backend over the Drive API v3, with
   its own browser sign-in. Hidden in the app unless the user enters an OAuth client of their own
   (Settings › General › Cloud › Expert); Manta ships none.
2. **Cloud folders that macOS already syncs** (`manta_vfs::cloud_local`): iCloud Drive and the
   File Provider folders in `~/Library/CloudStorage` (Google Drive for desktop, OneDrive, Dropbox,
   Box, …). These are local folders (`LocalFs`). Manta finds them, shows their download state
   and can start a download or remove one.

Both APIs are blocking. Call them on a background thread, never on the UI thread.

---

## 1. Google Drive backend

### API for the UI

```rust
use manta_vfs::gdrive::{self, AuthCancel};

// Has this build (or the runtime env / config file) got an OAuth client?
if !gdrive::is_configured() {
    // Show gdrive::NOT_CONFIGURED ("Google Drive is not set up in this build").
}

// Virtual folder names follow the UI language; both languages always resolve.
gdrive::set_language("de"); // "Meine Ablage", "Für mich freigegeben", "Geteilte Ablagen"

// "Sign in with Google" button, on a background thread:
let cancel = AuthCancel::new();          // keep a clone for the Cancel button: cancel.cancel()
let cfg = gdrive::sign_in(
    |url| { let _ = std::process::Command::new("open").arg(url).status(); },
    &cancel,
)?;                                       // stores the refresh token in the Keychain
// cfg: ConnectionConfig { protocol: Protocol::GoogleDrive, username: <email>,
//                         host: "drive.google.com", name: "Google Drive (<email>)", .. }
// Save it as a favorite like any other connection, then:
let fs = manta_vfs::connect(&cfg)?;       // SharedVfs (GDriveFs)
```

| Function | Blocking | Notes |
|---|---|---|
| `gdrive::is_configured() -> bool` | no | Whether an OAuth client is available. |
| `gdrive::sign_in(open_url, &AuthCancel) -> VfsResult<ConnectionConfig>` | up to 5 min | Runs the browser flow, stores the refresh token in the Keychain (service `manta-gdrive`, account = email). Errors: `Cancelled`, `Auth` (the user denied access, bad client), `Other` (timeout), `Unsupported(NOT_CONFIGURED)`. |
| `gdrive::authorize(&OAuthClient, open_url) -> VfsResult<Tokens>` | up to 5 min | The bare OAuth flow without storing anything. `authorize_with(.., &AuthCancel, timeout)` can be cancelled. |
| `manta_vfs::connect(&cfg)` | yes (one token refresh) | `cfg.protocol == Protocol::GoogleDrive`. `Auth("not signed in to Google Drive as …")` means the Keychain has no token: show "Sign in again". |
| `ConnectionConfig::google_drive(email, start_folder)` | no | Builds a connection, e.g. for a favorite with a starting folder (`initial_path`, such as `/My Drive/Projects`). |
| `gdrive::has_sign_in(email) -> bool` | Keychain read | Is a token stored? |
| `gdrive::sign_out(email)` | network | Revokes the token at Google (best effort) and deletes it from the Keychain. |
| `gdrive::Area::{MyDrive, SharedWithMe, SharedDrives}.path()` | no | Paths of the three virtual top-level folders in the current language. |

UI hints:

- The sidebar icon is `icons::CLOUD`, or Phosphor `google-drive-logo` if it is added.
  `Protocol::is_cloud()` is true for S3 and Google Drive.
- `fs.home()` is `/My Drive`. `cfg.initial_path` is honoured by the app, as for other backends.
- `Entry::permissions == Some(0o444)` marks read-only items: Google Docs exports, and Forms and
  Sites (`EntryKind::Other`). Disable "Edit" or "Save" for them. Rename, move and trash still work.
- Writing at `/`, at the top level of "Shared with me" or of "Shared drives" fails with
  `PermissionDenied` or `Unsupported`.
- `rename` replaces an existing target *file* (it goes to the trash), like the local backend.
- The OAuth success page in the browser says, in German and English: "you can close this window
  and return to Manta".

### Behaviour

- **Path model**
  - `/` shows "My Drive", "Shared with me" and "Shared drives".
  - Below that, each folder's listing is fetched page by page from `files.list`. It is cached
    for path→id resolution for 60 s, and every `list()` call refreshes it.
  - Shortcuts resolve to their targets.
- **Duplicate names**
  - Drive allows several items with the same name in one folder.
  - The oldest item (by `createdTime`, then id) keeps the name. The others get `name (2).ext`,
    `name (3).ext`, …, skipping names that already exist.
  - The mapping is deterministic and only changes when an older duplicate goes away.
- **Slashes:** a `/` in a Drive name appears as `∕` (U+2215) and is mapped back on create and rename.
- **Google-native files:** they are listed with an extension and exported on read with
  `files.export`.
  - Docs → `.docx`
  - Sheets → `.xlsx`
  - Slides → `.pptx`
  - Drawings → `.pdf`

  They are read-only: `open_write` returns `PermissionDenied`. Their size shows as 0 because
  Drive doesn't report one. Google caps exports at 10 MB, and bigger files fail with
  `Unsupported`.
- **Uploads**
  - Uploads use the resumable protocol in 8 MB chunks (`gdrive::UPLOAD_CHUNK`). Memory use is
    one chunk.
  - The file appears on Drive only when `flush()` completes. Dropping an unflushed writer cancels
    the session, so a cancelled copy leaves nothing behind.
  - Writing to an existing path updates that file: the same id, with its version history kept.
- **Delete moves to Drive's trash.** `remove_file`, `remove_dir` and `remove_tree` set
  `trashed=true`. Nothing is deleted permanently, and the user can restore items at
  drive.google.com → Trash for 30 days.
- **mtime:** `set_modified` sets `modifiedTime`, so copies keep their dates.
- **Errors and retries**
  - **Retries:** 429, 5xx, `rateLimitExceeded` and `userRateLimitExceeded` are retried up to
    6 times with exponential backoff and jitter. `Retry-After` is honoured.
  - **401:** refreshes the access token once and retries. Access tokens are also refreshed a
    minute before they expire.
  - **Other errors** map to `VfsError` with friendly text:
    - "storage is full"
    - "daily limit"
    - "Google Docs … read-only"
    - "sign in again" (revoked or expired grant)
    - "the Google Drive API is not enabled …"
- **Tokens:** tokens never appear in logs, and `Debug` redacts them. Refresh tokens live only in
  the Keychain. If Google rotates one, the new one is stored.

### Where the OAuth client comes from

**Manta never ships an OAuth client** and never acts as an OAuth publisher toward Google: Google
Drive normally comes through Google Drive for desktop (section 2). The native backend is hidden
from the UI unless the user brings a client of their own. The first match wins
(`OAuthClient::configured()`):

1. `MANTA_GDRIVE_CLIENT_ID` and `MANTA_GDRIVE_CLIENT_SECRET` in the **environment of the running
   app** (development and the self-test).
2. `~/Library/Application Support/Manta/gdrive-client.json`, containing
   `{"client_id": "…", "client_secret": "…"}` (mode 0600). The app writes it from **Settings ›
   General › Cloud › Expert: Your Own Google OAuth Client** (`OAuthClient::save_file`; an empty id
   removes the file).

There is no build-time client any more. Without one of these the backend reports
`NOT_CONFIGURED`, and the Connect dialog doesn't offer "Google Drive"
(`ConnectForm::gdrive`, set from `CloudSettings::own_client()`). With a client, the dialog has a
Google Drive tab (account + start folder); an empty account signs in with the browser
(`gdrive::sign_in`) before connecting.

### Setting up a Google OAuth client (by the user, Expert setting)

1. Open <https://console.cloud.google.com/>, then **Select a project → New project** (for example
   "Manta").
2. **APIs & Services → Library → Google Drive API → Enable.**
3. **Google Auth Platform → Branding** (formerly "OAuth consent screen"):
   - App name "Manta", a user support email and a developer contact email.
   - Audience: **External**. Leave the publishing status at **Testing**.
   - **Audience → Test users → Add users:** add the Google accounts that may sign in (at most 100).
     In Testing mode, only these accounts can sign in. Their refresh tokens **expire after
     7 days**, after which Manta asks them to sign in again.
4. **Data access → Add or remove scopes:** add `https://www.googleapis.com/auth/drive` ("See,
   edit, create and delete all of your Google Drive files"). This is a **restricted** scope.
5. **Clients → Create client → Application type: Desktop app**, name "Manta Desktop".
   Copy the **Client ID** (`….apps.googleusercontent.com`) and the **Client secret**.
   Desktop clients need no redirect URI; the loopback `http://127.0.0.1:<port>` is allowed
   automatically.
6. Build or run with the two values:
   ```sh
   export MANTA_GDRIVE_CLIENT_ID=1234-abc.apps.googleusercontent.com
   export MANTA_GDRIVE_CLIENT_SECRET=GOCSPX-…
   cargo run -p manta-vfs --bin manta-gdrive-selftest -- --sign-in   # browser sign-in + round trip
   cargo run --release                                                  # the app with Drive enabled
   ```
   `manta-gdrive-selftest --email you@gmail.com` reuses the stored sign-in. `--keep` keeps the
   test folder, and `--sign-out` removes the sign-in afterwards. The ignored live test runs with:
   ```sh
   MANTA_GDRIVE_TEST_EMAIL=you@gmail.com cargo test -p manta-vfs --test gdrive_live -- --ignored
   ```

**Public use (more than the test users)** means switching the app to **In production**. Because
`drive` is a restricted scope, Google then requires **app verification**:

- a verified domain with a homepage and a privacy policy;
- a demo video of the OAuth flow and the scope use;
- a justification of why full Drive access is needed (a file manager);
- a yearly **CASA security assessment** by a third-party assessor, which costs money.

Until the app is verified, unverified production apps show a warning screen and are capped at
100 users. Narrower scopes such as `drive.file` would avoid the assessment, but they only show
files that Manta created or that the user opened with the Google Picker, which is not useful for
a file manager.

### Limitations

- Docs, Sheets, Slides and Drawings are read-only exports (10 MB export limit). Forms, Sites,
  Maps and Apps Script are listed but can't be opened.
- Deleting is trash-only. Emptying the trash happens at drive.google.com.
- No permissions/sharing UI. "Shared with me" is read-mostly: nothing can be created at its top
  level, and trashing a file you don't own fails with "access denied".
- Shared drives can be browsed and written, but not created or deleted.
- Moving a folder into a shared drive is refused by Google ("Google Drive doesn't allow this").
- Listings are cached for path resolution for 60 s. Changes made elsewhere show up on the next
  `list()` of that folder.
- If a 5xx answer arrives mid-chunk, the chunk is retried as is, without querying the upload
  status first.
- One account per connection. Several accounts mean several favorites.

---

## 2. System cloud folders (iCloud Drive, File Provider)

### Discovery

```rust
use manta_vfs::cloud_local::{self, CloudLocation, CloudProvider};

let locations: Vec<CloudLocation> = cloud_local::discover_cloud_locations(); // fast (a few read_dirs); $MANTA_CLOUD_HOME overrides ~
for l in &locations {
    // l.display_name  "iCloud Drive", "Google Drive (name@gmail.com)", "OneDrive – Contoso Travel GmbH"
    // l.provider      CloudProvider::{ICloud, GoogleDrive, OneDrive, Dropbox, Box, Other}
    // l.provider_id   "icloud", "google-drive", "onedrive", "dropbox", "box", or e.g. "nextcloud"
    // l.path          ~/Library/Mobile Documents/com~apple~CloudDocs, ~/Library/CloudStorage/<name>
    // l.icon          SF Symbol name; l.provider.phosphor() is the Phosphor icon name
    // l.account       Some("name@gmail.com") / Some("ContosoTravelGmbH") / None
    // l.file_provider true for ~/Library/CloudStorage folders
}
let loc = cloud_local::location_of(path, &locations); // which location a path is in
```

- iCloud Drive comes first, if `~/Library/Mobile Documents/com~apple~CloudDocs` exists.
- Then come the folders in `~/Library/CloudStorage`, sorted by name. Hidden entries and files
  are skipped.
- A classic `~/Dropbox` (with a `.dropbox` marker) is included when no File Provider Dropbox exists.
- The folders are opened with `LocalFs`, like any other folder. For a sidebar section:
  one row per location, with `display_name` and the icon.

How folder names are parsed (`parse_cloud_storage_name`):

| Folder name | Display name |
|---|---|
| `GoogleDrive-name@gmail.com` | Google Drive (name@gmail.com) |
| `OneDrive-Personal` | OneDrive – Personal |
| `OneDrive-ContosoTravelGmbH` | OneDrive – Contoso Travel GmbH (spaces put back) |
| `OneDrive-SharedLibraries-…` | OneDrive – Shared Libraries |
| `Dropbox`, `Dropbox-Team` | Dropbox, Dropbox – Team |
| `Box-Box` | Box |
| `ProtonDrive-a@b.c-…`, `SynologyDrive-nas`, `Nextcloud-…` | Proton Drive (…), Synology Drive – nas, Nextcloud – … |

### iCloud Drive's app folders

Like Finder, iCloud Drive's top folder (`com~apple~CloudDocs`) also lists the apps' folders
(`~/Library/Mobile Documents/<container>/Documents`) under the app's name
(`NSFileManager displayNameAtPath:`). `LocalFs::list` adds them; their `Entry.path` is the
container's `Documents` folder. Which containers show is iCloud's decision; there is no public
API, so `crate::icloud` reads `brctl dump -i` and keeps the app libraries flagged
`DocumentScopePublic` without `no-documents`, matching their obfuscated ids (`i{4}d.md.o{6}n`)
against the folder names. If that fails, no app folders show. With `MANTA_CLOUD_HOME` set
(tests, demos), every container with something in `Documents` shows.

App folders can't be renamed, moved or deleted (`LocalFs` refuses with `PermissionDenied`, the UI
with a note); their contents can. `cloud_local::location_of` counts them as iCloud Drive, so the
chip, badges and path bar (`icloud::crumb_base`, `icloud::folder_label`) treat them as part of it.

With "File icons from macOS" on, an app folder shows its app's icon: `icloud::bundle_id` turns the
container's name into a bundle id (`iCloud~md~obsidian` -> `md.obsidian`, team prefixes dropped),
`thumbs::icloud_app` finds the app with `NSWorkspace URLForApplicationWithBundleIdentifier:`
(remembered), and the views draw its icon like an `.app` (`Method::AppIcon`). Without the app the
folder keeps the folder glyph. Quick Look and `NSWorkspace iconForFile:` both return a plain folder
for these folders, hence the detour.

### Free space

`Vfs::space` reports the free space where a folder is stored (`manta_core::Space`): the disk
(`NSURLVolumeAvailableCapacityForImportantUsageKey`, like Finder) for local folders and SMB
mounts, the iCloud quota (`brctl quota`) inside `Mobile Documents`, Drive's `about.storageQuota`,
SFTP's `fstatvfs@openssh.com` and WebDAV's RFC 4331 quota. The status bar asks off the UI thread
and keeps answers per folder (`crates/app/src/space.rs`: 10 s local, 60 s remote, stale after a
transfer).

### Sync status and badges

Reading the status never touches file contents, so it never triggers a download. It reads the
`NSURL` resource values `NSURLIsUbiquitousItemKey`, `NSURLUbiquitousItemDownloadingStatusKey`,
`…IsDownloadingKey` and `…IsUploadingKey`, plus the `SF_DATALESS` flag from `lstat`
(`st_flags`), which marks File Provider placeholders.

```rust
let state = cloud_local::item_status(&path);                 // one item
let all = cloud_local::folder_status(&dir)?;                 // Vec<(name, SyncState)>, sorted
let cache = cloud_local::SyncStatusCache::default();         // per folder; mtime + 3 s TTL
let states = cache.folder(&dir)?;                            // Arc<HashMap<name, SyncState>> (blocking)
let maybe = cache.peek(&dir);                                // UI thread: cached value only
cache.invalidate(&dir);                                      // after an action
```

Only query folders inside a cloud location (`location_of`). The value is `SyncState::Local`
everywhere else.

| `SyncState` | Meaning | Badge (SF Symbol / Phosphor) | Actions |
|---|---|---|---|
| `Local` | not a cloud item | — | — |
| `Current` | downloaded, up to date | — (or a subtle check) | Remove download |
| `CloudOnly` | only in the cloud (iCloud "not downloaded", File Provider dataless) | `icloud.and.arrow.down` / `cloud-arrow-down` | Download now |
| `Outdated` | downloaded, newer version in the cloud | `exclamationmark.icloud` / `cloud-warning` | Download now, Remove download |
| `Downloading` | being downloaded | `arrow.down.circle` / `arrow-circle-down` | — |
| `Uploading` | local changes being uploaded | `arrow.up.circle` / `cloud-arrow-up` | — |

`SyncState::sf_symbol()`, `phosphor()`, `can_download()` and `can_evict()` encode this table.

Checked read-only against real folders on this Mac:

- iCloud Drive and OneDrive report `Current` and `CloudOnly` through the ubiquity keys.
- Google Drive for desktop (`My Drive`) reports `CloudOnly` for streamed files.

### Actions

- **`cloud_local::start_download(path)`**: "Download now". Folders are handled recursively.
  - iCloud: `NSFileManager startDownloadingUbiquitousItemAtURL:`. It is asynchronous; poll the
    status to follow it.
  - File Provider domains: the same call where the provider supports it. If the call fails and
    the file is dataless, Manta **reads its first byte**, and macOS then materialises the whole
    file. That fallback blocks until the download finishes.
- **`cloud_local::evict(path)`**: "Remove download", through
  `NSFileManager evictUbiquitousItemAtURL:`.
  - This works for iCloud Drive.
  - For other File Provider domains it works only if the provider accepts it. Otherwise the call
    returns `VfsError::Unsupported`, with a hint to use Finder's "Remove Download".
    `NSFileProviderManager.evictItem` is not available to apps outside the provider, because it
    needs the provider's own domain object.
  - Nothing is deleted from the cloud.

In the app (`crates/app/src/cloud.rs`):

- Sidebar section **Cloud** between Locations and Devices, one row per location (provider name;
  the account is shown dimmed only when a provider appears more than once, always in the
  tooltip). Hidden when nothing is found. Rediscovered when `~/Library/CloudStorage` changes
  (live refresh watches it) and on the section's refresh button. Without a Google Drive folder a
  dismissible "Connect Google Drive…" row explains Google Drive for desktop
  (`Prefs::gdrive_hint_dismissed`).
- States of the visible cloud folders are read on a background thread (3 s TTL, re-read after
  live-refresh events and actions) and handed to the tab (`Tab::cloud`); List and Icons views
  draw the badge, the context menu offers **Download Now** / **Remove Download** only where they
  apply (`cloud::menu_offer`). Failures (e.g. a provider refusing eviction) become an error toast.
- `fileproviderctl` on current macOS has no evict command and `brctl download` is iCloud-only and
  undocumented, so neither is used.

Tests:

- `cloud::CloudProbe` abstracts the file system side; `FakeProbe` simulates `st_flags`
  (`SF_DATALESS`) on a temp fixture (`cloud_ux_tests.rs`). `MANTA_CLOUD_HOME` overrides the home
  folder for discovery.
- The tests never download or evict real files. Name parsing and discovery run on fixture
  folders in a temp `Library/CloudStorage`.
- A manual, read-only look at the real folders:
  `cargo test -p manta-vfs cloud_local -- --ignored --nocapture`.
