//! The size cache on disk, so sizes show at once after a restart.
//!
//! Entries are keyed like the in-memory cache: the folder's path and its
//! own mtime/ctime ([`StampKey`]). A folder whose stamp changed is measured
//! again; one whose stamp still matches is shown from here and re-measured
//! quietly in the background (something deeper inside may have changed).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::walker::{Category, StampKey, Totals};

/// File name in the plugin's data folder.
pub const FILE: &str = "size-cache.json";
/// At most this many folders are kept (the most recently measured).
pub const MAX_ENTRIES: usize = 20_000;
/// Bumped when the format changes; other versions are ignored.
const VERSION: u32 = 1;

/// One measured folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub stamp: StampKey,
    pub totals: Totals,
    /// When it was measured (seconds since the epoch).
    pub measured: u64,
}

#[derive(Serialize, Deserialize)]
struct Doc {
    version: u32,
    entries: Vec<Row>,
}

/// i128 stamps as strings: JSON numbers that large are not portable.
#[derive(Serialize, Deserialize)]
struct Row {
    path: PathBuf,
    mtime: String,
    ctime: String,
    logical: u64,
    on_disk: u64,
    files: u64,
    dirs: u64,
    unreadable: u64,
    by_cat: Vec<u64>,
    measured: u64,
}

/// Seconds since the epoch, now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The cache saved last time (empty when missing or unreadable).
pub fn load(path: &Path) -> HashMap<PathBuf, Stored> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let doc: Doc = match serde_json::from_str(&text) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("folder size cache {}: {e}", path.display());
            return HashMap::new();
        }
    };
    if doc.version != VERSION {
        return HashMap::new();
    }
    doc.entries
        .into_iter()
        .filter_map(|r| {
            let by_cat: [u64; Category::COUNT] = r.by_cat.try_into().ok()?;
            let stored = Stored {
                stamp: (r.mtime.parse().ok()?, r.ctime.parse().ok()?),
                totals: Totals {
                    logical: r.logical,
                    on_disk: r.on_disk,
                    files: r.files,
                    dirs: r.dirs,
                    unreadable: r.unreadable,
                    by_cat,
                    done: true,
                    cancelled: false,
                },
                measured: r.measured,
            };
            Some((r.path, stored))
        })
        .collect()
}

/// Write `entries` (the newest [`MAX_ENTRIES`]) atomically.
pub fn save<'a>(
    path: &Path,
    entries: impl Iterator<Item = (&'a PathBuf, &'a Stored)>,
) -> std::io::Result<()> {
    let mut rows: Vec<Row> = entries
        .map(|(p, s)| Row {
            path: p.clone(),
            mtime: s.stamp.0.to_string(),
            ctime: s.stamp.1.to_string(),
            logical: s.totals.logical,
            on_disk: s.totals.on_disk,
            files: s.totals.files,
            dirs: s.totals.dirs,
            unreadable: s.totals.unreadable,
            by_cat: s.totals.by_cat.to_vec(),
            measured: s.measured,
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.measured));
    rows.truncate(MAX_ENTRIES);
    let doc = Doc {
        version: VERSION,
        entries: rows,
    };
    let json = serde_json::to_vec(&doc).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(logical: u64, measured: u64) -> Stored {
        let mut by_cat = [0; Category::COUNT];
        by_cat[1] = logical;
        Stored {
            stamp: (1_700_000_000_123_456_789, -5),
            totals: Totals {
                logical,
                on_disk: logical + 4096,
                files: 3,
                dirs: 1,
                unreadable: 0,
                by_cat,
                done: true,
                cancelled: false,
            },
            measured,
        }
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join(FILE);
        let mut m = HashMap::new();
        m.insert(PathBuf::from("/a"), stored(10, 1));
        m.insert(PathBuf::from("/b/ü c"), stored(20, 2));
        save(&file, m.iter()).unwrap();
        assert_eq!(load(&file), m);
    }

    #[test]
    fn keeps_the_newest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let m: HashMap<PathBuf, Stored> = (0..MAX_ENTRIES as u64 + 5)
            .map(|i| (PathBuf::from(format!("/{i}")), stored(i, i)))
            .collect();
        save(&file, m.iter()).unwrap();
        let back = load(&file);
        assert_eq!(back.len(), MAX_ENTRIES);
        assert!(!back.contains_key(Path::new("/0")), "the oldest went");
        assert!(back.contains_key(Path::new(&format!("/{}", MAX_ENTRIES + 4))));
    }

    #[test]
    fn missing_or_broken_files_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        assert!(load(&file).is_empty());
        std::fs::write(&file, "{not json").unwrap();
        assert!(load(&file).is_empty());
        std::fs::write(&file, r#"{"version":99,"entries":[]}"#).unwrap();
        assert!(load(&file).is_empty());
    }
}
