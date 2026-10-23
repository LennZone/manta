//! Several panes: split, close, focus, the copy target and the Compare &
//! Sync pane picker (`app.rs`, `pane.rs`, `pane_set.rs`, `sync_view.rs`,
//! `dialogs.rs`).

use super::Entries;

pub const ENTRIES: Entries = &[
    ("cmd.split_pane", "Split Pane", "Bereich teilen"),
    ("cmd.close_pane", "Close Pane", "Bereich schließen"),
    ("cmd.focus_pane_left", "Focus Pane on the Left", "Bereich links aktivieren"),
    ("cmd.focus_pane_right", "Focus Pane on the Right", "Bereich rechts aktivieren"),
    ("cmd.kw.split_pane", "split pane column add new divide side by side", "teilen bereich spalte neu hinzufügen nebeneinander"),
    ("cmd.kw.close_pane", "close pane remove column", "schließen bereich entfernen spalte"),
    ("cmd.kw.focus_pane", "focus pane move left right next previous", "fokus bereich wechseln links rechts nächster vorheriger"),
    ("panes.needs_two", "Needs two panes – ⌘D splits the pane", "Benötigt zwei Bereiche – ⌘D teilt den Bereich"),
    ("panes.last_pane", "The last pane can't be closed", "Der letzte Bereich kann nicht geschlossen werden"),
    ("panes.no_room", "No room for another pane", "Kein Platz für einen weiteren Bereich"),
    ("panes.no_room.detail", "Each pane needs at least {min} points. Make the window wider or close a pane.", "Jeder Bereich braucht mindestens {min} Punkte. Mach das Fenster breiter oder schließe einen Bereich."),
    ("panes.target_tip", "Copy and Move (F5 / F6) go to this pane", "Kopieren und Bewegen (F5 / F6) gehen in diesen Bereich"),
    ("panes.sync_pick_a", "Compare this pane as A", "Diesen Bereich als A vergleichen"),
    ("panes.sync_pick_b", "Compare this pane as B", "Diesen Bereich als B vergleichen"),
    ("panes.pane_n", "Pane {n}", "Bereich {n}"),
    ("toolbar.copy_to_pane", "Copy to Pane {n} ({name})", "In Bereich {n} ({name}) kopieren"),
    ("toolbar.move_to_pane", "Move to Pane {n} ({name})", "In Bereich {n} ({name}) bewegen"),
    ("connect.subtitle.pane", "The server opens in pane {n}.", "Der Server wird in Bereich {n} geöffnet."),
    ("connect.subtitle.single", "The server opens in the pane.", "Der Server wird im Bereich geöffnet."),
    ("dialog.transfer.to", "To", "Nach"),
    ("dialog.transfer.target_tip", "Choose the pane to copy to", "Wähle den Bereich, in den kopiert wird"),
    ("sync.pane_a", "Pane A", "Bereich A"),
    ("sync.pane_b", "Pane B", "Bereich B"),
    ("sync.pick_tip", "Choose which pane to compare", "Wähle den Bereich, der verglichen wird"),
];
