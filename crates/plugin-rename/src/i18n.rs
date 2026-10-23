//! Batch Rename strings, English and German (see `manta_plugin::i18n`).
//! German follows Finder: "Umbenennen", "Suffix" for file extensions,
//! "Widerrufen", "du", „…“ quotes and " …".

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

/// Translate: `tr!("key")`, `tr!("key", count = n)`, `tr!("key", name = v)`.
macro_rules! tr {
    ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
}
pub(crate) use tr;

const ENTRIES: Entries = &[
    // Plugin, commands, panel
    ("info.name", "Batch Rename", "Stapelweise umbenennen"),
    (
        "info.description",
        "Rename many files at once with rules, a live preview and undo.",
        "Benenne viele Dateien auf einmal um – mit Regeln, Live-Vorschau und Widerrufen.",
    ),
    ("cmd.batch", "Batch Rename…", "Stapelweise umbenennen …"),
    (
        "cmd.undo",
        "Undo Batch Rename",
        "Stapelweises Umbenennen widerrufen",
    ),
    ("panel.title", "Batch Rename", "Stapelweise umbenennen"),
    (
        "panel.idle",
        "Select files and choose Batch Rename… (⇧⌘R)",
        "Wähle Dateien aus und dann „Stapelweise umbenennen …“ (⇧⌘R)",
    ),
    // Toasts and the footer banner
    (
        "toast.nothing_to_undo",
        "Nothing to undo",
        "Nichts zu widerrufen",
    ),
    (
        "banner.nothing_renamed",
        "Nothing was renamed",
        "Es wurde nichts umbenannt",
    ),
    (
        "banner.nothing_renamed_error",
        "Nothing was renamed: {error}",
        "Es wurde nichts umbenannt: {error}",
    ),
    (
        "toast.renamed.one",
        "Renamed {count} item",
        "{count} Objekt umbenannt",
    ),
    (
        "toast.renamed.other",
        "Renamed {count} items",
        "{count} Objekte umbenannt",
    ),
    (
        "toast.renamed_partial",
        "Renamed {done} of {total} items · “{name}”: {error}{more}",
        "{done} von {total} Objekten umbenannt · „{name}“: {error}{more}",
    ),
    ("toast.undo", "Undo", "Widerrufen"),
    (
        "toast.undid.one",
        "Undid the batch rename ({count} item)",
        "Umbenennen widerrufen ({count} Objekt)",
    ),
    (
        "toast.undid.other",
        "Undid the batch rename ({count} items)",
        "Umbenennen widerrufen ({count} Objekte)",
    ),
    (
        "toast.undo_failed",
        "Couldn't undo the rename of “{name}”: {error}",
        "Das Umbenennen von „{name}“ konnte nicht widerrufen werden: {error}",
    ),
    (
        "toast.preset_saved",
        "Saved preset “{name}”",
        "Vorlage „{name}“ gesichert",
    ),
    // Rule types (Add Rule menu, card titles)
    ("rule.replace", "Find & Replace", "Suchen & Ersetzen"),
    (
        "rule.replace.hint",
        "Text or regex, with $1 groups",
        "Text oder Regex, mit $1-Gruppen",
    ),
    ("rule.insert", "Insert Text", "Text einfügen"),
    (
        "rule.insert.hint",
        "At the start, end or a position",
        "Am Anfang, am Ende oder an einer Position",
    ),
    ("rule.remove", "Remove Characters", "Zeichen entfernen"),
    (
        "rule.remove.hint",
        "First, last or a range of characters",
        "Erste, letzte oder ein Bereich von Zeichen",
    ),
    ("rule.case", "Change Case", "Groß-/Kleinschreibung"),
    (
        "rule.case.hint",
        "lower, UPPER, Title, Sentence",
        "klein, GROSS, Titel, Satz",
    ),
    ("rule.number", "Numbering", "Nummerierung"),
    (
        "rule.number.hint",
        "1, 2, 3… with padding",
        "1, 2, 3 … mit führenden Nullen",
    ),
    ("rule.date", "Date", "Datum"),
    (
        "rule.date.hint",
        "Modified date or today",
        "Änderungsdatum oder heute",
    ),
    ("rule.extension", "Extension", "Suffix"),
    (
        "rule.extension.hint",
        "Change, lower/upper or remove",
        "Ändern, klein/groß oder entfernen",
    ),
    ("rule.clean", "Clean Up", "Aufräumen"),
    (
        "rule.clean.hint",
        "Whitespace, dashes, umlauts",
        "Leerraum, Striche, Umlaute",
    ),
    // Rule summaries (collapsed cards)
    (
        "summary.replace",
        "“{find}” → “{replace}”",
        "„{find}“ → „{replace}“",
    ),
    (
        "summary.nothing_to_find",
        "Nothing to find yet",
        "Noch kein Suchtext",
    ),
    ("summary.insert", "Insert “{text}”", "„{text}“ einfügen"),
    (
        "summary.remove.one",
        "Remove {count} character",
        "{count} Zeichen entfernen",
    ),
    (
        "summary.remove.other",
        "Remove {count} characters",
        "{count} Zeichen entfernen",
    ),
    ("summary.case", "{mode} case", "Schreibung: {mode}"),
    (
        "summary.number",
        "From {start}, step {step}",
        "Ab {start}, Schritt {step}",
    ),
    ("summary.extension", "Extension: {mode}", "Suffix: {mode}"),
    ("summary.clean", "Clean up", "Aufräumen"),
    // Preview issues
    ("issue.empty", "Empty name", "Leerer Name"),
    ("issue.contains", "Contains “{char}”", "Enthält „{char}“"),
    (
        "issue.invalid_char",
        "Invalid character",
        "Ungültiges Zeichen",
    ),
    ("issue.too_long", "Name too long", "Name zu lang"),
    ("issue.duplicate", "Duplicate name", "Doppelter Name"),
    ("issue.exists", "Already exists", "Existiert bereits"),
    // Sheet header
    (
        "scope.selected.one",
        "{count} selected item",
        "{count} ausgewähltes Objekt",
    ),
    (
        "scope.selected.other",
        "{count} selected items",
        "{count} ausgewählte Objekte",
    ),
    ("scope.in", "in “{name}”", "in „{name}“"),
    (
        "scope.nothing_selected",
        "Nothing selected",
        "Keine Auswahl",
    ),
    (
        "scope.all_files.one",
        "renaming the only file in “{name}”",
        "die einzige Datei in „{name}“ wird umbenannt",
    ),
    (
        "scope.all_files.other",
        "renaming all {count} files in “{name}”",
        "alle {count} Dateien in „{name}“ werden umbenannt",
    ),
    ("scope.remote", "on {remote}", "auf {remote}"),
    ("presets.button", "Presets", "Vorlagen"),
    (
        "presets.button.tip",
        "Save, load and delete rule sets",
        "Regelsätze sichern, laden und löschen",
    ),
    ("presets.saved", "SAVED RULE SETS", "GESICHERTE REGELSÄTZE"),
    ("presets.none", "No presets yet", "Noch keine Vorlagen"),
    ("presets.rules.one", "{count} rule", "{count} Regel"),
    ("presets.rules.other", "{count} rules", "{count} Regeln"),
    ("presets.delete", "Delete preset", "Vorlage löschen"),
    ("presets.load", "Load these rules", "Diese Regeln laden"),
    (
        "presets.save_current",
        "SAVE CURRENT RULES",
        "AKTUELLE REGELN SICHERN",
    ),
    ("presets.name_hint", "Preset name", "Name der Vorlage"),
    ("presets.save", "Save", "Sichern"),
    (
        "presets.replaces",
        "Replaces the preset with this name",
        "Ersetzt die Vorlage mit diesem Namen",
    ),
    // Rules column
    ("rules.title", "Rules", "Regeln"),
    (
        "rules.order",
        "applied top to bottom · {n}",
        "von oben nach unten · {n}",
    ),
    ("rules.add", "Add Rule", "Regel hinzufügen"),
    (
        "rules.add.tip",
        "Add a rule at the end of the list",
        "Eine Regel am Ende der Liste hinzufügen",
    ),
    ("rules.empty", "No rules yet", "Noch keine Regeln"),
    (
        "rules.empty.note",
        "Use “Add Rule” to build the new names",
        "Mit „Regel hinzufügen“ baust du die neuen Namen",
    ),
    ("card.remove", "Remove rule", "Regel entfernen"),
    ("card.move_down", "Move down", "Nach unten"),
    ("card.move_up", "Move up", "Nach oben"),
    ("card.on", "Rule is on", "Regel ist an"),
    ("card.off", "Rule is off", "Regel ist aus"),
    ("card.off_summary", "Off · {summary}", "Aus · {summary}"),
    // Rule fields
    ("field.apply_to", "Apply to", "Gilt für"),
    ("target.name", "Name", "Name"),
    ("target.extension", "Extension", "Suffix"),
    ("target.both", "Both", "Beides"),
    ("field.find", "Find", "Suchen"),
    ("hint.regex", "Regular expression", "Regulärer Ausdruck"),
    ("hint.text", "Text", "Text"),
    ("field.replace", "Replace", "Ersetzen"),
    (
        "hint.replacement_regex",
        "Replacement, $1 for groups",
        "Ersatz, $1 für Gruppen",
    ),
    ("hint.replacement", "Replacement", "Ersatz"),
    ("opt.regex", "Regex", "Regex"),
    ("opt.match_case", "Match case", "Groß/klein beachten"),
    ("occurrence.first", "First", "Erstes"),
    ("occurrence.all", "All", "Alle"),
    ("field.text", "Text", "Text"),
    (
        "hint.text_to_insert",
        "Text to insert",
        "Einzufügender Text",
    ),
    ("field.at", "At", "Stelle"),
    ("at.start", "Start", "Anfang"),
    ("at.end", "End", "Ende"),
    ("at.position", "Position", "Position"),
    ("field.remove", "Remove", "Entfernen"),
    ("remove.first", "First", "Erste"),
    ("remove.last", "Last", "Letzte"),
    ("remove.range", "Range", "Bereich"),
    ("field.count", "Count", "Anzahl"),
    ("caption.characters", "characters", "Zeichen"),
    ("caption.from", "from", "ab"),
    ("field.case", "Case", "Schreibung"),
    ("case.lower", "lower", "klein"),
    ("case.upper", "UPPER", "GROSS"),
    ("case.title", "Title", "Titel"),
    ("case.sentence", "Sentence", "Satz"),
    ("field.start", "Start", "Beginn"),
    ("caption.step", "Step", "Schritt"),
    ("caption.digits", "Digits", "Stellen"),
    ("field.place", "Place", "Position"),
    ("place.prefix", "Prefix", "Davor"),
    ("place.suffix", "Suffix", "Dahinter"),
    ("place.replace", "Replace name", "Name ersetzen"),
    ("field.separator", "Separator", "Trenner"),
    ("hint.none", "none", "keiner"),
    ("field.date", "Date", "Datum"),
    ("date.modified", "Modified", "Geändert"),
    ("date.today", "Today", "Heute"),
    ("field.format", "Format", "Format"),
    ("date.presets", "Presets", "Vorlagen"),
    ("field.extension", "Extension", "Suffix"),
    ("ext.keep", "Keep", "Behalten"),
    ("ext.change", "Change", "Ändern"),
    ("ext.remove", "Remove", "Entfernen"),
    ("field.new", "New", "Neu"),
    ("hint.new_ext", "e.g. jpg", "z. B. jpg"),
    ("clean.trim", "Trim", "Trimmen"),
    (
        "clean.collapse",
        "Collapse spaces",
        "Leerzeichen zusammenfassen",
    ),
    ("field.spaces", "Spaces", "Leerzeichen"),
    ("spaces.keep", "Keep", "Behalten"),
    (
        "clean.umlauts",
        "Transliterate umlauts (ä → ae, é → e)",
        "Umlaute umschreiben (ä → ae, é → e)",
    ),
    // Preview
    ("preview.title", "Preview", "Vorschau"),
    (
        "preview.count.one",
        "{count} item in pane order",
        "{count} Objekt in Listenreihenfolge",
    ),
    (
        "preview.count.other",
        "{count} items in pane order",
        "{count} Objekte in Listenreihenfolge",
    ),
    ("preview.original", "Original name", "Bisheriger Name"),
    ("preview.new", "New name", "Neuer Name"),
    (
        "preview.reading",
        "Reading the folder…",
        "Ordner wird gelesen …",
    ),
    (
        "preview.error",
        "Couldn't read the folder: {error}",
        "Der Ordner konnte nicht gelesen werden: {error}",
    ),
    (
        "preview.no_files",
        "No files to rename",
        "Keine Dateien zum Umbenennen",
    ),
    ("preview.empty_name", "(empty)", "(leer)"),
    // Footer
    ("footer.rule_error", "Rule {n}: ", "Regel {n}: "),
    (
        "footer.no_change",
        "No names change yet",
        "Noch ändert sich kein Name",
    ),
    (
        "footer.files.one",
        "{count} file will be renamed",
        "{count} Datei wird umbenannt",
    ),
    (
        "footer.files.other",
        "{count} files will be renamed",
        "{count} Dateien werden umbenannt",
    ),
    (
        "footer.items.one",
        "{count} item will be renamed",
        "{count} Objekt wird umbenannt",
    ),
    (
        "footer.items.other",
        "{count} items will be renamed",
        "{count} Objekte werden umbenannt",
    ),
    (
        "footer.conflicts.one",
        "{count} conflict",
        "{count} Konflikt",
    ),
    (
        "footer.conflicts.other",
        "{count} conflicts",
        "{count} Konflikte",
    ),
    ("footer.unchanged", "{n} unchanged", "{n} unverändert"),
    ("btn.rename", "Rename", "Umbenennen"),
    ("btn.renaming", "Renaming…", "Wird umbenannt …"),
    ("btn.rename.tip", "Rename (↩)", "Umbenennen (↩)"),
    (
        "btn.resolve_first",
        "Resolve the conflicts first",
        "Löse zuerst die Konflikte",
    ),
    ("btn.cancel", "Cancel", "Abbrechen"),
    ("btn.cancel.tip", "Close (Esc)", "Schließen (Esc)"),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::CATALOG;

    /// English words German uses as they are.
    const SAME: &[&str] = &["Regex", "Text", "Position", "Format"];

    fn src() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(&CATALOG, SAME);
        assert!(p.is_empty(), "{}", p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let m = check::missing_keys(&CATALOG, &src(), 100);
        assert!(m.is_empty(), "{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let calls = [
            "row(ui, \"",
            "style::checkbox(ui, ",
            "style::caption(ui, \"",
            "style::text_field(ui, ",
            "style::button(ui, \"",
            "style::button_small(ui, \"",
            "icon_button(ui, ",
            "centered(ui, body, \"",
            "job.append(\"",
            "menu_section(ui, \"",
            "switch(ui, ",
            "text: format!(\"",
            "(Target::",
            "(Place::",
            "(ExtMode::",
            "(CaseMode::",
            "(RemoveMode::",
            "(InsertAt::",
            "(DateSource::",
            "=> \"",
        ];
        // Format tokens and file names aren't UI words.
        let allow = [
            "\"YYYY-MM-DD\"",
            "PRESETS_FILE",
            "LAST_FILE",
            "Some(\"",
            "('/') =>",
            "('·') =>",
        ];
        let hits = check::hard_coded_strings(&src(), &calls, &allow);
        assert!(
            hits.is_empty(),
            "{} hard-coded UI strings (use tr!):\n{}",
            hits.len(),
            hits.join("\n")
        );
    }

    #[test]
    fn german_strings() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("cmd.batch"), "Stapelweise umbenennen …");
            assert_eq!(tr!("footer.files", count = 3), "3 Dateien werden umbenannt");
            assert_eq!(tr!("toast.renamed", count = 1), "1 Objekt umbenannt");
        });
        with_lang(Lang::En, || {
            assert_eq!(tr!("toast.renamed", count = 2), "Renamed 2 items")
        });
    }
}
