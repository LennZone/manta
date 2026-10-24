//! Command titles, categories and palette keywords (`commands.rs`).
//! German keywords include the English ones so both find a command.

use super::Entries;

pub const ENTRIES: Entries = &[
    ("cmd.open", "Open", "Öffnen"),
    ("cmd.quick_look", "Quick Look", "Schnellansicht"),
    ("cmd.get_info", "Get Info", "Informationen"),
    (
        "cmd.enclosing_folder",
        "Enclosing Folder",
        "Übergeordneter Ordner",
    ),
    ("cmd.back", "Back", "Zurück"),
    ("cmd.forward", "Forward", "Vorwärts"),
    ("cmd.refresh", "Refresh", "Aktualisieren"),
    (
        "cmd.copy_to_other",
        "Copy to Other Pane…",
        "In anderen Bereich kopieren …",
    ),
    (
        "cmd.move_to_other",
        "Move to Other Pane…",
        "In anderen Bereich bewegen …",
    ),
    ("cmd.rename", "Rename…", "Umbenennen …"),
    ("cmd.new_folder", "New Folder…", "Neuer Ordner …"),
    (
        "cmd.move_to_trash",
        "Move to Trash",
        "In den Papierkorb legen",
    ),
    (
        "cmd.delete_permanently",
        "Delete Immediately…",
        "Sofort löschen …",
    ),
    ("cmd.empty_trash", "Empty Trash…", "Papierkorb entleeren …"),
    ("cmd.select_all", "Select All", "Alles auswählen"),
    (
        "cmd.toggle_hidden",
        "Show Hidden Files",
        "Unsichtbare Dateien einblenden",
    ),
    ("cmd.new_tab", "New Tab", "Neuer Tab"),
    ("cmd.close_tab", "Close Tab", "Tab schließen"),
    ("cmd.switch_pane", "Switch Pane", "Bereich wechseln"),
    ("cmd.edit_path", "Edit Path", "Pfad bearbeiten"),
    ("cmd.go_to_path", "Go to Path…", "Gehe zum Pfad …"),
    ("cmd.filter", "Filter Files", "Dateien filtern"),
    (
        "cmd.add_to_favorites",
        "Add Folder to Favorites",
        "Ordner zu Favoriten hinzufügen",
    ),
    (
        "cmd.connect",
        "Connect to Server…",
        "Mit Server verbinden …",
    ),
    (
        "cmd.transfers",
        "Show Transfers",
        "Übertragungen einblenden",
    ),
    (
        "cmd.compare_sync",
        "Compare & Sync…",
        "Vergleichen und synchronisieren …",
    ),
    ("cmd.command_palette", "Command Palette", "Befehlspalette"),
    ("cmd.settings", "Settings…", "Einstellungen …"),
    ("cmd.help", "Open Manual", "Handbuch öffnen"),
    (
        "cmd.support",
        "Manta Website & Support…",
        "Manta-Website & Support …",
    ),
    (
        "cmd.source_code",
        "Source Code on GitHub…",
        "Quellcode auf GitHub …",
    ),
    ("cmd.report_issue", "Report an Issue…", "Fehler melden …"),
    ("cmd.suggest_idea", "Suggest an Idea…", "Idee vorschlagen …"),
    (
        "cmd.third_party_licenses",
        "Third-Party Licences",
        "Lizenzen von Drittanbietern",
    ),
    ("cmd.reload_plugins", "Reload Plugins", "Plugins neu laden"),
    (
        "cmd.browse_plugins",
        "Browse Plugins…",
        "Plugins entdecken …",
    ),
    ("cmd.compress", "Compress…", "Komprimieren …"),
    ("cmd.extract_here", "Extract Here", "Hier entpacken"),
    (
        "cmd.open_with_app",
        "Open With Default App",
        "Mit Standard-App öffnen",
    ),
    (
        "cmd.open_shell_in_container",
        "Open Shell in Container",
        "Shell im Container öffnen",
    ),
    (
        "cmd.appearance_light",
        "Appearance: Light",
        "Erscheinungsbild: Hell",
    ),
    (
        "cmd.appearance_dark",
        "Appearance: Dark",
        "Erscheinungsbild: Dunkel",
    ),
    (
        "cmd.appearance_system",
        "Appearance: Auto (System)",
        "Erscheinungsbild: Automatisch (System)",
    ),
    ("cmd.shortcut.space", "Space", "Leertaste"),
    // Categories
    ("cmd.cat.navigate", "Navigate", "Navigieren"),
    ("cmd.cat.file", "File", "Ablage"),
    ("cmd.cat.select", "Select", "Auswahl"),
    ("cmd.cat.view", "View", "Darstellung"),
    ("cmd.cat.go", "Go", "Gehe zu"),
    ("cmd.cat.tools", "Tools", "Werkzeuge"),
    // Palette keywords
    ("cmd.kw.open", "enter launch", "enter launch starten"),
    (
        "cmd.kw.get_info",
        "info properties details permissions size tags",
        "info eigenschaften details zugriffsrechte größe tags",
    ),
    (
        "cmd.kw.quick_look",
        "preview view",
        "preview view vorschau übersicht ansehen",
    ),
    (
        "cmd.kw.enclosing_folder",
        "parent up",
        "parent up hoch oben eltern",
    ),
    (
        "cmd.kw.back",
        "history previous",
        "history previous verlauf vorherige",
    ),
    (
        "cmd.kw.forward",
        "history next",
        "history next verlauf nächste",
    ),
    ("cmd.kw.refresh", "reload", "reload neu laden"),
    (
        "cmd.kw.copy_to_other",
        "duplicate transfer f5",
        "duplicate transfer f5 duplizieren übertragen",
    ),
    (
        "cmd.kw.move_to_other",
        "transfer cut f6",
        "transfer cut f6 verschieben ausschneiden",
    ),
    ("cmd.kw.rename", "name f2", "name f2 namen"),
    (
        "cmd.kw.new_folder",
        "create directory mkdir",
        "create directory mkdir erstellen verzeichnis anlegen",
    ),
    (
        "cmd.kw.move_to_trash",
        "delete remove trash bin rm",
        "delete remove trash bin rm löschen entfernen papierkorb",
    ),
    (
        "cmd.kw.empty_trash",
        "trash bin clear erase empty",
        "trash bin clear erase empty papierkorb leeren entleeren",
    ),
    (
        "cmd.kw.delete_permanently",
        "remove erase rm permanent forever",
        "remove erase rm permanent forever entfernen endgültig dauerhaft",
    ),
    ("cmd.kw.select_all", "all", "all alle"),
    (
        "cmd.kw.toggle_hidden",
        "dotfiles invisible",
        "dotfiles invisible versteckt ausblenden",
    ),
    ("cmd.kw.new_tab", "tab", "tab"),
    ("cmd.kw.close_tab", "tab", "tab"),
    (
        "cmd.kw.switch_pane",
        "focus other pane",
        "focus other pane fokus andere seite",
    ),
    (
        "cmd.kw.edit_path",
        "path location breadcrumb",
        "path location breadcrumb ort pfadleiste",
    ),
    (
        "cmd.kw.go_to_path",
        "folder path location open jump cd tab",
        "folder path location open jump cd tab ordner ort springen",
    ),
    ("cmd.kw.filter", "search find", "search find suchen finden"),
    (
        "cmd.kw.add_to_favorites",
        "bookmark star sidebar",
        "bookmark star sidebar lesezeichen stern seitenleiste",
    ),
    (
        "cmd.kw.connect",
        "sftp ftp ftps ssh remote server",
        "sftp ftp ftps ssh remote server verbinden entfernt",
    ),
    (
        "cmd.kw.transfers",
        "queue progress jobs",
        "queue progress jobs warteschlange fortschritt aufträge",
    ),
    (
        "cmd.kw.compare_sync",
        "diff synchronize mirror",
        "diff synchronize mirror spiegeln abgleichen",
    ),
    ("cmd.kw.command_palette", "commands", "commands befehle"),
    (
        "cmd.kw.support",
        "support help website homepage contact feedback question",
        "support help website contact hilfe webseite homepage kontakt feedback frage",
    ),
    (
        "cmd.kw.source_code",
        "source code github repository repo open source mit licence license contribute develop",
        "source code github quellcode repository repo open source mit lizenz mitwirken entwickeln",
    ),
    (
        "cmd.kw.report_issue",
        "bug issue problem feedback report crash error github",
        "bug issue problem feedback report crash error github fehler melden absturz problem rückmeldung",
    ),
    (
        "cmd.kw.suggest_idea",
        "idea feature request suggestion feedback wish improvement github",
        "idea feature request suggestion feedback idee vorschlag wunsch verbesserung funktion rückmeldung",
    ),
    (
        "cmd.kw.third_party_licenses",
        "licenses licences open source credits acknowledgements notices copyright legal",
        "licenses open source lizenzen drittanbieter danksagung hinweise urheberrecht rechtliches",
    ),
    (
        "cmd.kw.reload_plugins",
        "plugins reload restart rescan develop extensions",
        "plugins neu laden neustarten erweiterungen entwickeln",
    ),
    (
        "cmd.kw.browse_plugins",
        "plugins directory extensions community download find website",
        "plugins verzeichnis erweiterungen community herunterladen finden website",
    ),
    (
        "cmd.kw.help",
        "help manual handbook guide documentation docs faq",
        "help manual handbook guide documentation docs faq hilfe handbuch anleitung dokumentation",
    ),
    (
        "cmd.kw.settings",
        "preferences options plugins extensions",
        "preferences options plugins extensions optionen erweiterungen",
    ),
    (
        "cmd.kw.compress",
        "zip tar archive pack gzip xz zstd",
        "zip tar archive pack gzip xz zstd archiv packen",
    ),
    (
        "cmd.kw.extract_here",
        "unzip untar unpack archive decompress",
        "unzip untar unpack archive decompress auspacken archiv dekomprimieren",
    ),
    (
        "cmd.kw.open_with_app",
        "launch default application finder",
        "launch default application finder programm standard",
    ),
    (
        "cmd.kw.open_shell_in_container",
        "docker kubectl kubernetes pod exec terminal sh bash",
        "docker kubectl kubernetes pod exec terminal sh bash",
    ),
    (
        "cmd.kw.appearance",
        "theme dark light mode night day",
        "theme dark light mode night day thema dunkel hell modus nacht tag",
    ),
];
