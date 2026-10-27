//! App preferences (Settings → General), stored in
//! `config_dir/Manta/settings.json`.

use std::path::{Path, PathBuf};

use manta_ops::ConflictPolicy;

use crate::i18n::Language;
pub use crate::theme::{Accent, Appearance};
use serde::{Deserialize, Serialize};

/// "If an item already exists". New installs (and settings files without
/// the key) ask; a stored choice is kept as it is — settings files always
/// stored the old default `skip`, so a deliberate "Skip" can't be told
/// apart from an untouched one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPref {
    /// Ask at each conflict (Finder's way).
    #[default]
    Ask,
    Skip,
    OverwriteIfNewer,
    Overwrite,
    Rename,
}

impl ConflictPref {
    pub const ALL: [ConflictPref; 5] = [
        ConflictPref::Ask,
        ConflictPref::Skip,
        ConflictPref::OverwriteIfNewer,
        ConflictPref::Overwrite,
        ConflictPref::Rename,
    ];

    pub fn policy(self) -> ConflictPolicy {
        match self {
            ConflictPref::Ask => ConflictPolicy::Ask,
            ConflictPref::Skip => ConflictPolicy::Skip,
            ConflictPref::OverwriteIfNewer => ConflictPolicy::OverwriteIfNewer,
            ConflictPref::Overwrite => ConflictPolicy::Overwrite,
            ConflictPref::Rename => ConflictPolicy::Rename,
        }
    }
}

/// What a drop of files (between panes, from other apps) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropAction {
    /// Ask with the copy / move dialog, Copy preselected (⌘: Move).
    #[default]
    Ask,
    /// Copy without asking (⌘ held: move).
    Copy,
    /// Move without asking (⌥ held: copy).
    Move,
}

impl DropAction {
    pub const ALL: [DropAction; 3] = [DropAction::Ask, DropAction::Copy, DropAction::Move];

    pub fn label(self) -> &'static str {
        match self {
            DropAction::Ask => t!("settings.drop.ask"),
            DropAction::Copy => t!("settings.drop.copy"),
            DropAction::Move => t!("settings.drop.move"),
        }
    }

    /// Without modifier keys the drop moves.
    pub fn moves_by_default(self) -> bool {
        self == DropAction::Move
    }
}

/// What Space does on a selected file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceAction {
    /// Apple's Quick Look panel for local files (macOS); remote and archive
    /// items still use the built-in preview.
    #[default]
    QuickLook,
    /// Manta's own preview window, always.
    BuiltIn,
}

impl SpaceAction {
    pub const ALL: [SpaceAction; 2] = [SpaceAction::QuickLook, SpaceAction::BuiltIn];

    pub fn label(self) -> &'static str {
        match self {
            SpaceAction::QuickLook => t!("settings.space.quick_look"),
            SpaceAction::BuiltIn => t!("settings.space.built_in"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    /// New tabs (and the panes at launch) show hidden files.
    #[serde(default)]
    pub show_hidden: bool,
    /// Preselected "If an item already exists" choice for copy and move.
    #[serde(default)]
    pub conflict: ConflictPref,
    /// What dropping files does: ask (Copy preselected), copy or move.
    #[serde(default)]
    pub drop_action: DropAction,
    /// Enter / double-click opens zip and tar archives as folders in the
    /// pane (⌥↩ opens them with the default app).
    #[serde(default = "yes")]
    pub open_archives: bool,
    /// Space opens Quick Look (macOS) or the built-in preview.
    #[serde(default)]
    pub space_action: SpaceAction,
    /// Light, dark or follow the system.
    #[serde(default)]
    pub appearance: Appearance,
    /// Accent colour (selection, buttons, focus ring).
    #[serde(default)]
    pub accent: Accent,
    /// UI language; System follows macOS's preferred languages.
    #[serde(default)]
    pub language: Language,
    /// Each folder reopens in the view (Icons, List, Columns, Gallery) it
    /// was last shown in; off: the view belongs to the pane.
    #[serde(default)]
    pub remember_view_per_folder: bool,
    /// Download small images (< 5 MB) from servers in the background for
    /// thumbnails; off: server files show their type icon.
    #[serde(default)]
    pub remote_thumbnails: bool,
    /// Files show the icon macOS has for their type (and apps their own),
    /// like Finder; off: Manta's glyphs.
    #[serde(default = "yes")]
    pub system_icons: bool,
    /// The newest one-time "what's new" notice the user has dismissed
    /// (see `app::WHATS_NEW`). New installs start at the current one.
    #[serde(default)]
    pub whats_new_seen: u32,
    /// The sidebar's "Connect Google Drive …" hint was dismissed.
    #[serde(default)]
    pub gdrive_hint_dismissed: bool,
}

fn yes() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        serde_json::from_str("{}").expect("defaults")
    }
}

impl Prefs {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Manta").join("settings.json"))
    }

    /// Missing or unreadable files give the defaults.
    pub fn load(path: &Path) -> Prefs {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Prefs::load(&path), Prefs::default());
        let p = Prefs {
            show_hidden: true,
            conflict: ConflictPref::Rename,
            open_archives: false,
            ..Prefs::default()
        };
        p.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), p);
        std::fs::write(&path, "{\"show_hidden\": true}").unwrap();
        assert_eq!(
            Prefs::load(&path).conflict,
            ConflictPref::Ask,
            "never chosen: ask"
        );
        std::fs::write(&path, "{\"conflict\": \"skip\"}").unwrap();
        assert_eq!(
            Prefs::load(&path).conflict,
            ConflictPref::Skip,
            "a stored choice is kept"
        );
        assert_eq!(Prefs::default().conflict, ConflictPref::Ask);
        assert!(
            Prefs::load(&path).open_archives,
            "archives open as folders by default"
        );
    }

    #[test]
    fn drops_ask_by_default() {
        assert_eq!(Prefs::default().drop_action, DropAction::Ask);
        let p: Prefs = serde_json::from_str("{\"drop_action\": \"move\"}").unwrap();
        assert_eq!(p.drop_action, DropAction::Move);
        assert!(p.drop_action.moves_by_default() && !DropAction::Ask.moves_by_default());
    }

    #[test]
    fn space_action_defaults_to_quick_look() {
        assert_eq!(Prefs::default().space_action, SpaceAction::QuickLook);
        let p: Prefs = serde_json::from_str("{\"show_hidden\": true}").unwrap();
        assert_eq!(p.space_action, SpaceAction::QuickLook);
        let p: Prefs = serde_json::from_str("{\"space_action\": \"built_in\"}").unwrap();
        assert_eq!(p.space_action, SpaceAction::BuiltIn);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"space_action\":\"built_in\""), "{json}");
    }

    #[test]
    fn appearance_accent_language() {
        let d = Prefs::default();
        assert_eq!(d.appearance, Appearance::System);
        assert_eq!(d.accent, Accent::Blue);
        assert_eq!(d.language, Language::System);
        let p: Prefs = serde_json::from_str(
            "{\"appearance\":\"dark\",\"accent\":\"graphite\",\"language\":\"de\"}",
        )
        .unwrap();
        assert_eq!(p.appearance, Appearance::Dark);
        assert_eq!(p.accent, Accent::Graphite);
        assert_eq!(p.language, Language::German);
        let back: Prefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}
