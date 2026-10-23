//! English and German strings of the Folder Sizes plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`.

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
    // Plugin
    ("plugin.name", "Folder Sizes", "Ordnergrößen"),
    ("plugin.description", "Real folder sizes in the file list and a Disk Usage panel that shows what's using space.", "Echte Ordnergrößen in der Dateiliste und ein Panel „Speicherbelegung“, das zeigt, was Platz belegt."),
    ("cmd.show", "Show Disk Usage", "Speicherbelegung einblenden"),
    ("cmd.calculate", "Calculate Size", "Größe berechnen"),
    ("panel.title", "Disk Usage", "Speicherbelegung"),
    ("toast.local_only", "Folder sizes are available for local folders only", "Ordnergrößen gibt es nur für lokale Ordner"),
    // Settings
    ("settings.auto", "Calculate folder sizes automatically", "Ordnergrößen automatisch berechnen"),
    ("settings.auto.tip", "Measure every folder you look at in the background. Results are cached.", "Jeden angezeigten Ordner im Hintergrund messen. Die Ergebnisse werden zwischengespeichert."),
    ("settings.manual", "Only on demand", "Nur bei Bedarf"),
    ("settings.manual.tip", "Folders show “—” until you right-click them and choose Calculate Size. The Disk Usage panel (⌥⌘U) always measures.", "Ordner zeigen „—“, bis du sie mit der rechten Maustaste anklickst und „Größe berechnen“ wählst. Das Panel „Speicherbelegung“ (⌥⌘U) misst immer."),
    // Panel
    ("empty.local.title", "Local folders only", "Nur lokale Ordner"),
    ("empty.local.body", "Disk usage is measured for folders on this Mac. Open a local folder in the active pane to see what’s using space.", "Die Speicherbelegung wird für Ordner auf diesem Mac gemessen. Öffne einen lokalen Ordner im aktiven Bereich, um zu sehen, was Platz belegt."),
    ("error.unreadable", "This folder couldn’t be read", "Dieser Ordner konnte nicht gelesen werden"),
    ("reading", "Reading folder…", "Ordner wird gelesen …"),
    ("empty.folder", "This folder is empty.", "Dieser Ordner ist leer."),
    ("measuring", "Measuring…", "Wird gemessen …"),
    ("empty.no_space", "Nothing here takes up space.", "Hier belegt nichts Platz."),
    ("largest", "Largest Items", "Größte Objekte"),
    ("counting", "Counting…", "Wird gezählt …"),
    ("files.one", "{count} file", "{count} Datei"),
    ("files.other", "{count} files", "{count} Dateien"),
    ("smaller.one", "{count} smaller item", "{count} kleineres Objekt"),
    ("smaller.other", "{count} smaller items", "{count} kleinere Objekte"),
    ("share_of", "{size} · {pct} of “{parent}”", "{size} · {pct} von „{parent}“"),
    ("on_disk", "{size} on disk", "{size} auf dem Volume"),
    ("some_unreadable", "Some folders inside couldn’t be read.", "Einige Ordner darin konnten nicht gelesen werden."),
    ("click_hint", "Click to zoom in · double-click to open", "Klicken zum Vergrößern · Doppelklicken zum Öffnen"),
    ("legend.folders", "Folders (shaded)", "Ordner (schattiert)"),
    // Sizes
    ("bytes.one", "{count} byte", "{count} Byte"),
    ("bytes.other", "{count} bytes", "{count} Byte"),
    // Categories
    ("cat.images", "Images", "Bilder"),
    ("cat.video", "Video", "Videos"),
    ("cat.audio", "Audio", "Audio"),
    ("cat.documents", "Documents", "Dokumente"),
    ("cat.code", "Code", "Code"),
    ("cat.archives", "Archives", "Archive"),
    ("cat.apps", "Apps", "Apps"),
    ("cat.misc", "Other", "Andere"),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::*;

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(&CATALOG, &["Audio", "Code", "Apps"]);
        assert!(p.is_empty(), "{}", p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let m = check::missing_keys(&CATALOG, &src, 25);
        assert!(m.is_empty(), "{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let hits = check::hard_coded_strings(&src, &["empty_state(", "swatch_label("], &[]);
        assert!(hits.is_empty(), "{}", hits.join("\n"));
    }

    #[test]
    fn german_titles() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("panel.title"), "Speicherbelegung");
            assert_eq!(tr!("files", count = 1), "1 Datei");
            assert_eq!(tr!("files", count = 1200), "1.200 Dateien");
        });
        with_lang(Lang::En, || assert_eq!(tr!("panel.title"), "Disk Usage"));
    }
}
