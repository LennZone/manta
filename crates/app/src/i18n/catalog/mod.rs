//! The string catalog. Source tables: `(key, English, German)` per entry,
//! grouped by area (`<area>.rs`). Plurals are two entries, `key.one` and
//! `key.other`. Placeholders are `{name}`; every language must use the
//! same ones.
//!
//! Translations: `<lang>/<area>.rs` with `(key, text)` per entry for zh
//! (Simplified Chinese), es, fr, it and ru — same keys, plural forms per
//! CLDR (see `manta_plugin::i18n`). `scripts/i18n/skeleton.py` adds new
//! source keys to them (marked `// TODO`), `scripts/i18n/check.py` checks
//! them without cargo.
//!
//! Every language follows Apple's macOS terminology (Finder). German:
//! "Ablage", "Einstellungen …", "In den Papierkorb legen", "Informationen".

pub use manta_plugin::i18n::{Entries, Translations};

use super::Locale;

mod app;
mod commands;
mod common;
mod dialogs;
mod help;
mod pane;
mod panels;
mod panes;
mod settings;
mod transfers;
mod updates;
mod viewer;
mod views;

pub const ALL: &[Entries] = &[
    common::ENTRIES,
    commands::ENTRIES,
    settings::ENTRIES,
    transfers::ENTRIES,
    app::ENTRIES,
    dialogs::ENTRIES,
    pane::ENTRIES,
    panels::ENTRIES,
    panes::ENTRIES,
    viewer::ENTRIES,
    updates::ENTRIES,
    help::ENTRIES,
    views::ENTRIES,
];

// Translations: one folder per language, one file per area.
pub mod es;
pub mod fr;
pub mod it;
pub mod ru;
pub mod zh;

/// Every translation table with its language.
pub fn translations() -> impl Iterator<Item = (Locale, Translations)> {
    let langs: [(Locale, &[Translations]); 5] = [
        (Locale::Zh, zh::ALL),
        (Locale::Es, es::ALL),
        (Locale::Fr, fr::ALL),
        (Locale::It, it::ALL),
        (Locale::Ru, ru::ALL),
    ];
    langs
        .into_iter()
        .flat_map(|(l, parts)| parts.iter().map(move |&t| (l, t)))
}

/// The source tables with a title (glossary sections).
#[cfg(test)]
pub const AREAS: &[(&str, Entries)] = &[
    ("Common", common::ENTRIES),
    ("Commands and menus", commands::ENTRIES),
    ("Main window", app::ENTRIES),
    ("File pane and sidebar", pane::ENTRIES),
    ("Panes", panes::ENTRIES),
    ("Views", views::ENTRIES),
    ("Dialogs", dialogs::ENTRIES),
    ("Panels", panels::ENTRIES),
    ("Transfers", transfers::ENTRIES),
    ("Settings", settings::ENTRIES),
    ("Viewer", viewer::ENTRIES),
    ("Updates", updates::ENTRIES),
    ("Help", help::ENTRIES),
];
