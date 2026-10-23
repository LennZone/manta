//! English and German strings of the Markdown Preview plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`. The previewed
//! document itself is never translated.

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

/// Messages made on the loader thread keep their catalog key, so they show
/// in the language of the moment they're drawn (see [`show_msg`]).
pub fn msg_key(key: &'static str) -> String {
    debug_assert!(CATALOG.has(key), "{key}");
    key.to_string()
}

/// A message from [`msg_key`] in the active language; other text (an OS
/// error) as it is.
pub fn show_msg(msg: &str) -> &str {
    match CATALOG.tr_key(msg) {
        "" => msg,
        t => t,
    }
}

const ENTRIES: Entries = &[
    // Plugin
    ("plugin.name", "Markdown Preview", "Markdown-Vorschau"),
    ("plugin.description", "A live, nicely typeset preview of the selected Markdown file or the folder's README.", "Eine schön gesetzte Live-Vorschau der ausgewählten Markdown-Datei oder der README des Ordners."),
    ("cmd.toggle", "Toggle Markdown Preview", "Markdown-Vorschau ein-/ausblenden"),
    ("panel.title", "Markdown", "Markdown"),
    ("toast.missing", "“{name}” doesn't exist", "„{name}“ existiert nicht"),
    // Settings
    ("settings.web_images", "Load images from the web (http/https)", "Bilder aus dem Internet laden (http/https)"),
    ("settings.web_images.tip", "Off by default for privacy: loading a remote image tells its server that you opened the file. Local images always show.", "Zum Schutz deiner Privatsphäre standardmäßig aus: Beim Laden eines Bilds aus dem Internet erfährt dessen Server, dass du die Datei geöffnet hast. Lokale Bilder werden immer angezeigt."),
    // Header
    ("back", "Back", "Zurück"),
    ("reveal", "Reveal in Pane", "Im Bereich zeigen"),
    ("edit", "Open in Editor", "Im Editor öffnen"),
    ("edit.local_only", "Open in Editor — local files only", "Im Editor öffnen – nur lokale Dateien"),
    ("contents", "Contents", "Inhalt"),
    ("contents.caption", "CONTENTS", "INHALT"),
    ("readme.tip", "Nothing is selected, so the folder's README is shown", "Nichts ist ausgewählt, daher wird die README des Ordners angezeigt"),
    // Empty and error states
    ("empty.title", "Select a Markdown file", "Wähle eine Markdown-Datei aus"),
    ("empty.body", "Select a .md, .markdown or .mdx file in the active pane.", "Wähle im aktiven Bereich eine Datei mit der Endung .md, .markdown oder .mdx aus."),
    ("empty.body.readme", "Select a .md, .markdown or .mdx file in the active pane. A folder's README shows here when nothing is selected.", "Wähle im aktiven Bereich eine Datei mit der Endung .md, .markdown oder .mdx aus. Ist nichts ausgewählt, erscheint hier die README des Ordners."),
    ("empty.not_markdown", "“{name}” isn't a Markdown file. Select a .md, .markdown or .mdx file in the active pane.", "„{name}“ ist keine Markdown-Datei. Wähle im aktiven Bereich eine Datei mit der Endung .md, .markdown oder .mdx aus."),
    ("error.cant_read", "Can't read “{name}”", "„{name}“ kann nicht gelesen werden"),
    ("error.too_large.title", "“{name}” is too large", "„{name}“ ist zu groß"),
    ("error.too_large.body", "It's {size}. The preview shows files up to {max}.", "Die Datei ist {size} groß. Die Vorschau zeigt Dateien bis {max}."),
    ("error.too_large.button", "Show First 2 MB", "Erste 2 MB zeigen"),
    ("partial", "Showing the first {shown} of {total}.", "Die ersten {shown} von {total} werden angezeigt."),
    ("error.not_text", "This isn't a text file.", "Dies ist keine Textdatei."),
    ("error.folder", "It's a folder.", "Das ist ein Ordner."),
    ("error.not_found", "The file doesn't exist (any more).", "Die Datei existiert nicht (mehr)."),
    ("error.permission", "You don't have permission to read it.", "Du hast keine Berechtigung, sie zu lesen."),
    ("error.download", "download failed", "Download fehlgeschlagen"),
    // Document chrome
    ("copy", "Copy", "Kopieren"),
    ("copied", "Copied", "Kopiert"),
    ("loading", "Loading…", "Wird geladen …"),
    ("image.unsupported", "This kind of image isn't shown", "Diese Art von Bild wird nicht angezeigt"),
    ("image.svg", "SVG images aren't shown", "SVG-Bilder werden nicht angezeigt"),
    ("image.web_off", "Remote images are off for privacy. Turn them on in Settings → Plugins → Markdown Preview.", "Bilder aus dem Internet sind zum Schutz deiner Privatsphäre aus. Du kannst sie unter „Einstellungen“ → „Plugins“ → „Markdown-Vorschau“ einschalten."),
    ("image.failed", "Couldn't load the image: {error}", "Das Bild konnte nicht geladen werden: {error}"),
    ("image.too_large", "too large", "zu groß"),
    ("alert.note", "Note", "Hinweis"),
    ("alert.tip", "Tip", "Tipp"),
    ("alert.important", "Important", "Wichtig"),
    ("alert.warning", "Warning", "Warnung"),
    ("alert.caution", "Caution", "Vorsicht"),
    // Sizes
    ("bytes.one", "{count} byte", "{count} Byte"),
    ("bytes.other", "{count} bytes", "{count} Byte"),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::*;

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(&CATALOG, &[]);
        assert!(p.is_empty(), "{}", p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let m = check::missing_keys(&CATALOG, &src, 30);
        assert!(m.is_empty(), "{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let calls = ["empty_state(", "notice(ui, col, \"", "placeholder(ui, "];
        let hits = check::hard_coded_strings(&src, &calls, &[]);
        assert!(hits.is_empty(), "{}", hits.join("\n"));
    }

    #[test]
    fn loader_messages_are_keys() {
        for k in [
            "error.not_text",
            "error.folder",
            "error.not_found",
            "error.permission",
            "error.download",
            "image.too_large",
        ] {
            assert!(CATALOG.has(k), "{k}");
        }
        with_lang(Lang::De, || {
            assert_eq!(show_msg(&msg_key("error.folder")), "Das ist ein Ordner.")
        });
        assert_eq!(show_msg("disk I/O error"), "disk I/O error");
    }

    #[test]
    fn german_titles() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("cmd.toggle"), "Markdown-Vorschau ein-/ausblenden");
            assert_eq!(crate::panel::format_size(1_500_000), "1,5 MB");
            assert_eq!(crate::panel::format_size(12), "12 Byte");
        });
    }
}
