//! Plugins that ship with Manta: embedded into the binary at build time
//! (`build.rs` reads the top-level `plugins/` folder and the `hello-python`
//! example), listed in Settings → Plugins → Available and installed with
//! one click ([`crate::host::PluginHost::install_bundled`]).

use std::borrow::Cow;
use std::cmp::Ordering;
use std::path::{Component, Path};

use crate::manifest::{Manifest, MANIFEST_FILE};

pub(crate) struct RawFile {
    path: &'static str,
    bytes: &'static [u8],
    executable: bool,
}

pub(crate) struct RawPlugin {
    folder: &'static str,
    files: &'static [RawFile],
}

include!(concat!(env!("OUT_DIR"), "/bundled.rs"));

/// One file of a bundled plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledFile {
    /// Relative to the plugin folder, `/`-separated.
    pub path: String,
    pub bytes: Cow<'static, [u8]>,
    /// Written with mode 0755 (scripts the manifest starts directly).
    pub executable: bool,
}

/// A plugin that ships with Manta.
#[derive(Debug, Clone, PartialEq)]
pub struct BundledPlugin {
    /// Folder name in the source tree (e.g. "quick-actions").
    pub folder: String,
    pub manifest: Manifest,
    pub files: Vec<BundledFile>,
}

impl BundledPlugin {
    /// Build one from files (tests); `None` if `plugin.toml` is missing or
    /// invalid.
    pub fn from_files(folder: &str, files: Vec<BundledFile>) -> Option<BundledPlugin> {
        let toml = files.iter().find(|f| f.path == MANIFEST_FILE)?;
        let text = std::str::from_utf8(&toml.bytes).ok()?;
        let manifest = match Manifest::parse(text, &Path::new(folder).join(MANIFEST_FILE)) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("bundled plugin {folder}: {e}");
                return None;
            }
        };
        Some(BundledPlugin {
            folder: folder.to_string(),
            manifest,
            files,
        })
    }

    /// Write the plugin's files into `dir` (created).
    pub fn write_to(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        for f in &self.files {
            let rel = Path::new(&f.path);
            // Embedded paths are relative and plain; refuse anything else.
            if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err(std::io::Error::other(format!(
                    "bad path in bundled plugin: {}",
                    f.path
                )));
            }
            let to = dir.join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&to, &f.bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = if f.executable { 0o755 } else { 0o644 };
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        }
        Ok(())
    }
}

/// The plugins embedded in this binary (invalid manifests are skipped with
/// a log line).
pub fn embedded() -> Vec<BundledPlugin> {
    BUNDLED
        .iter()
        .filter_map(|p| {
            let files = p
                .files
                .iter()
                .map(|f| BundledFile {
                    path: f.path.to_string(),
                    bytes: Cow::Borrowed(f.bytes),
                    executable: f.executable,
                })
                .collect();
            BundledPlugin::from_files(p.folder, files)
        })
        .collect()
}

/// Compare two versions semver-style: `1.10.0 > 1.9.2`, a release is newer
/// than its pre-releases (`1.0.0 > 1.0.0-beta.2 > 1.0.0-beta.1`), build
/// metadata (`+…`) is ignored, missing parts count as 0 (`1.2 == 1.2.0`).
/// Versions that aren't numbers fall back to comparing the text.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn split(v: &str) -> (Vec<u64>, Option<&str>, bool) {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let v = v.split('+').next().unwrap_or("");
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v, None),
        };
        let mut ok = true;
        let nums = core
            .split('.')
            .map(|n| {
                n.parse::<u64>().unwrap_or_else(|_| {
                    ok = false;
                    0
                })
            })
            .collect();
        (nums, pre, ok)
    }
    let (an, ap, aok) = split(a);
    let (bn, bp, bok) = split(b);
    if !aok || !bok {
        return a.trim().cmp(b.trim());
    }
    for i in 0..an.len().max(bn.len()) {
        let (x, y) = (
            an.get(i).copied().unwrap_or(0),
            bn.get(i).copied().unwrap_or(0),
        );
        match x.cmp(&y) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let xs: Vec<&str> = x.split('.').collect();
            let ys: Vec<&str> = y.split('.').collect();
            for i in 0..xs.len().max(ys.len()) {
                let o = match (xs.get(i), ys.get(i)) {
                    (None, Some(_)) => Ordering::Less,
                    (Some(_), None) => Ordering::Greater,
                    (Some(p), Some(q)) => match (p.parse::<u64>(), q.parse::<u64>()) {
                        (Ok(m), Ok(n)) => m.cmp(&n),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        _ => p.cmp(q),
                    },
                    (None, None) => Ordering::Equal,
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        use Ordering::*;
        for (a, b, o) in [
            ("1.10.0", "1.9.2", Greater),
            ("1.2", "1.2.0", Equal),
            ("v2.0.0", "1.99.99", Greater),
            ("1.0.0", "1.0.0-beta.2", Greater),
            ("1.0.0-beta.2", "1.0.0-beta.10", Less),
            ("1.0.0-alpha", "1.0.0-alpha.1", Less),
            ("1.0.0-rc.1", "1.0.0-beta", Greater),
            ("0.1.0+build.5", "0.1.0", Equal),
            ("0.1.0", "0.2.0", Less),
        ] {
            assert_eq!(compare_versions(a, b), o, "{a} vs {b}");
        }
    }

    #[test]
    fn embedded_plugins_are_valid() {
        let all = embedded();
        assert_eq!(all.len(), BUNDLED.len(), "every bundled plugin.toml parses");
        let hello = all
            .iter()
            .find(|p| p.manifest.id == "hello-python")
            .expect("hello-python is bundled");
        assert!(hello.files.iter().any(|f| f.path == "main.py"));
        assert!(all.iter().all(|p| p
            .files
            .iter()
            .all(|f| !f.path.starts_with("tests/") && !f.path.contains("__pycache__"))));
        // Plugins that became built-ins aren't offered any more.
        for (old, _) in crate::host::RETIRED_EXTERNAL {
            assert!(
                all.iter().all(|p| p.manifest.id != *old),
                "{old} is built in now"
            );
        }

        let tmp = tempfile::tempdir().unwrap();
        hello.write_to(tmp.path()).unwrap();
        let m = Manifest::load(tmp.path()).unwrap();
        assert_eq!(m, hello.manifest);
    }

    #[test]
    fn write_refuses_escaping_paths() {
        let p = BundledPlugin {
            folder: "x".into(),
            manifest: embedded().remove(0).manifest,
            files: vec![BundledFile {
                path: "../evil".into(),
                bytes: Cow::Borrowed(b"x"),
                executable: false,
            }],
        };
        let tmp = tempfile::tempdir().unwrap();
        assert!(p.write_to(&tmp.path().join("p")).is_err());
        assert!(!tmp.path().join("evil").exists());
    }
}
