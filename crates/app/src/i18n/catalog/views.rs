//! The pane views (Icons, List, Columns, Gallery) and their settings
//! (`views.rs`, `thumbs.rs`, `pane.rs`, `settings.rs`). German follows
//! Finder: "Symbole", "Liste", "Spalten", "Galerie".

use super::Entries;

pub const ENTRIES: Entries = &[
    ("view.icons", "Icons", "Symbole"),
    ("view.list", "List", "Liste"),
    ("view.columns", "Columns", "Spalten"),
    ("view.gallery", "Gallery", "Galerie"),
    ("view.switch_tip", "View", "Darstellung"),
    ("view.options", "View Options", "Darstellungsoptionen"),
    ("view.sort_by", "Sort By", "Sortieren nach"),
    ("view.ascending", "Ascending", "Aufsteigend"),
    ("view.descending", "Descending", "Absteigend"),
    ("view.icon_size", "Icon Size", "Symbolgröße"),
    ("view.column_empty", "No items", "Keine Objekte"),
    ("view.items.one", "{count} item", "{count} Objekt"),
    ("view.items.other", "{count} items", "{count} Objekte"),
    ("view.selected_n.one", "{count} item selected", "{count} Objekt ausgewählt"),
    ("view.selected_n.other", "{count} items selected", "{count} Objekte ausgewählt"),
    ("view.information", "Information", "Informationen"),
    ("view.info.kind", "Kind", "Art"),
    ("view.info.size", "Size", "Größe"),
    ("view.info.modified", "Modified", "Geändert"),
    ("view.info.permissions", "Permissions", "Zugriffsrechte"),
    ("view.info.original", "Original", "Originalobjekt"),
    ("cmd.view_icons", "View as Icons", "Als Symbole"),
    ("cmd.view_list", "View as List", "Als Liste"),
    ("cmd.view_columns", "View as Columns", "Als Spalten"),
    ("cmd.view_gallery", "View as Gallery", "Als Galerie"),
    ("cmd.kw.view", "view icons list columns gallery grid thumbnails miller finder", "darstellung ansicht symbole liste spalten galerie raster vorschaubilder finder"),
    ("settings.view_per_folder", "Remember view per folder", "Ansicht pro Ordner merken"),
    ("settings.view_per_folder.note", "Each folder opens in the view it was last shown in (Icons, List, Columns, Gallery).", "Jeder Ordner öffnet sich in der Darstellung, in der er zuletzt gezeigt wurde (Symbole, Liste, Spalten, Galerie)."),
    ("settings.remote_thumbnails", "Load thumbnails for server files", "Vorschaubilder für Server-Dateien laden"),
    ("settings.system_icons", "File icons from macOS", "Dateisymbole von macOS"),
    ("settings.system_icons.note", "Files show the icon macOS has for their type, apps their own, like in Finder. Off: Manta's symbols.", "Dateien zeigen das Symbol, das macOS für ihren Typ hat, Apps ihr eigenes – wie im Finder. Aus: Mantas Symbole."),
    ("settings.remote_thumbnails.note", "Downloads small images (under 5 MB) in the background. Off: server files show their type icon.", "Lädt kleine Bilder (unter 5 MB) im Hintergrund. Aus: Server-Dateien zeigen ihr Typsymbol."),
];
