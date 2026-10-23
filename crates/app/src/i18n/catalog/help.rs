//! Help window (`help/`) and the Help menu.

use super::Entries;

pub const ENTRIES: Entries = &[
    ("help.window_title", "Manta Help", "Manta-Hilfe"),
    ("help.search", "Search the Manual", "Handbuch durchsuchen"),
    ("help.back", "Back", "Zurück"),
    ("help.forward", "Forward", "Vorwärts"),
    ("help.no_results", "No results", "Keine Treffer"),
    ("help.results.one", "{count} result", "{count} Treffer"),
    ("help.results.other", "{count} results", "{count} Treffer"),
    ("help.chapter", "Chapter", "Kapitel"),
    ("help.button", "Help", "Hilfe"),
    (
        "help.button.tip",
        "Open the manual on this topic",
        "Handbuch zu diesem Thema öffnen",
    ),
    ("menu.help", "Help", "Hilfe"),
    ("menu.file", "File", "Ablage"),
    ("menu.edit", "Edit", "Bearbeiten"),
    ("menu.view", "View", "Darstellung"),
    ("menu.go", "Go", "Gehe zu"),
    ("menu.window", "Window", "Fenster"),
    ("menu.manual", "Manta Manual", "Manta-Handbuch"),
];
