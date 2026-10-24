//! `plugin.toml`: the manifest of an external plugin.
//!
//! ```toml
//! id = "hello-python"
//! name = "Hello Python"
//! version = "0.1.0"
//! description = "Example plugin"
//! author = "Anna"
//! api_version = 1
//! command = ["python3", "main.py"]   # run with cwd = the plugin folder
//! capabilities = ["decorations", "chips"]   # informational
//! permissions = ["read_files"]             # declared, shown before install
//! license = "MIT"
//! repository = "https://github.com/anna/hello-python"
//! min_manta_version = "0.4.0"
//!
//! # Optional: name and description in other languages (Settings, menus).
//! [localized.de]
//! name = "Hallo Python"
//! description = "Beispiel-Plugin"
//!
//! [[commands]]
//! id = "hello"
//! title = "Say Hello"
//! context = "palette"
//!
//! [[panels]]
//! id = "info"
//! title = "File Info"
//! placement = "right_sidebar"
//! ```

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::protocol::{CommandDecl, PanelDecl, PROTOCOL_VERSION};
use crate::PluginInfo;

pub const MANIFEST_FILE: &str = "plugin.toml";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub api_version: u32,
    /// Program and arguments, e.g. `["python3", "main.py"]`. A program
    /// containing `/` is resolved relative to the plugin folder.
    pub command: Vec<String>,
    /// What the plugin uses (informational, shown in Settings).
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub panels: Vec<PanelDecl>,
    /// `[localized.<lang>]` tables: name and description in other
    /// languages, keyed by language code ("de").
    #[serde(default)]
    pub localized: BTreeMap<String, LocalizedText>,
    /// What the plugin does with the user's Mac, as declared by its author
    /// ([`PERMISSIONS`]). Shown in the install and trust dialogs. Not
    /// enforced: external plugins run with the user's rights.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// SPDX licence id, e.g. `"MIT"`.
    #[serde(default)]
    pub license: Option<String>,
    /// Web page of the plugin (`https://`).
    #[serde(default)]
    pub homepage: Option<String>,
    /// Source code (`https://`).
    #[serde(default)]
    pub repository: Option<String>,
    /// The oldest Manta version the plugin works with (`"0.4.0"`).
    #[serde(default)]
    pub min_manta_version: Option<String>,
}

/// The permissions a plugin can declare (`permissions` in `plugin.toml`).
/// Unknown values are kept and shown as written.
pub const PERMISSIONS: &[&str] = &[
    "read_files",
    "write_files",
    "network",
    "run_programs",
    "terminal",
];

/// The version of this Manta (the workspace version).
pub const MANTA_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A manifest's name and description in one language.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LocalizedText {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Why a manifest can't be used. `Display` gives a message for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ManifestError {}

/// Id prefixes of the built-in plugins: `manta` and `manta.*`.
pub const RESERVED_PREFIXES: &[&str] = &["manta"];

/// Plugin ids: lowercase letters, digits, `.`, `-`, `_`; 2–64 chars;
/// starting with a letter or digit. The [`RESERVED_PREFIXES`] are reserved.
pub fn valid_id(id: &str) -> Result<(), String> {
    let ok_chars = id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'));
    if id.len() < 2 || id.len() > 64 {
        return Err(format!("id \"{id}\" must be 2 to 64 characters long"));
    }
    if !ok_chars || !id.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return Err(format!(
            "id \"{id}\" may only contain lowercase letters, digits, '.', '-' and '_', \
             and must start with a letter or digit"
        ));
    }
    if RESERVED_PREFIXES
        .iter()
        .any(|p| id == *p || id.strip_prefix(p).is_some_and(|r| r.starts_with('.')))
    {
        return Err(format!("id \"{id}\" is reserved for built-in plugins"));
    }
    Ok(())
}

/// Command and panel ids: no `:` (it separates plugin and command ids).
fn valid_item_id(kind: &str, id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err(format!("a {kind} has an empty id"));
    }
    if id.contains(':') || id.chars().any(char::is_whitespace) {
        return Err(format!(
            "{kind} id \"{id}\" may not contain ':' or whitespace"
        ));
    }
    Ok(())
}

impl Manifest {
    /// Parse and validate manifest text. `path` is used in error messages.
    pub fn parse(text: &str, path: &Path) -> Result<Manifest, ManifestError> {
        let err = |message: String| ManifestError {
            path: path.to_path_buf(),
            message,
        };
        let m: Manifest = toml::from_str(text).map_err(|e| {
            // toml's messages are multi-line with a source excerpt; keep
            // the first line plus the excerpt, trimmed.
            err(e.to_string().trim().to_string())
        })?;
        m.validate().map_err(err)?;
        Ok(m)
    }

    /// Read `<dir>/plugin.toml`.
    pub fn load(dir: &Path) -> Result<Manifest, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| ManifestError {
            path: path.clone(),
            message: if e.kind() == std::io::ErrorKind::NotFound {
                format!("no {MANIFEST_FILE} in this folder")
            } else {
                format!("can't read it: {e}")
            },
        })?;
        Manifest::parse(&text, &path)
    }

    pub fn validate(&self) -> Result<(), String> {
        valid_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err("\"name\" must not be empty".into());
        }
        if self.version.trim().is_empty() {
            return Err("\"version\" must not be empty".into());
        }
        if self.api_version != PROTOCOL_VERSION {
            return Err(format!(
                "plugin needs api_version {}, but this Manta supports api_version {}",
                self.api_version, PROTOCOL_VERSION
            ));
        }
        match self.command.first() {
            None => {
                return Err(
                    "\"command\" must list the program to run, e.g. [\"python3\", \"main.py\"]"
                        .into(),
                )
            }
            Some(p) if p.trim().is_empty() => {
                return Err("\"command\": the program name is empty".into())
            }
            _ => {}
        }
        for (key, url) in [
            ("homepage", &self.homepage),
            ("repository", &self.repository),
        ] {
            if let Some(u) = url {
                if !u.starts_with("https://") || u.len() > 500 {
                    return Err(format!("\"{key}\" must be an https:// URL"));
                }
            }
        }
        if let Some(min) = &self.min_manta_version {
            let ok = !min.is_empty()
                && min.split('.').count() <= 3
                && min
                    .split('.')
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
            if !ok {
                return Err(format!(
                    "\"min_manta_version\" \"{min}\" must look like \"0.4.0\""
                ));
            }
            if crate::bundled::compare_versions(min, MANTA_VERSION) == std::cmp::Ordering::Greater {
                return Err(format!(
                    "plugin needs Manta {min} or newer (this is Manta {MANTA_VERSION})"
                ));
            }
        }
        let mut seen = HashSet::new();
        for c in &self.commands {
            valid_item_id("command", &c.id)?;
            if c.title.trim().is_empty() {
                return Err(format!("command \"{}\" has an empty title", c.id));
            }
            if !seen.insert(&c.id) {
                return Err(format!("command id \"{}\" is used twice", c.id));
            }
        }
        let mut seen = HashSet::new();
        for p in &self.panels {
            valid_item_id("panel", &p.id)?;
            if !seen.insert(&p.id) {
                return Err(format!("panel id \"{}\" is used twice", p.id));
            }
        }
        Ok(())
    }

    /// Id, name, version … with name and description in the app's language
    /// ([`crate::locale`]) when the manifest has them.
    pub fn info(&self) -> PluginInfo {
        let locale = crate::locale();
        // "zh-Hans" matches [localized.zh-Hans] or [localized.zh].
        let lang = locale.split(['-', '_']).next().unwrap_or("en");
        let loc = self
            .localized
            .get(locale.as_str())
            .or_else(|| self.localized.get(lang));
        PluginInfo {
            id: self.id.clone(),
            name: loc
                .and_then(|l| l.name.clone())
                .unwrap_or_else(|| self.name.clone()),
            version: self.version.clone(),
            description: loc
                .and_then(|l| l.description.clone())
                .unwrap_or_else(|| self.description.clone()),
            author: self.author.clone(),
            builtin: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandContext, PanelPlacement};

    const GOOD: &str = r#"
id = "hello-python"
name = "Hello Python"
version = "0.1.0"
description = "Example"
author = "Anna"
api_version = 1
command = ["python3", "main.py"]
capabilities = ["decorations"]

[[commands]]
id = "file-info"
title = "Show file info"
context = "file_context_menu"
shortcut = "⌃⌥I"

[[commands]]
id = "hello"
title = "Say hello"

[[panels]]
id = "info"
title = "File Info"
placement = "right_sidebar"
"#;

    fn parse(s: &str) -> Result<Manifest, String> {
        Manifest::parse(s, Path::new("plugin.toml")).map_err(|e| e.message)
    }

    #[test]
    fn parses_full_manifest() {
        let m = parse(GOOD).unwrap();
        assert_eq!(m.id, "hello-python");
        assert_eq!(m.command, vec!["python3", "main.py"]);
        assert_eq!(m.commands.len(), 2);
        assert_eq!(m.commands[0].context, CommandContext::FileContextMenu);
        assert_eq!(m.commands[1].context, CommandContext::Palette);
        assert_eq!(m.panels[0].placement, PanelPlacement::RightSidebar);
        assert!(!m.panels[0].default_open);
        let info = m.info();
        assert!(!info.builtin);
        assert_eq!(info.author, "Anna");
    }

    #[test]
    fn localized_name_follows_the_language() {
        let m = parse(
            "id = \"ab\"\nname = \"Quick Actions\"\ndescription = \"Run things\"\nversion = \"1\"\napi_version = 1\ncommand = [\"./run\"]\n\n[localized.de]\nname = \"Schnellaktionen\"\n",
        )
        .unwrap();
        let de = crate::i18n::with_lang(crate::i18n::Lang::De, || m.info());
        assert_eq!(de.name, "Schnellaktionen");
        assert_eq!(de.description, "Run things", "falls back per field");
        let en = crate::i18n::with_lang(crate::i18n::Lang::En, || m.info());
        assert_eq!(en.name, "Quick Actions");
    }

    #[test]
    fn minimal_manifest() {
        let m = parse(
            "id = \"ab\"\nname = \"A\"\nversion = \"1\"\napi_version = 1\ncommand = [\"./run\"]\n",
        )
        .unwrap();
        assert!(m.description.is_empty() && m.commands.is_empty() && m.panels.is_empty());
    }

    #[test]
    fn clear_errors() {
        let e = parse(&GOOD.replace("api_version = 1", "api_version = 7")).unwrap_err();
        assert!(
            e.contains("api_version 7") && e.contains("supports api_version 1"),
            "{e}"
        );
        let e = parse(&GOOD.replace("hello-python", "Hello Python")).unwrap_err();
        assert!(e.contains("lowercase"), "{e}");
        let e = parse(&GOOD.replace("hello-python", "manta.git")).unwrap_err();
        assert!(e.contains("reserved"), "{e}");
        assert!(valid_id("mantarays").is_ok());
        let e = parse(&GOOD.replace("[\"python3\", \"main.py\"]", "[]")).unwrap_err();
        assert!(e.contains("command"), "{e}");
        let e = parse(&GOOD.replace("id = \"hello\"", "id = \"file-info\"")).unwrap_err();
        assert!(e.contains("used twice"), "{e}");
        let e = parse(&GOOD.replace("id = \"hello\"", "id = \"a:b\"")).unwrap_err();
        assert!(e.contains("':'"), "{e}");
        let e = parse(&GOOD.replace("name = \"Hello Python\"\n", "")).unwrap_err();
        assert!(e.contains("name"), "{e}");
        let e = parse("id = ").unwrap_err();
        assert!(!e.is_empty());
    }

    #[test]
    fn optional_publishing_fields() {
        let extra = "permissions = [\"read_files\", \"network\"]\nlicense = \"MIT\"\n\
            repository = \"https://github.com/anna/hello\"\nhomepage = \"https://anna.dev\"\nmin_manta_version = \"0.1\"\n";
        let m = parse(&GOOD.replace("[[commands]]", &format!("{extra}\n[[commands]]"))).unwrap();
        assert_eq!(m.permissions, ["read_files", "network"]);
        assert_eq!(m.license.as_deref(), Some("MIT"));
        assert_eq!(
            m.repository.as_deref(),
            Some("https://github.com/anna/hello")
        );
        let with =
            |line: &str| parse(&GOOD.replace("[[commands]]", &format!("{line}\n[[commands]]")));
        assert!(with("repository = \"http://x.org\"")
            .unwrap_err()
            .contains("https://"));
        assert!(with("homepage = \"javascript:alert(1)\"")
            .unwrap_err()
            .contains("https://"));
        assert!(with("min_manta_version = \"999.0.0\"")
            .unwrap_err()
            .contains("needs Manta 999.0.0 or newer"));
        assert!(with("min_manta_version = \"latest\"")
            .unwrap_err()
            .contains("0.4.0"));
        assert!(with(&format!("min_manta_version = \"{MANTA_VERSION}\"")).is_ok());
        // Unknown permissions are kept (shown as written), not rejected.
        assert_eq!(
            with("permissions = [\"camera\"]").unwrap().permissions,
            ["camera"]
        );
    }

    #[test]
    fn load_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let e = Manifest::load(dir.path()).unwrap_err();
        assert!(e.message.contains("no plugin.toml"), "{e}");
        std::fs::write(dir.path().join(MANIFEST_FILE), GOOD).unwrap();
        assert_eq!(Manifest::load(dir.path()).unwrap().name, "Hello Python");
    }
}
