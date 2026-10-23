//! Named rule sets, saved as `presets.json` in the plugin's data dir, and
//! the rules used last (`last.json`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rules::Rule;

pub const PRESETS_FILE: &str = "presets.json";
pub const LAST_FILE: &str = "last.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Presets {
    #[serde(default)]
    pub items: Vec<Preset>,
}

impl Presets {
    /// Load from `dir` (missing or broken file: no presets).
    pub fn load(dir: &Path) -> Presets {
        std::fs::read_to_string(dir.join(PRESETS_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        write_json(dir, PRESETS_FILE, self)
    }

    /// Add or replace (same name, ignoring case) a preset; sorted by name.
    pub fn upsert(&mut self, name: &str, rules: Vec<Rule>) {
        let name = name.trim().to_string();
        match self
            .items
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&name))
        {
            Some(p) => {
                p.name = name;
                p.rules = rules;
            }
            None => self.items.push(Preset { name, rules }),
        }
        self.items.sort_by_key(|p| p.name.to_lowercase());
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|p| p.name != name);
        self.items.len() != before
    }

    pub fn get(&self, name: &str) -> Option<&Preset> {
        self.items.iter().find(|p| p.name == name)
    }
}

/// The rules of the last batch rename (empty if none).
pub fn load_last(dir: &Path) -> Vec<Rule> {
    std::fs::read_to_string(dir.join(LAST_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_last(dir: &Path, rules: &[Rule]) -> Result<(), String> {
    write_json(dir, LAST_FILE, &rules)
}

/// Write atomically (temp file + rename).
fn write_json<T: Serialize>(dir: &Path, file: &str, value: &T) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{file}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(file)).map_err(|e| e.to_string())
}
