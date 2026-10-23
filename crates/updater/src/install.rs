//! Replacing the running app bundle and relaunching it.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::bundle;
use crate::error::{Result, UpdateError};
use crate::prepare::PreparedApp;

// ---------------------------------------------------------------- location checks

/// Why the running app can't be replaced in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocationProblem {
    /// Started with `cargo run` or as a bare binary.
    NotBundled,
    /// macOS runs the app from a random read-only copy (App Translocation).
    Translocated,
    /// The app is on a disk image or another read-only volume.
    ReadOnlyVolume,
}

impl std::fmt::Display for LocationProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LocationProblem::NotBundled => "it is not running from an app bundle (development build). Build a release with scripts/build-release.sh.",
            LocationProblem::Translocated => "macOS runs it from a temporary, read-only location (App Translocation). Move Manta.app to the Applications folder with Finder and open it from there.",
            LocationProblem::ReadOnlyVolume => "it runs from a disk image or read-only volume. Drag Manta.app to the Applications folder and open it from there.",
        })
    }
}

/// `…/Name.app/Contents/MacOS/<exe>` → `…/Name.app`.
pub fn bundle_of_executable(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_app = app
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| app.to_path_buf())
}

/// The pure location rule (`read_only`: the bundle's volume is mounted read-only).
pub fn check_location(bundle: Option<&Path>, read_only: bool) -> Result<PathBuf, LocationProblem> {
    let bundle = bundle.ok_or(LocationProblem::NotBundled)?;
    let s = bundle.to_string_lossy();
    if s.contains("/AppTranslocation/") {
        return Err(LocationProblem::Translocated);
    }
    if read_only {
        return Err(LocationProblem::ReadOnlyVolume);
    }
    Ok(bundle.to_path_buf())
}

/// Whether `path` lies on a read-only mount (a DMG, a locked volume).
pub fn is_read_only(path: &Path) -> bool {
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: statfs fills the zeroed struct; c is a valid C string.
    unsafe {
        let mut st: libc::statfs = std::mem::zeroed();
        if libc::statfs(c.as_ptr(), &mut st) != 0 {
            return false;
        }
        st.f_flags & (libc::MNT_RDONLY as u32) != 0
    }
}

/// The bundle of a given executable, after the location checks.
pub fn bundle_for_executable(exe: &Path) -> Result<PathBuf> {
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let bundle = bundle_of_executable(&exe);
    let ro = bundle.as_deref().is_some_and(is_read_only);
    check_location(bundle.as_deref(), ro).map_err(|p| UpdateError::Location(p.to_string()))
}

/// The running app's bundle, after the location checks.
pub fn running_bundle() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| UpdateError::io("current executable", e))?;
    bundle_for_executable(&exe)
}

// ---------------------------------------------------------------- plan

/// What happens to the old bundle after a successful swap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackupDisposal {
    /// Into the user's Trash (default; recoverable).
    #[default]
    Trash,
    /// Deleted right away (tests).
    Delete,
    /// Left next to the app as `.<name>.old-<n>` (tests, debugging).
    Keep,
}

/// A checked plan to replace `target` with the prepared app. Nothing on disk
/// has changed yet; [`InstallPlan::apply`] does the work.
#[derive(Debug)]
pub struct InstallPlan {
    pub prepared: PreparedApp,
    pub target: PathBuf,
    /// The folder isn't writable for this user: one admin prompt is needed.
    pub needs_admin: bool,
    pub backup: BackupDisposal,
}

/// Result of [`InstallPlan::apply`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOutcome {
    pub installed: PathBuf,
    /// Where the old version went (Trash, a kept backup), if it still exists.
    pub backup: Option<PathBuf>,
}

/// Plans replacing `current_bundle` (checks location and writability).
pub fn install(prepared: PreparedApp, current_bundle: &Path) -> Result<InstallPlan> {
    let ro = is_read_only(current_bundle);
    let target = check_location(Some(current_bundle), ro)
        .map_err(|p| UpdateError::Location(p.to_string()))?;
    if !target.is_dir() {
        return Err(UpdateError::Install(format!(
            "{} does not exist",
            target.display()
        )));
    }
    let parent = target
        .parent()
        .ok_or_else(|| UpdateError::Install("the app has no parent folder".into()))?;
    let needs_admin = !(writable(parent) && owned_or_writable(&target));
    Ok(InstallPlan {
        prepared,
        target,
        needs_admin,
        backup: BackupDisposal::Trash,
    })
}

fn writable(p: &Path) -> bool {
    let Ok(c) = CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: valid C string.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

fn owned_or_writable(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    std::fs::symlink_metadata(p).is_ok_and(|m| m.uid() == uid) || writable(p)
}

/// A unique hidden sibling of `target`: `.<name>.<tag>-<pid>-<n>`.
fn sibling(target: &Path, tag: &str) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    target.with_file_name(format!(
        ".{name}.{tag}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ))
}

fn ditto(src: &Path, dst: &Path) -> Result<()> {
    let out = Command::new("/usr/bin/ditto")
        .arg(src)
        .arg(dst)
        .output()
        .map_err(|e| UpdateError::io("ditto", e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(UpdateError::Install(format!(
            "copy failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Atomic exchange of two paths on the same APFS volume.
fn rename_swap(a: &Path, b: &Path) -> std::io::Result<()> {
    let ca = CString::new(a.as_os_str().as_bytes())?;
    let cb = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: valid C strings.
    if unsafe { libc::renamex_np(ca.as_ptr(), cb.as_ptr(), libc::RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

impl InstallPlan {
    /// Swaps the bundles. Without admin rights: the new app is moved (or
    /// copied) next to the old one, then both are exchanged atomically
    /// (`renamex_np(RENAME_SWAP)`, falling back to two renames with rollback),
    /// the result is checked, and the old one goes to the Trash. With
    /// `needs_admin`, the same steps run as one `osascript … with administrator
    /// privileges` call (see [`admin_script`]).
    pub fn apply(self) -> Result<InstallOutcome> {
        if self.needs_admin {
            return self.apply_admin();
        }
        let target = &self.target;
        let stage = sibling(target, "update");
        if std::fs::rename(&self.prepared.app_path, &stage).is_err() {
            // Different volume (or the temp dir is elsewhere): copy instead.
            if let Err(e) = ditto(&self.prepared.app_path, &stage) {
                let _ = std::fs::remove_dir_all(&stage);
                return Err(e);
            }
        }

        // After this block, `old` holds the previous version.
        let old = match rename_swap(&stage, target) {
            Ok(()) => stage.clone(),
            Err(_) => {
                let old = sibling(target, "old");
                std::fs::rename(target, &old).map_err(|e| {
                    let _ = std::fs::remove_dir_all(&stage);
                    UpdateError::Install(format!("could not move the current app aside: {e}"))
                })?;
                if let Err(e) = std::fs::rename(&stage, target) {
                    let _ = std::fs::rename(&old, target);
                    let _ = std::fs::remove_dir_all(&stage);
                    return Err(UpdateError::Install(format!(
                        "could not move the new app into place: {e}"
                    )));
                }
                old
            }
        };

        // Check the result; roll back if the bundle at `target` isn't the new version.
        let ok = bundle::read_bundle_info(target)
            .ok()
            .is_some_and(|i| i.version == self.prepared.version.to_string());
        if !ok {
            let _ = std::fs::remove_dir_all(target);
            let _ = std::fs::rename(&old, target);
            return Err(UpdateError::Install(
                "the new app could not be verified after the swap; the old version was restored"
                    .into(),
            ));
        }
        // Let Finder/Launch Services notice the change.
        let _ = Command::new("/usr/bin/touch").arg(target).status();

        let backup = match self.backup {
            BackupDisposal::Keep => Some(old),
            BackupDisposal::Delete => {
                let _ = std::fs::remove_dir_all(&old);
                None
            }
            BackupDisposal::Trash => {
                let old = visible_backup_name(&old, target);
                match move_to_trash(&old) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("could not move the old app to the Trash ({e}); deleting it");
                        let _ = std::fs::remove_dir_all(&old);
                        None
                    }
                }
            }
        };
        Ok(InstallOutcome {
            installed: target.clone(),
            backup,
        })
    }

    fn apply_admin(self) -> Result<InstallOutcome> {
        let stage = sibling(&self.target, "update");
        let old = sibling(&self.target, "old");
        let script = admin_script(&self.prepared.app_path, &self.target, &stage, &old)?;
        let apple = format!(
            "do shell script \"{}\" with prompt \"Manta wants to install an update.\" with administrator privileges",
            applescript_escape(&script)
        );
        let mut child = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(apple)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| UpdateError::io("osascript", e))?;
        // The user may take a while to type the password; give up after 5 minutes.
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
        let status = loop {
            if let Some(s) = child
                .try_wait()
                .map_err(|e| UpdateError::io("osascript", e))?
            {
                break s;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(UpdateError::Install(
                    "timed out waiting for the administrator password".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        if !status.success() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut err);
            }
            if err.contains("-128") {
                return Err(UpdateError::Cancelled);
            }
            return Err(UpdateError::Install(format!(
                "administrator step failed: {}",
                err.trim()
            )));
        }
        let ok = bundle::read_bundle_info(&self.target)
            .ok()
            .is_some_and(|i| i.version == self.prepared.version.to_string());
        if !ok {
            return Err(UpdateError::Install(
                "the new app could not be verified after installing".into(),
            ));
        }
        Ok(InstallOutcome {
            installed: self.target,
            backup: None,
        })
    }
}

/// Renames the hidden backup to `Manta <old version>.app` before it goes to
/// the Trash (Finder hides dot-files there, so the user couldn't restore it).
/// Returns the path to trash (the original one if renaming isn't possible).
fn visible_backup_name(old: &Path, target: &Path) -> PathBuf {
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Manta".into());
    let version = bundle::read_bundle_info(old)
        .map(|i| i.version)
        .unwrap_or_else(|_| "old".into());
    let nice = target.with_file_name(format!("{stem} {version}.app"));
    if !nice.exists() && std::fs::rename(old, &nice).is_ok() {
        nice
    } else {
        old.to_path_buf()
    }
}

/// Single-quotes a path for `/bin/sh`.
pub fn shell_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The shell script run as root when the app's folder isn't writable. Short
/// on purpose so it can be audited: copy the new app next to the old one with
/// the old one's owner, swap with rollback, delete the old copy.
pub fn admin_script(src: &Path, target: &Path, stage: &Path, old: &Path) -> Result<String> {
    for p in [src, target, stage, old] {
        let s = p.to_string_lossy();
        if !p.is_absolute() || s.contains(['\n', '\r', '\0']) {
            return Err(UpdateError::Install(format!("refusing unusual path {s:?}")));
        }
    }
    let (src, target, stage, old) = (
        shell_quote(src),
        shell_quote(target),
        shell_quote(stage),
        shell_quote(old),
    );
    Ok(format!(
        "set -e; \
/usr/bin/ditto {src} {stage} || {{ /bin/rm -rf {stage}; exit 1; }}; \
/usr/sbin/chown -R \"$(/usr/bin/stat -f %u:%g {target})\" {stage}; \
/bin/mv {target} {old}; \
if /bin/mv {stage} {target}; then /bin/rm -rf {old}; else /bin/mv {old} {target}; /bin/rm -rf {stage}; exit 1; fi"
    ))
}

/// Moves a file or folder to the user's Trash (`NSFileManager`), falling back
/// to Finder via AppleScript. Returns the location in the Trash if known.
pub fn move_to_trash(path: &Path) -> std::io::Result<Option<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let mut out = None;
        if NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut out))
            .is_ok()
        {
            let p = out
                .and_then(|u| u.path())
                .map(|s| PathBuf::from(s.to_string()));
            return Ok(p);
        }
    }
    let script = format!(
        "tell application \"Finder\" to delete (POSIX file \"{}\" as alias)",
        applescript_escape(&path.to_string_lossy())
    );
    let ok = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .is_ok_and(|o| o.status.success());
    if ok {
        Ok(None)
    } else {
        Err(std::io::Error::other(
            "neither NSFileManager nor Finder could move it to the Trash",
        ))
    }
}

/// Starts a detached helper that waits until process `pid` has exited (at
/// most ~60 s), then opens `app`. Call it right before quitting.
pub fn relaunch_after_exit(pid: u32, app: &Path) -> std::io::Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"i=0; while kill -0 "$1" 2>/dev/null && [ $i -lt 300 ]; do sleep 0.2; i=$((i+1)); done; exec /usr/bin/open "$2""#)
        .arg("manta-relaunch")
        .arg(pid.to_string())
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}
