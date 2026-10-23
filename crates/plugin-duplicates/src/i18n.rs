//! English and German strings of the Duplicates plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`. Carried over 1:1
//! from the former Python plugin's `dup_i18n.py`.

use manta_plugin::i18n::{Catalog, Entries, Lang};

mod es;
mod fr;
mod it;
mod ru;
mod zh;

pub static CATALOG: Catalog = Catalog::with(
    ENTRIES,
    &[
        (Lang::Zh, zh::ENTRIES),
        (Lang::Es, es::ENTRIES),
        (Lang::Fr, fr::ENTRIES),
        (Lang::It, it::ENTRIES),
        (Lang::Ru, ru::ENTRIES),
    ],
);

macro_rules! tr {
    ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
}
pub(crate) use tr;

const ENTRIES: Entries = &[
    // Plugin (Settings → Plugins)
    ("plugin.name", "Duplicates", "Duplikate"),
    ("plugin.description", "Finds identical files in a folder and its subfolders (size, then a quick hash, then SHA-256) and moves the extra copies to the Trash.", "Findet identische Dateien in einem Ordner und seinen Unterordnern (Größe, dann ein schneller Hash, dann SHA-256) und legt die überzähligen Kopien in den Papierkorb."),
    // Commands and panels
    ("cmd.scan", "Find Duplicates in This Folder", "Duplikate in diesem Ordner suchen"),
    ("cmd.panel", "Show Duplicates Panel", "Duplikate-Panel einblenden"),
    ("cmd.settings", "Edit Duplicates Settings…", "Duplikate-Einstellungen bearbeiten …"),
    ("cmd.scan_here", "Find Duplicates Here", "Duplikate hier suchen"),
    ("cmd.keep_here", "Keep Duplicates in This Folder", "Duplikate in diesem Ordner behalten"),
    ("panel.duplicates", "Duplicates", "Duplikate"),
    ("panel.confirm", "Move to Trash", "In den Papierkorb legen"),
    // Keep rules
    ("rule.oldest", "Oldest", "Älteste"),
    ("rule.newest", "Newest", "Neueste"),
    ("rule.shortest", "Shortest Path", "Kürzester Pfad"),
    ("rule.folder", "In Folder", "Im Ordner"),
    ("rule.in", "In “{name}”", "In „{name}“"),
    ("rule.a_folder", "a folder", "einem Ordner"),
    ("rule.unset", "In Folder (not set)", "Im Ordner (nicht festgelegt)"),
    // Options
    ("opt.section", "Options", "Optionen"),
    ("opt.keep", "Keep", "Behalten"),
    ("opt.min_size", "Minimum size", "Mindestgröße"),
    ("opt.hidden", "Hidden files", "Unsichtbare Dateien"),
    ("opt.included", "Included", "Einbezogen"),
    ("opt.skipped", "Skipped", "Übersprungen"),
    ("opt.skipped_key", "Skipped", "Ausgelassen"),
    ("opt.packages", "packages", "Pakete"),
    ("opt.nothing", "nothing", "nichts"),
    ("btn.keep_rule", "Keep Rule", "Behalten-Regel"),
    ("btn.min_size", "Min Size", "Mindestgröße"),
    ("btn.hidden", "Hidden", "Unsichtbare"),
    ("btn.settings", "Settings…", "Einstellungen …"),
    // Groups
    ("group.kept", "Kept ({why}) · modified {date}", "Bleibt ({why}) · geändert {date}"),
    ("group.chosen", "chosen by you", "von dir gewählt"),
    ("group.goes", "Goes to the Trash · modified {date}\nClick to keep this copy instead", "Kommt in den Papierkorb · geändert {date}\nKlicke, um stattdessen diese Kopie zu behalten"),
    ("group.each", "{size} each · {total}", "je {size} · {total}"),
    ("btn.trash_extras", "Trash Extras", "Kopien entfernen"),
    ("btn.reveal", "Reveal", "Im Finder zeigen"),
    ("btn.select", "Select in Pane", "Im Bereich auswählen"),
    // Panel
    ("scan.title", "Scanning “{name}”", "„{name}“ wird durchsucht"),
    ("scan.listing", "Listing files … {n} found", "Dateien werden aufgelistet … {n} gefunden"),
    ("scan.hashing", "Hashed {done} / {total} files", "{done} / {total} Dateien geprüft"),
    ("scan.verifying", "Verifying {done} / {total} files", "{done} / {total} Dateien werden verifiziert"),
    ("btn.cancel", "Cancel", "Abbrechen"),
    ("trash.progress", "Moving to the Trash … {done} / {total}", "In den Papierkorb legen … {done} / {total}"),
    ("intro", "Finds identical files in “{name}” and its subfolders.", "Sucht identische Dateien in „{name}“ und seinen Unterordnern."),
    ("btn.find", "Find Duplicates", "Duplikate suchen"),
    ("result.title", "“{name}”", "„{name}“"),
    ("none_found", "No duplicates found.", "Keine Duplikate gefunden."),
    ("key.groups", "Groups", "Gruppen"),
    ("key.files", "Duplicate files", "Doppelte Dateien"),
    ("key.reclaimable", "Reclaimable", "Freizugeben"),
    ("btn.trash_all", "Move Extras to Trash", "Kopien in den Papierkorb"),
    ("note.sep", "; ", "; "),
    ("note.end", ".", "."),
    ("note.skipped.one", "skipped {folders}", "{folders} übersprungen"),
    ("note.skipped.other", "skipped {folders}", "{folders} übersprungen"),
    ("note.hard_links", "{links} counted once", "{links} einmal gezählt"),
    ("note.unreadable", "{n} unreadable", "{n} nicht lesbar"),
    ("stale", "Options changed: scan again to apply them.", "Optionen geändert: Suche erneut, um sie anzuwenden."),
    ("btn.scan_folder", "Scan “{name}”", "„{name}“ durchsuchen"),
    ("btn.scan_again", "Scan Again", "Erneut suchen"),
    ("btn.clear", "Clear", "Leeren"),
    ("more", "{groups} more", "{groups} mehr"),
    ("btn.more", "Show More", "Mehr zeigen"),
    ("remote", "Duplicates works on folders on this Mac. The active pane shows {label}; switch to a local folder to scan it.", "Duplikate funktioniert mit Ordnern auf diesem Mac. Der aktive Bereich zeigt {label}; wechsle zu einem lokalen Ordner, um ihn zu durchsuchen."),
    ("remote.server", "this server", "diesen Server"),
    // Messages and toasts
    ("msg.cancelled", "Scan cancelled.", "Suche abgebrochen."),
    ("msg.failed", "Scan failed: {error}", "Suche fehlgeschlagen: {error}"),
    ("toast.local_only", "Duplicates works on local folders only.", "Duplikate funktioniert nur mit lokalen Ordnern."),
    ("toast.keep_here", "Duplicates: keeping copies in “{name}” first", "Duplikate: Kopien in „{name}“ werden bevorzugt behalten"),
    ("toast.save_failed", "Couldn't save the Duplicates settings: {error}", "Die Duplikate-Einstellungen konnten nicht gesichert werden: {error}"),
    ("toast.open_failed", "Couldn't open {path}: {error}", "{path} konnte nicht geöffnet werden: {error}"),
    ("toast.reveal_failed", "Couldn't reveal: {error}", "Konnte nicht im Finder gezeigt werden: {error}"),
    ("toast.selected_some", "Selected {n} of {total} extras; the others are in other folders.", "{n} von {total} Kopien ausgewählt; die anderen liegen in anderen Ordnern."),
    ("toast.moved", "Moved {files} to the Trash · {size} freed", "{files} in den Papierkorb gelegt · {size} frei"),
    ("toast.moved_part", "Moved {files} to the Trash ({size})", "{files} in den Papierkorb gelegt ({size})"),
    ("toast.changed", "{files} changed since the scan and stayed", "{files} seit der Suche geändert und behalten"),
    ("toast.failed.one", "{count} file couldn't be moved", "{count} Datei konnte nicht bewegt werden"),
    ("toast.failed.other", "{count} files couldn't be moved", "{count} Dateien konnten nicht bewegt werden"),
    // Confirmation
    ("confirm.title", "Move {files} to the Trash?", "{files} in den Papierkorb legen?"),
    ("confirm.body", "This frees {size}. One copy of each file stays where it is; you can put the others back from the Trash.", "Dadurch werden {size} frei. Je eine Kopie jeder Datei bleibt, wo sie ist; die anderen kannst du aus dem Papierkorb zurücklegen."),
    ("confirm.more", "and {n} more", "und {n} weitere"),
    ("btn.move", "Move to Trash", "In den Papierkorb"),
    // Counts
    ("files.one", "{count} file", "{count} Datei"),
    ("files.other", "{count} files", "{count} Dateien"),
    ("folders.one", "{count} folder", "{count} Ordner"),
    ("folders.other", "{count} folders", "{count} Ordner"),
    ("links.one", "{count} hard link", "{count} fester Link"),
    ("links.other", "{count} hard links", "{count} feste Links"),
    ("groups.one", "{count} group", "{count} Gruppe"),
    ("groups.other", "{count} groups", "{count} Gruppen"),
    ("bytes.one", "{count} byte", "{count} Byte"),
    ("bytes.other", "{count} bytes", "{count} Byte"),
    // Formats
    ("date.this_year", "{day} {month}, {time}", "{day}. {month}, {time}"),
    ("date.other_year", "{day} {month} {year}", "{day}. {month} {year}"),
    ("settings.broken", "settings.json: {error}", "settings.json: {error}"),
    ("settings.not_object", "settings.json must hold an object", "settings.json muss ein Objekt enthalten"),
    ("settings.bad_value", "settings.json: bad value for {key} (default {default} used)", "settings.json: ungültiger Wert für {key} (Standard {default} verwendet)"),
    // Settings → Plugins (native settings area)
    ("settings.skip_packages", "Skip packages (.app, .photoslibrary, …)", "Pakete überspringen (.app, .photoslibrary, …)"),
    ("settings.include_hidden", "Include hidden files", "Unsichtbare Dateien einbeziehen"),
    ("settings.edit_json", "Edit settings.json …", "settings.json bearbeiten …"),
    ("settings.edit_json.tip", "Exclusions and a custom trash command live in settings.json.", "Ausschlüsse und ein eigener Papierkorb-Befehl stehen in settings.json."),
];

/// Abbreviated month name for 1..12 ("Sep" / "Sept." / "sept." / "9月"),
/// see `manta_plugin::i18n::month_abbr`.
pub fn month(m: u32) -> &'static str {
    manta_plugin::i18n::month_abbr(manta_plugin::i18n::lang(), m.clamp(1, 12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_plugin::i18n::check;
    use std::path::Path;

    #[test]
    fn catalog_is_complete_and_translated() {
        // Same in both languages on purpose: a quoted name, a file name.
        let p = check::catalog_problems(&CATALOG, &["“{name}”", "settings.json: {error}"]);
        assert!(p.is_empty(), "{p:#?}");
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing = check::missing_keys(&CATALOG, &src, 60);
        assert!(missing.is_empty(), "{missing:#?}");
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let hits = check::hard_coded_strings(&src, &[], &[]);
        assert!(hits.is_empty(), "{hits:#?}");
    }

    #[test]
    fn german_style() {
        for &(k, _, de) in CATALOG.entries() {
            if de.contains('“') {
                assert!(de.contains('„'), "{k}");
            }
            assert!(!de.contains("..."), "{k}");
            if de.contains('…') {
                assert!(de.contains(" …"), "{k}");
            }
        }
    }

    #[test]
    fn months() {
        manta_plugin::i18n::with_lang(manta_plugin::i18n::Lang::De, || {
            assert_eq!(month(9), "Sept.")
        });
        manta_plugin::i18n::with_lang(manta_plugin::i18n::Lang::En, || assert_eq!(month(9), "Sep"));
    }
}
