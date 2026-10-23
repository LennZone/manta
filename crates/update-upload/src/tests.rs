use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use manta_core::{Entry, Protocol, Vfs, VfsError, VfsPath, VfsResult};
use manta_vfs::LocalFs;

use super::*;

/// A local folder that behaves like a picky FTP server.
struct Picky {
    inner: LocalFs,
    /// Refuse to rename over an existing file.
    no_overwrite: bool,
    /// Report this size for every stat (a truncated upload).
    lie_size: Option<u64>,
    /// Report missing paths and existing folders as a vague 550 (like
    /// `550 '/updates' cannot be listed`) instead of NotFound/AlreadyExists.
    vague: bool,
    renames: AtomicUsize,
    deleted: Mutex<Vec<String>>,
}

impl Picky {
    fn new() -> Picky {
        Picky {
            inner: LocalFs::new(),
            no_overwrite: false,
            lie_size: None,
            vague: false,
            renames: AtomicUsize::new(0),
            deleted: Mutex::default(),
        }
    }
}

impl Vfs for Picky {
    fn label(&self) -> String {
        "picky".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Ftps
    }
    fn home(&self) -> VfsResult<VfsPath> {
        self.inner.home()
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.inner.list(dir)
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let mut e = self.inner.stat(path).map_err(|e| match e {
            VfsError::NotFound(p) if self.vague => {
                VfsError::PermissionDenied(format!("550 '{p}' cannot be listed"))
            }
            e => e,
        })?;
        if let (Some(n), false) = (self.lie_size, e.is_dir()) {
            e.size = n;
        }
        Ok(e)
    }
    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.inner.create_dir(path).map_err(|e| match e {
            VfsError::AlreadyExists(p) if self.vague => {
                VfsError::PermissionDenied(format!("550 {p}: exists"))
            }
            e => e,
        })
    }
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.deleted
            .lock()
            .unwrap()
            .push(path.file_name().to_string());
        self.inner.remove_file(path)
    }
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.inner.remove_dir(path)
    }
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.renames.fetch_add(1, Ordering::SeqCst);
        if self.no_overwrite && self.inner.stat(to).is_ok() {
            return Err(VfsError::AlreadyExists(to.to_string()));
        }
        self.inner.rename(from, to)
    }
    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        self.inner.open_read(path)
    }
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.inner.open_write(path)
    }
}

fn vpath(p: &std::path::Path) -> VfsPath {
    VfsPath::new(p.to_str().unwrap())
}

fn release_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let files = [
        ("Manta-9.9.9.zip", vec![1u8; 300_000]),
        ("update.json.sig", b"sig".to_vec()),
        ("update.json", b"{}".to_vec()),
    ];
    files
        .iter()
        .map(|(n, d)| {
            let p = dir.join(n);
            std::fs::write(&p, d).unwrap();
            p
        })
        .collect()
}

#[test]
fn uploads_in_order_via_partial_names_into_a_new_folder() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let files = release_files(src.path());
    let remote = dst.path().join("manta/releases");
    let fs = Picky::new();
    let mut lines = Vec::new();
    upload_all(&fs, &vpath(&remote), &files, &mut |l| lines.push(l)).unwrap();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("uploaded Manta-9.9.9.zip (300000 bytes"));
    assert!(lines[1].starts_with("uploaded update.json.sig"));
    assert!(lines[2].starts_with("uploaded update.json "));
    for f in &files {
        let name = f.file_name().unwrap();
        assert_eq!(
            std::fs::read(remote.join(name)).unwrap(),
            std::fs::read(f).unwrap()
        );
    }
    let left: Vec<_> = std::fs::read_dir(&remote)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        !left.iter().any(|n| n.ends_with(PARTIAL_SUFFIX)),
        "{left:?}"
    );
    assert!(fs.deleted.lock().unwrap().is_empty());
}

#[test]
fn replaces_the_target_when_the_server_refuses_to_rename_over_it() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let files = release_files(src.path());
    std::fs::write(dst.path().join("update.json"), b"old feed").unwrap();
    std::fs::write(dst.path().join("unrelated.txt"), b"keep me").unwrap();
    let fs = Picky {
        no_overwrite: true,
        ..Picky::new()
    };
    upload_all(&fs, &vpath(dst.path()), &files[2..], &mut |_| {}).unwrap();
    assert_eq!(
        std::fs::read(dst.path().join("update.json")).unwrap(),
        b"{}"
    );
    assert_eq!(
        *fs.deleted.lock().unwrap(),
        ["update.json"],
        "only the target is deleted"
    );
    assert_eq!(
        std::fs::read(dst.path().join("unrelated.txt")).unwrap(),
        b"keep me"
    );
    assert_eq!(fs.renames.load(Ordering::SeqCst), 2);
}

#[test]
fn a_size_mismatch_stops_before_the_rename() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let files = release_files(src.path());
    let fs = Picky {
        lie_size: Some(42),
        ..Picky::new()
    };
    let err = upload_all(&fs, &vpath(dst.path()), &files, &mut |_| {}).unwrap_err();
    assert!(err.contains("uploaded 42 bytes, expected 300000"), "{err}");
    assert_eq!(
        fs.renames.load(Ordering::SeqCst),
        0,
        "a bad upload never replaces the live file"
    );
    assert!(!dst.path().join("Manta-9.9.9.zip").exists());
}

#[test]
fn missing_local_files_fail_before_connecting_anything() {
    let dst = tempfile::tempdir().unwrap();
    let fs = Picky::new();
    let err = upload_all(
        &fs,
        &vpath(dst.path()),
        &[PathBuf::from("/nonexistent/x.zip")],
        &mut |_| {},
    )
    .unwrap_err();
    assert!(err.contains("no such file"));
}

#[test]
fn destination_urls() {
    let d = Destination::parse("ftp://ftp.example.com/").unwrap();
    assert_eq!(
        (d.host.as_str(), d.port, d.user.as_deref(), d.dir.as_str()),
        ("ftp.example.com", 21, None, "/")
    );
    let d = Destination::parse("ftp://me@example.com:2121/www/manta/").unwrap();
    assert_eq!(
        (d.host.as_str(), d.port, d.user.as_deref(), d.dir.as_str()),
        ("example.com", 2121, Some("me"), "/www/manta")
    );
    let d = Destination::parse("ftps://example.com").unwrap();
    assert_eq!(d.dir.as_str(), "/");
    assert!(Destination::parse("sftp://example.com/").is_err());
    assert!(Destination::parse("ftp://example.com:x/").is_err());
    assert!(Destination::parse("ftp:///dir/").is_err());
}

fn site(dir: &std::path::Path) {
    for (rel, data) in [
        ("index.html", "<html>"),
        ("impressum.html", "imp"),
        ("README.md", "readme"),
        ("LICENSE.txt", "lic"),
        (".DS_Store", "x"),
        ("assets/.DS_Store", "x"),
        ("assets/css/style.css", "body{}"),
        ("assets/js/main.js", "js"),
        ("assets/img/a.webp", "img"),
    ] {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
}

fn excludes() -> Vec<String> {
    vec!["README.md".into(), ".DS_Store".into()]
}

#[test]
fn collect_tree_excludes_and_puts_pages_last() {
    let src = tempfile::tempdir().unwrap();
    site(src.path());
    let files = collect_tree(src.path(), &excludes()).unwrap();
    let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(
        rels,
        [
            "assets/css/style.css",
            "assets/img/a.webp",
            "assets/js/main.js",
            "LICENSE.txt",
            "impressum.html",
            "index.html"
        ]
    );
    assert_eq!(files[0].size, 6);
    // A pattern with a slash is a relative path (file or folder).
    let files = collect_tree(
        src.path(),
        &["assets/img".into(), "assets/js/main.js".into()],
    )
    .unwrap();
    assert!(!files
        .iter()
        .any(|f| f.rel.starts_with("assets/img") || f.rel == "assets/js/main.js"));
    assert!(files.iter().any(|f| f.rel == "README.md"));
    assert!(collect_tree(&src.path().join("nope"), &[]).is_err());
}

#[test]
fn collect_tree_refuses_symlinks_and_partial_names() {
    let src = tempfile::tempdir().unwrap();
    site(src.path());
    std::os::unix::fs::symlink("/etc/hosts", src.path().join("assets/hosts")).unwrap();
    let err = collect_tree(src.path(), &[]).unwrap_err();
    assert!(err.contains("symbolic links"), "{err}");
    assert!(collect_tree(src.path(), &["hosts".into()]).is_ok());
    std::fs::write(src.path().join("x.uploading"), b"").unwrap();
    assert!(collect_tree(src.path(), &["hosts".into()])
        .unwrap_err()
        .contains(PARTIAL_SUFFIX));
}

#[test]
fn upload_tree_creates_folders_and_leaves_other_remote_files_alone() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    site(src.path());
    // The live server already has the update feed and an old page.
    std::fs::create_dir_all(dst.path().join("updates")).unwrap();
    std::fs::write(dst.path().join("updates/update.json"), b"feed").unwrap();
    std::fs::write(dst.path().join("index.html"), b"old").unwrap();
    std::fs::write(dst.path().join("index.htm"), b"placeholder").unwrap();
    let fs = Picky {
        no_overwrite: true,
        ..Picky::new()
    };
    let files = collect_tree(src.path(), &excludes()).unwrap();
    let mut lines = Vec::new();
    upload_tree(&fs, &vpath(dst.path()), &files, &mut |l| lines.push(l)).unwrap();
    assert_eq!(lines.len(), 6);
    assert!(lines[5].starts_with("uploaded index.html "), "{lines:?}");
    for f in &files {
        assert_eq!(
            std::fs::read(dst.path().join(&f.rel)).unwrap(),
            std::fs::read(&f.local).unwrap(),
            "{}",
            f.rel
        );
    }
    assert_eq!(
        std::fs::read(dst.path().join("updates/update.json")).unwrap(),
        b"feed"
    );
    assert_eq!(
        std::fs::read(dst.path().join("index.htm")).unwrap(),
        b"placeholder"
    );
    assert!(!dst.path().join("README.md").exists());
    assert!(!dst.path().join(".DS_Store").exists());
    assert_eq!(
        *fs.deleted.lock().unwrap(),
        ["index.html"],
        "only the replaced page is deleted"
    );
}

#[test]
fn ensure_dir_creates_parents_and_accepts_existing_folders() {
    let dst = tempfile::tempdir().unwrap();
    let fs = Picky::new();
    ensure_dir(&fs, &vpath(&dst.path().join("updates"))).unwrap();
    ensure_dir(&fs, &vpath(&dst.path().join("updates"))).unwrap();
    ensure_dir(&fs, &vpath(&dst.path().join("download/old/x"))).unwrap();
    assert!(dst.path().join("download/old/x").is_dir());
    std::fs::write(dst.path().join("file"), b"").unwrap();
    assert!(ensure_dir(&fs, &vpath(&dst.path().join("file")))
        .unwrap_err()
        .contains("not a folder"));
}

#[test]
fn ensure_dir_copes_with_vague_ftp_errors() {
    let dst = tempfile::tempdir().unwrap();
    let fs = Picky {
        vague: true,
        ..Picky::new()
    };
    ensure_dir(&fs, &vpath(&dst.path().join("updates/sub"))).unwrap();
    assert!(dst.path().join("updates/sub").is_dir());
    // MKD on an existing folder fails vaguely; a second stat shows it's there.
    assert!(fs.create_dir(&vpath(&dst.path().join("updates"))).is_err());
    ensure_dir(&fs, &vpath(&dst.path().join("updates"))).unwrap();
}

#[test]
fn list_lines_shows_folders_first() {
    let dst = tempfile::tempdir().unwrap();
    std::fs::write(dst.path().join("index.htm"), b"12345").unwrap();
    std::fs::create_dir(dst.path().join("updates")).unwrap();
    let lines = list_lines(&Picky::new(), &vpath(dst.path())).unwrap();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].starts_with("d ") && lines[0].ends_with("  updates/"),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("- ")
            && lines[1].contains("     5  ")
            && lines[1].ends_with("  index.htm"),
        "{lines:?}"
    );
}

#[test]
fn upload_file_as_rejects_path_names() {
    let src = tempfile::tempdir().unwrap();
    let files = release_files(src.path());
    let fs = Picky::new();
    assert!(upload_file_as(&fs, &VfsPath::root(), &files[2], "a/b").is_err());
    assert!(upload_file_as(&fs, &VfsPath::root(), &files[2], "..").is_err());
}
