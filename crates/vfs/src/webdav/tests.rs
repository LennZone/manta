//! The WebDAV backend against the in-memory server ([`super::fake_server`]).

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use manta_core::{
    Auth, ConnectionConfig, Protocol, SharedVfs, Vfs, VfsError, VfsPath, WebDavOptions,
};
use manta_ops::{ConflictPolicy, Job, JobKind, JobState, TransferManager};

use super::fake_server::{AuthMode, FakeDav};
use super::*;

fn config(server: &FakeDav, root: &str, password: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: "fake".into(),
        protocol: Protocol::WebDav,
        host: "127.0.0.1".into(),
        port: server.port,
        username: "alice".into(),
        auth: Auth::Password(password.into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: Some(Box::new(WebDavOptions {
            root: root.into(),
            preset: "generic".into(),
        })),
        smb: None,
    }
}

fn connect(server: &FakeDav) -> WebDavFs {
    WebDavFs::connect(&config(server, &server.prefix, "s3cret pass")).expect("connect")
}

fn read_all(fs: &dyn Vfs, p: &str) -> Vec<u8> {
    let mut out = Vec::new();
    fs.open_read(&VfsPath::new(p))
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn body(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i % 251) as u8 ^ (i / 251) as u8)
        .collect()
}

#[test]
fn browse_and_edit_with_digest() {
    let server = FakeDav::start("/remote.php/dav/files/alice", AuthMode::Digest);
    {
        let mut st = server.st();
        let d = server.path("/Fotos & Videos");
        st.add_dir(&d);
        st.add_file(&format!("{d}/Ünïcode 1.jpg"), b"jpeg");
        st.add_file(&server.path("/notes.txt"), b"hello");
        st.add_file(&server.path("/.hidden"), b"");
    }
    let fs = connect(&server);
    assert_eq!(fs.label(), "alice@127.0.0.1 (WebDAV)");
    assert_eq!(fs.protocol(), Protocol::WebDav);
    let mut names: Vec<(String, bool, u64, bool)> = fs
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| (e.name.clone(), e.is_dir(), e.size, e.hidden))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            (".hidden".to_string(), false, 0, true),
            ("Fotos & Videos".to_string(), true, 0, false),
            ("notes.txt".to_string(), false, 5, false),
        ]
    );
    let sub = fs.list(&VfsPath::new("/Fotos & Videos")).unwrap();
    assert_eq!(sub.len(), 1);
    assert_eq!(sub[0].path, VfsPath::new("/Fotos & Videos/Ünïcode 1.jpg"));
    assert!(sub[0].modified.is_some());
    assert!(matches!(
        fs.list(&VfsPath::new("/notes.txt")),
        Err(VfsError::NotADirectory(_))
    ));
    assert!(matches!(
        fs.stat(&VfsPath::new("/missing")),
        Err(VfsError::NotFound(_))
    ));
    assert_eq!(read_all(&fs, "/Fotos & Videos/Ünïcode 1.jpg"), b"jpeg");

    // Write, mkdir, rename, copy, delete.
    let mut w = fs.open_write(&VfsPath::new("/new #1.txt")).unwrap();
    w.write_all(b"abc").unwrap();
    w.write_all(b"def").unwrap();
    w.flush().unwrap();
    drop(w);
    assert_eq!(
        server.st().file(&server.path("/new #1.txt")).unwrap(),
        b"abcdef"
    );
    fs.create_dir(&VfsPath::new("/Neu")).unwrap();
    assert!(matches!(
        fs.create_dir(&VfsPath::new("/Neu")),
        Err(VfsError::AlreadyExists(_))
    ));
    assert!(matches!(
        fs.create_dir(&VfsPath::new("/no/such/parent")),
        Err(VfsError::NotFound(_))
    ));
    fs.rename(
        &VfsPath::new("/new #1.txt"),
        &VfsPath::new("/Neu/moved.txt"),
    )
    .unwrap();
    assert_eq!(read_all(&fs, "/Neu/moved.txt"), b"abcdef");
    fs.server_copy(&VfsPath::new("/Neu"), &VfsPath::new("/Neu 2"))
        .unwrap();
    assert_eq!(read_all(&fs, "/Neu 2/moved.txt"), b"abcdef");
    assert!(matches!(
        fs.server_copy(&VfsPath::new("/Neu"), &VfsPath::new("/Neu 2")),
        Err(VfsError::AlreadyExists(_))
    ));
    assert!(matches!(
        fs.remove_dir(&VfsPath::new("/Neu")),
        Err(VfsError::DirectoryNotEmpty(_))
    ));
    assert!(fs.remove_file(&VfsPath::new("/Neu")).is_err(), "a folder");
    fs.remove_file(&VfsPath::new("/Neu/moved.txt")).unwrap();
    fs.remove_dir(&VfsPath::new("/Neu")).unwrap();
    fs.remove_tree(&VfsPath::new("/Neu 2")).unwrap();
    assert!(fs.stat(&VfsPath::new("/Neu 2")).is_err());

    // Digest signed every request after the first challenge.
    let st = server.st();
    assert_eq!(st.unauthorized, 1, "one challenge, then preemptive");
}

#[test]
fn wrong_password_and_basic_auth() {
    let server = FakeDav::start("/dav", AuthMode::Digest);
    let err = WebDavFs::connect(&config(&server, "/dav", "wrong"))
        .err()
        .expect("refused");
    assert!(matches!(err, VfsError::Auth(_)), "{err:?}");

    let server = FakeDav::start("", AuthMode::Basic);
    let fs = WebDavFs::connect(&config(&server, "", "s3cret pass")).expect("basic");
    fs.put(&VfsPath::new("/x"), b"1").unwrap();
    assert_eq!(read_all(&fs, "/x"), b"1");
    let err = WebDavFs::connect(&config(&server, "", "nope"))
        .err()
        .unwrap();
    assert!(matches!(err, VfsError::Auth(_)), "{err:?}");

    // No such root folder.
    let err = WebDavFs::connect(&config(&server, "/nope", "s3cret pass"))
        .err()
        .unwrap();
    assert!(matches!(err, VfsError::NotFound(_)), "{err:?}");
}

#[test]
fn stale_nonce_is_renewed() {
    let server = FakeDav::start("", AuthMode::Digest);
    server.st().nonce_uses = 2;
    let fs = connect(&server);
    for i in 0..6 {
        fs.put(&VfsPath::new(&format!("/f{i}")), b"x").unwrap();
    }
    assert_eq!(fs.list(&VfsPath::root()).unwrap().len(), 6);
}

#[test]
fn large_uploads_stream_and_cancel_cleanly() {
    let server = FakeDav::start("/dav", AuthMode::Digest);
    let fs = connect(&server);
    let big = body(BUFFERED_MAX + 1_234_567);
    let mut w = fs.open_write(&VfsPath::new("/big.bin")).unwrap();
    for c in big.chunks(256 * 1024) {
        w.write_all(c).unwrap();
    }
    w.flush().unwrap();
    drop(w);
    assert_eq!(server.st().file("/dav/big.bin").unwrap(), &big);
    assert!(
        server
            .st()
            .log
            .iter()
            .any(|(m, p, chunked)| m == "PUT" && p == "/dav/big.bin" && *chunked),
        "streamed with chunked encoding"
    );

    // Dropped without flush: the server never gets a complete file.
    let mut w = fs.open_write(&VfsPath::new("/cancelled.bin")).unwrap();
    for c in big.chunks(256 * 1024) {
        w.write_all(c).unwrap();
    }
    drop(w);
    std::thread::sleep(Duration::from_millis(100));
    assert!(server.st().file("/dav/cancelled.bin").is_none());

    // Small and empty files: one PUT with a length.
    let mut w = fs.open_write(&VfsPath::new("/empty")).unwrap();
    w.flush().unwrap();
    assert_eq!(server.st().file("/dav/empty").unwrap().len(), 0);
    assert!(w.write_all(b"late").is_err(), "after completion");
}

#[test]
fn ranged_reads_and_retries() {
    let server = FakeDav::start("", AuthMode::None);
    let content = body(100_000);
    server.st().add_file("/a.bin", &content);
    let anon = ConnectionConfig {
        username: String::new(),
        auth: Auth::None,
        ..config(&server, "", "")
    };
    let fs = WebDavFs::connect(&anon).unwrap();
    assert_eq!(fs.label(), "127.0.0.1 (WebDAV)");
    let caps = fs.supports_resume();
    assert!(caps.read_at && !caps.append && !caps.upload_session);
    let mut out = Vec::new();
    fs.open_read_at(&VfsPath::new("/a.bin"), 70_000)
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    assert_eq!(out, content[70_000..]);
    // A server without ranges: skipped locally.
    server.st().ranges = false;
    let mut out = Vec::new();
    fs.open_read_at(&VfsPath::new("/a.bin"), 99_000)
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    assert_eq!(out, content[99_000..]);

    // 503s are retried.
    server.st().fail_503_next = 2;
    assert_eq!(fs.stat(&VfsPath::new("/a.bin")).unwrap().size, 100_000);
    server.st().fail_503_next = 10;
    let err = fs.stat(&VfsPath::new("/a.bin")).unwrap_err();
    assert!(err.is_transient(), "{err:?}");
    server.st().fail_503_next = 0;

    assert!(fs.content_tag(&VfsPath::new("/a.bin")).is_some());
}

#[test]
fn modification_times() {
    let server = FakeDav::start("", AuthMode::None);
    server.st().add_file("/f", b"1");
    let fs = connect(&server);
    let t = Utc.with_ymd_and_hms(2020, 5, 6, 7, 8, 9).unwrap();
    fs.set_modified(&VfsPath::new("/f"), t).unwrap();
    assert_eq!(fs.stat(&VfsPath::new("/f")).unwrap().modified, Some(t));
    server.st().allow_mtime = false;
    assert!(matches!(
        fs.set_modified(&VfsPath::new("/f"), t),
        Err(VfsError::Unsupported(_))
    ));
    // Not asked again.
    let before = server.st().count("PROPPATCH");
    let _ = fs.set_modified(&VfsPath::new("/f"), t);
    assert_eq!(server.st().count("PROPPATCH"), before);
}

#[test]
fn presets() {
    assert_eq!(
        preset_root("nextcloud", " alice ").as_deref(),
        Some("/remote.php/dav/files/alice")
    );
    assert_eq!(preset_root("generic", "x").as_deref(), Some(""));
    assert_eq!(preset_root("nope", "x"), None);
}

fn wait(tm: &TransferManager, id: u64) -> manta_ops::JobStatus {
    let start = Instant::now();
    loop {
        let st = tm.snapshot().into_iter().find(|s| s.id == id).expect("job");
        if st.state.is_finished() {
            return st;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "timeout: {st:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Copies through the transfer engine both ways, and a copy within the
/// server uses COPY instead of moving the bytes through Manta.
#[test]
fn transfers_through_the_engine() {
    let server = FakeDav::start("/dav", AuthMode::Digest);
    let fs: SharedVfs = Arc::new(connect(&server));
    let td = tempfile::tempdir().unwrap();
    let content = body(300_000);
    std::fs::create_dir(td.path().join("up")).unwrap();
    std::fs::write(td.path().join("up/a.bin"), &content).unwrap();
    std::fs::write(td.path().join("up/b.txt"), b"bee").unwrap();
    let local: SharedVfs = Arc::new(crate::LocalFs::new());
    let tm = TransferManager::new(2);

    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: local.clone(),
        sources: vec![VfsPath::new(&td.path().join("up").to_string_lossy())],
        dst_fs: Some(fs.clone()),
        dst_dir: Some(VfsPath::root()),
        conflict: ConflictPolicy::Overwrite,
    });
    let st = wait(&tm, id);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(server.st().file("/dav/up/a.bin").unwrap(), &content);
    assert!(server.st().file("/dav/up/.a.bin.manta-part").is_none());

    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: fs.clone(),
        sources: vec![VfsPath::new("/up")],
        dst_fs: Some(fs.clone()),
        dst_dir: Some(VfsPath::new("/copy")),
        conflict: ConflictPolicy::Overwrite,
    });
    let st = wait(&tm, id);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(server.st().file("/dav/copy/up/b.txt").unwrap(), b"bee");
    assert!(server.st().count("COPY") >= 1);

    let back = td.path().join("back");
    std::fs::create_dir(&back).unwrap();
    let id = tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: fs.clone(),
        sources: vec![VfsPath::new("/copy/up")],
        dst_fs: Some(local),
        dst_dir: Some(VfsPath::new(&back.to_string_lossy())),
        conflict: ConflictPolicy::Overwrite,
    });
    let st = wait(&tm, id);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(std::fs::read(back.join("up/a.bin")).unwrap(), content);
}
