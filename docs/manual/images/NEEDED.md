# Screenshots needed

Every image the manual references, by file name.

**Generated images** (✅ below) come from the synthetic test fixtures and are written by the `render_manual_images*` tests in all seven UI languages (`de`, `en`, `zh-Hans`, `es`, `fr`, `it`, `ru`; the UI language is switched per run, Chinese with the PingFang SC font like the app), light theme, cropped, scaled to at most 1600 px wide and stored as 256-colour PNGs:

```sh
MANTA_MANUAL_IMAGES=1 cargo test -p manta render_manual_images -- --test-threads=1
# only some languages:
MANTA_MANUAL_IMAGES=1 MANTA_MANUAL_LANGS=zh-Hans,ru cargo test -p manta render_manual_images -- --test-threads=1
```

Dark variants (`NAME-dark.png`) aren't generated yet; the help window shows the light image in dark mode until they exist. Images that are still missing appear in the help window as a quiet placeholder with the alt text. Each one is needed in **every language** (`de/NAME.png`, `en/NAME.png`, `zh-Hans/NAME.png`, …, see `../README.md`) unless noted; the column "de/en" says "both" for those. A language without its own copy of an image shows the English one (then any other language's). "Theme" says which appearance to render; "both" means a light and a dark variant (`NAME.png` and `NAME-dark.png`), otherwise light only.

Window size for full-window shots: 1440 × 900 pt at 2x, accent colour Blue, default font size. Use neutral sample data (no real names, hosts or e-mail addresses): a home folder `anna`, a project `manta-demo`, a server `files.example.com`.

| File | Chapter | What it should show | Theme | de/en | Status |
|---|---|---|---|---|---|
| `overview-two-panes.png` | 01 | Main window: toolbar, sidebar (Favorites, Locations, Devices, Servers), two panes with a few folders and files, status bar. | both | both | ✅ generated (light) |
| `default-file-manager-prompt.png` | 01 | The "Use Manta as your default file manager?" sheet with Set as Default / Not Now / Don't Ask Again. | light | both | ✅ generated (light) |
| `command-palette.png` | 01 | Command palette open with "new fo" typed, matching rows with shortcuts. | both | both | ✅ generated (light) |
| `panes-three.png` | 02 | Three panes, the middle one active (accent outline). | light | both | ✅ generated (light) |
| `panes-target-badge.png` | 02 | Three panes; active pane with a selection; target pane with faint outline and → badge in its path bar (tooltip "Copy and Move (F5 / F6) go to this pane" visible). | light | both | ✅ generated (light) |
| `views-overview.png` | 03 | 2×2 collage of one folder of photos in Icons, List, Columns and Gallery view, path bar with the View switcher visible in each. | light | both | ✅ generated (light; 2×2 collage of single-pane renders) |
| `view-options.png` | 03 | Close-up of a path bar in Icons view: View switcher (Icons selected) and the open View Options popover (Sort By Name, Ascending, Icon Size slider). | both | both | ✅ generated (light) |
| `view-list.png` | 03 | List view sorted by Size (descending), Folder Sizes values incl. one "≥ … GB" still counting. | light | both | ✅ generated (light) |
| `view-columns.png` | 03 | Column view three levels deep, a JPEG selected so the preview column shows the thumbnail and Information (Kind, Size, Modified, Permissions). | light | both | ✅ generated (light) |
| `view-gallery.png` | 03 | Gallery view of a photo folder: large photo, name and "Kind · size · date" line, filmstrip with the selected item ringed. | both | both | ✅ generated (light) |
| `path-bar.png` | 04 | Close-up of a path bar on an SFTP server: location chip ("SFTP · user@host") with the "Back to Local" arrow, breadcrumb, a Git chip and a Project Info chip, filter field. | both | both | ✅ generated (light; two path bars stacked: an in-memory SFTP server with Back to Local, and a local Git project with the Git and Project chips) |
| `go-to-path.png` | 04 | Go to Path popup with `~/Do` typed, "Matching folders" and "Recent" lists, footer with key hints. | light | both | ✅ generated (light; a fake home folder for `~`) |
| `copy-dialog.png` | 05 | Copy sheet for 3 items with 3 panes: From, To picker, "If an item already exists" options. | light | both | ✅ generated (light) |
| `transfers-panel.png` | 05 | Transfers panel with one running copy (progress, speed, time left, pause/stop icons) and one finished job. | light | both | ✅ generated (light) |
| `transfers-paused-waiting.png` | 05 | Transfers panel with the "No internet connection" banner, one job "Paused · 1.2 GB of 3.0 GB", one "Waiting for connection … (attempt 3, next in 8 s)" with Try Again Now icon, header buttons Pause All / Resume All. | light | both | ✅ generated (light) |
| `resume-prompt.png` | 05 | Launch prompt "Resume interrupted transfers?" with two rows (one "needs files.example.com (SFTP)"), the explanatory text, Discard / Resume buttons. | light | both | ✅ generated (light) |
| `preview-builtin.png` | 06 | Built-in preview window with a text file from an SFTP server. | light | both | ✅ generated (light) |
| `sidebar.png` | 07 | Sidebar with Favorites (incl. a server favorite), Locations, Devices, Servers (one connected), Containers (a Compose group), Cloud. Cloud is built in now. | both | both | ✅ generated (light) |
| `favorites-drag.png` | 07 | Two folders being dragged onto Favorites: highlighted section, insertion line with ring, "Add 2 folders to Favorites" badge at the pointer. | light | both | ✅ generated (light) |
| `connect-sftp.png` | 08 | Connect to Server sheet, SFTP, Sign in with "Key File", Add to Favorites on. | light | both | ✅ generated (light) |
| `connect-ftps-data-channel.png` | 08 | Connect sheet, FTPS, Data channel segmented control with Automatic selected and its note. | light | both | ✅ generated (light) |
| `connect-s3.png` | 08 | Connect sheet, S3, Provider "Cloudflare R2" with the Account ID field. | light | both | ✅ generated (light) |
| `connect-container.png` | 08 | Connect sheet, Container, Docker, list of containers (one stopped, read-only). | light | both | ✅ generated (light) |
| `connect-webdav.png` | 08 | Connect sheet, WebDAV, HTTPS, Server type "Nextcloud" with the WebDAV path filled in. | light | both | ✅ generated (light) |
| `connect-smb.png` | 08 | Connect sheet, SMB, share "Team" chosen from the list of the server's shares. | light | both | ✅ generated (light) |
| `cloud-sidebar-badges.png` | 09 | Cloud section in the sidebar (iCloud Drive, Google Drive, OneDrive) and a Google Drive folder with "only in cloud", "downloading" and downloaded files; context menu with Download Now / Remove Download. Built in now; `render_cloud` (crates/app/src/cloud_ux_tests.rs) renders German light/dark versions. | both | both | ✅ generated (light) |
| `archive-as-folder.png` | 10 | A ZIP open as a folder: breadcrumb `Downloads › backup.zip › docs`, lock chip "read-only archive". | light | both | ✅ generated (light) |
| `compress-sheet.png` | 10 | Compress sheet: format zip, name, Level slider, "Exclude .DS_Store and __MACOSX" on. | light | both | ✅ generated (light) |
| `terminal.png` | 11 | A pane with the terminal below it, showing a `cd` and an echoed `cp -R` file action with "done". | dark | both | ✅ generated (dark; a scripted container shell through the terminal plugin's remote override, never a login shell) |
| `git-panel.png` | 12 | File list with Git badges (modified, untracked, staged) and the Git panel with Staged/Changes groups and a commit message. | both | both | ✅ generated (light; a throwaway repository, made-up authors, no global Git config) |
| `compare-sync-ab.png` | 13 | Compare & Sync with three panes: cards "Pane A" / "Pane B" with the pane pop-up open ("1 · manta-demo", "2 · Downloads  (B)", "3 · …"), statuses "Only in A", "B newer"; behind it a pane path bar with the A/B buttons. | light | both | ✅ generated (light) |
| `compare-sync.png` | 13 | Compare & Sync window, two panes, status legend, rows with several statuses, direction →, "Sync N Items" button. | light | both | ✅ generated (light) |
| `settings-plugins.png` | 14 | Settings → Plugins with the Built-in list. | light | both | ✅ generated (light) |
| `disk-usage.png` | 14 | Disk Usage panel: treemap plus "Largest Items". | both | both | ✅ generated (light) |
| `batch-rename.png` | 14 | Batch Rename sheet with two rules (Find & Replace, Numbering) and the live preview with highlights. | light | both | ✅ generated (light) |
| `project-info.png` | 14 | Project panel for a Laravel demo project: header tags, buttons, Recent, Artisan and Composer sections. | light | both | ✅ generated (light) |
| `image-tools.png` | 14 | Image panel with a photo: dimensions, colour profile, EXIF camera section, GPS warning. | light | both | ✅ generated (light) |
| `duplicates.png` | 14 | Duplicates panel after a scan: summary (groups, duplicate files, reclaimable) and two groups with "Kept". | light | both | ✅ generated (light) |
| `plugin-confirm.png` | 15 | "Turn on “Hello Python”?" confirmation showing Runs / In. | light | both | ✅ generated (light) |
| `settings-general.png` | 16 | Settings → General, top part (Appearance, Language, Default file manager, Files). | both | both | ✅ generated (light) |
| `software-update.png` | 17 | Software Update window: "Manta 0.2.0 is available — you have 0.1.3", What's new, the three buttons. | light | both | ✅ generated (light) |
| `install-dmg.png` | 01 | The disk image window (`packaging/dmg` background, window title "Manta"): the Manta icon, a half-transparent copy being dragged with the pointer, the "Applications" shortcut ringed. No macOS text, so only `en/`. | light | en only | ✅ illustration (see below) |
| `gatekeeper-blocked.png` | 01, 19 | macOS alert "“Manta” Not Opened" / "Apple could not verify …" with Done (ringed, pointer) and Move to Trash. | light | both | ✅ illustration (see below) |
| `gatekeeper-open-anyway.png` | 01, 19 | System Settings › Privacy & Security scrolled to Security: "Allow applications from", "“Manta” was blocked to protect your Mac." with Open Anyway (ringed, pointer). | light | both | ✅ illustration (see below) |
| `gatekeeper-confirm.png` | 01 | macOS alert "Open “Manta”?" with Open Anyway (ringed, pointer) and Done. | light | both | ✅ illustration (see below) |

## Status per language

All 39 generated images (✅ above) exist in every language. Apple's Quick Look panel is system UI, not drawn by Manta: the manual describes it in words and shows no image of it.

**Install illustrations** (`install-dmg.png`, `gatekeeper-*.png`): the disk image window, the Gatekeeper alerts and System Settings are macOS UI, and real screenshots would exist only in the Mac's own language (and need a blocked app). `scripts/manual/install_images/generate.py` draws them instead as HTML/CSS styled like macOS 26 (light, system font, PingFang SC for Chinese) and renders them at 2x with headless Google Chrome (throwaway profile; nothing is installed, launched or changed). All macOS text is Apple's own localized string from `scripts/manual/install_images/strings.json`, which `extract_strings.py` reads from the system's `.loctable` files (CoreServicesUIAgent, the Privacy & Security settings extension, System Settings) and which records the table and key of every string. Each page checks itself for clipped text. Full colour, not 256 colours (the gradients would band):

```sh
python3 scripts/manual/install_images/extract_strings.py --check   # after a macOS update: are the strings still the same?
python3 scripts/manual/install_images/generate.py                  # all four, all languages (install-dmg.png only in en/)
python3 scripts/manual/install_images/generate.py --lang ru --only gatekeeper-open-anyway
```


| Language | Folder | Generated | Known problems in the renders (UI, not the crop) |
|---|---|---|---|
| German | `de/` | 39/39 ✅ | — |
| English | `en/` | 39/39 ✅ | — |
| Simplified Chinese | `zh-Hans/` | 39/39 ✅ (PingFang SC, no tofu) | — |
| Spanish | `es/` | 39/39 ✅ | — |
| French | `fr/` | 39/39 ✅ | — |
| Italian | `it/` | 39/39 ✅ | — |
| Russian | `ru/` | 39/39 ✅ | — |

Sample data that stays English in every language on purpose (fixtures, not UI strings): the job titles in `transfers-panel.png`, the release notes in `software-update.png`, the README text in `preview-builtin.png`, the commit message and history in `git-panel.png`, the shell session in `terminal.png` (only the result line of the echoed copy is translated, as in the app).
