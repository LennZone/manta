# Keyboard Shortcuts {#tastenkuerzel}

Every Manta shortcut at a glance, grouped by topic. The command palette (**⇧⌘P**) shows each command's shortcut next to it – plugin commands too. The menu bar lists every command with its shortcut as well (**File**, **Edit**, **View**, **Go**, **Window**), like Finder – so apps that show the active app's shortcuts find all of Manta's. Clicking a menu command runs it.

Symbols: **⌘** Command · **⇧** Shift · **⌥** Option · **⌃** Control · **↩** Return · **⌫** Delete (backspace) · **⌦** Forward Delete · **⇥** Tab

## General {#allgemein}

| Shortcut | Command |
|---|---|
| ⇧⌘P or ⌘P | Command Palette |
| ⌘, | Settings… |
| ⌘? | Open Manual (this manual; in the Help window: the search field) |
| ⌘K | Connect to Server… |
| ⌥⌘L | Show Transfers |
| ⇧⌘S | Compare & Sync… |
| ⌘H | Hide Manta |
| ⌥⌘H | Hide Others |
| ⌘Q | Quit Manta |

## Panes and tabs {#bereiche}

| Shortcut | Command |
|---|---|
| ⇥ | Switch Pane (to the next one, wrapping around) |
| ⌥⌘← / ⌥⌘→ | Focus Pane on the Left / Right |
| ⌘D | Split Pane |
| ⇧⌘D | Close Pane |
| ⌘T | New Tab |
| ⌘W | Close Tab (in Settings, Compare & Sync and the preview: close that window) |

## Views {#ansichten}

| Shortcut | Command |
|---|---|
| ⌘1 | View as Icons |
| ⌘2 | View as List |
| ⌘3 | View as Columns |
| ⌘4 | View as Gallery |
| ⌘ + scroll or pinch | Change the icon size (Icons view) |
| ← / → | Column view: go to the column on the left / open the selected folder as the next column |
| ← / → | Icons and Gallery view: previous / next item |

The shortcuts act on the active pane. See [Views](03-ansichten.md).

## Navigation {#navigation}

| Shortcut | Command |
|---|---|
| ↩ or ⌘↓ | Open (archives as folders) |
| ⌥↩ | Open With Default App |
| ⌘↑ or ⌫ | Enclosing Folder |
| ⌘[ or ⌘← | Back |
| ⌘] or ⌘→ | Forward |
| ⌘L | Edit Path |
| ⇧⌘G | Go to Path… |
| ⌘R | Refresh |
| ⇧⌘. | Show/hide hidden files |
| ⌘F | Filter Files |
| ⌃⌘T | Add Folder to Favorites |

## Selection {#auswahl}

| Shortcut | Effect |
|---|---|
| ↑ / ↓ | Previous / next item |
| ⇧↑ / ⇧↓ | Extend the selection |
| Page Up / Page Down | 20 rows up / down |
| Home / End | First / last item |
| ⌘A | Select All |
| esc | Clear the selection (or close the preview) |
| Type letters | Jump to the matching name |

## Files {#dateien}

| Shortcut | Command |
|---|---|
| Space or ⌘Y | Quick Look / preview (Space or esc closes it) |
| F5 | Copy to Other Pane… |
| F6 | Move to Other Pane… |
| F2 | Rename… (several items: batch rename) |
| ⇧⌘R | Batch Rename… |
| ⇧⌘N | New Folder… |
| ⌘⌫ or ⌦ | Move to Trash (on servers: delete permanently after asking, see [Delete](05-dateien.md#loeschen)) |
| ⌥⌘⌫ | Delete Immediately… (permanent, after asking) |
| ⇧⌘⌫ | Empty Trash… (after asking, see [Trash](07-favoriten-seitenleiste.md#papierkorb)) |
| ⌥⌘C | Compress… |
| ⌥⌘E | Extract Here |
| ⌘ while dragging | Preselect Move instead of Copy (without a key Copy is preselected; ⌥ preselects Copy explicitly) |

> [!NOTE]
> On many Mac keyboards you have to hold **fn** (🌐) for **F2**, **F5** and **F6** – unless you turned on "Use F1, F2, etc. keys as standard function keys" in System Settings → **Keyboard**.

## Plugins {#plugins}

| Shortcut | Command | Plugin |
|---|---|---|
| ⌃` | Toggle Terminal | Terminal |
| ⇧⌘B | Toggle Git Panel | Git |
| ⌘↩ | Commit (in the Git panel) | Git |
| ⌥⌘U | Show Disk Usage | Folder Sizes |
| ⌥⌘M | Toggle Markdown Preview | Markdown Preview |
| ⌥⌘R | Re-run Last Script | Project Info |

Your own Quick Actions can have a shortcut of their own (the `shortcut` field in `actions.json`).

## In dialogs and windows {#dialoge}

| Shortcut | Effect |
|---|---|
| ↩ | Confirm (Copy, Create, Rename …) |
| esc | Cancel the dialog |
| ⌘W | Close Settings, Compare & Sync or the preview |

**Command palette**: ↑ ↓ to choose · ↩ to run · esc to close

**Go to Path**: ↩ opens in a new tab · ⌘↩ opens in this tab · ⇥ completes · esc cancels

## When shortcuts don't work {#fokus}

- In a **text field** (filter, path, dialog fields) only **⇧⌘P**, **⇧⌘G**, **⌘,** and **⌘?** work; all other keys belong to the field.
- In the **terminal**, **⌘C**, **⌘V**, **⌘A**, **⌘K**, **⌘←**, **⌘→** and **⌘⌫** belong to the terminal. Click into the file list to give the keyboard back to Manta.
- In the **Settings**, **Compare & Sync** and **preview** windows only their own shortcuts work.

## Differences from Finder {#finder}

| Shortcut | Finder | Manta |
|---|---|---|
| ⌘D | Duplicate | Split Pane |
| ⇧⌘D | Open Desktop | Close Pane |
| ⇧⌘P | Show Preview | Command Palette |
| ⌘R | Show Original | Refresh |
| ⌥⌘C | Copy pathname | Compress… |
| ⌥⌘L | Open Downloads | Show Transfers |
| ⌫ | – | Enclosing Folder |

Like in Finder: **⌘⌫** moves to the Trash, **⌥⌘⌫** deletes immediately, **⇧⌘⌫** empties the Trash, **⌘T** opens a new tab, **⌘Y** opens Quick Look, **⇧⌘N**, **⇧⌘G**, **⇧⌘.**, **⌘[** / **⌘]**, **⌘↑** / **⌘↓**, **⌘1** – **⌘4**, **⌘K** and **⌃⌘T** (Add to Sidebar/Favorites). Dragging preselects Copy, also on the same volume (Finder moves there); **⌘** preselects Move.

> [!NOTE]
> **Changed in this version:** **⌘T** opens a new tab (before: **⇧⌘T**), Transfers moved to **⌥⌘L** (before: **⌘T**), **⌘⌫** moves local items to the Trash (before: permanent delete; that is now **⌥⌘⌫**). Manta shows this once after the update.
