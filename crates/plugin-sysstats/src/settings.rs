//! Plugin settings, stored as `settings.json` in the plugin's data dir.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Show the GPU load (when the Mac reports one).
    #[serde(default = "yes")]
    pub show_gpu: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings { show_gpu: true }
    }
}

const FILE: &str = "settings.json";

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(dir);
            let tmp = dir.join("settings.json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join(FILE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Settings::load(tmp.path()).show_gpu);
        Settings { show_gpu: false }.save(tmp.path());
        assert!(!Settings::load(tmp.path()).show_gpu);
        std::fs::write(tmp.path().join(FILE), "{}").unwrap();
        assert!(Settings::load(tmp.path()).show_gpu);
    }
}
