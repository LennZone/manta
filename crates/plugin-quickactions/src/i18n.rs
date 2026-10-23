//! English and German strings of the Quick Actions plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`. Carried over from
//! the Python plugin's `qa_i18n.py`. Keys ending in `.one` / `.other` are
//! plurals; `default.<id>` are the titles of the shipped default actions
//! (used while their English text in actions.json is unchanged).

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

/// German texts that are the same as the English ones on purpose.
pub const SAME_IN_GERMAN: &[&str] = &["in {path}"];

/// A key built at runtime (`default.<id>`), `None` if unknown.
pub fn lookup(key: &str) -> Option<&'static str> {
    CATALOG.has(key).then(|| CATALOG.tr_key(key))
}

/// Plural of a runtime key (`key.one` / `key.other`, CLDR forms per
/// language) with `{count}` filled in (thousands separator of the language).
pub fn plural_key(key: &str, count: usize) -> Option<String> {
    CATALOG.plural_key(key, count as u64)
}

pub fn is_german() -> bool {
    manta_plugin::i18n::lang() == Lang::De
}

const ENTRIES: Entries = &[
    // Plugin
    ("plugin.name", "Quick Actions", "Schnellaktionen"),
    ("plugin.description", "Your own commands for the selection: resize images, zip, open in VS Code, checksums and more. Configured in actions.json.", "Deine eigenen Befehle für die Auswahl: Bilder skalieren, ZIP erstellen, in VS Code öffnen, Prüfsummen und mehr. Eingerichtet in actions.json."),
    ("cmd.edit", "Edit Quick Actions…", "Schnellaktionen bearbeiten …"),
    ("cmd.reload", "Reload Quick Actions", "Schnellaktionen neu laden"),
    ("cmd.panel", "Show Quick Actions Panel", "Schnellaktionen-Panel einblenden"),
    ("panel.actions", "Quick Actions", "Schnellaktionen"),
    ("panel.confirm", "Quick Action", "Schnellaktion"),
    ("load.error", "Quick Actions: {error}", "Schnellaktionen: {error}"),
    ("load.ok.one", "Loaded {count} quick action", "{count} Schnellaktion geladen"),
    ("load.ok.other", "Loaded {count} quick actions", "{count} Schnellaktionen geladen"),
    ("load.skipped.one", "Loaded {count} quick action; {skipped} skipped: {error}", "{count} Schnellaktion geladen, {skipped} übersprungen: {error}"),
    ("load.skipped.other", "Loaded {count} quick actions; {skipped} skipped: {error}", "{count} Schnellaktionen geladen, {skipped} übersprungen: {error}"),
    ("panel.waiting", "Waiting for Manta…", "Warte auf Manta …"),
    ("panel.in_archive", "Inside an archive", "In einem Archiv"),
    ("panel.remote", "Remote folder", "Entfernter Ordner"),
    ("panel.local_only_archive", "Quick Actions work on local folders only. Extract the items first (⌥⌘E).", "Schnellaktionen funktionieren nur in lokalen Ordnern. Entpacke die Objekte zuerst (⌥⌘E)."),
    ("panel.local_only", "Quick Actions work on local folders only.", "Schnellaktionen funktionieren nur in lokalen Ordnern."),
    ("panel.nothing_selected", "Nothing selected", "Keine Auswahl"),
    ("panel.select_hint", "Select files to see what you can do with them.", "Wähle Dateien aus, um zu sehen, was du mit ihnen tun kannst."),
    ("panel.in_folder", "in {path}", "in {path}"),
    ("panel.no_actions", "No quick actions for this selection.", "Keine Schnellaktionen für diese Auswahl."),
    ("panel.last_runs", "Last runs", "Zuletzt ausgeführt"),
    ("panel.nothing_run", "Nothing run yet.", "Noch nichts ausgeführt."),
    ("panel.cancel_named", "Cancel “{title}”", "„{title}“ abbrechen"),
    ("panel.edit", "Edit Actions…", "Aktionen bearbeiten …"),
    ("panel.reload", "Reload", "Neu laden"),
    ("items.one", "{count} item", "{count} Objekt"),
    ("items.other", "{count} items", "{count} Objekte"),
    ("status.waiting", "waiting", "wartet"),
    ("status.running", "running · {time}", "läuft · {time}"),
    ("status.in_terminal", "in Terminal", "im Terminal"),
    ("status.failed", "failed · {time}", "fehlgeschlagen · {time}"),
    ("status.cancelled", "cancelled", "abgebrochen"),
    ("status.more", "(+{count} more)", "(+{count} weitere)"),
    ("toast.cancelled", "Cancelled “{title}”", "„{title}“ abgebrochen"),
    ("toast.failed", "“{title}” failed: {why}", "„{title}“ fehlgeschlagen: {why}"),
    ("toast.exit_status", "exit status {code}", "Exit-Status {code}"),
    ("toast.no_output", "(no output)", "(keine Ausgabe)"),
    ("toast.done", "{title}: done", "{title}: fertig"),
    ("toast.running", "Running “{title}” on {what}…", "„{title}“ läuft für {what} …"),
    ("toast.local_only", "Quick Actions work on local folders only", "Schnellaktionen funktionieren nur in lokalen Ordnern"),
    ("toast.not_available", "“{title}” isn't available here: {why}", "„{title}“ ist hier nicht verfügbar: {why}"),
    ("toast.plan_error", "“{title}”: {error}", "„{title}“: {error}"),
    ("toast.gone", "That quick action no longer exists (actions.json changed)", "Diese Schnellaktion gibt es nicht mehr (actions.json wurde geändert)"),
    ("toast.open_error", "Couldn't open {path}: {error}", "{path} konnte nicht geöffnet werden: {error}"),
    ("confirm.and_more", "{names} and {count} more", "{names} und {count} weitere"),
    ("confirm.run", "Run", "Ausführen"),
    ("cancel", "Cancel", "Abbrechen"),
    ("why.min.one", "select at least {count} item", "wähle mindestens {count} Objekt aus"),
    ("why.min.other", "select at least {count} items", "wähle mindestens {count} Objekte aus"),
    ("why.max.one", "select at most {count} item", "wähle höchstens {count} Objekt aus"),
    ("why.max.other", "select at most {count} items", "wähle höchstens {count} Objekte aus"),
    ("why.folders", "it doesn't work on folders", "sie funktioniert nicht mit Ordnern"),
    ("why.ext", "it doesn't work on “{ext}” files", "sie funktioniert nicht mit „{ext}“-Dateien"),
    ("why.these", "it doesn't work on these files", "sie funktioniert nicht mit diesen Dateien"),
    ("why.dest", "the other pane doesn't show a local folder", "der andere Bereich zeigt keinen lokalen Ordner"),
    ("err.object", "an action must be an object", "Eine Aktion muss ein Objekt sein"),
    ("err.id", "action id {id}: use letters, digits, '.', '-' or '_'", "Aktions-ID {id}: Verwende Buchstaben, Ziffern, '.', '-' oder '_'"),
    ("err.title", "action {id}: “title” is missing", "Aktion {id}: „title“ fehlt"),
    ("err.command", "action {id}: “command” is missing", "Aktion {id}: „command“ fehlt"),
    ("err.applies", "action {id}: “applies_to” must be a list", "Aktion {id}: „applies_to“ muss eine Liste sein"),
    ("err.mode", "action {id}: “mode” must be one of {values}", "Aktion {id}: „mode“ muss einer dieser Werte sein: {values}"),
    ("err.output", "action {id}: “output” must be one of {values}", "Aktion {id}: „output“ muss einer dieser Werte sein: {values}"),
    ("err.min", "action {id}: “min” must be a number >= 0", "Aktion {id}: „min“ muss eine Zahl >= 0 sein"),
    ("err.max", "action {id}: “max” must be a number >= min", "Aktion {id}: „max“ muss eine Zahl >= min sein"),
    ("err.mix", "action {id}: use either {file}-style or {files}-style placeholders", "Aktion {id}: Verwende entweder Platzhalter wie {file} oder wie {files}"),
    ("err.json", "actions.json isn't valid JSON: {error}", "actions.json ist kein gültiges JSON: {error}"),
    ("err.shape", "actions.json must hold {\"actions\": [ … ]}", "actions.json muss {\"actions\": [ … ]} enthalten"),
    ("err.dup", "action id {id} is used twice", "Die Aktions-ID {id} wird doppelt verwendet"),
    ("err.no_value", "{placeholder} has no value here", "{placeholder} hat hier keinen Wert"),
    ("default.resize-50", "Resize Images to 50%", "Bilder auf 50 % verkleinern"),
    ("default.resize-50.message.one", "Resized {count} image", "{count} Bild verkleinert"),
    ("default.resize-50.message.other", "Resized {count} images", "{count} Bilder verkleinert"),
    ("default.to-jpeg", "Convert to JPEG", "In JPEG umwandeln"),
    ("default.to-jpeg.message.one", "Converted {count} image to JPEG", "{count} Bild in JPEG umgewandelt"),
    ("default.to-jpeg.message.other", "Converted {count} images to JPEG", "{count} Bilder in JPEG umgewandelt"),
    ("default.to-png", "Convert to PNG", "In PNG umwandeln"),
    ("default.to-png.message.one", "Converted {count} image to PNG", "{count} Bild in PNG umgewandelt"),
    ("default.to-png.message.other", "Converted {count} images to PNG", "{count} Bilder in PNG umgewandelt"),
    ("default.zip", "Compress to ZIP", "Als ZIP komprimieren"),
    ("default.zip-many", "Compress to ZIP", "Als ZIP komprimieren"),
    ("default.extract", "Extract Archive", "Archiv entpacken"),
    ("default.extract.message.one", "Extracted {count} archive", "{count} Archiv entpackt"),
    ("default.extract.message.other", "Extracted {count} archives", "{count} Archive entpackt"),
    ("default.vscode", "Open in VS Code", "In VS Code öffnen"),
    ("default.terminal-app", "Open in Terminal.app", "In Terminal.app öffnen"),
    ("default.copy-path", "Copy Path", "Pfad kopieren"),
    ("default.copy-path.message.one", "Copied {count} path", "{count} Pfad kopiert"),
    ("default.copy-path.message.other", "Copied {count} paths", "{count} Pfade kopiert"),
    ("default.reveal", "Reveal in Finder", "Im Finder zeigen"),
    ("default.chmod-x", "Make Executable", "Ausführbar machen"),
    ("default.chmod-x.message.one", "Made {count} file executable", "{count} Datei ausführbar gemacht"),
    ("default.chmod-x.message.other", "Made {count} files executable", "{count} Dateien ausführbar gemacht"),
    ("default.count-lines", "Count Lines", "Zeilen zählen"),
    ("default.sha256", "Calculate SHA-256", "SHA-256 berechnen"),
    ("default.rsync-other", "Copy to Other Pane (rsync)", "Per rsync in anderen Bereich"),
    ("default.rsync-other.confirm.one", "Copy {count} item to the other pane with rsync? Existing files with the same names are overwritten.", "{count} Objekt mit rsync in den anderen Bereich kopieren? Vorhandene Dateien mit gleichem Namen werden überschrieben."),
    ("default.rsync-other.confirm.other", "Copy {count} items to the other pane with rsync? Existing files with the same names are overwritten.", "{count} Objekte mit rsync in den anderen Bereich kopieren? Vorhandene Dateien mit gleichem Namen werden überschrieben."),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::*;

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(&CATALOG, SAME_IN_GERMAN);
        assert!(p.is_empty(), "{}", p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let m = check::missing_keys(&CATALOG, &src, 40);
        assert!(m.is_empty(), "{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let hits = check::hard_coded_strings(&src, &[], &[]);
        assert!(hits.is_empty(), "{}", hits.join("\n"));
    }

    #[test]
    fn every_default_action_has_a_german_title() {
        for id in crate::core::defaults().keys() {
            assert!(CATALOG.has(&format!("default.{id}")), "{id}");
            with_lang(Lang::De, || {
                assert_ne!(lookup(&format!("default.{id}")), lookup("nope"))
            });
        }
    }

    #[test]
    fn plurals_and_lookups() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("items", count = 1), "1 Objekt");
            assert_eq!(tr!("items", count = 1200), "1.200 Objekte");
            assert_eq!(
                plural_key("default.copy-path.message", 2).as_deref(),
                Some("2 Pfade kopiert")
            );
            assert_eq!(tr!("cmd.edit"), "Schnellaktionen bearbeiten …");
        });
        with_lang(Lang::En, || {
            assert_eq!(tr!("items", count = 1), "1 item");
            assert_eq!(plural_key("nope", 1), None);
        });
    }
}
