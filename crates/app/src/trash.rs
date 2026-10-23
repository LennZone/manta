//! Moving local items to the macOS Trash (⌘⌫, like Finder).
//!
//! The real implementation calls `NSFileManager trashItemAtURL:…`, which
//! records where the item came from, so Finder's "Put Back" works. Remote
//! backends have no Trash; the app deletes there permanently after asking.
//!
//! The sidebar shows the Trash folder (`~/.Trash`) with its full / empty
//! state ([`probe`]) and empties it ([`empty`]). macOS protects that folder:
//! listing it needs Full Disk Access. Without it the state still comes from
//! `stat` (APFS), and emptying goes through Finder.
//!
//! "Put Back" isn't offered inside Manta: Finder keeps where an item came
//! from in its private `.DS_Store` records of the Trash, which have no
//! public API. Finder's own "Put Back" works for items Manta trashed.
//!
//! The app holds a [`SharedTrasher`] so tests can swap in a fake.

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub trait Trasher: Send + Sync {
    /// Move `path` to the Trash; `Ok(Some(new location))` when the system
    /// reports it.
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, String>;

    /// The Trash folder the sidebar shows (`~/.Trash`; a temp folder in
    /// tests). `None`: no Trash on this system.
    fn dir(&self) -> Option<PathBuf>;

    /// Empty the Trash through Finder: used when Manta can't list the
    /// Trash folder itself (no Full Disk Access).
    fn empty_with_finder(&self) -> Result<(), String>;
}

pub type SharedTrasher = Arc<dyn Trasher>;

/// The user's Trash via `NSFileManager` (macOS only).
pub struct SystemTrash;

impl Trasher for SystemTrash {
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSFileManager, NSString, NSURL};
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let mut out = None;
            NSFileManager::defaultManager()
                .trashItemAtURL_resultingItemURL_error(&url, Some(&mut out))
                .map_err(|e| e.localizedDescription().to_string())?;
            Ok(out
                .and_then(|u| u.path())
                .map(|s| PathBuf::from(s.to_string())))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err("Moving to the Trash isn't supported on this system".into())
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        if cfg!(target_os = "macos") {
            dirs::home_dir().map(|h| h.join(".Trash"))
        } else {
            None
        }
    }

    fn empty_with_finder(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            // Finder empties the Trash of every volume, as its own menu
            // item does (the hardened runtime allows it: Apple Events
            // entitlement). macOS may ask once whether Manta may control
            // Finder.
            let out = std::process::Command::new("/usr/bin/osascript")
                .args(["-e", "tell application \"Finder\" to empty trash"])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("Emptying the Trash isn't supported on this system".into())
        }
    }
}

pub fn system() -> SharedTrasher {
    Arc::new(SystemTrash)
}

/// Outcome of trashing several items on a worker thread.
#[derive(Debug, Clone, Default)]
pub struct TrashOutcome {
    /// Items that went to the Trash.
    pub moved: Vec<PathBuf>,
    /// Where they ended up (when the system said so), for cleanup in tests.
    pub locations: Vec<PathBuf>,
    /// `(path, error)` for items that stayed.
    pub failed: Vec<(PathBuf, String)>,
}

/// Trash every path, continuing after failures.
pub fn trash_all(trasher: &dyn Trasher, paths: &[PathBuf]) -> TrashOutcome {
    let mut out = TrashOutcome::default();
    for p in paths {
        match trasher.trash(p) {
            Ok(loc) => {
                out.moved.push(p.clone());
                out.locations.extend(loc);
            }
            Err(e) => out.failed.push((p.clone(), e)),
        }
    }
    out
}

/// What the sidebar knows about the Trash folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrashStatus {
    /// Manta can list the folder. macOS protects `~/.Trash`: listing it
    /// needs Full Disk Access.
    pub readable: bool,
    /// Items in the Trash (`.DS_Store` aside); `None` when unknown.
    pub items: Option<usize>,
}

impl TrashStatus {
    /// Full or empty, when known.
    pub fn full(&self) -> Option<bool> {
        self.items.map(|n| n > 0)
    }
}

/// Finder's private bookkeeping file in the Trash (Put Back data), not an
/// item the user put there.
const DS_STORE: &str = ".DS_Store";

/// Look at the Trash folder: readable, and how many items it holds.
pub fn probe(dir: &Path) -> TrashStatus {
    match std::fs::read_dir(dir) {
        Ok(rd) => TrashStatus {
            readable: true,
            items: Some(
                rd.filter_map(Result::ok)
                    .filter(|e| e.file_name() != DS_STORE)
                    .count(),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TrashStatus {
            readable: true,
            items: Some(0),
        },
        Err(_) => TrashStatus {
            readable: false,
            items: count_without_listing(dir),
        },
    }
}

/// The number of items in `dir` without listing it (which needs Full Disk
/// Access for `~/.Trash`; `stat` doesn't). APFS keeps a directory's entry
/// count in its link count (2 + entries) and reports 32 bytes per link as
/// its size; that size check tells APFS apart from file systems that only
/// count sub folders. `None` elsewhere.
fn count_without_listing(dir: &Path) -> Option<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(dir).ok()?;
        let links = m.nlink();
        if links < 2 || m.size() != 32 * links {
            return None;
        }
        let ds = dir.join(DS_STORE).symlink_metadata().is_ok();
        Some(((links - 2) as usize).saturating_sub(usize::from(ds)))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

/// A listing error that means "not allowed" (`EACCES`, or macOS privacy
/// protection's `EPERM`).
pub fn is_permission_error(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("permission denied")
        || e.contains("operation not permitted")
        || e.contains("os error 1)")
}

/// `path` is the Trash folder `trash` or inside it.
pub fn is_in_trash(trash: Option<&Path>, path: &Path) -> bool {
    trash.is_some_and(|t| path.starts_with(t))
}

/// Items and total size in the Trash (for the "Empty Trash" question).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrashSummary {
    pub items: usize,
    pub bytes: u64,
}

/// Count and measure what is in the Trash; `None` if it can't be listed.
/// Symlinks are not followed.
pub fn summary(dir: &Path) -> Option<TrashSummary> {
    fn size_of(p: &Path) -> u64 {
        let Ok(m) = p.symlink_metadata() else {
            return 0;
        };
        if m.is_dir() {
            std::fs::read_dir(p)
                .map(|rd| rd.filter_map(Result::ok).map(|e| size_of(&e.path())).sum())
                .unwrap_or(0)
        } else {
            m.len()
        }
    }
    let rd = std::fs::read_dir(dir).ok()?;
    let mut s = TrashSummary { items: 0, bytes: 0 };
    for e in rd
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != DS_STORE)
    {
        s.items += 1;
        s.bytes += size_of(&e.path());
    }
    Some(s)
}

/// What emptying the Trash did.
#[derive(Debug, Clone, Default)]
pub struct EmptyOutcome {
    /// Items deleted.
    pub removed: usize,
    /// `(item, error)` for items that stayed (locked, in use, …).
    pub failed: Vec<(PathBuf, String)>,
    /// Finder emptied it (Manta can't list the folder): no counts.
    pub by_finder: bool,
}

/// Delete everything in the Trash folder `dir` permanently, item by item,
/// skipping (and reporting) what can't be deleted. Finder's `.DS_Store`
/// stays. `Err` if the folder can't be listed.
pub fn empty_dir(dir: &Path) -> Result<EmptyOutcome, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut out = EmptyOutcome::default();
    for e in rd
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != DS_STORE)
    {
        let p = e.path();
        let is_dir = p.symlink_metadata().map(|m| m.is_dir()).unwrap_or(false);
        let r = if is_dir {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        };
        match r {
            Ok(()) => out.removed += 1,
            Err(err) => out.failed.push((p, err.to_string())),
        }
    }
    Ok(out)
}

/// Empty the Trash of `trasher`: item by item when its folder can be
/// listed, else through Finder.
pub fn empty(trasher: &dyn Trasher) -> Result<EmptyOutcome, String> {
    let dir = trasher.dir().ok_or("no Trash")?;
    if probe(&dir).readable {
        empty_dir(&dir)
    } else {
        trasher.empty_with_finder()?;
        Ok(EmptyOutcome {
            by_finder: true,
            ..Default::default()
        })
    }
}

/// A Trash for tests: moves items into a folder and remembers the calls.
#[cfg(test)]
pub struct FakeTrash {
    pub dir: PathBuf,
    pub calls: std::sync::Mutex<Vec<PathBuf>>,
    /// How often "Finder" was asked to empty it.
    pub finder_empties: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl FakeTrash {
    pub fn new(dir: &Path) -> Arc<Self> {
        std::fs::create_dir_all(dir).unwrap();
        Arc::new(FakeTrash {
            dir: dir.to_path_buf(),
            calls: Default::default(),
            finder_empties: Default::default(),
        })
    }
}

#[cfg(test)]
impl Trasher for FakeTrash {
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        self.calls.lock().unwrap().push(path.to_path_buf());
        let to = self.dir.join(path.file_name().unwrap_or_default());
        std::fs::rename(path, &to).map_err(|e| e.to_string())?;
        Ok(Some(to))
    }

    fn dir(&self) -> Option<PathBuf> {
        Some(self.dir.clone())
    }

    fn empty_with_finder(&self) -> Result<(), String> {
        self.finder_empties
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_all_continues_after_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = FakeTrash::new(&tmp.path().join("Trash"));
        let a = tmp.path().join("a.txt");
        std::fs::write(&a, "x").unwrap();
        let missing = tmp.path().join("missing.txt");
        let out = trash_all(fake.as_ref(), &[missing.clone(), a.clone()]);
        assert_eq!(out.moved, vec![a.clone()]);
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.failed[0].0, missing);
        assert!(!a.exists());
        assert!(tmp.path().join("Trash/a.txt").exists());
    }

    #[test]
    fn probe_counts_items_but_not_finders_bookkeeping() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Trash");
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(
            probe(&dir),
            TrashStatus {
                readable: true,
                items: Some(0)
            }
        );
        std::fs::write(dir.join(DS_STORE), "x").unwrap();
        assert_eq!(probe(&dir).full(), Some(false));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        std::fs::create_dir(dir.join("b")).unwrap();
        assert_eq!(
            probe(&dir),
            TrashStatus {
                readable: true,
                items: Some(2)
            }
        );
        assert_eq!(probe(&tmp.path().join("missing")).items, Some(0));
    }

    /// Without permission to list the folder (like `~/.Trash` without Full
    /// Disk Access) the count comes from `stat` on APFS.
    #[cfg(unix)]
    #[test]
    fn probe_without_permission_still_knows_full_or_empty() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Trash");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(DS_STORE), "x").unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o300)).unwrap();
        let st = probe(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!st.readable);
        // APFS: 1 item (.DS_Store aside); other file systems: unknown.
        assert!(matches!(st.items, Some(1) | None), "{st:?}");
    }

    #[test]
    fn permission_errors_are_recognised() {
        assert!(is_permission_error("Operation not permitted (os error 1)"));
        assert!(is_permission_error("Permission denied (os error 13)"));
        assert!(!is_permission_error(
            "No such file or directory (os error 2)"
        ));
        assert!(!is_permission_error("os error 13)"));
    }

    #[test]
    fn empty_dir_deletes_everything_and_reports_what_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Trash");
        std::fs::create_dir_all(dir.join("folder/sub")).unwrap();
        std::fs::write(dir.join("folder/sub/x.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        std::fs::write(dir.join(DS_STORE), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path(), dir.join("link")).unwrap();
        let s = summary(&dir).unwrap();
        assert_eq!(s.items, if cfg!(unix) { 3 } else { 2 });
        assert_eq!(
            s.bytes,
            1005 + if cfg!(unix) {
                dir.join("link").symlink_metadata().unwrap().len()
            } else {
                0
            }
        );
        let out = empty_dir(&dir).unwrap();
        assert!(out.failed.is_empty(), "{:?}", out.failed);
        assert_eq!(out.removed, s.items);
        assert_eq!(probe(&dir).items, Some(0));
        assert!(dir.join(DS_STORE).exists(), "Finder's file stays");
        assert!(tmp.path().exists(), "symlinks aren't followed");
    }

    #[test]
    fn empty_uses_finder_when_the_folder_cant_be_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = FakeTrash::new(&tmp.path().join("Trash"));
        std::fs::write(fake.dir.join("a"), "x").unwrap();
        let out = empty(fake.as_ref()).unwrap();
        assert_eq!((out.removed, out.by_finder), (1, false));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake.dir, std::fs::Permissions::from_mode(0o300)).unwrap();
            let out = empty(fake.as_ref());
            std::fs::set_permissions(&fake.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(out.unwrap().by_finder);
            assert_eq!(
                fake.finder_empties
                    .load(std::sync::atomic::Ordering::SeqCst),
                1
            );
        }
    }

    /// The real Trash: the item leaves its folder and shows up in the
    /// user's Trash (removed again afterwards).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "touches the real ~/.Trash; run explicitly with --ignored"]
    fn system_trash_moves_to_the_users_trash() {
        let tmp = tempfile::tempdir().unwrap();
        let name = format!("manta-trash-test-{}.txt", std::process::id());
        let f = tmp.path().join(&name);
        std::fs::write(&f, "bye").unwrap();
        let out = trash_all(&SystemTrash, std::slice::from_ref(&f));
        if let Some((_, e)) = out.failed.first() {
            // A sandbox without access to ~/.Trash: nothing to check here.
            eprintln!("system trash unavailable: {e}");
            return;
        }
        assert!(!f.exists(), "left its folder");
        let loc = out.locations.first().expect("trash location").clone();
        assert!(loc.exists(), "is in the Trash: {}", loc.display());
        assert!(
            loc.components().any(|c| c.as_os_str() == ".Trash"),
            "{}",
            loc.display()
        );
        std::fs::remove_file(&loc).unwrap();
    }
}
