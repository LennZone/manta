# Performance

How Manta copes with very large folders, how that is measured, and what the update download
is made of. For building and testing in general see [development.md](development.md).

## Very large folders

### The benchmark

`crates/app/src/bench_listing_tests.rs` is an ignored test that drives the real app
(`MantaApp` in an `egui_kittest` harness, with the real built-in plugins) against:

- local folders with **100,000** and **300,000** items (1 % folders, 20 % small PNGs, 5 %
  hidden files, the rest documents with mixed natural-sort names), made once in
  `$TMPDIR/manta-bench-<n>` and reused (delete them by hand);
- an in-memory **SFTP-looking server** (`MemFs` behind a wrapper reporting
  `Protocol::Sftp`) with 100,000 items.

```sh
cargo test -p manta --profile perf bench_large_folders -- --ignored --nocapture --test-threads=1
# MANTA_BENCH_SIZES=300000          only some sizes
# MANTA_BENCH_OUT=results.md        also write the tables below to a file
```

The `perf` profile (`Cargo.toml`) is the release profile without fat LTO and with line
tables, so it builds in a few minutes and works with profilers (`sample`, Instruments).
Frames are headless egui frames (layout, widgets, plugin hand-off; no GPU), which is where
per-item costs show up. All times are wall clock on an Apple M3 Pro (36 GB, macOS 26), with
other builds running in the background, so single numbers vary by ±30 %; the before/after
differences below are far larger than that.

What is measured: listing the folder (backend only), opening it in a pane until the first
items and the whole listing are on screen and the longest frame meanwhile, idle and scroll
frames, sorting by size / date / name, filtering, type-to-select, Select All and the frames
while everything is selected, the first frame and scroll frames of the Icons, Columns and
Gallery views, the visible thumbnails in the Icons view, and a reload (⌘R) of the folder.

### Before and after

| | 100,000 local | | 300,000 local | | 100,000 remote | |
|---|---|---|---|---|---|---|
| | before | after | before | after | before | after |
| listing (backend) | 366 ms | 280 ms | 3,398 ms | 774 ms | 11 ms | 11 ms |
| first items on screen | 668 ms | **18 ms** | 3,603 ms | **20 ms** | 129 ms | 58 ms |
| whole listing on screen | 668 ms | 254 ms | 3,603 ms | 860 ms | 129 ms | 61 ms |
| longest frame while loading | 143 ms | 11 ms | 527 ms | 12 ms | 115 ms | 7 ms |
| sort by name | 136 ms | 6 ms | 504 ms | 9 ms | 93 ms | 2 ms |
| sort by size | 80 ms | 29 ms | 338 ms | 88 ms | 124 ms | 41 ms |
| filter "9" | 56 ms | 3 ms | 210 ms | 9 ms | 39 ms | 2 ms |
| filter cleared | 146 ms | 3 ms | 513 ms | 9 ms | 93 ms | 2 ms |
| type-to-select (per letter) | 3.6 ms | 0.1 ms | 22 ms | 0.4 ms | 2.4 ms | 0.1 ms |
| frame, everything selected (median) | 112 ms | 0.2 ms | 567 ms | 0.2 ms | 55 ms | 0.2 ms |
| scroll frame, everything selected | 256 ms | 0.7 ms | 1,132 ms | 0.7 ms | 114 ms | 0.6 ms |
| scroll frame, Icons / Columns / Gallery | 5 ms | 0.5 ms | 29–30 ms | 0.4–0.7 ms | 2 ms | 0.3–0.4 ms |
| reload (⌘R) until shown | 857 ms | 193 ms | 6,504 ms | 820 ms | – | – |
| longest frame during the reload | – | 5 ms | – | 18 ms | – | – |

The list view's scroll and idle frames were already fine (0.5–2 ms): all four views draw only
the visible rows (`ScrollArea::show_rows`). The full results of the last run are at the end
of this file.

### What was slow, and what changed

- **Listing (local).** `readdir` plus one `lstat` per file needs a kernel vnode per file;
  with more files than the vnode cache holds (`kern.maxvnodes`, about 260,000 here) every
  `lstat` went to the file system: 3.4 s for 300,000 files. `LocalFs` now lists with
  `getattrlistbulk(2)` (name, type, date, mode, flags and size of hundreds of entries per
  system call, read from the directory itself), like Finder; symlinks still get `lstat` /
  `stat` so their target decides kind and size. Falls back to `readdir` if the call fails.
  Same speed for folders that fit in the cache, 3–4× faster for the ones that don't.
- **First items at once.** `Vfs::list_parts` hands a listing over in parts as it is read
  (`LocalFs`: one part per `getattrlistbulk` call; other backends: the whole listing at
  once). A folder opened fresh shows what was read once it has 5,000 entries or after
  40 ms; the complete listing replaces it and keeps the cursor and selection made
  meanwhile. Reloads keep the old listing until the new one is complete (no flicker).
- **Sorting off the UI thread.** The listing thread sorts the entries in the tab's order;
  the UI's own sort then finds them in order (linear). The natural sort comparison
  (`util::natural_cmp`) no longer allocates (it built two `String`s per digit run and
  lowercased per character): sorting 300,000 names went from 504 ms to 9 ms.
- **Filter and type-to-select** compare case-insensitively without lowercasing every name
  into a new string (ASCII fast path; other names as before).
- **Per-frame work over the whole selection.** The status bar size, the context menu's
  state, every menu item's enabled state (`cmd_enabled`, asked each frame for the menu bar)
  and the Columns preview copied or scanned all selected entries every frame, the plugin
  hand-off hashed all selected names every frame: 570 ms per frame with 300,000 selected.
  The selection is now a `pane::Selection` with a generation that changes with every edit,
  and `Tab::selection_summary` (selected view indices, total size, "contains an archive")
  is computed once per change of selection, listing, view order or folder sizes.
- **Keeping the selection after a re-sort or filter** built a set of all visible names
  (300,000 `String` copies); now one pass with lookups in the (small) selection.
- **Freeing a replaced listing** (300,000 entries: tens of ms) happens on a background
  thread.
- `Tab::select_names` (a plugin selecting many items) was quadratic: now one pass.

### Known remaining costs

- **Selecting 300,000 items** costs one frame of about 100 ms (40 ms for 100,000): the
  plugins get the selection as a list of paths (`PaneState::selection`) and the built-in
  plugins each keep a copy. Sharing it (`Arc<[VfsPath]>`) would need a change of the plugin
  API types; later frames are unaffected.
- **Sorting by size or date** sorts on the UI thread (30–90 ms for 300,000): ties fall back
  to the name. Only on a header click.
- **The first frame of the Columns view** copies the active folder's listing for the column
  cache (34 ms for 300,000 items), once per listing.
- **Remote backends** list in one piece (SFTP, FTP, S3 return pages internally but hand
  over the whole folder); `Vfs::list_parts` is where they can stream later.
- Formatted sizes and dates are not cached: only the visible rows (about 40) are formatted
  per frame, well under 0.1 ms.

## Update size

The update ZIP is the signed `Manta.app` (`ditto -c -k`); nearly all of it is the binary.
The binary embeds the user manual, including 217 screenshots (31 per language, 7
languages). They stay PNGs in `docs/manual/images` (GitHub shows them; the screenshot test
writes them) but are embedded as **lossless WebP**, converted at build time by
`crates/app/build.rs` with `cwebp` (`-lossless -z 6`, cached in the build folder; pixels are
identical, a test decodes every embedded image and compares it with its PNG). Release builds
require `cwebp` (`MANTA_REQUIRE_CWEBP=1` in `scripts/build-release.sh`).

| arm64 release (fat LTO, stripped) | before (PNG) | after (WebP) |
|---|---|---|
| embedded screenshots | 16.7 MB | 11.2 MB |
| binary | 49.7 MB | 44.2 MB |
| binary as ZIP (`ditto -c -k`) | 32.1 MB | 26.8 MB |

Lossy WebP was not used: it is barely smaller than lossless for these flat UI screenshots
(some got larger) and blurs small text.

Where the rest of the binary goes (unstripped release build, symbols grouped by crate;
approximate, as LTO merges code): about 21 MB of code — the app ~2.7 MB, egui / eframe /
epaint ~1.9 MB, wgpu and naga (GPU backend, shader compiler) ~2.2 MB, the built-in plugins
~3 MB together, OpenSSL / libssh2 (SFTP) ~1.1 MB, image codecs, rustls, regex, fonts
shaping — and about 22 MB of data, half of it the screenshots, then fonts, Unicode and
codec tables. `opt-level = "s"` saved 17 % of the code (see the comment in `Cargo.toml`) but
costs speed in compression, hashing and image decoding; with fat LTO a per-crate
`opt-level` hardly changes the result, so it was left as it is.

## Full results (after)

Last run of `bench_large_folders` (`MANTA_BENCH_OUT`).

#### local, 100,000 items

| Measurement | Result |
|---|---|
| LocalFs::list (median of 3) | 280.4 ms |
| for comparison: readdir + lstat per file, metadata only (median of 3) | 255.3 ms |
| first items on screen | 18.1 ms |
| whole listing on screen | 253.9 ms |
| longest frame while loading | 10.6 ms |
| listing in memory (entries + view) | 30.5 MB |
| idle frame | 0.3 ms (p95 0.7) |
| scroll frame (List) | 0.6 ms (p95 1.6) |
| sort by size | 29.0 ms |
| sort by date | 12.5 ms |
| sort by name | 5.9 ms |
| filter "9" | 3.3 ms |
| filter "99" | 2.6 ms |
| filter "999" | 2.2 ms |
| filter cleared | 2.6 ms |
| type-ahead (3 letters) | 0.1 ms |
| type-ahead (no match) | 1.0 ms |
| select all | 3.7 ms |
| frame with all selected | 0.2 ms (p95 39.8) |
| scroll frame, all selected | 0.7 ms (p95 1.3) |
| select one again | 1.5 ms |
| Icons: first frame | 1.2 ms |
| Icons: scroll frame | 0.5 ms (p95 0.6) |
| Icons: visible thumbnails done | 13.0 ms |
| Icons: scroll frame with thumbnails | 0.6 ms (p95 0.8) |
| Columns: first frame | 15.3 ms |
| Columns: scroll frame | 0.4 ms (p95 0.4) |
| Gallery: first frame | 2.6 ms |
| Gallery: scroll frame | 0.7 ms (p95 0.7) |
| reload (⌘R) until shown | 193.3 ms |
| longest frame during the reload | 5.2 ms |
| process memory growth (RSS, all of the above) | 149 MB |

#### local, 300,000 items

| Measurement | Result |
|---|---|
| LocalFs::list (median of 3) | 774.3 ms |
| for comparison: readdir + lstat per file, metadata only (median of 3) | 2420.1 ms |
| first items on screen | 19.7 ms |
| whole listing on screen | 860.3 ms |
| longest frame while loading | 12.1 ms |
| listing in memory (entries + view) | 74.2 MB |
| idle frame | 0.2 ms (p95 0.3) |
| scroll frame (List) | 0.5 ms (p95 0.6) |
| sort by size | 87.7 ms |
| sort by date | 22.3 ms |
| sort by name | 9.2 ms |
| filter "9" | 8.6 ms |
| filter "99" | 7.6 ms |
| filter "999" | 7.0 ms |
| filter cleared | 8.6 ms |
| type-ahead (3 letters) | 0.4 ms |
| type-ahead (no match) | 4.1 ms |
| select all | 12.8 ms |
| frame with all selected | 0.2 ms (p95 104.4) |
| scroll frame, all selected | 0.7 ms (p95 1.2) |
| select one again | 3.9 ms |
| Icons: first frame | 1.0 ms |
| Icons: scroll frame | 0.5 ms (p95 0.5) |
| Icons: visible thumbnails done | 11.9 ms |
| Icons: scroll frame with thumbnails | 0.5 ms (p95 0.6) |
| Columns: first frame | 33.5 ms |
| Columns: scroll frame | 0.4 ms (p95 0.4) |
| Gallery: first frame | 1.9 ms |
| Gallery: scroll frame | 0.7 ms (p95 0.7) |
| reload (⌘R) until shown | 819.6 ms |
| longest frame during the reload | 17.5 ms |
| process memory growth (RSS, all of the above) | 311 MB |

#### remote (in-memory SFTP), 100,000 items

| Measurement | Result |
|---|---|
| remote list (median of 3) | 10.7 ms |
| first items on screen | 58.0 ms |
| whole listing on screen | 61.4 ms |
| longest frame while loading | 7.0 ms |
| listing in memory (entries + view) | 15.6 MB |
| idle frame | 0.2 ms (p95 0.5) |
| scroll frame (List) | 0.5 ms (p95 0.9) |
| sort by size | 40.8 ms |
| sort by date | 34.4 ms |
| sort by name | 2.4 ms |
| filter "9" | 2.3 ms |
| filter cleared | 2.4 ms |
| type-ahead (3 letters) | 0.1 ms |
| select all | 3.8 ms |
| frame with all selected | 0.2 ms (p95 24.1) |
| scroll frame, all selected | 0.6 ms (p95 0.6) |
| Icons: first frame | 2.6 ms |
| Icons: scroll frame | 0.4 ms (p95 0.9) |
| Columns: first frame | 3.9 ms |
| Columns: scroll frame | 0.3 ms (p95 0.4) |
| Gallery: first frame | 3.4 ms |
| Gallery: scroll frame | 0.3 ms (p95 0.3) |

The "before" numbers come from the same benchmark on the commit before these changes
(8c59de1; its "time to first paint" is the "whole listing on screen" row, since that
version showed nothing until the listing was complete).
