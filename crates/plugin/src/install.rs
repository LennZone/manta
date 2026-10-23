//! Installing external plugins from a folder or an archive.
//!
//! [`prepare`] runs on a background thread when the user picks something in
//! Settings → Plugins → Install Plugin…: an archive (`.zip` or
//! `.mantaplugin`, which is a zip) is unpacked into a hidden staging folder
//! inside the plugins folder, `plugin.toml` is validated, and a
//! [`PluginReview`] collects what the confirmation sheet shows (name,
//! author, version, declared permissions, source, licence, whether macOS
//! quarantined the download). Nothing is installed before the user
//! confirms; cancelling deletes the staging folder.
//!
//! **Quarantine.** Files downloaded from the internet carry the
//! `com.apple.quarantine` attribute. Copying a folder keeps it, and
//! unpacking a quarantined archive gives every unpacked file the archive's
//! quarantine value (as Archive Utility does). Manta never removes it on its
//! own: only when the user switches the plugin on and confirms the trust
//! sheet, which says so ([`clear_quarantine`]). Before that, a native
//! program in the plugin would be blocked by Gatekeeper.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::manifest::{Manifest, MANIFEST_FILE};

/// File name extensions installed as archives.
pub const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "mantaplugin"];
/// Limits for unpacking (a plugin is small; this stops zip bombs).
pub const MAX_ARCHIVE_ENTRIES: usize = 10_000;
pub const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;
/// Prefix of staging folders in the plugins folder (hidden, never scanned).
pub const STAGING_PREFIX: &str = ".review-";

const QUARANTINE: &str = "com.apple.quarantine";

/// What the install / trust sheet shows about a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginReview {
    pub id: String,
    /// In the app's language when the manifest has a translation.
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    /// As declared in `plugin.toml` (see [`crate::manifest::PERMISSIONS`]).
    pub permissions: Vec<String>,
    /// The program and its arguments, joined with spaces.
    pub command: String,
    /// What the user picked: a plugin folder or an archive.
    pub source: PathBuf,
    /// The folder the files are installed from: the unpacked archive in a
    /// staging folder, or `source` itself.
    pub root: PathBuf,
    /// The staging folder to delete afterwards (archives only).
    pub staging: Option<PathBuf>,
    /// Version of an installed plugin with the same id (an update).
    pub installed_version: Option<String>,
    /// Some file carries macOS's quarantine flag (downloaded).
    pub quarantined: bool,
    /// Number of files and their total size.
    pub files: usize,
    pub bytes: u64,
    /// Where the plugin lives once installed (trust sheet).
    pub folder: Option<PathBuf>,
}

impl PluginReview {
    /// The review of a plugin folder (installed, or picked for install).
    pub fn of_folder(dir: &Path, m: &Manifest) -> PluginReview {
        let info = m.info();
        let (files, bytes) = tree_size(dir);
        PluginReview {
            id: m.id.clone(),
            name: info.name,
            version: m.version.clone(),
            description: info.description,
            author: m.author.clone(),
            license: m.license.clone(),
            homepage: m.homepage.clone(),
            repository: m.repository.clone(),
            permissions: m.permissions.clone(),
            command: m.command.join(" "),
            source: dir.to_path_buf(),
            root: dir.to_path_buf(),
            staging: None,
            installed_version: None,
            quarantined: is_quarantined_tree(dir),
            files,
            bytes,
            folder: Some(dir.to_path_buf()),
        }
    }
}

/// `path` looks like a plugin archive (by extension).
pub fn is_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ARCHIVE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Validate what the user picked and stage it (see the module docs).
/// `staging` is a new folder (inside the plugins folder) for archives.
pub fn prepare(src: &Path, staging: &Path) -> Result<PluginReview, String> {
    if src.is_dir() {
        let m = Manifest::load(src).map_err(|e| e.message)?;
        let mut r = PluginReview::of_folder(src, &m);
        r.folder = None;
        return Ok(r);
    }
    if !is_archive(src) {
        return Err(
            "choose a plugin folder (with a plugin.toml) or a .zip or .mantaplugin file".into(),
        );
    }
    let _ = std::fs::remove_dir_all(staging);
    if let Err(e) = unpack(src, staging) {
        let _ = std::fs::remove_dir_all(staging);
        return Err(e);
    }
    let result = (|| {
        let root = plugin_root(staging)
            .ok_or("the archive has no plugin.toml (at the top or in one folder)")?;
        let m = Manifest::load(&root).map_err(|e| {
            // Report the path inside the archive, not the staging folder.
            let inner = e
                .path
                .strip_prefix(staging)
                .unwrap_or(&e.path)
                .display()
                .to_string();
            format!("{inner}: {}", e.message)
        })?;
        let mut r = PluginReview::of_folder(&root, &m);
        r.source = src.to_path_buf();
        r.staging = Some(staging.to_path_buf());
        r.folder = None;
        r.quarantined |= quarantine_of(src).is_some();
        Ok(r)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(staging);
    }
    result
}

/// The folder in `dir` that holds `plugin.toml`: `dir` itself, or its only
/// sub-folder (as zipping a folder in the Finder makes it). `__MACOSX` and
/// dot files are ignored.
pub fn plugin_root(dir: &Path) -> Option<PathBuf> {
    if dir.join(MANIFEST_FILE).is_file() {
        return Some(dir.to_path_buf());
    }
    let subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            !n.starts_with('.') && n != "__MACOSX"
        })
        .map(|e| e.path())
        .collect();
    match subs.as_slice() {
        [one] if one.is_dir() && one.join(MANIFEST_FILE).is_file() => Some(one.clone()),
        _ => None,
    }
}

/// Unpack a zip archive into `dest`, safely: no absolute paths, no `..`,
/// no symlinks, at most [`MAX_ARCHIVE_ENTRIES`] entries and
/// [`MAX_UNPACKED_BYTES`]. Unix permissions (the executable bit) are kept;
/// a quarantined archive passes its quarantine value on to every file.
pub fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("can't open the archive: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a zip archive: {e}"))?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "the archive has more than {MAX_ARCHIVE_ENTRIES} entries"
        ));
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let quarantine = quarantine_of(archive);
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("damaged archive: {e}"))?;
        let name = entry
            .name()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| format!("entry {i}"));
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("the archive contains an unsafe path: {name}"));
        };
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(format!("the archive contains an unsafe path: {name}"));
        }
        if rel
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == "__MACOSX")
        {
            continue;
        }
        let mode = entry.unix_mode();
        if mode.is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(format!(
                "the archive contains a symbolic link, which plugins can't use: {name}"
            ));
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| format!("can't write {name}: {e}"))?;
        // Count what is really written, not what the header claims.
        let budget = MAX_UNPACKED_BYTES.saturating_sub(total);
        let n = std::io::copy(&mut (&mut entry).take(budget + 1), &mut f)
            .map_err(|e| format!("can't unpack {name}: {e}"))?;
        total += n;
        if total > MAX_UNPACKED_BYTES {
            return Err(format!(
                "the archive unpacks to more than {} MB",
                MAX_UNPACKED_BYTES / 1024 / 1024
            ));
        }
        drop(f);
        #[cfg(unix)]
        if let Some(m) = mode {
            use std::os::unix::fs::PermissionsExt;
            // Keep rwx bits only (no setuid/setgid/sticky).
            let _ = std::fs::set_permissions(
                &out,
                std::fs::Permissions::from_mode((m & 0o777) | 0o600),
            );
        }
        if let Some(q) = &quarantine {
            set_quarantine(&out, q);
        }
    }
    Ok(())
}

/// Make the plugin's program executable when it is a file inside the
/// plugin folder (`command[0]` with a `/`, like `./run.sh` or
/// `bin/plugin`) that lost its executable bit, e.g. in a zip made on
/// another system. Only that one file is changed.
pub fn make_command_executable(dir: &Path, m: &Manifest) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Some(prog) = m
            .command
            .first()
            .filter(|p| p.contains('/') && !p.starts_with('/'))
        else {
            return;
        };
        let path = dir.join(prog);
        let inside = std::fs::canonicalize(&path)
            .ok()
            .zip(std::fs::canonicalize(dir).ok())
            .is_some_and(|(p, d)| p.starts_with(d));
        if !inside || !path.is_file() {
            return;
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            let mode = meta.permissions().mode();
            if mode & 0o111 == 0 {
                let _ =
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode | 0o111));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (dir, m);
}

/// Number of files and their size below `dir` (symlinks not followed).
fn tree_size(dir: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    walk(dir, 0, &mut |_, meta| {
        if meta.is_file() {
            files += 1;
            bytes += meta.len();
        }
        true
    });
    (files, bytes)
}

/// Visit the entries below `dir` (at most 20 000, 32 levels); stop when
/// `f` returns false.
fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path, &std::fs::Metadata) -> bool) -> bool {
    if depth > 32 {
        return true;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return true;
    };
    for (n, e) in rd.flatten().enumerate() {
        if n > 20_000 {
            return false;
        }
        let p = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if !f(&p, &meta) {
            return false;
        }
        if meta.is_dir() && !walk(&p, depth + 1, f) {
            return false;
        }
    }
    true
}

/// Some file or folder in `dir` (or `dir` itself) is quarantined.
pub fn is_quarantined_tree(dir: &Path) -> bool {
    if quarantine_of(dir).is_some() {
        return true;
    }
    let mut found = false;
    walk(dir, 0, &mut |p, _| {
        found = quarantine_of(p).is_some();
        !found
    });
    found
}

/// Remove the quarantine flag from `dir` and everything in it. Returns how
/// many entries had it. Call only after the user confirmed.
pub fn clear_quarantine(dir: &Path) -> usize {
    let mut n = usize::from(remove_quarantine(dir));
    walk(dir, 0, &mut |p, _| {
        n += usize::from(remove_quarantine(p));
        true
    });
    n
}

#[cfg(target_os = "macos")]
fn c_path(p: &Path) -> Option<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(p.as_os_str().as_bytes()).ok()
}

/// The `com.apple.quarantine` value of `path`, if it has one.
pub fn quarantine_of(path: &Path) -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let p = c_path(path)?;
        let name = std::ffi::CString::new(QUARANTINE).ok()?;
        let mut buf = vec![0u8; 512];
        // SAFETY: valid NUL-terminated strings and a buffer of the given size.
        let n = unsafe {
            libc::getxattr(
                p.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        if n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, QUARANTINE);
        None
    }
}

/// Give `path` the quarantine value `value` (best effort).
pub fn set_quarantine(path: &Path, value: &[u8]) {
    #[cfg(target_os = "macos")]
    {
        let (Some(p), Ok(name)) = (c_path(path), std::ffi::CString::new(QUARANTINE)) else {
            return;
        };
        // SAFETY: valid NUL-terminated strings; `value` outlives the call.
        unsafe {
            libc::setxattr(
                p.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                libc::XATTR_NOFOLLOW,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (path, value);
}

fn remove_quarantine(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let (Some(p), Ok(name)) = (c_path(path), std::ffi::CString::new(QUARANTINE)) else {
            return false;
        };
        // SAFETY: valid NUL-terminated strings.
        unsafe { libc::removexattr(p.as_ptr(), name.as_ptr(), libc::XATTR_NOFOLLOW) == 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MANIFEST: &str = "id = \"zipped\"\nname = \"Zipped\"\nversion = \"1.2.0\"\nauthor = \"Anna\"\napi_version = 1\n\
        command = [\"./run.sh\"]\npermissions = [\"read_files\", \"network\"]\nlicense = \"MIT\"\n\
        repository = \"https://example.org/zipped\"\n";

    fn zip_file(path: &Path, entries: &[(&str, &[u8], u32)]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (name, data, mode) in entries {
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn unpacks_an_archive_with_one_folder_and_reviews_it() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("zipped-1.2.0.mantaplugin");
        zip_file(
            &archive,
            &[
                ("zipped/plugin.toml", MANIFEST.as_bytes(), 0o644),
                ("zipped/run.sh", b"#!/bin/sh\ncat\n", 0o755),
                ("__MACOSX/zipped/._run.sh", b"junk", 0o644),
            ],
        );
        set_quarantine(&archive, b"0083;00000000;Safari;");
        let staging = tmp.path().join(".review-1");
        let r = prepare(&archive, &staging).unwrap();
        assert_eq!(
            (r.id.as_str(), r.version.as_str(), r.author.as_str()),
            ("zipped", "1.2.0", "Anna")
        );
        assert_eq!(r.permissions, ["read_files", "network"]);
        assert_eq!(r.license.as_deref(), Some("MIT"));
        assert_eq!(r.root, staging.join("zipped"));
        assert_eq!(r.staging.as_deref(), Some(staging.as_path()));
        assert_eq!(r.files, 2);
        assert!(!staging.join("__MACOSX").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(staging.join("zipped/run.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executable bit kept");
        }
        if cfg!(target_os = "macos") && quarantine_of(&archive).is_some() {
            assert!(r.quarantined);
            assert!(
                quarantine_of(&staging.join("zipped/run.sh")).is_some(),
                "quarantine passed on"
            );
            assert_eq!(clear_quarantine(&staging), 2, "plugin.toml and run.sh");
            assert!(!is_quarantined_tree(&staging));
        }
    }

    #[test]
    fn refuses_unsafe_and_invalid_archives() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("evil.zip");
        zip_file(
            &a,
            &[
                ("../evil.sh", b"x", 0o755),
                ("plugin.toml", MANIFEST.as_bytes(), 0o644),
            ],
        );
        let e = prepare(&a, &tmp.path().join(".review-a")).unwrap_err();
        assert!(e.contains("unsafe path"), "{e}");
        assert!(!tmp.path().join(".review-a").exists(), "staging removed");
        assert!(!tmp.path().join("evil.sh").exists());

        let b = tmp.path().join("none.zip");
        zip_file(&b, &[("readme.txt", b"hi", 0o644)]);
        let e = prepare(&b, &tmp.path().join(".review-b")).unwrap_err();
        assert!(e.contains("no plugin.toml"), "{e}");

        let c = tmp.path().join("bad.zip");
        zip_file(&c, &[("p/plugin.toml", b"id = \"Bad Id\"", 0o644)]);
        let e = prepare(&c, &tmp.path().join(".review-c")).unwrap_err();
        assert!(e.starts_with("p/plugin.toml"), "{e}");

        let d = tmp.path().join("not.zip");
        std::fs::write(&d, b"not a zip").unwrap();
        assert!(prepare(&d, &tmp.path().join(".review-d"))
            .unwrap_err()
            .contains("not a zip"));
        let e = prepare(&tmp.path().join("x.txt"), &tmp.path().join(".review-e")).unwrap_err();
        assert!(e.contains(".mantaplugin"), "{e}");
    }

    #[test]
    fn makes_only_the_plugin_program_executable() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("run.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(tmp.path().join("other.sh"), "#!/bin/sh\n").unwrap();
        let m = Manifest::parse(MANIFEST, Path::new("plugin.toml")).unwrap();
        make_command_executable(tmp.path(), &m);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |n: &str| {
                std::fs::metadata(tmp.path().join(n))
                    .unwrap()
                    .permissions()
                    .mode()
            };
            assert_ne!(mode("run.sh") & 0o100, 0);
            assert_eq!(mode("other.sh") & 0o111, 0);
        }
    }
}
