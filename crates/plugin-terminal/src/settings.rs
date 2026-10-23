//! User settings, persisted as JSON in the plugin's data directory.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Shell to run; empty means `$SHELL` (fallback `/bin/zsh`).
    pub shell: String,
    pub font_size: f32,
    /// Option sends ESC+key instead of typing the layout's character.
    pub option_as_meta: bool,
    /// When you `cd` in the terminal, the pane above follows.
    pub pane_follows_terminal: bool,
    pub scrollback_lines: usize,
    /// Show copy/move/delete/… done in the GUI as shell commands.
    pub show_file_actions: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shell: String::new(),
            font_size: 12.0,
            // Off by default: many layouts (e.g. German) need Option for
            // characters like @ ~ | { }.
            option_as_meta: false,
            pane_follows_terminal: true,
            scrollback_lines: 10_000,
            show_file_actions: true,
        }
    }
}

pub const MIN_SCROLLBACK: usize = 1_000;
pub const MAX_SCROLLBACK: usize = 200_000;

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(dir.join("settings.json"), json)
    }

    pub fn sanitized(mut self) -> Settings {
        self.font_size = if self.font_size.is_finite() {
            self.font_size.clamp(8.0, 32.0)
        } else {
            12.0
        };
        self.scrollback_lines = self.scrollback_lines.clamp(MIN_SCROLLBACK, MAX_SCROLLBACK);
        self.shell = self.shell.trim().to_owned();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let s = Settings {
            shell: "/bin/bash".into(),
            font_size: 14.0,
            option_as_meta: true,
            pane_follows_terminal: false,
            scrollback_lines: 5000,
            show_file_actions: false,
        };
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), s);
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"font_size": 100, "scrollback_lines": 3}"#,
        )
        .unwrap();
        let l = Settings::load(dir.path());
        assert_eq!(l.font_size, 32.0);
        assert_eq!(l.scrollback_lines, MIN_SCROLLBACK);
        assert!(l.pane_follows_terminal);
        assert!(
            l.show_file_actions,
            "on by default, also for older settings files"
        );
    }
}
