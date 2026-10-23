//! Upload release files to the update web space (see `docs/updates.md`).
//!
//! Used by `scripts/publish-update.sh` through the `manta-update-upload`
//! binary. The transfer goes through Manta's own FTP backend
//! (`manta_vfs::connect`), so the release path and the app's FTPS panes share
//! one implementation: TLS login, data connections that resume the control
//! connection's TLS session, the data channel policy (clear data connections
//! when the network resets encrypted ones) and the PASV/EPSV fallbacks.
//!
//! Each file goes to `NAME.uploading` first and is renamed afterwards (the
//! target is deleted first if the server refuses to rename over it), so
//! clients never fetch half a file. Sizes are checked after the upload and
//! after the rename. Files are uploaded in the order given; the publish
//! script passes the ZIP, then `update.json.sig`, then `update.json`.
//!
//! [`upload_tree`] uploads a local folder recursively (`--tree`, e.g. a static
//! website) with the same per-file rules; it only writes the paths that exist
//! locally and never deletes anything else.
//! [`list_lines`] lists a remote folder for `--list`.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use manta_core::{EntryKind, Vfs, VfsError, VfsPath};

/// Suffix of the temporary upload name.
pub const PARTIAL_SUFFIX: &str = ".uploading";

/// Where to upload: `ftp://[user@]host[:port]/dir/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    /// Remote folder (absolute, as the FTP account sees it).
    pub dir: VfsPath,
}

impl Destination {
    pub fn parse(url: &str) -> Result<Destination, String> {
        let rest = url
            .strip_prefix("ftp://")
            .or_else(|| url.strip_prefix("ftps://"))
            .ok_or_else(|| {
                format!("{url}: expected ftp://host/dir/ (explicit TLS is always used)")
            })?;
        let (authority, dir) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (user, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u.to_string()).filter(|u| !u.is_empty()), h),
            None => (None, authority),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') || h.ends_with(']') => (
                h,
                p.parse::<u16>()
                    .map_err(|_| format!("{url}: invalid port {p}"))?,
            ),
            _ => (hostport, 21),
        };
        if host.is_empty() {
            return Err(format!("{url}: no host"));
        }
        Ok(Destination {
            host: host.to_string(),
            port,
            user,
            dir: VfsPath::new(dir),
        })
    }
}

/// Create `dir` and its parents if they're missing (MKD; "exists" is fine).
///
/// FTP servers report a missing folder in many ways (`550 cannot be listed`,
/// `550 no such file`), so any failed stat leads to a MKD, and a failed MKD
/// is accepted when the folder is there afterwards.
pub fn ensure_dir(fs: &dyn Vfs, dir: &VfsPath) -> Result<(), String> {
    if dir.is_root() {
        return Ok(());
    }
    match fs.stat(dir) {
        Ok(e) if e.is_dir() => return Ok(()),
        Ok(_) => return Err(format!("{dir} exists but is not a folder")),
        Err(_) => {}
    }
    if let Some(parent) = dir.parent() {
        ensure_dir(fs, &parent)?;
    }
    match fs.create_dir(dir) {
        Ok(()) | Err(VfsError::AlreadyExists(_)) => Ok(()),
        Err(e) => match fs.stat(dir) {
            Ok(entry) if entry.is_dir() => Ok(()),
            _ => Err(format!("cannot create {dir}: {e}")),
        },
    }
}

fn remote_size(fs: &dyn Vfs, path: &VfsPath) -> Result<u64, String> {
    fs.stat(path)
        .map(|e| e.size)
        .map_err(|e| format!("{path}: {e}"))
}

/// Upload one file into `dir` (via `NAME.uploading` + rename) and verify
/// its size. Returns the number of bytes.
pub fn upload_file(fs: &dyn Vfs, dir: &VfsPath, local: &Path) -> Result<u64, String> {
    let name = local
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: not a file name", local.display()))?;
    upload_file_as(fs, dir, local, name)
}

/// [`upload_file`] under the remote name `name`.
pub fn upload_file_as(
    fs: &dyn Vfs,
    dir: &VfsPath,
    local: &Path,
    name: &str,
) -> Result<u64, String> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(format!("{name:?}: not a file name"));
    }
    let size = std::fs::metadata(local)
        .map_err(|e| format!("{}: {e}", local.display()))?
        .len();
    let partial = dir.join(&format!("{name}{PARTIAL_SUFFIX}"));
    let target = dir.join(name);

    let mut src = File::open(local).map_err(|e| format!("{}: {e}", local.display()))?;
    let mut dst = fs
        .open_write(&partial)
        .map_err(|e| format!("{partial}: {e}"))?;
    copy(&mut src, &mut *dst).map_err(|e| format!("uploading {name}: {e}"))?;
    // flush() completes the transfer and reports the server's verdict.
    dst.flush().map_err(|e| format!("uploading {name}: {e}"))?;
    drop(dst);
    let uploaded = remote_size(fs, &partial)?;
    if uploaded != size {
        return Err(format!(
            "{partial}: uploaded {uploaded} bytes, expected {size}"
        ));
    }

    if let Err(first) = fs.rename(&partial, &target) {
        // Some servers refuse to rename over an existing file.
        match fs.remove_file(&target) {
            Ok(()) | Err(VfsError::NotFound(_)) => {}
            Err(e) => return Err(format!("cannot replace {target} ({first}): {e}")),
        }
        fs.rename(&partial, &target)
            .map_err(|e| format!("cannot rename {partial} to {target}: {e}"))?;
    }
    let live = remote_size(fs, &target)?;
    if live != size {
        return Err(format!(
            "{target}: {live} bytes on the server, expected {size}"
        ));
    }
    Ok(size)
}

fn copy(src: &mut dyn Read, dst: &mut dyn Write) -> io::Result<u64> {
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        dst.write_all(&buf[..n])?;
        total += n as u64;
    }
}

/// Upload `files` into `dir` in the given order; `say` gets one line per
/// file. Stops at the first error.
pub fn upload_all(
    fs: &dyn Vfs,
    dir: &VfsPath,
    files: &[PathBuf],
    say: &mut dyn FnMut(String),
) -> Result<(), String> {
    for f in files {
        if !f.is_file() {
            return Err(format!("{}: no such file", f.display()));
        }
    }
    ensure_dir(fs, dir)?;
    for f in files {
        let start = Instant::now();
        let bytes = upload_file(fs, dir, f)?;
        let secs = start.elapsed().as_secs_f64();
        say(format!(
            "uploaded {} ({bytes} bytes, {secs:.1} s)",
            f.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- folders

/// One file of a local folder tree: its `/`-separated path relative to the
/// folder and its local path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    pub rel: String,
    pub local: PathBuf,
    pub size: u64,
}

/// True if `rel` (a relative `/` path) matches an exclude pattern: a pattern
/// with a `/` is a relative path (the file or everything below that folder),
/// one without matches that name at any depth.
fn excluded(rel: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|pat| {
        let pat = pat.trim_matches('/');
        if pat.contains('/') {
            rel == pat || rel.starts_with(&format!("{pat}/"))
        } else {
            rel.split('/').any(|c| c == pat)
        }
    })
}

/// The regular files below `root` (symlinks are refused, not followed),
/// minus `excludes`. Order: files in subfolders first, then the top-level
/// files, `.html` pages last, so a page never goes live before the assets it
/// references.
pub fn collect_tree(root: &Path, excludes: &[String]) -> Result<Vec<TreeFile>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        excludes: &[String],
        out: &mut Vec<TreeFile>,
    ) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .collect::<Result<_, _>>()
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let name = e
                .file_name()
                .into_string()
                .map_err(|n| format!("{}: not UTF-8: {n:?}", dir.display()))?;
            let rel_path = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let rel: Vec<String> = rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let rel = rel.join("/");
            if excluded(&rel, excludes) {
                continue;
            }
            if name.ends_with(PARTIAL_SUFFIX) {
                return Err(format!(
                    "{}: local file names must not end in {PARTIAL_SUFFIX}",
                    path.display()
                ));
            }
            let ft = e
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if ft.is_symlink() {
                return Err(format!(
                    "{}: symbolic links are not uploaded (exclude it or copy the file)",
                    path.display()
                ));
            } else if ft.is_dir() {
                walk(root, &path, excludes, out)?;
            } else if ft.is_file() {
                let size = e
                    .metadata()
                    .map_err(|e| format!("{}: {e}", path.display()))?
                    .len();
                out.push(TreeFile {
                    rel,
                    local: path,
                    size,
                });
            }
        }
        Ok(())
    }
    if !root.is_dir() {
        return Err(format!("{}: no such folder", root.display()));
    }
    let mut out = Vec::new();
    walk(root, root, excludes, &mut out)?;
    out.sort_by_key(|f| {
        (
            !f.rel.contains('/'),
            f.rel.ends_with(".html") || f.rel.ends_with(".htm"),
            f.rel.clone(),
        )
    });
    Ok(out)
}

/// Upload `files` (from [`collect_tree`]) below the remote folder `dir`,
/// creating subfolders as needed. Only these paths are written; nothing on
/// the server is deleted except a file being replaced by its new version.
pub fn upload_tree(
    fs: &dyn Vfs,
    dir: &VfsPath,
    files: &[TreeFile],
    say: &mut dyn FnMut(String),
) -> Result<(), String> {
    let mut made = std::collections::HashSet::new();
    for f in files {
        let target = dir.join(&f.rel);
        let parent = target.parent().unwrap_or_else(VfsPath::root);
        if made.insert(parent.clone()) {
            ensure_dir(fs, &parent)?;
        }
        let start = Instant::now();
        let bytes = upload_file_as(fs, &parent, &f.local, target.file_name())?;
        let secs = start.elapsed().as_secs_f64();
        say(format!("uploaded {} ({bytes} bytes, {secs:.1} s)", f.rel));
    }
    Ok(())
}

// ---------------------------------------------------------------- listing

/// `ls -l`-like lines for the remote folder `dir` (folders first).
pub fn list_lines(fs: &dyn Vfs, dir: &VfsPath) -> Result<Vec<String>, String> {
    let mut entries = fs.list(dir).map_err(|e| format!("{dir}: {e}"))?;
    entries.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries
        .iter()
        .map(|e| {
            let kind = match e.kind {
                EntryKind::Directory => 'd',
                EntryKind::Symlink => 'l',
                EntryKind::File => '-',
                EntryKind::Other => '?',
            };
            let when = e
                .modified
                .map(|m| m.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "-".repeat(16));
            let name = if e.is_dir() {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let link = e
                .link_target
                .as_ref()
                .map(|t| format!(" -> {t}"))
                .unwrap_or_default();
            format!("{kind} {:>12}  {when}  {name}{link}", e.size)
        })
        .collect())
}

#[cfg(test)]
mod tests;
