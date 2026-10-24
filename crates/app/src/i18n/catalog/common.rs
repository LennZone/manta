//! Shared words, settings, appearance and language.

use super::Entries;

pub const ENTRIES: Entries = &[
    ("common.ok", "OK", "OK"),
    ("common.cancel", "Cancel", "Abbrechen"),
    ("common.close", "Close", "Schließen"),
    ("language.system", "System", "System"),
    ("appearance.system", "Auto", "Automatisch"),
    ("appearance.light", "Light", "Hell"),
    ("appearance.dark", "Dark", "Dunkel"),
    ("accent.blue", "Blue", "Blau"),
    ("accent.purple", "Purple", "Lila"),
    ("accent.pink", "Pink", "Rosa"),
    ("accent.red", "Red", "Rot"),
    ("accent.orange", "Orange", "Orange"),
    ("accent.yellow", "Yellow", "Gelb"),
    ("accent.green", "Green", "Grün"),
    ("accent.graphite", "Graphite", "Graphit"),
    // Formatting (util.rs)
    ("date.today", "Today {time}", "Heute {time}"),
    ("date.yesterday", "Yesterday {time}", "Gestern {time}"),
    (
        "error.name_empty",
        "Name must not be empty",
        "Der Name darf nicht leer sein.",
    ),
    (
        "error.name_reserved",
        "\".\" and \"..\" are reserved",
        "„.“ und „..“ sind reserviert.",
    ),
    (
        "error.name_slash",
        "Name must not contain \"/\"",
        "Der Name darf kein „/“ enthalten.",
    ),
    (
        "error.save_settings",
        "Couldn't save settings",
        "Die Einstellungen konnten nicht gesichert werden.",
    ),
];
