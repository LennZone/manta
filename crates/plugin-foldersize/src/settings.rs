//! Plugin settings, stored as `settings.json` in the plugin's data dir.

use std::path::Path;

use manta_plugin::i18n::format_decimal;
use serde::{Deserialize, Serialize};

use crate::i18n::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Measure every sub-folder of the shown folders automatically.
    /// `false`: only on demand ("Calculate Size").
    #[serde(default = "yes")]
    pub auto: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto: true }
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

/// Like the file list: decimal units, "1.2 GB" / "1,2 GB".
pub fn format_size(bytes: u64) -> String {
    if bytes < 1000 {
        return tr!("bytes", count = bytes);
    }
    let mut value = bytes as f64;
    let mut unit = "";
    for u in (0..6).map(manta_plugin::i18n::size_unit) {
        value /= 1000.0;
        unit = u;
        if value < 1000.0 {
            break;
        }
    }
    if value < 10.0 {
        format!("{} {unit}", format_decimal(value, 1))
    } else {
        format!("{value:.0} {unit}")
    }
}

/// "12,345" / "12.345".
pub use manta_plugin::i18n::format_count;

#[cfg(test)]
mod tests {
    use super::*;
    use manta_plugin::i18n::{with_lang, Lang};

    #[test]
    fn round_trip_and_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Settings::load(tmp.path()).auto);
        Settings { auto: false }.save(tmp.path());
        assert!(!Settings::load(tmp.path()).auto);
        std::fs::write(tmp.path().join(FILE), "{}").unwrap();
        assert!(Settings::load(tmp.path()).auto);
    }

    #[test]
    fn formats() {
        assert_eq!(format_size(999), "999 bytes");
        assert_eq!(format_size(1_234_000_000), "1.2 GB");
        assert_eq!(format_count(1234567), "1,234,567");
        assert_eq!(format_count(12), "12");
        with_lang(Lang::De, || {
            assert_eq!(format_size(999), "999 Byte");
            assert_eq!(format_size(1_234_000_000), "1,2 GB");
            assert_eq!(format_count(1234567), "1.234.567");
        });
    }
}
