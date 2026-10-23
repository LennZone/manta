//! English and German strings of the Image Tools plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`. Carried over 1:1
//! from the former Python plugin's `it_i18n.py`.

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
    ("plugin.name", "Image Tools", "Bildwerkzeuge"),
    ("plugin.description", "Image details with preview and EXIF, \"lo\"/\"XL\" badges, and resize, convert, optimize-for-web, @2x sets, metadata stripping and data URIs for the selection.", "Bilddetails mit Vorschau und EXIF, „lo“/„XL“-Badges sowie Skalieren, Konvertieren, Fürs Web optimieren, @2x-Sets, Metadaten entfernen und Data-URIs für die Auswahl."),
    ("retina.exists", "“{name}” already exists", "„{name}“ gibt es schon"),
    ("preview.failed", "Can't preview this image", "Für dieses Bild gibt es keine Vorschau"),
    ("cmd.panel", "Show Image Panel", "Bild-Panel einblenden"),
    ("cmd.resize", "Resize…", "Größe ändern …"),
    ("cmd.convert", "Convert to…", "Konvertieren in …"),
    ("cmd.rotate_left", "Rotate 90° Left", "90° nach links drehen"),
    ("cmd.rotate_right", "Rotate 90° Right", "90° nach rechts drehen"),
    ("cmd.rotate_180", "Rotate 180°", "180° drehen"),
    ("cmd.flip_h", "Flip Horizontal", "Horizontal spiegeln"),
    ("cmd.flip_v", "Flip Vertical", "Vertikal spiegeln"),
    ("cmd.strip", "Strip Metadata…", "Metadaten entfernen …"),
    ("cmd.optimize", "Optimize for Web…", "Fürs Web optimieren …"),
    ("cmd.retina", "Make @2x/@1x Set…", "@2x/@1x-Set erstellen …"),
    ("cmd.datauri", "Copy as Data URI", "Als Data-URI kopieren"),
    ("panel.image", "Image", "Bild"),
    ("panel.tools", "Image Tools", "Bildwerkzeuge"),
    ("op.resize", "Resize", "Größe ändern"),
    ("op.convert", "Convert", "Konvertieren"),
    ("op.rotate", "Rotate & Flip", "Drehen & Spiegeln"),
    ("op.strip", "Strip Metadata", "Metadaten entfernen"),
    ("op.optimize", "Optimize for Web", "Fürs Web optimieren"),
    ("op.retina", "Make @2x/@1x Set", "@2x/@1x-Set erstellen"),
    ("ing.resize", "Resizing", "Größe ändern:"),
    ("ing.convert", "Converting", "Konvertieren:"),
    ("ing.rotate", "Rotating", "Drehen:"),
    ("ing.strip", "Stripping", "Metadaten entfernen:"),
    ("ing.optimize", "Optimizing", "Optimieren:"),
    ("ing.retina", "Making", "Erstellen:"),
    ("done.resize", "Resized {what}", "Größe von {what} geändert"),
    ("done.convert", "Converted {what}", "{what} konvertiert"),
    ("done.rotate", "Rotated {what}", "{what} gedreht"),
    ("done.strip", "Stripped {what}", "Metadaten von {what} entfernt"),
    ("done.optimize", "Optimized {what}", "{what} optimiert"),
    ("done.retina", "Made {what}", "{what} erstellt"),
    ("quality.60", "Smaller", "Kleiner"),
    ("quality.82", "Balanced", "Ausgewogen"),
    ("quality.90", "High", "Hoch"),
    ("quality.100", "Maximum", "Maximal"),
    ("images.one", "{count} image", "{count} Bild"),
    ("images.other", "{count} images", "{count} Bilder"),
    ("sets.one", "{count} @2x/@1x set", "{count} @2x/@1x-Set"),
    ("sets.other", "{count} @2x/@1x sets", "{count} @2x/@1x-Sets"),
    ("originals.one", "{count} original", "{count} Original"),
    ("originals.other", "{count} originals", "{count} Originale"),
    ("frames.one", "{count} frame", "{count} Frame"),
    ("frames.other", "{count} frames", "{count} Frames"),
    ("bytes.one", "{count} byte", "{count} Byte"),
    ("bytes.other", "{count} bytes", "{count} Byte"),
    ("make_sets.one", "Make {count} Set", "{count} Set erstellen"),
    ("make_sets.other", "Make {count} Sets", "{count} Sets erstellen"),
    ("tip.animated", "animated", "animiert"),
    ("tip.xl", "Large for the web (over {size}): Optimize for Web shrinks it", "Groß fürs Web (über {size}): „Fürs Web optimieren“ verkleinert es"),
    ("tip.lo", "Low resolution: longest edge under {px} px", "Niedrige Auflösung: längste Kante unter {px} px"),
    ("caption.animated", "Animated: the preview shows the first frame", "Animiert: Die Vorschau zeigt das erste Frame"),
    ("tag.large", "Large for the web", "Groß fürs Web"),
    ("tag.low", "Low resolution", "Niedrige Auflösung"),
    ("tag.animated", "Animated", "Animiert"),
    ("tag.animated_frames", "Animated · {frames}", "Animiert · {frames}"),
    ("tag.alpha", "Transparency", "Transparenz"),
    ("tag.progressive", "Progressive", "Progressiv"),
    ("tag.interlaced", "Interlaced", "Interlaced (Zeilensprung)"),
    ("tag.gps", "GPS location", "GPS-Standort"),
    ("key.dimensions", "Dimensions", "Abmessungen"),
    ("key.aspect", "Aspect ratio", "Seitenverhältnis"),
    ("key.file_size", "File size", "Dateigröße"),
    ("key.format", "Format", "Format"),
    ("key.profile", "Colour profile", "Farbprofil"),
    ("key.dpi", "DPI", "DPI"),
    ("key.alpha", "Alpha", "Alphakanal"),
    ("key.animated", "Animated", "Animiert"),
    ("key.total_size", "Total size", "Gesamtgröße"),
    ("key.file_sizes", "File sizes", "Dateigrößen"),
    ("key.formats", "Formats", "Formate"),
    ("key.over_2mb", "Over 2 MB", "Über 2 MB"),
    ("key.coordinates", "Coordinates", "Koordinaten"),
    ("key.width", "Width", "Breite"),
    ("key.height", "Height", "Höhe"),
    ("key.size", "Size", "Größe"),
    ("key.colours", "Colours", "Farben"),
    ("val.rotated", "{dims} (rotated by EXIF)", "{dims} (per EXIF gedreht)"),
    ("val.bytes", "{size} ({bytes} bytes)", "{size} ({bytes} Byte)"),
    ("val.no_profile", "None (browsers assume sRGB)", "Keins (Browser nehmen sRGB an)"),
    ("val.yes", "Yes", "Ja"),
    ("val.no", "No", "Nein"),
    ("val.yes_frames", "Yes · {frames}", "Ja · {frames}"),
    ("val.unreadable", "{format} (size unreadable)", "{format} (Größe nicht lesbar)"),
    ("section.camera", "Camera", "Kamera"),
    ("section.location", "Location", "Ort"),
    ("section.actions", "Actions", "Aktionen"),
    ("section.largest", "Largest", "Die größten"),
    ("section.working", "Working", "In Arbeit"),
    ("working.detail", "{done} of {total}", "{done} von {total}"),
    ("working.line", "{verb} {name} …", "{verb} {name} …"),
    ("gps.warning", "This photo tells everyone where it was taken. Strip the metadata before you publish it.", "Dieses Foto verrät allen, wo es aufgenommen wurde. Entferne die Metadaten, bevor du es veröffentlichst."),
    ("btn.open_maps", "Open in Maps", "In Karten öffnen"),
    ("btn.cancel", "Cancel", "Abbrechen"),
    ("btn.back", "Back", "Zurück"),
    ("exif.camera", "Camera", "Kamera"),
    ("exif.lens", "Lens", "Objektiv"),
    ("exif.exposure", "Exposure", "Belichtung"),
    ("exif.focal", "Focal length", "Brennweite"),
    ("exif.equiv", "{fl} ({mm} mm equiv.)", "{fl} (entspr. {mm} mm)"),
    ("exif.taken", "Taken", "Aufgenommen"),
    ("exif.software", "Software", "Software"),
    ("gps.format", "{lat}°{ns}, {lon}°{ew}", "{lat}°{ns}, {lon}°{ew}"),
    ("gps.n", "N", "N"),
    ("gps.s", "S", "S"),
    ("gps.e", "E", "O"),
    ("gps.w", "W", "W"),
    ("date.exif", "{day} {month} {year}, {time}", "{day}. {month} {year}, {time}"),
    ("state.waiting", "Waiting for Manta…", "Warte auf Manta …"),
    ("state.remote", "Remote folder", "Entfernter Ordner"),
    ("state.remote_note", "Image Tools works on local folders. Copy the images here first.", "Die Bildwerkzeuge funktionieren mit lokalen Ordnern. Kopiere die Bilder zuerst hierher."),
    ("state.none", "No image selected", "Kein Bild ausgewählt"),
    ("state.none_note", "Select a PNG, JPEG, GIF, WebP, HEIC or TIFF file to see its size, colour profile and EXIF data.", "Wähle eine PNG-, JPEG-, GIF-, WebP-, HEIC- oder TIFF-Datei aus, um Größe, Farbprofil und EXIF-Daten zu sehen."),
    ("state.cant_read", "Can't read {name}", "{name} kann nicht gelesen werden"),
    ("state.error", "Image Tools: {error}", "Bildwerkzeuge: {error}"),
    ("modal.heading_one", "{op} “{name}”", "{op}: „{name}“"),
    ("modal.heading_many", "{op} {images}", "{op}: {images}"),
    ("confirm.replace", "Replace {what}? The originals can't be restored afterwards.", "{what} ersetzen? Die Originale können danach nicht wiederhergestellt werden."),
    ("confirm.replace_one", "Replace {what}? The original can't be restored afterwards.", "{what} ersetzen? Das Original kann danach nicht wiederhergestellt werden."),
    ("confirm.overwrite", "Overwrite {what}? The originals can't be restored afterwards.", "{what} überschreiben? Die Originale können danach nicht wiederhergestellt werden."),
    ("confirm.overwrite_one", "Overwrite {what}? The original can't be restored afterwards.", "{what} überschreiben? Das Original kann danach nicht wiederhergestellt werden."),
    ("confirm.btn_replace", "Replace {what}", "{what} ersetzen"),
    ("confirm.btn_overwrite", "Overwrite {what}", "{what} überschreiben"),
    ("resize.size", "Size", "Größe"),
    ("resize.keeps", "keeps the aspect ratio", "behält das Seitenverhältnis"),
    ("resize.smaller", "already smaller", "schon kleiner"),
    ("resize.edge", "longest edge", "längste Kante"),
    ("resize.custom", "Custom…", "Eigene …"),
    ("resize.width_px", "width {px} px", "Breite {px} px"),
    ("resize.height_px", "height {px} px", "Höhe {px} px"),
    ("resize.set_height", "Set height instead", "Stattdessen Höhe festlegen"),
    ("resize.set_width", "Set width instead", "Stattdessen Breite festlegen"),
    ("convert.format", "Format", "Format"),
    ("fmt.jpeg", "photos, no transparency", "Fotos, keine Transparenz"),
    ("fmt.png", "lossless, transparency", "verlustfrei, Transparenz"),
    ("fmt.heic", "small, Apple devices", "klein, Apple-Geräte"),
    ("fmt.tiff", "print, lossless", "Druck, verlustfrei"),
    ("fmt.webp", "web, via cwebp", "Web, mit cwebp"),
    ("convert.webp_needs", "WebP needs cwebp: brew install webp", "WebP braucht cwebp: brew install webp"),
    ("convert.quality", "Quality", "Qualität"),
    ("rotate.transform", "Transform", "Transformation"),
    ("strip.what", "What it does", "Was passiert"),
    ("strip.builtin", "built in, lossless (JPEG, PNG, WebP)", "integriert, verlustfrei (JPEG, PNG, WebP)"),
    ("strip.note", "Removes EXIF (camera, GPS location, dates), XMP, IPTC and comments. Pixels are untouched; the colour profile and orientation are kept.", "Entfernt EXIF (Kamera, GPS-Standort, Datumsangaben), XMP, IPTC und Kommentare. Die Pixel bleiben unverändert; Farbprofil und Ausrichtung bleiben erhalten."),
    ("strip.heic_note", "HEIC and TIFF need exiftool (brew install exiftool). sips can't do it: it copies the EXIF data, GPS included, into every file it writes.", "HEIC und TIFF brauchen exiftool (brew install exiftool). Mit sips geht es nicht: Es kopiert die EXIF-Daten samt GPS in jede Datei, die es schreibt."),
    ("opt.size", "longest edge at most {px} px (never enlarged)", "längste Kante höchstens {px} px (nie vergrößert)"),
    ("opt.format", "JPEG, quality {q} (PNG kept for transparency)", "JPEG, Qualität {q} (PNG bleibt bei Transparenz)"),
    ("opt.colours", "converted to sRGB", "in sRGB umgewandelt"),
    ("retina.files", "Files", "Dateien"),
    ("retina.original", "original", "Original"),
    ("retina.original_renamed", "original, renamed", "Original, umbenannt"),
    ("retina.half", "half size", "halbe Größe"),
    ("retina.more", "and {images} more", "und {images} mehr"),
    ("save.as", "Save as", "Sichern als"),
    ("save.next_to", "Next to the original", "Neben dem Original"),
    ("save.subfolder", "In a subfolder", "In einem Unterordner"),
    ("save.replace", "Replace originals", "Originale ersetzen"),
    ("save.overwrite", "Overwrite originals", "Originale überschreiben"),
    ("save.no_undo", "can't be undone", "nicht widerrufbar"),
    ("save.trash", "originals go to the Trash", "Originale kommen in den Papierkorb"),
    ("save.asks", "Asks before it overwrites anything", "Fragt nach, bevor etwas überschrieben wird"),
    ("run.resize", "Resize", "Größe ändern"),
    ("run.convert", "Convert", "Konvertieren"),
    ("run.rotate", "Apply", "Anwenden"),
    ("run.strip", "Strip Metadata", "Metadaten entfernen"),
    ("run.optimize", "Optimize", "Optimieren"),
    ("run.n", "{verb} {n}", "{verb} ({n})"),
    ("job.progress", "{verb} {images} …", "{verb} {images} …"),
    ("job.cancelled", "Cancelled after {what}", "Abgebrochen nach {what}"),
    ("job.failed", "{op} failed: {name}: {error}", "{op} fehlgeschlagen: {name}: {error}"),
    ("job.sizes", " · {before} → {after}", " · {before} → {after}"),
    ("job.some_failed", "{images} failed ({name}: {error})", "{images} fehlgeschlagen ({name}: {error})"),
    ("job.skipped", "{images} skipped ({why})", "{images} übersprungen ({why})"),
    ("job.trashed", "{originals} moved to the Trash", "{originals} in den Papierkorb gelegt"),
    ("skip.unreadable", "unreadable size", "Größe nicht lesbar"),
    ("skip.small", "already small enough", "schon klein genug"),
    ("err.read_size", "can't read the size", "Größe nicht lesbar"),
    ("toast.local_only", "Image Tools works on local folders only", "Die Bildwerkzeuge funktionieren nur mit lokalen Ordnern"),
    ("toast.select_first", "Select one or more images first", "Wähle zuerst ein oder mehrere Bilder aus"),
    ("toast.needs_sips", "Image Tools needs sips (part of macOS)", "Die Bildwerkzeuge brauchen sips (Teil von macOS)"),
    ("toast.busy", "Image Tools is still working on the last job", "Die Bildwerkzeuge arbeiten noch am letzten Auftrag"),
    ("toast.select_one", "Select one image to copy it as a data URI", "Wähle ein Bild aus, um es als Data-URI zu kopieren"),
    ("toast.cant_copy", "Can't copy as a data URI: {why}", "Kopieren als Data-URI nicht möglich: {why}"),
    ("toast.cant_read", "Couldn't read {name}: {error}", "{name} konnte nicht gelesen werden: {error}"),
    ("toast.pbcopy", "pbcopy failed: {error}", "pbcopy fehlgeschlagen: {error}"),
    ("toast.copied", "Copied “{name}” as a data URI ({size})", "„{name}“ als Data-URI kopiert ({size})"),
    ("toast.copied_big", "Over 10 KB: a separate file usually loads faster", "Über 10 KB: Eine eigene Datei lädt meist schneller"),
    ("toast.no_gps", "This image has no GPS location", "Dieses Bild hat keinen GPS-Standort"),
    ("uri.unreadable", "can't read the file", "Die Datei kann nicht gelesen werden"),
    ("uri.too_big", "{size} is too big for a data URI (max {max})", "{size} ist zu groß für eine Data-URI (max. {max})"),
    ("uri.format", "browsers can't show this format", "Browser können dieses Format nicht anzeigen"),
    ("strip.needs_exiftool", "Stripping {format} files needs exiftool (brew install exiftool)", "Zum Entfernen bei {format}-Dateien wird exiftool gebraucht (brew install exiftool)"),
    ("webp.needs_cwebp", "WebP needs cwebp (brew install webp)", "WebP braucht cwebp (brew install webp)"),
];

/// Abbreviated month name for 1..12 ("Sep" / "Sept." / "sept." / "9月"),
/// see `manta_plugin::i18n::month_abbr`.
pub fn month(m: u32) -> &'static str {
    manta_plugin::i18n::month_abbr(manta_plugin::i18n::lang(), m.clamp(1, 12) - 1)
}

/// Decimal points between digits become commas in German ("8.4 MB").
pub fn dec(s: &str) -> String {
    manta_plugin::i18n::localize_decimal(s)
}

/// 12,345 / 12.345
pub fn num(n: u64) -> String {
    manta_plugin::i18n::format_count(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_plugin::i18n::{check, with_lang};
    use std::path::Path;

    /// English texts that are the same in German (names, units).
    const SAME: &[&str] = &[
        "N",
        "S",
        "W",
        "DPI",
        "Software",
        "{verb} {name} …",
        "{verb} {images} …",
        " · {before} → {after}",
        "{count} Original",
        "{count} frame",
        "{count} frames",
    ];

    #[test]
    fn catalog_is_complete_and_translated() {
        let p = check::catalog_problems(&CATALOG, SAME);
        assert!(p.is_empty(), "{p:#?}");
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing = check::missing_keys(&CATALOG, &src, 100);
        assert!(missing.is_empty(), "{missing:#?}");
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // "XL" and "lo" are badge symbols, the same in every language.
        let hits = check::hard_coded_strings(
            &src,
            &[],
            &["Badge { text:", "text: \"XL\"", "text: \"lo\""],
        );
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
    fn helpers() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("images", count = 1234), "1.234 Bilder");
            assert_eq!(dec("8.4 MB · f/1.8"), "8,4 MB · f/1,8");
            assert_eq!(month(9), "Sept.");
        });
        with_lang(Lang::En, || assert_eq!(tr!("images", count = 1), "1 image"));
    }
}
