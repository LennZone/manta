//! Local file system backend on top of `std::fs`.
//!
//! A [`VfsPath`] maps 1:1 to an absolute OS path.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use manta_core::{Entry, EntryKind, Protocol, Vfs, VfsError, VfsPath, VfsResult};

/// The local file system.
pub struct LocalFs;

impl LocalFs {
    pub fn new() -> Self {
        LocalFs
    }
}

impl Default for LocalFs {
    fn default() -> Self {
        Self::new()
    }
}

fn os_path(path: &VfsPath) -> PathBuf {
    PathBuf::from(path.as_str())
}

/// Map an `io::Error` for `path` to the matching [`VfsError`] variant.
pub(crate) fn map_io(err: io::Error, path: &VfsPath) -> VfsError {
    let p = path.to_string();
    match err.kind() {
        io::ErrorKind::NotFound => VfsError::NotFound(p),
        io::ErrorKind::PermissionDenied => VfsError::PermissionDenied(p),
        io::ErrorKind::AlreadyExists => VfsError::AlreadyExists(p),
        io::ErrorKind::DirectoryNotEmpty => VfsError::DirectoryNotEmpty(p),
        io::ErrorKind::NotADirectory => VfsError::NotADirectory(p),
        _ => VfsError::Io(err),
    }
}

fn kind_of(meta: &Metadata) -> EntryKind {
    let ft = meta.file_type();
    if ft.is_dir() {
        EntryKind::Directory
    } else if ft.is_file() {
        EntryKind::File
    } else if ft.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    }
}

#[cfg(target_os = "macos")]
fn has_hidden_flag(meta: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const UF_HIDDEN: u32 = 0x8000;
    meta.st_flags() & UF_HIDDEN != 0
}

#[cfg(not(target_os = "macos"))]
fn has_hidden_flag(_meta: &Metadata) -> bool {
    false
}

/// Build an [`Entry`] for `path` from its `lstat` metadata. Symlinks are
/// resolved: a link to a directory gets `kind = Directory` (so it can be
/// navigated into); size/mtime/permissions come from the target if it exists.
fn entry_from_lstat(path: &VfsPath, name: String, lmeta: &Metadata) -> Entry {
    let os = os_path(path);
    let mut kind = kind_of(lmeta);
    let mut link_target = None;
    let mut meta_for_attrs: Option<Metadata> = None;

    if lmeta.file_type().is_symlink() {
        link_target = fs::read_link(&os)
            .ok()
            .map(|t| t.to_string_lossy().into_owned());
        if let Ok(target_meta) = fs::metadata(&os) {
            if target_meta.is_dir() {
                kind = EntryKind::Directory;
            }
            meta_for_attrs = Some(target_meta);
        }
    }
    let meta = meta_for_attrs.as_ref().unwrap_or(lmeta);

    let hidden = name.starts_with('.') || has_hidden_flag(lmeta);
    Entry {
        size: if kind == EntryKind::Directory {
            0
        } else {
            meta.len()
        },
        modified: meta.modified().ok().map(DateTime::<Utc>::from),
        permissions: Some(meta.permissions().mode() & 0o7777),
        link_target,
        hidden,
        kind,
        name,
        path: path.clone(),
    }
}

/// Best-effort entry for something we could not stat.
fn unknown_entry(path: VfsPath, name: String) -> Entry {
    Entry {
        hidden: name.starts_with('.'),
        name,
        path,
        kind: EntryKind::Other,
        size: 0,
        modified: None,
        permissions: None,
        link_target: None,
    }
}

impl Vfs for LocalFs {
    fn label(&self) -> String {
        "Local".into()
    }

    fn protocol(&self) -> Protocol {
        Protocol::Local
    }

    fn home(&self) -> VfsResult<VfsPath> {
        let home = dirs::home_dir()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .ok_or_else(|| VfsError::Other("cannot determine home directory".into()))?;
        Ok(VfsPath::new(&home.to_string_lossy()))
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let os = os_path(dir);
        let meta = fs::metadata(&os).map_err(|e| map_io(e, dir))?;
        if !meta.is_dir() {
            return Err(VfsError::NotADirectory(dir.to_string()));
        }
        let rd = fs::read_dir(&os).map_err(|e| map_io(e, dir))?;
        let mut out = Vec::new();
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(e) => {
                    log::warn!("skipping unreadable entry in {dir}: {e}");
                    continue;
                }
            };
            let name = item.file_name().to_string_lossy().into_owned();
            let path = dir.join(&name);
            match fs::symlink_metadata(item.path()) {
                Ok(m) => out.push(entry_from_lstat(&path, name, &m)),
                Err(e) => {
                    log::debug!("cannot stat {path}: {e}");
                    out.push(unknown_entry(path, name));
                }
            }
        }
        Ok(out)
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let m = fs::symlink_metadata(os_path(path)).map_err(|e| map_io(e, path))?;
        let name = if path.is_root() {
            "/".to_string()
        } else {
            path.file_name().to_string()
        };
        Ok(entry_from_lstat(path, name, &m))
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        fs::create_dir(os_path(path)).map_err(|e| map_io(e, path))
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        let os = os_path(path);
        let m = fs::symlink_metadata(&os).map_err(|e| map_io(e, path))?;
        if m.is_dir() {
            return Err(VfsError::Other(format!("is a directory: {path}")));
        }
        fs::remove_file(&os).map_err(|e| map_io(e, path))
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let os = os_path(path);
        let m = fs::symlink_metadata(&os).map_err(|e| map_io(e, path))?;
        if !m.is_dir() {
            return Err(VfsError::NotADirectory(path.to_string()));
        }
        fs::remove_dir(&os).map_err(|e| map_io(e, path))
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        fs::rename(os_path(from), os_path(to)).map_err(|e| {
            // Attribute "not found" to the source, everything else to the target.
            if e.kind() == io::ErrorKind::NotFound && !Path::new(from.as_str()).exists() {
                map_io(e, from)
            } else {
                map_io(e, to)
            }
        })
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let f = File::open(os_path(path)).map_err(|e| map_io(e, path))?;
        let m = f.metadata().map_err(|e| map_io(e, path))?;
        if m.is_dir() {
            return Err(VfsError::Other(format!("is a directory: {path}")));
        }
        Ok(Box::new(f))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let f = File::create(os_path(path)).map_err(|e| map_io(e, path))?;
        Ok(Box::new(f))
    }

    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        use std::io::{Seek, SeekFrom};
        let mut f = File::open(os_path(path)).map_err(|e| map_io(e, path))?;
        let m = f.metadata().map_err(|e| map_io(e, path))?;
        if m.is_dir() {
            return Err(VfsError::Other(format!("is a directory: {path}")));
        }
        if offset > m.len() {
            return Err(VfsError::Other(format!(
                "{path}: file is shorter ({} bytes) than the resume offset {offset}",
                m.len()
            )));
        }
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| map_io(e, path))?;
        Ok(Box::new(f))
    }

    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let f = File::options()
            .create(true)
            .append(true)
            .open(os_path(path))
            .map_err(|e| map_io(e, path))?;
        Ok(Box::new(f))
    }

    fn supports_resume(&self) -> manta_core::ResumeCaps {
        manta_core::ResumeCaps {
            read_at: true,
            append: true,
            upload_session: false,
        }
    }

    fn set_permissions(&self, path: &VfsPath, mode: u32) -> VfsResult<()> {
        fs::set_permissions(os_path(path), fs::Permissions::from_mode(mode & 0o7777))
            .map_err(|e| map_io(e, path))
    }

    fn set_modified(&self, path: &VfsPath, mtime: DateTime<Utc>) -> VfsResult<()> {
        let file = File::options()
            .write(true)
            .open(os_path(path))
            .or_else(|_| File::open(os_path(path)))
            .map_err(|e| map_io(e, path))?;
        file.set_modified(mtime.into()).map_err(|e| map_io(e, path))
    }

    fn local_path(&self, path: &VfsPath) -> Option<PathBuf> {
        Some(os_path(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, VfsPath, LocalFs) {
        let td = tempfile::tempdir().expect("tempdir");
        // Canonicalize: on macOS the temp dir lives behind the /var -> /private/var symlink.
        let canon = fs::canonicalize(td.path()).expect("canonicalize");
        let root = VfsPath::new(&canon.to_string_lossy());
        (td, root, LocalFs::new())
    }

    fn write(fs_: &LocalFs, p: &VfsPath, data: &[u8]) {
        let mut w = fs_.open_write(p).expect("open_write");
        w.write_all(data).expect("write");
        w.flush().expect("flush");
    }

    fn read(fs_: &LocalFs, p: &VfsPath) -> Vec<u8> {
        let mut r = fs_.open_read(p).expect("open_read");
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).expect("read");
        buf
    }

    #[test]
    fn home_and_label() {
        let l = LocalFs::new();
        assert_eq!(l.label(), "Local");
        assert_eq!(l.protocol(), Protocol::Local);
        assert!(!l.home().expect("home").is_root() || std::env::var("HOME").as_deref() == Ok("/"));
    }

    #[test]
    fn read_write_roundtrip_and_stat() {
        let (_td, root, l) = setup();
        let p = root.join("hello.txt");
        write(&l, &p, b"hello world");
        assert_eq!(read(&l, &p), b"hello world");
        let e = l.stat(&p).expect("stat");
        assert_eq!(e.name, "hello.txt");
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.size, 11);
        assert!(!e.hidden);
        assert!(e.modified.is_some());
        assert!(e.permissions.is_some());
        assert_eq!(e.path, p);
        assert_eq!(l.local_path(&p), Some(PathBuf::from(p.as_str())));
        // Truncate on rewrite.
        write(&l, &p, b"x");
        assert_eq!(read(&l, &p), b"x");
    }

    #[test]
    fn list_entries() {
        let (_td, root, l) = setup();
        write(&l, &root.join("a.txt"), b"abc");
        write(&l, &root.join(".hidden"), b"");
        l.create_dir(&root.join("sub")).expect("mkdir");
        let mut entries = l.list(&root).expect("list");
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec![".hidden", "a.txt", "sub"]);
        assert!(entries[0].hidden);
        assert_eq!(entries[1].size, 3);
        assert!(entries[2].is_dir());
        assert_eq!(entries[2].path, root.join("sub"));
    }

    #[test]
    fn symlinks() {
        let (_td, root, l) = setup();
        l.create_dir(&root.join("dir")).expect("mkdir");
        write(&l, &root.join("file"), b"12345");
        std::os::unix::fs::symlink(root.join("dir").as_str(), root.join("ldir").as_str())
            .expect("symlink");
        std::os::unix::fs::symlink("file", root.join("lfile").as_str()).expect("symlink");
        std::os::unix::fs::symlink("missing", root.join("broken").as_str()).expect("symlink");

        let ldir = l.stat(&root.join("ldir")).expect("stat");
        assert_eq!(ldir.kind, EntryKind::Directory);
        assert_eq!(ldir.link_target.as_deref(), Some(root.join("dir").as_str()));

        let lfile = l.stat(&root.join("lfile")).expect("stat");
        assert_eq!(lfile.kind, EntryKind::Symlink);
        assert_eq!(lfile.link_target.as_deref(), Some("file"));
        assert_eq!(lfile.size, 5);

        let broken = l.stat(&root.join("broken")).expect("stat");
        assert_eq!(broken.kind, EntryKind::Symlink);

        // Listing through a symlinked dir works; removing the link keeps the target.
        assert!(l.list(&root.join("ldir")).expect("list").is_empty());
        l.remove_file(&root.join("ldir")).expect("rm link");
        assert!(l.stat(&root.join("dir")).is_ok());
        assert_eq!(l.list(&root).expect("list").len(), 4);
    }

    #[test]
    fn create_remove_dir() {
        let (_td, root, l) = setup();
        let d = root.join("d");
        l.create_dir(&d).expect("mkdir");
        assert!(matches!(l.create_dir(&d), Err(VfsError::AlreadyExists(_))));
        write(&l, &d.join("f"), b"x");
        assert!(matches!(
            l.remove_dir(&d),
            Err(VfsError::DirectoryNotEmpty(_))
        ));
        assert!(matches!(
            l.remove_dir(&d.join("f")),
            Err(VfsError::NotADirectory(_))
        ));
        assert!(l.remove_file(&d).is_err());
        l.remove_file(&d.join("f")).expect("rm");
        l.remove_dir(&d).expect("rmdir");
        assert!(matches!(l.stat(&d), Err(VfsError::NotFound(_))));
    }

    #[test]
    fn rename_entries() {
        let (_td, root, l) = setup();
        let a = root.join("a");
        let b = root.join("b");
        write(&l, &a, b"data");
        l.rename(&a, &b).expect("rename");
        assert!(matches!(l.stat(&a), Err(VfsError::NotFound(_))));
        assert_eq!(read(&l, &b), b"data");
        match l.rename(&a, &root.join("c")) {
            Err(VfsError::NotFound(p)) => assert_eq!(p, a.to_string()),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn resume_read_at_and_append() {
        let (_td, root, l) = setup();
        let a = root.join("a");
        write(&l, &a, b"0123456789");
        let mut out = Vec::new();
        l.open_read_at(&a, 4)
            .expect("read_at")
            .read_to_end(&mut out)
            .expect("read");
        assert_eq!(out, b"456789");
        assert!(l.open_read_at(&a, 11).is_err());
        {
            let mut w = l.open_append(&a).expect("append");
            w.write_all(b"abc").expect("write");
        }
        assert_eq!(read(&l, &a), b"0123456789abc");
        let caps = l.supports_resume();
        assert!(caps.read_at && caps.append && !caps.upload_session);
    }

    #[test]
    fn error_mapping() {
        let (_td, root, l) = setup();
        let missing = root.join("nope");
        assert!(matches!(l.stat(&missing), Err(VfsError::NotFound(_))));
        assert!(matches!(l.list(&missing), Err(VfsError::NotFound(_))));
        assert!(matches!(l.open_read(&missing), Err(VfsError::NotFound(_))));
        assert!(matches!(
            l.remove_file(&missing),
            Err(VfsError::NotFound(_))
        ));
        assert!(matches!(
            l.open_write(&root.join("nope/x")),
            Err(VfsError::NotFound(_))
        ));
        let f = root.join("f");
        write(&l, &f, b"");
        assert!(matches!(l.list(&f), Err(VfsError::NotADirectory(_))));
        assert!(l.open_read(&root).is_err());
    }

    #[test]
    fn permissions() {
        let (_td, root, l) = setup();
        let f = root.join("f");
        write(&l, &f, b"secret");
        l.set_permissions(&f, 0o600).expect("chmod");
        assert_eq!(l.stat(&f).expect("stat").permissions, Some(0o600));

        // Unreadable directory -> PermissionDenied (skip when running as root).
        let d = root.join("locked");
        l.create_dir(&d).expect("mkdir");
        l.set_permissions(&d, 0o000).expect("chmod");
        let res = l.list(&d);
        l.set_permissions(&d, 0o755).expect("chmod back");
        let is_root = std::env::var("USER").as_deref() == Ok("root");
        if !is_root {
            assert!(matches!(res, Err(VfsError::PermissionDenied(_))), "{res:?}");
        }
    }
}
