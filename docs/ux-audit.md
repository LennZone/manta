# Manta UX audit

Source: headless renders of the real `MantaApp` against a fixture tree
(`cargo test -p manta ux_tests`; PNGs go to `$MANTA_RENDER_DIR`,
default `target/ux-renders/`), light and dark, 1280×800 and 900×600, at
2 px per point, plus a read-through of `crates/app/src`.

Priority: **P0** = broken or misleading, **P1** = hurts everyday use,
**P2** = polish.

Status (re-checked in the second pass):

- **Fixed**: verified in a render or in the code.
- **Partly**: some of the fix is in place, the rest is noted.
- **Open**: not done.

## P0: broken or misleading

| # | Finding | Where | Status |
|---|---|---|---|
| 1 | **Missing glyphs render as tofu.** egui 0.37 ships no emoji font, so `🗀` (New folder), `＋` (Add current folder) and others show as empty boxes. The rest of the icons come from Apple Color Emoji at random sizes and in a different style from egui's own glyphs (`⬅`, `🔄`, `👁` are colourful, `◌` is a hairline). | toolbar, sidebar | Fixed (render): vendored Phosphor icon font with one consistent stroke weight. Second pass: `↩` (U+21A9) still fell back to Apple Color Emoji in the palette footer and preview; it now uses Phosphor's return glyph (`icons::RETURN`) |
| 2 | **Path bar pushes the filter field and spinner off-screen.** The breadcrumb never truncates, so in temp dirs, home dirs on narrow windows and deep remote paths the filter (⌘F) and the loading spinner can't be seen. | `pane::header` | Fixed (render, 900 px): the breadcrumb collapses leading parts into "…", and the filter has its own slot |
| 3 | **The "Empty folder" label is drawn below the table** and is clipped by the window edge. An empty folder looks like a broken pane. | `pane::file_table` | Fixed (render) |
| 4 | **Errors look like data.** A failed listing shows the raw `not found: /long/path` in red over an empty table with headers. There's no retry and no way back. | `pane_ui` | Fixed (render): centered error state in plain words with Try Again and Go Back. Second pass: it still printed the raw backend string under the explanation; now the raw text is shown only when there's no plain explanation |
| 5 | **The loading state is almost invisible.** It shows only "…" in the tab title and a spinner that is often off-screen (see #2). | pane | Fixed (render `pathbar_loading_*`, `loading_*`): spinner centred in the path bar, plus a delayed centered spinner |
| 6 | **Delete wording contradicts itself.** The context menu says "Move to Trash / Delete…", but the dialog says items are deleted permanently. | `pane::context_menu` | Fixed (render + code): "Delete…" everywhere (no "Trash" left), red destructive button |
| 7 | **The right pane's border is clipped** at the window edge, and in the 900-px render the Modified and Permissions columns are cut off with no way to see them. | `ui.columns(2)` layout | Fixed (render, 900 px) |

## P1: everyday friction

| # | Finding | Status |
|---|---|---|
| 8 | **Active and inactive panes look the same apart from a thin outline.** Selection is the same light blue in both, so it's easy to press F5 in the wrong pane. | Fixed (render + code): active selection is solid accent with white text, inactive selection uses `selection_inactive` grey, and the active pane has an accent outline and an accent location chip. (There's no tinted header strip, contrary to the first audit.) |
| 9 | **There is no keyboard-focus indicator for the cursor row** except an underline that only shows when the row isn't selected. | Fixed (render) |
| 10 | **Sidebar items are centered buttons**, so icons don't line up and hover/selected states are missing. There is no indicator for the current location. | Fixed (render; status dots in code only, no connected server in the fixtures) |
| 11 | **Toolbar icons are unlabeled emoji with no grouping logic.** Copy and move show as "📋→" and "✂→". "Show hidden" is a hairline circle. Tooltips mix "(⌘[)" styles. | Fixed (render + code) |
| 12 | **Dates are ISO `2026-10-20 09:01`**, which is dense and not how Finder reads. | Fixed (render) |
| 13 | **Sizes are left-aligned and proportional**, so magnitudes don't line up. | Fixed (render) |
| 14 | **File icons are all the same grey page.** You can't tell images, archives, code or symlinks apart, and symlinks to folders look like folders. | Fixed (render). Second pass: the symlink badge never showed for links to folders (the local backend reports them as folders with `link_target` set); it now does |
| 15 | **Status messages are tiny grey text in the status bar.** Errors persist silently there, and successes ("Connected to …") are easy to miss. | Partly (render + code): toasts bottom right, info toasts dismiss after 4 s. Error toasts don't stay until closed as first claimed: they expire after 30 s |
| 16 | **No discoverability for shortcuts.** Many actions (⌘L, ⌘F, ⌘[) are only in the README. | Fixed (render) |
| 17 | **Panes are fixed 50/50.** | Fixed (code) |
| 18 | **Connect dialog:** the auth error is the raw backend string ("Username/PublicKey combination invalid"). The primary button isn't visually primary. Enter submits even from the Cancel button. Validation only happens on submit. There's no hint about where favorites go. | Fixed (render + code): friendly error with the cause underneath, inline validation, accent primary button, segmented protocol control. Enter on a focused Cancel button cancels (egui activates the focused button; `finish` only submits while nothing else was chosen). Second pass: a focused field had no visible border (egui outlines it in `selection.stroke`, which is white here); framed fields now get the standard focus ring |
| 19 | **Copy/Move/Delete dialogs list full absolute paths in tiny monospace.** The destination is buried. The conflict policy uses four radio buttons in a vertical list. | Fixed (render). Second pass: folders showed a file icon; the forms now know which items are folders |
| 20 | **The transfers panel is a list of full-width progress bars with the percentage inside**, a spinner overlapping the bar, and finished and running jobs that look alike. | Fixed (render) |
| 21 | **Compare & Sync uses colour only** (WCAG 1.4.1). The "Different" red reads as an error, and the table's column widths waste space. | Fixed (render). Second pass: the location cards had different heights (one path wrapped over two lines because the truncation measured an already truncated galley) and didn't fill the width. They're now laid out as one row of equal-height cards with start-truncated paths and the direction control centred between them |
| 22 | **Tabs**: a pill style that looks like a button. The close "×" always shows. Loading is shown as "…". | Fixed (render) |

## P2: polish

| # | Finding | Status |
|---|---|---|
| 23 | Typography is egui's default Ubuntu Light, which looks foreign on macOS. There's no type scale (headings are 18-px light). | Fixed (render) |
| 24 | Colour tokens are spread around (`Color32::GRAY`, `DARK_GRAY` inline). Contrast of weak text on stripes is below AA in dark mode. | Partly: `theme::Palette` holds the semantic tokens and a test checks contrast (secondary text ≥ 4.5:1). A few control colours are still inline in `widgets.rs` (field fill, segmented track and thumb, dark danger button) |
| 25 | Hidden files are dimmed but nothing else. | Fixed (render) |
| 26 | Sidebar section headers are ALL CAPS and use small-strong-weak text. | Fixed (render) |
| 27 | First launch shows an empty "Favorites" with "No favorites yet" and nothing else. | Fixed (render): "Press ⌘D to keep the current folder here." and a palette hint in the status bar. Second pass: the wrapped second line started left of the first; it's now an indented block |
| 28 | The preview window title uses an emoji eye, and its metadata is a long path. | Fixed (render) |
| 29 | Permissions column is always shown. | Fixed (render) |
| 30 | The Connect "Remember password" warning sits in a tooltip only. | Fixed (code; not in a render because "Add to Favorites" is off in the fixtures) |

## Second pass: alignment and new findings

| # | Finding | Status |
|---|---|---|
| 31 | **Placeholders and breadcrumbs were not on one baseline.** Measured at 2 px/pt: crumbs and typed filter text at y = 62, the "Filter" placeholder at 61 and the location-chip label at 60. egui lays a `hint_text` out as a separate atom (half a point off the typed text, and in the default 13 pt font unless told otherwise: the palette hint was 13 pt next to 17 pt typed text). The chip label was centred by its box, not placed on the row's baseline. | Fixed (crops `pathbar_*`, `pathbar_before_after_*`): `widgets::show_with_hint` paints the placeholder at the empty text galley's own `galley_pos` in the field's font; bar texts in other sizes are placed with `widgets::body_baseline` / `paint_on_baseline`. Every text in the path bar now sits on y = 62, in light and dark, empty, filled and loading |
| 32 | **The ⌘L path editor's text started flush against the field edge**, about 6 pt left of the first crumb, and the inset constant was silently ignored: with an explicit `frame`, egui ignores `TextEdit::margin`. | Fixed (crop `pathbar_edit_*`): the inset is the frame's inner margin, and the field starts one "/" further left, so "usr" in "/usr/local" lands on the exact pixel column where the "usr" crumb was. No jump when switching modes |
| 33 | Segmented control ids came from the allocation's auto id, which shifts when the row's content changes. | Fixed (unit test `segmented_controls_on_one_row_are_independent`): explicit `id_salt`, and each segment reports itself to AccessKit as a radio button |
| 34 | Placeholder-only fields had no accessible name once the hint was painted by hand. | Fixed (kittest accessibility check): the placeholder is set on the AccessKit node; hint-less fields have explicit names |
| 35 | No way to jump to a typed or pasted path in a new tab. | Fixed (renders `goto_*`, behaviour tests): "Go to Path…" popup (⇧⌘G, toolbar signpost button, palette). See README |

## Not done (backlog)

- Inline rename in the table (Finder style). Rename still uses a dialog.
- Column chooser and per-column persistence.
- Keychain integration (needs a backend API; see the API requests in the report).
- Ask per conflict during a transfer (needs `ops` support).
- "Test connection" without opening a pane (needs a cheap `manta_vfs::probe`; see the API requests).
- Real trash support for local deletes (needs a `Vfs::trash`).
- A dedicated accessibility pass with VoiceOver (AccessKit labels for icon-only buttons are set through the tooltips only).
