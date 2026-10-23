//! One-time import of the user data from RustLift, the app's former name.
//!
//! This module is migration code: it is the one place (with the deprecation
//! aliases and the reserved plugin-id prefixes) that still names the old app.
//!
//! Runs in `main` before anything reads or writes the support folder:
//!
//! 1. `~/Library/Application Support/rustlift` is moved to `…/Manta` when the
//!    new folder doesn't exist yet (an empty new folder counts as missing).
//!    A plain rename first; across volumes copy, verify, then delete.
//! 2. Internal ids are rewritten inside the (new) support folder:
//!    `plugins.json` keys and values (`rustlift.git` → `manta.git`, also in
//!    panel keys like `rustlift.git/git/sidebar`) and the plugin data folders
//!    `plugin-data/rustlift.*` → `plugin-data/manta.*`.
//! 3. `~/Library/Logs/RustLift` is moved to `~/Library/Logs/Manta`, and
//!    `rustlift.log*` inside it renamed to `manta.log*`.
//!
//! Every step is idempotent: a second run finds nothing to do. Conflicts (both
//! old and new exist with content) never overwrite anything; the old copy is
//! left where it is and logged. The default file viewer (`NSFileViewer`) is
//! handled separately in `default_viewer::adopt_old_bundle_id`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Migration: the support folder name of RustLift.
const OLD_SUPPORT_NAME: &str = "rustlift";
/// Migration: RustLift's log folder below `~/Library/Logs`.
const OLD_LOGS_NAME: &str = "RustLift";
/// Migration: RustLift's built-in plugin id prefix.
const OLD_ID_PREFIX: &str = "rustlift.";
/// Migration: RustLift's log file name (and `.1` backup).
const OLD_LOG_FILE: &str = "rustlift.log";
const NEW_ID_PREFIX: &str = "manta.";
const NEW_LOG_FILE: &str = "manta.log";

/// Set when this launch imported data; the app shows a toast once.
static IMPORTED: AtomicBool = AtomicBool::new(false);

/// True once after a launch that imported RustLift data.
pub fn take_imported() -> bool {
    IMPORTED.swap(false, Ordering::Relaxed)
}

/// Where the old and new data live.
#[derive(Debug, Clone)]
pub struct Paths {
    pub old_support: PathBuf,
    pub new_support: PathBuf,
    pub old_logs: Option<PathBuf>,
    pub new_logs: Option<PathBuf>,
}

impl Paths {
    /// The real locations (`None` without a config dir).
    pub fn platform() -> Option<Paths> {
        let config = dirs::config_dir()?;
        let logs = if cfg!(target_os = "macos") {
            dirs::home_dir().map(|h| h.join("Library/Logs"))
        } else {
            None
        };
        Some(Paths {
            old_support: config.join(OLD_SUPPORT_NAME),
            new_support: config.join("Manta"),
            old_logs: logs.as_ref().map(|l| l.join(OLD_LOGS_NAME)),
            new_logs: logs.map(|l| l.join("Manta")),
        })
    }
}

/// What happened, for the log (written before the logger exists, so the
/// lines are collected and logged by `main` afterwards).
#[derive(Debug, Default)]
pub struct Report {
    /// Something was moved or rewritten (show the toast).
    pub imported: bool,
    pub info: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    fn info(&mut self, s: String) {
        self.info.push(s);
    }
    fn warn(&mut self, s: String) {
        self.warnings.push(s);
    }
    /// Hand the lines to the logger and remember the toast.
    pub fn log(&self) {
        for l in &self.info {
            log::info!("migration from RustLift: {l}");
        }
        for l in &self.warnings {
            log::warn!("migration from RustLift: {l}");
        }
        if self.imported {
            IMPORTED.store(true, Ordering::Relaxed);
        }
    }
}

/// Run the migration with the real `rename`.
pub fn run(paths: &Paths) -> Report {
    run_with(paths, &|a, b| std::fs::rename(a, b))
}

/// [`run`] with an injectable rename (tests simulate a cross-volume move).
pub fn run_with(paths: &Paths, rename: &dyn Fn(&Path, &Path) -> io::Result<()>) -> Report {
    let mut r = Report::default();
    if paths.old_support.is_dir() {
        if is_missing_or_empty(&paths.new_support) {
            match move_dir(&paths.old_support, &paths.new_support, rename) {
                Ok(how) => {
                    r.imported = true;
                    r.info(format!(
                        "moved {} to {} ({how})",
                        paths.old_support.display(),
                        paths.new_support.display()
                    ));
                }
                Err(e) => r.warn(format!(
                    "could not move {} to {}: {e}",
                    paths.old_support.display(),
                    paths.new_support.display()
                )),
            }
        } else {
            r.warn(format!(
                "both {} and {} exist; keeping the new one, the old folder is left untouched",
                paths.old_support.display(),
                paths.new_support.display()
            ));
        }
    }
    if paths.new_support.is_dir() {
        rewrite_ids(&paths.new_support, rename, &mut r);
    }
    if let (Some(old), Some(new)) = (&paths.old_logs, &paths.new_logs) {
        migrate_logs(old, new, rename, &mut r);
    }
    r
}

fn is_missing_or_empty(p: &Path) -> bool {
    match std::fs::read_dir(p) {
        Ok(mut rd) => rd.next().is_none(),
        Err(e) => e.kind() == io::ErrorKind::NotFound,
    }
}

/// Move `old` to `new` (which is missing or an empty folder). Rename first;
/// on a cross-volume error copy into a temporary sibling, verify, rename it
/// into place and only then delete `old`.
fn move_dir(
    old: &Path,
    new: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<&'static str> {
    if new.is_dir() {
        std::fs::remove_dir(new)?; // empty (checked by the caller)
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match rename(old, new) {
        Ok(()) => return Ok("renamed"),
        Err(e) if is_cross_device(&e) => {}
        Err(e) => return Err(e),
    }
    let tmp = new.with_file_name(format!(
        ".{}.migrating",
        new.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?; // left over from an interrupted run
    }
    copy_tree(old, &tmp, 0)?;
    if let Err(e) = verify_tree(old, &tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(io::Error::other(format!(
            "copy differs from the original: {e}"
        )));
    }
    std::fs::rename(&tmp, new)?;
    std::fs::remove_dir_all(old)?;
    Ok("copied across volumes, verified, old folder deleted")
}

fn is_cross_device(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18) // EXDEV
}

fn copy_tree(src: &Path, dst: &Path, depth: usize) -> io::Result<()> {
    if depth > 64 {
        return Err(io::Error::other("folder nesting too deep"));
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&from)?, &to)?;
        } else if ft.is_dir() {
            copy_tree(&from, &to, depth + 1)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Same names, kinds and file contents in both trees.
fn verify_tree(a: &Path, b: &Path) -> Result<(), String> {
    let list = |p: &Path| -> Result<Vec<_>, String> {
        let mut v: Vec<_> = std::fs::read_dir(p)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.file_name())
            .collect();
        v.sort();
        Ok(v)
    };
    let (la, lb) = (list(a)?, list(b)?);
    if la != lb {
        return Err(format!("{} has different entries", b.display()));
    }
    for name in la {
        let (pa, pb) = (a.join(&name), b.join(&name));
        let (ma, mb) = (
            std::fs::symlink_metadata(&pa).map_err(|e| e.to_string())?,
            std::fs::symlink_metadata(&pb).map_err(|e| e.to_string())?,
        );
        if ma.is_dir() != mb.is_dir() || ma.file_type().is_symlink() != mb.file_type().is_symlink()
        {
            return Err(format!("{} has a different type", pb.display()));
        }
        if ma.file_type().is_symlink() {
            continue;
        }
        if ma.is_dir() {
            verify_tree(&pa, &pb)?;
        } else if ma.len() != mb.len()
            || std::fs::read(&pa).map_err(|e| e.to_string())?
                != std::fs::read(&pb).map_err(|e| e.to_string())?
        {
            return Err(format!("{} differs", pb.display()));
        }
    }
    Ok(())
}

/// `rustlift.x…` → `manta.x…` (strings that don't start with the old prefix stay).
fn new_id(s: &str) -> Option<String> {
    s.strip_prefix(OLD_ID_PREFIX)
        .map(|rest| format!("{NEW_ID_PREFIX}{rest}"))
}

/// Rewrite every object key and string value that starts with `rustlift.`.
fn rewrite_json(v: &mut serde_json::Value) -> bool {
    use serde_json::Value;
    match v {
        Value::String(s) => match new_id(s) {
            Some(n) => {
                *s = n;
                true
            }
            None => false,
        },
        Value::Array(a) => a.iter_mut().fold(false, |ch, x| rewrite_json(x) | ch),
        Value::Object(map) => {
            let old = std::mem::take(map);
            let mut changed = false;
            for (k, mut val) in old {
                changed |= rewrite_json(&mut val);
                let key = match new_id(&k) {
                    Some(n) => {
                        changed = true;
                        n
                    }
                    None => k,
                };
                // A key already present under the new id wins (newer data).
                map.entry(key).or_insert(val);
            }
            changed
        }
        _ => false,
    }
}

fn rewrite_ids(root: &Path, rename: &dyn Fn(&Path, &Path) -> io::Result<()>, r: &mut Report) {
    let state = root.join("plugins.json");
    if let Ok(text) = std::fs::read_to_string(&state) {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(mut v) => {
                if rewrite_json(&mut v) {
                    let tmp = root.join(".plugins.json.migrating");
                    let res = serde_json::to_string_pretty(&v)
                        .map_err(io::Error::other)
                        .and_then(|json| std::fs::write(&tmp, json))
                        .and_then(|()| std::fs::rename(&tmp, &state));
                    match res {
                        Ok(()) => {
                            r.imported = true;
                            r.info("rewrote the plugin ids in plugins.json".into());
                        }
                        Err(e) => r.warn(format!("could not rewrite {}: {e}", state.display())),
                    }
                }
            }
            Err(e) => r.warn(format!(
                "{} is not valid JSON ({e}); left as is",
                state.display()
            )),
        }
    }

    let data = root.join("plugin-data");
    let Ok(rd) = std::fs::read_dir(&data) else {
        return;
    };
    let mut olds: Vec<_> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    olds.sort();
    for name in olds {
        let Some(new) = new_id(&name) else { continue };
        let (from, to) = (data.join(&name), data.join(&new));
        if !from.is_dir() {
            continue;
        }
        if !is_missing_or_empty(&to) {
            r.warn(format!(
                "{} already exists; {} left untouched",
                to.display(),
                from.display()
            ));
            continue;
        }
        let res = (|| {
            if to.is_dir() {
                std::fs::remove_dir(&to)?;
            }
            rename(&from, &to)
        })();
        match res {
            Ok(()) => {
                r.imported = true;
                r.info(format!("plugin-data/{name} → plugin-data/{new}"));
            }
            Err(e) => r.warn(format!("could not rename {}: {e}", from.display())),
        }
    }
    rename_in_quick_action_comments(&data.join("manta.quickactions/actions.json"), r);
}

/// The Quick Actions file starts as a copy of a commented template that named
/// the old app. Rewrite the name in `//` comment lines only; the user's
/// actions themselves are never changed. Runs after the folder rename (next
/// launch at the latest), idempotent.
fn rename_in_quick_action_comments(path: &Path, r: &mut Report) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut changed = false;
    let out: Vec<String> = text
        .split_inclusive('\n')
        .map(|line| {
            if line.trim_start().starts_with("//") && line.contains("RustLift") {
                changed = true;
                line.replace("RustLift", "Manta")
            } else {
                line.to_string()
            }
        })
        .collect();
    if !changed {
        return;
    }
    let tmp = path.with_extension("json.migrating");
    match std::fs::write(&tmp, out.concat()).and_then(|()| std::fs::rename(&tmp, path)) {
        Ok(()) => {
            r.imported = true;
            r.info("renamed the app in the comments of the Quick Actions file".into());
        }
        Err(e) => r.warn(format!("could not update {}: {e}", path.display())),
    }
}

fn migrate_logs(
    old: &Path,
    new: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
    r: &mut Report,
) {
    if !old.is_dir() {
        return;
    }
    if is_missing_or_empty(new) {
        match move_dir(old, new, rename) {
            Ok(_) => r.info(format!("moved the logs to {}", new.display())),
            Err(e) => {
                r.warn(format!("could not move the logs {}: {e}", old.display()));
                return;
            }
        }
    } else {
        // Both exist: bring over what doesn't collide, then drop the old folder if empty.
        if let Ok(rd) = std::fs::read_dir(old) {
            for e in rd.flatten() {
                let name = e.file_name();
                let mut target = new.join(log_name(&name.to_string_lossy()));
                if target.exists() {
                    target = new.join(&name); // keep the old name next to the new log
                }
                if !target.exists() {
                    if let Err(err) = std::fs::rename(e.path(), &target) {
                        r.warn(format!("could not move {}: {err}", e.path().display()));
                    }
                }
            }
        }
        if std::fs::remove_dir(old).is_err() {
            r.warn(format!(
                "{} is not empty after the move; left in place",
                old.display()
            ));
        }
    }
    // rustlift.log, rustlift.log.1 → manta.log, manta.log.1 (unless those exist).
    if let Ok(rd) = std::fs::read_dir(new) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let renamed = log_name(&name);
            if renamed != name && !new.join(&renamed).exists() {
                let _ = std::fs::rename(e.path(), new.join(renamed));
            }
        }
    }
}

/// `rustlift.log[.1]` → `manta.log[.1]`; other names unchanged.
fn log_name(name: &str) -> String {
    match name.strip_prefix(OLD_LOG_FILE) {
        Some(rest) => format!("{NEW_LOG_FILE}{rest}"),
        None => name.to_string(),
    }
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
