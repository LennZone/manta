//! Backend tests against the fake Drive server (fake_server.rs).

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{EntryKind, SharedVfs, Vfs, VfsError, VfsPath};

use super::fake_server::{FakeDrive, CODE, EMAIL, REFRESH, ROOT_ID};
use super::*;

const DOC: &str = "application/vnd.google-apps.document";
const SHEET: &str = "application/vnd.google-apps.spreadsheet";

fn client(srv: &FakeDrive) -> OAuthClient {
    OAuthClient::new("test-client", "test-secret").with_endpoints(Endpoints::local(&srv.base()))
}

struct Fixture {
    srv: FakeDrive,
    fs: Arc<GDriveFs>,
}

/// My Drive: Docs/ (with x.txt), two "a.txt", a Doc "Plan", a Sheet
/// "Budget"; shared with me: shared.pdf; shared drive "Team" with team.txt.
fn setup() -> Fixture {
    let srv = FakeDrive::start();
    {
        let mut st = srv.state.lock().unwrap();
        st.drives.push(("drive1".into(), "Team".into()));
        let docs = st.add("Docs", tree::FOLDER_MIME, ROOT_ID, b"");
        st.add("x.txt", "text/plain", &docs, b"x");
        st.add("a.txt", "text/plain", ROOT_ID, b"first");
        st.add("a.txt", "text/plain", ROOT_ID, b"second");
        st.add("Plan", DOC, ROOT_ID, b"");
        st.add("Budget", SHEET, ROOT_ID, b"");
        let shared = st.add(
            "shared.pdf",
            "application/pdf",
            "someone-elses-folder",
            b"pdf",
        );
        st.files.get_mut(&shared).unwrap().shared_with_me = true;
        st.add("team.txt", "text/plain", "drive1", b"team");
    }
    let store = Arc::new(MemoryStore::new());
    store.save(EMAIL, REFRESH).unwrap();
    let mut fs = GDriveFs::open(EMAIL, client(&srv), store).expect("connect");
    fs.tune_for_tests(256 * 1024);
    Fixture {
        srv,
        fs: Arc::new(fs),
    }
}

fn my(p: &str) -> VfsPath {
    Area::MyDrive.path().join(p)
}

fn read_all(fs: &dyn Vfs, p: &VfsPath) -> Vec<u8> {
    let mut v = Vec::new();
    fs.open_read(p)
        .expect("open_read")
        .read_to_end(&mut v)
        .expect("read");
    v
}

fn names(fs: &dyn Vfs, p: &VfsPath) -> Vec<String> {
    let mut v: Vec<String> = fs
        .list(p)
        .expect("list")
        .into_iter()
        .map(|e| e.name)
        .collect();
    v.sort();
    v
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[test]
fn root_shows_the_three_areas() {
    let f = setup();
    let root = f.fs.list(&VfsPath::root()).unwrap();
    assert_eq!(root.len(), 3);
    assert!(root.iter().all(|e| e.kind == EntryKind::Directory));
    assert_eq!(f.fs.home().unwrap(), Area::MyDrive.path());
    assert_eq!(f.fs.protocol(), manta_core::Protocol::GoogleDrive);
    assert!(f.fs.label().contains(EMAIL));
    // Both languages resolve.
    assert_eq!(
        names(&*f.fs, &VfsPath::new("/Meine Ablage")),
        names(&*f.fs, &VfsPath::new("/My Drive"))
    );
    assert_eq!(
        names(&*f.fs, &VfsPath::new("/Für mich freigegeben")),
        vec!["shared.pdf"]
    );
    assert_eq!(names(&*f.fs, &VfsPath::new("/Shared drives")), vec!["Team"]);
    assert_eq!(
        names(&*f.fs, &VfsPath::new("/Shared drives/Team")),
        vec!["team.txt"]
    );
    assert_eq!(
        read_all(&*f.fs, &VfsPath::new("/Shared drives/Team/team.txt")),
        b"team"
    );
    assert!(matches!(
        f.fs.stat(&VfsPath::new("/Nope")),
        Err(VfsError::NotFound(_))
    ));
}

#[test]
fn listing_follows_pages_and_disambiguates() {
    let f = setup();
    f.srv.state.lock().unwrap().page_cap = 2;
    let before = f.srv.state.lock().unwrap().list_calls;
    let n = names(&*f.fs, &Area::MyDrive.path());
    assert_eq!(
        n,
        vec!["Budget.xlsx", "Docs", "Plan.docx", "a (2).txt", "a.txt"]
    );
    assert!(
        f.srv.state.lock().unwrap().list_calls - before >= 3,
        "paged"
    );
    // The oldest duplicate keeps the plain name; stable across listings.
    assert_eq!(read_all(&*f.fs, &my("a.txt")), b"first");
    assert_eq!(read_all(&*f.fs, &my("a (2).txt")), b"second");
    assert_eq!(names(&*f.fs, &Area::MyDrive.path()), n);
    let e = f.fs.stat(&my("a (2).txt")).unwrap();
    assert_eq!((e.kind, e.size), (EntryKind::File, 6));
    assert!(e.modified.is_some());
}

#[test]
fn google_docs_are_exported_read_only() {
    let f = setup();
    let doc = my("Plan.docx");
    let st = f.fs.stat(&doc).unwrap();
    assert_eq!(st.permissions, Some(0o444));
    let body = String::from_utf8(read_all(&*f.fs, &doc)).unwrap();
    assert_eq!(
        body,
        "EXPORT Plan AS application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    let body = String::from_utf8(read_all(&*f.fs, &my("Budget.xlsx"))).unwrap();
    assert!(body.ends_with("spreadsheetml.sheet"), "{body}");
    assert!(matches!(
        f.fs.open_write(&doc),
        Err(VfsError::PermissionDenied(_))
    ));
    // Renaming keeps it a Doc: the export extension is not stored on Drive.
    f.fs.rename(&doc, &my("Plan 2026.docx")).unwrap();
    assert_eq!(f.srv.state.lock().unwrap().by_name("Plan 2026").len(), 1);
    assert!(f.fs.stat(&my("Plan 2026.docx")).is_ok());
}

#[test]
fn resumable_upload_in_chunks_and_update() {
    let f = setup();
    let data = noise(700 * 1024, 7);
    let path = my("Docs/up.bin");
    let mut w = f.fs.open_write(&path).unwrap();
    for c in data.chunks(10_000) {
        w.write_all(c).unwrap();
    }
    // Nothing is visible before the upload completes.
    assert!(f.srv.state.lock().unwrap().by_name("up.bin").is_empty());
    w.flush().unwrap();
    drop(w);
    {
        let st = f.srv.state.lock().unwrap();
        let files = st.by_name("up.bin");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].data, data);
        assert_eq!(
            st.chunk_ranges,
            vec![
                "bytes 0-262143/*",
                "bytes 262144-524287/*",
                &format!("bytes 524288-{}/{}", data.len() - 1, data.len())
            ]
        );
    }
    assert_eq!(read_all(&*f.fs, &path), data);
    assert_eq!(f.fs.stat(&path).unwrap().size, data.len() as u64);

    // Overwrite: the same file id gets new content.
    let id = f.srv.state.lock().unwrap().by_name("up.bin")[0].id.clone();
    let mut w = f.fs.open_write(&path).unwrap();
    w.write_all(b"small").unwrap();
    w.flush().unwrap();
    let st = f.srv.state.lock().unwrap();
    assert_eq!(st.by_name("up.bin").len(), 1);
    assert_eq!(st.files[&id].data, b"small");
}

#[test]
fn upload_edge_cases() {
    let f = setup();
    // Empty file.
    let mut w = f.fs.open_write(&my("empty")).unwrap();
    w.flush().unwrap();
    assert_eq!(f.srv.state.lock().unwrap().by_name("empty")[0].data, b"");
    assert_eq!(
        f.srv.state.lock().unwrap().chunk_ranges.last().unwrap(),
        "bytes */0"
    );

    // The server confirms only part of a chunk: the rest is re-sent.
    f.srv.state.lock().unwrap().short_ack_once = true;
    let data = noise(600 * 1024, 3);
    let mut w = f.fs.open_write(&my("short.bin")).unwrap();
    w.write_all(&data).unwrap();
    w.flush().unwrap();
    assert_eq!(
        f.srv.state.lock().unwrap().by_name("short.bin")[0].data,
        data
    );

    // Dropped without flush: the session is cancelled, nothing appears.
    let mut w = f.fs.open_write(&my("cancelled.bin")).unwrap();
    w.write_all(&noise(300 * 1024, 5)).unwrap();
    drop(w);
    let st = f.srv.state.lock().unwrap();
    assert!(st.by_name("cancelled.bin").is_empty());
    assert_eq!(st.cancelled_uploads, 1);
}

#[test]
fn rename_move_mkdir_and_mtime() {
    let f = setup();
    f.fs.create_dir(&my("New")).unwrap();
    assert!(matches!(
        f.fs.create_dir(&my("New")),
        Err(VfsError::AlreadyExists(_))
    ));
    f.fs.rename(&my("Docs/x.txt"), &my("Docs/y.txt")).unwrap();
    assert_eq!(names(&*f.fs, &my("Docs")), vec!["y.txt"]);
    f.fs.rename(&my("Docs/y.txt"), &my("New/y.txt")).unwrap();
    assert!(names(&*f.fs, &my("Docs")).is_empty());
    assert_eq!(names(&*f.fs, &my("New")), vec!["y.txt"]);
    let new_id = f.srv.state.lock().unwrap().by_name("New")[0].id.clone();
    assert_eq!(
        f.srv.state.lock().unwrap().by_name("y.txt")[0].parents,
        vec![new_id]
    );
    // Move a folder to the root of My Drive.
    f.fs.rename(&my("New"), &my("Docs/New")).unwrap();
    f.fs.rename(&my("Docs/New"), &my("Moved")).unwrap();
    assert_eq!(names(&*f.fs, &my("Moved")), vec!["y.txt"]);
    assert!(f.fs.rename(&my("Moved"), &my("Moved/inside")).is_err());

    let t = chrono::DateTime::parse_from_rfc3339("2020-05-06T07:08:09Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    f.fs.set_modified(&my("Moved/y.txt"), t).unwrap();
    assert_eq!(f.fs.stat(&my("Moved/y.txt")).unwrap().modified, Some(t));

    // Nothing can be created at fixed places.
    assert!(f.fs.create_dir(&VfsPath::new("/x")).is_err());
    assert!(matches!(
        f.fs.open_write(&Area::SharedWithMe.path().join("n.txt")),
        Err(VfsError::PermissionDenied(_))
    ));
    assert!(f.fs.remove_dir(&Area::MyDrive.path()).is_err());
}

#[test]
fn delete_moves_to_trash() {
    let f = setup();
    f.fs.remove_file(&my("a.txt")).unwrap();
    {
        let st = f.srv.state.lock().unwrap();
        let a: Vec<_> = st
            .by_name("a.txt")
            .into_iter()
            .filter(|f| f.data == b"first")
            .collect();
        assert!(a[0].trashed, "trashed, not deleted");
    }
    // The other duplicate now has the plain name.
    assert_eq!(read_all(&*f.fs, &my("a.txt")), b"second");
    assert!(matches!(
        f.fs.remove_dir(&my("Docs")),
        Err(VfsError::DirectoryNotEmpty(_))
    ));
    f.fs.remove_tree(&my("Docs")).unwrap();
    assert!(f.srv.state.lock().unwrap().by_name("Docs")[0].trashed);
    assert!(!names(&*f.fs, &Area::MyDrive.path()).contains(&"Docs".to_string()));
    assert!(f
        .fs
        .remove_tree(&VfsPath::new("/Shared drives/Team"))
        .is_err());
}

#[test]
fn expired_access_token_is_refreshed_on_401() {
    let f = setup();
    let before = f.srv.state.lock().unwrap().refreshes;
    // The server forgets the current token (as if it expired early).
    f.srv.state.lock().unwrap().access_token = "revoked-elsewhere".into();
    assert_eq!(names(&*f.fs, &my("Docs")), vec!["x.txt"]);
    assert_eq!(f.srv.state.lock().unwrap().refreshes, before + 1);
    // A revoked refresh token: a clear "sign in again".
    let store = Arc::new(MemoryStore::new());
    store.save(EMAIL, "1//revoked").unwrap();
    match GDriveFs::open(EMAIL, client(&f.srv), store) {
        Err(VfsError::Auth(m)) => assert!(m.contains("sign in"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    match GDriveFs::open(
        "nobody@example.com",
        client(&f.srv),
        Arc::new(MemoryStore::new()),
    ) {
        Err(VfsError::Auth(m)) => assert!(m.contains("not signed in"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn rate_limits_back_off_and_retry() {
    let f = setup();
    f.srv.state.lock().unwrap().rate_limit_next = 2;
    let start = Instant::now();
    assert_eq!(names(&*f.fs, &my("Docs")), vec!["x.txt"]);
    assert!(start.elapsed() < Duration::from_secs(5));
    f.srv.state.lock().unwrap().fail_503_next = 2;
    assert_eq!(read_all(&*f.fs, &my("a.txt")), b"first");
    // Persistent rate limiting ends in a friendly error.
    f.srv.state.lock().unwrap().rate_limit_next = 100;
    match f.fs.list(&my("Docs")) {
        Err(VfsError::Connection(m)) => assert!(m.contains("rate-limiting"), "{m}"),
        other => panic!("{other:?}"),
    }
}

/// The browser half of the PKCE flow: read the consent URL like Google
/// would, then hit the loopback redirect like the browser would.
fn fake_browser(srv: &FakeDrive, url: &str, answer: &'static str) {
    let q = oauth::parse_query(url.split_once('?').unwrap().1);
    let get = |k: &str| {
        q.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    assert_eq!(get("code_challenge_method"), "S256");
    {
        let mut st = srv.state.lock().unwrap();
        st.auth_challenge = get("code_challenge");
        st.auth_redirect = get("redirect_uri");
    }
    let redirect = get("redirect_uri");
    let state = get("state");
    std::thread::spawn(move || {
        let addr = redirect.trim_start_matches("http://").to_string();
        // A favicon request first, which the flow must ignore.
        let get_page = |target: String| {
            let mut s = std::net::TcpStream::connect(&addr).unwrap();
            write!(s, "GET {target} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        };
        assert!(get_page("/favicon.ico".into()).starts_with("HTTP/1.1 404"));
        let page = match answer {
            "code" => get_page(format!("/?state={state}&code={CODE}&scope=x")),
            "wrong-state" => get_page(format!("/?state=other&code={CODE}")),
            _ => get_page(format!("/?state={state}&error=access_denied")),
        };
        if answer == "code" {
            assert!(
                page.contains("zu Manta zurückkehren") && page.contains("return to Manta"),
                "{page}"
            );
        }
    });
}

#[test]
fn pkce_loopback_sign_in() {
    let srv = FakeDrive::start();
    let c = client(&srv);
    let tokens = authorize_with(
        &c,
        |url| fake_browser(&srv, url, "code"),
        &AuthCancel::new(),
        Duration::from_secs(20),
    )
    .expect("authorize");
    assert_eq!(tokens.email, EMAIL);
    assert_eq!(tokens.refresh_token, REFRESH);
    // The fresh tokens work without another refresh.
    let refreshes = srv.state.lock().unwrap().refreshes;
    let fs = GDriveFs::from_tokens(&tokens, c.clone(), Arc::new(MemoryStore::new()));
    assert_eq!(fs.list(&VfsPath::root()).unwrap().len(), 3);
    fs.list(&Area::MyDrive.path()).unwrap();
    assert_eq!(srv.state.lock().unwrap().refreshes, refreshes);

    // The user clicks "Cancel" on Google's consent page.
    let r = authorize_with(
        &c,
        |url| fake_browser(&srv, url, "denied"),
        &AuthCancel::new(),
        Duration::from_secs(20),
    );
    assert!(
        matches!(r, Err(VfsError::Auth(ref m)) if m.contains("denied")),
        "{r:?}"
    );
    // A redirect with a foreign state is ignored; the flow times out.
    let r = authorize_with(
        &c,
        |url| fake_browser(&srv, url, "wrong-state"),
        &AuthCancel::new(),
        Duration::from_secs(2),
    );
    assert!(
        matches!(r, Err(VfsError::Other(ref m)) if m.contains("timed out")),
        "{r:?}"
    );
    // A wrong client is reported as such.
    let bad = OAuthClient::new("other-client", "x").with_endpoints(Endpoints::local(&srv.base()));
    let r = authorize_with(
        &bad,
        |url| fake_browser(&srv, url, "code"),
        &AuthCancel::new(),
        Duration::from_secs(20),
    );
    assert!(
        matches!(r, Err(VfsError::Auth(ref m)) if m.contains("OAuth client")),
        "{r:?}"
    );
}

#[test]
fn transfer_manager_copies_local_to_drive_and_back() {
    use manta_ops::{ConflictPolicy, Job, JobKind, JobState, TransferManager};
    let f = setup();
    let src = tempfile::tempdir().unwrap();
    let tree_root = src.path().join("project");
    std::fs::create_dir_all(tree_root.join("sub")).unwrap();
    let big = noise(600 * 1024, 11);
    std::fs::write(tree_root.join("big.bin"), &big).unwrap();
    std::fs::write(tree_root.join("sub/note.txt"), b"hello drive").unwrap();
    std::fs::write(tree_root.join("empty.txt"), b"").unwrap();

    let local: SharedVfs = Arc::new(crate::LocalFs::new());
    let drive: SharedVfs = f.fs.clone();
    let tm = TransferManager::new(2);
    let run = |job: Job| {
        tm.clear_finished();
        tm.enqueue(job);
        let start = Instant::now();
        while tm.is_busy() {
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "transfer timeout"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        for st in tm.snapshot() {
            assert_eq!(st.state, JobState::Done, "{st:?}");
            assert!(st.errors.is_empty(), "{:?}", st.errors);
        }
    };
    run(Job {
        kind: JobKind::Copy,
        src_fs: local.clone(),
        sources: vec![VfsPath::new(tree_root.to_str().unwrap())],
        dst_fs: Some(drive.clone()),
        dst_dir: Some(my("Docs")),
        conflict: ConflictPolicy::Overwrite,
    });
    assert_eq!(
        names(&*drive, &my("Docs/project")),
        vec!["big.bin", "empty.txt", "sub"]
    );
    assert_eq!(read_all(&*drive, &my("Docs/project/big.bin")), big);
    // mtimes were carried over (set_modified).
    let local_mtime = local
        .stat(&VfsPath::new(tree_root.join("big.bin").to_str().unwrap()))
        .unwrap()
        .modified;
    let remote_mtime = drive.stat(&my("Docs/project/big.bin")).unwrap().modified;
    assert_eq!(
        local_mtime.map(|t| t.timestamp_millis()),
        remote_mtime.map(|t| t.timestamp_millis())
    );

    let back = tempfile::tempdir().unwrap();
    run(Job {
        kind: JobKind::Copy,
        src_fs: drive.clone(),
        sources: vec![my("Docs/project"), my("Plan.docx")],
        dst_fs: Some(local.clone()),
        dst_dir: Some(VfsPath::new(back.path().to_str().unwrap())),
        conflict: ConflictPolicy::Overwrite,
    });
    assert_eq!(
        std::fs::read(back.path().join("project/big.bin")).unwrap(),
        big
    );
    assert_eq!(
        std::fs::read(back.path().join("project/sub/note.txt")).unwrap(),
        b"hello drive"
    );
    assert_eq!(
        std::fs::read(back.path().join("project/empty.txt")).unwrap(),
        b""
    );
    assert!(std::fs::read_to_string(back.path().join("Plan.docx"))
        .unwrap()
        .starts_with("EXPORT Plan"));
}

#[test]
fn ranged_download_and_resumed_upload_session() {
    use std::sync::Mutex;
    let f = setup();
    // Range GET.
    let data = noise(700 * 1024, 9);
    let path = my("Docs/big.bin");
    let mut w = f.fs.open_write(&path).unwrap();
    w.write_all(&data).unwrap();
    w.flush().unwrap();
    drop(w);
    let mut tail = Vec::new();
    f.fs.open_read_at(&path, 300_000)
        .unwrap()
        .read_to_end(&mut tail)
        .unwrap();
    assert!(tail == data[300_000..]);
    assert_eq!(f.srv.state.lock().unwrap().ranged_gets, 1);
    let caps = f.fs.supports_resume();
    assert!(caps.read_at && caps.upload_session && !caps.append);
    // Google Docs exports can't seek: the head is skipped instead.
    let mut doc = String::new();
    f.fs.open_read_at(&my("Plan.docx"), 7)
        .unwrap()
        .read_to_string(&mut doc)
        .unwrap();
    assert!(doc.starts_with("Plan AS"), "{doc}");

    // A resumable upload interrupted after two chunks (network drop: the
    // writer is dropped without flush) keeps its session.
    let last: Arc<Mutex<Option<manta_core::UploadSession>>> = Arc::default();
    let l = last.clone();
    let target = my("Docs/resumed.bin");
    let up =
        f.fs.open_upload(
            &target,
            data.len() as u64,
            None,
            Arc::new(move |s| *l.lock().unwrap() = Some(s.clone())),
        )
        .unwrap();
    assert_eq!(up.offset, 0);
    let mut w = up.writer;
    w.write_all(&data[..600 * 1024]).unwrap();
    drop(w);
    let session = last.lock().unwrap().clone().expect("session checkpointed");
    assert_eq!(session.committed, 512 * 1024);
    assert_eq!(
        f.srv.state.lock().unwrap().cancelled_uploads,
        0,
        "kept for the resume"
    );
    assert!(f
        .srv
        .state
        .lock()
        .unwrap()
        .by_name("resumed.bin")
        .is_empty());

    // Continue: Drive is asked for the uploaded range, then the rest goes up.
    let up =
        f.fs.open_upload(&target, data.len() as u64, Some(&session), Arc::new(|_| {}))
            .unwrap();
    assert_eq!(up.offset, 512 * 1024);
    let mut w = up.writer;
    w.write_all(&data[512 * 1024..]).unwrap();
    w.flush().unwrap();
    drop(w);
    {
        let st = f.srv.state.lock().unwrap();
        assert!(
            st.chunk_ranges.contains(&format!("bytes */{}", data.len())),
            "{:?}",
            st.chunk_ranges
        );
        assert_eq!(st.by_name("resumed.bin")[0].data, data);
    }

    // An expired / cancelled session starts over.
    let l = last.clone();
    let up =
        f.fs.open_upload(
            &my("Docs/gone.bin"),
            data.len() as u64,
            None,
            Arc::new(move |s| *l.lock().unwrap() = Some(s.clone())),
        )
        .unwrap();
    let mut w = up.writer;
    w.write_all(&data[..300 * 1024]).unwrap();
    drop(w);
    let session = last.lock().unwrap().clone().unwrap();
    f.fs.abort_upload(&my("Docs/gone.bin"), &session).unwrap();
    let up =
        f.fs.open_upload(
            &my("Docs/gone.bin"),
            data.len() as u64,
            Some(&session),
            Arc::new(|_| {}),
        )
        .unwrap();
    assert_eq!(up.offset, 0);
}

#[test]
fn space_is_the_accounts_quota() {
    let f = setup();
    let s = f.fs.space(&my("Docs")).unwrap().expect("quota");
    assert_eq!(s.total, Some(15 * 1024 * 1024 * 1024));
    assert_eq!(s.available, 10 * 1024 * 1024 * 1024);
    // Shared drives belong to the organisation.
    assert_eq!(f.fs.space(&Area::SharedDrives.path()).unwrap(), None);
}
