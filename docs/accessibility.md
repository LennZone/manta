# Accessibility

How Manta works with VoiceOver, the keyboard alone and the macOS display
accommodations (WCAG 2.2 AA spirit, Apple HIG). User-facing text: manual
chapter 16 ("Bedienungshilfen / Accessibility") and chapter 18 ("Bedienung
ohne Maus / Keyboard-only use").

## How it works

- **AccessKit → NSAccessibility.** eframe's default features include
  `accesskit` (`eframe = "0.37"` with default features; `accesskit_macos`
  is in `Cargo.lock`), so egui's tree reaches VoiceOver. Custom-painted
  widgets add their nodes through `crate::a11y` (`name`, `file_item`,
  `list`, `disclosure`, `region`, `live`) or `Response::widget_info`.
- **Tooltips name icon buttons.** `widgets::tooltip` gives an unnamed,
  role-less clickable the tooltip's first line as name (role Button), the
  rest as description and the shortcut as `keyboard_shortcut`.
- **No glyphs in names.** `a11y::SpeechCleanup` (an egui plugin,
  `output_hook`) strips Phosphor glyphs (Private Use Area) from every
  label/value/description before the tree leaves the app, and localises
  egui's "Resize panel".
- **Live regions.** Toasts are `Status` (polite) or `Alert` (assertive,
  errors) nodes with `live` set: VoiceOver announces "3 items copied" and
  error toasts when they appear.
- **VoiceOver follows the cursor row.** While the file list has the
  keyboard, the cursor row is the window's (and the list stop's)
  `active_descendant`, so arrow keys announce the row. The command palette
  does the same for its selected command.
- **System display options** (`a11y.rs`): `NSWorkspace
  accessibilityDisplayShouldReduceMotion / IncreaseContrast /
  ReduceTransparency`, read at launch and again on
  `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`.
  - Reduce motion: egui `animation_time` 0, no scroll animation,
    `a11y::anim()` for our own animations (toast fade, hover fades, switch,
    active-pane border), steady favourite highlight instead of the pulse,
    no pulsing upload badge. (Manta has no thumbnail fades or pane split
    animations; spinners stay, like macOS progress indicators.)
  - Increase contrast: `Palette::variant(.., high_contrast)` – darker
    separators/borders, near-body secondary text, stronger hover; thicker
    focus ring.
  - Reduce transparency: `a11y::scrim()` drops the translucent backdrops of
    dialogs, sheets, the palette, Go to Path and the app-modal scrim.
- **Contrast.** Unit tests in `theme.rs` check every accent × light/dark ×
  normal/high contrast: body text ≥ 7:1, secondary **and tertiary** text
  ≥ 4.5:1 on all surfaces, focus ring ≥ 3:1; high contrast: secondary ≥ 7:1,
  borders/separators ≥ 3:1. `text_tertiary` was raised (light 150 → 102,
  dark 112 → 150) and `text_secondary` adjusted to keep the hierarchy; the
  old faint grey lives on as `text_disabled` for disabled controls only.

## Keyboard model

| Where | Keys |
|---|---|
| File list (default) | arrows, ↩, Space, typing; **⇥ / ⇧⇥ next / previous pane** |
| Anywhere | **⌃F5** focus the toolbar (macOS "Move focus to the window toolbar") |
| Controls | ⇥ / ⇧⇥ through toolbar → sidebar → panes (path bar, column headers, file list) → status bar, wrapping; arrows move spatially (sidebar, option lists); Space / ↩ activate; **esc** back to the file list (selection kept) |
| File list stop | one Tab stop per pane (`pane::list_anchor`); while focused, the list keys work and Tab continues to the next control |
| Dialogs / sheets | egui modal layer keeps focus inside; the control that had focus before lets go so Tab starts in the dialog; on close the focus is restored (one frame later, after egui drops the modal layer) – also after Settings / Software Update |

The status bar's contents are drawn after the panes (its room is reserved
first), so the Tab order follows the visual order. File rows, column resize
handles, the pane divider and the empty path-bar area are not Tab stops
(`Sense::CLICK | Sense::DRAG`). A single focus ring (`a11y::paint_focus_ring`,
accent colour, 2.5 pt / 3 pt with Increase contrast) is drawn around any
focused widget; text fields keep their own ring. Mouse clicks never focus
buttons, so the ring only shows for keyboard use.

## Audit (main window, dialogs, panels)

Roles and names as VoiceOver gets them (German in parentheses where useful).

| Area | Element | Role | Accessible name / value | Tab |
|---|---|---|---|---|
| Window | toolbar / sidebar / pane / status bar | Toolbar / Navigation / Group / Group | "Toolbar", "Sidebar", "Pane: Projects, active", "Status bar" | – |
| Toolbar | Back, Forward, Enclosing Folder, Go to Path, Refresh, New Folder, Rename, Move to Trash, Copy/Move to right pane, Quick Look, Show hidden (toggle), Compare & Sync, Settings, Transfers, Connect | Button (dimmed when unavailable, pressed state for toggles) | tooltip text, shortcut as keyboard shortcut, reason as description when disabled | yes |
| Toolbar | command palette launcher | Button | "Command Palette" | yes |
| Sidebar | section titles | Heading | "Favorites", "Locations", … | – |
| Sidebar | places, favourites, servers, devices, Trash | Button, selected = current | name (+ detail, "connected"); path / server as description | yes, ↑↓ |
| Sidebar | +, rescan, eject | Button | tooltip | yes |
| Path bar | location chip | Button | place ("Local", server) | yes |
| Path bar | back to local, "…" enclosing folders | Button | tooltip | yes |
| Path bar | breadcrumbs | Link | folder name; full path + hint as description | yes |
| Path bar | view switcher (segmented) | RadioButton ×4, toggled | "Icons", "List", "Columns", "Gallery" | yes |
| Path bar | filter (magnifier) / filter field | Button / TextInput | "Filter Files" | yes |
| Pane | tabs, + tab, split, close pane | Button / Tab | title / tooltip | yes |
| Columns | headers | Button | "Sort by Name" … | yes |
| File list | list stop | List | "Files in Projects" (active descendant = cursor row) | yes (one per pane) |
| File list | rows (List, Columns), cells (Icons), filmstrip (Gallery) | Row in Table (row count) | "README.md, Text, 214 bytes, modified Today 09:41" + cloud state + plugin (Git) status; selected; position "3 of 13" | no (arrow keys) |
| Gallery | big preview | Image | file description | no |
| Status bar | item count / selection | Label | "13 items · 1 selected (214 bytes)" | – |
| Status bar | plugin status items (CPU, RAM, …) | Button | full text | yes |
| Status bar | transfer pill, update pill | Button | "Copying 3 items … 20 %"; details as description | yes |
| Status bar | palette hint | Button | "Command Palette" (value = visible text) | yes |
| Transfers panel | job | ListItem | "Copy 3 items, 20 %. 10 B of 50 B · …" | – |
| Transfers panel | Stop / Retry / Clear | Button | label | yes |
| Toasts | toast | Status (polite) / Alert (assertive) | "title. detail" | – |
| Toasts | action, dismiss | Button | label, "Dismiss" | yes |
| Dialogs | Connect: protocol, auth | RadioButton | "SFTP" … / "Password" … | yes |
| Dialogs | Connect fields | TextInput / PasswordInput | label or placeholder ("Port", "Username", "Password") | yes |
| Dialogs | Copy/Move/Drop, Delete, Empty Trash, Conflict, Host key, Compress, What's New | Button, ComboBox, CheckBox | localized labels | yes, trapped |
| Settings | tabs | Tab, selected | "General", "Plugins" | yes |
| Settings | rows with switch | CheckBox (named by the row title via `label_inputs_by`) | "Show hidden files" | yes |
| Settings | appearance, accent swatches | RadioButton | "Auto"/"Light"/"Dark", colour names | yes |
| Settings | plugin log disclosure | DisclosureTriangle, expanded | label | yes |
| Help | contents | TreeItem + DisclosureTriangle ("Sections of …") | chapter titles | yes |
| Help | search, Back/Forward | TextInput, Button | "Search", "Back", "Forward" | yes |
| Help | page text | Label (static text, not a Tab stop) | paragraph text | – |
| Update window | buttons, pill | Button | localized | yes |
| Command palette | field | TextInput "Commands" (active descendant = selected command) | | yes |
| Command palette | commands | ListBoxOption, selected, n of m | "New Folder, File, ⇧⌘N" | no (↑↓) |
| Context menus | items | MenuItem (egui) | label without glyph | arrows |
| Terminal panel | terminal | TextInput (plugin) | settings fields `labelled_by` their labels | yes |
| Git panel | header, close, rows | Button / Label | "Close Git", status text | yes |

## Tests

`crates/app/src/a11y_ux_tests.rs` (kittest, on the AccessKit tree):

- `every_control_has_a_role_and_a_name` – 15 screens (main window, Icons,
  Columns, Gallery, Connect, Delete, Copy, Settings ×2, Help, palette,
  toasts, transfers panel, transfer pill, Git panel) in German and English:
  no actionable node without role or name, no glyph in a name.
- `toolbar_buttons_have_localized_names`, `file_rows_say_name_kind_size_and_selection`,
  `toasts_are_live_regions`, `cursor_row_is_what_voiceover_follows`.
- Keyboard: `ctrl_f5_and_tab_walk_toolbar_sidebar_panes_and_status_bar`,
  `file_list_tab_stop_keeps_the_list_keys`,
  `shift_tab_in_the_file_list_goes_to_the_previous_pane`,
  `esc_from_a_control_keeps_the_selection`,
  `dialogs_keep_the_focus_inside_and_give_it_back`,
  `focus_returns_after_settings_close`, `delete_dialog_buttons_are_reachable_by_tab`.
- Display options: `reduce_motion_stops_the_favourite_pulse`,
  `increase_contrast_switches_the_palette`, `a11y::tests::*`.
- Contrast: `theme::tests::text_tokens_meet_wcag_aa`,
  `theme::tests::increase_contrast_is_stronger`.
- `render_focus_rings` (with a GPU and `MANTA_RENDER_DIR`): focus ring PNGs.
- `MANTA_A11Y_DUMP=dir cargo test -p manta a11y` writes every audited
  AccessKit tree as text.

## Limits

- A real VoiceOver session can't run headless; the tests check the tree
  VoiceOver receives. Try it by hand with ⌘F5 before a release.
- Links inside manual paragraphs are not separate accessibility nodes (the
  paragraph is read as text; links work by click).
- Plugin UIs drawn by external plugins get names only as far as their blocks
  carry labels; the built-in plugins' own painted widgets (image tools,
  duplicates) use their own palettes.
