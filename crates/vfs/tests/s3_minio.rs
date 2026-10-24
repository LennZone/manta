//! S3 backend against a real MinIO server.
//!
//! Starts a local `minio` binary, or `minio/minio` (falling back to the
//! MinIO-compatible `rustfs/rustfs`) in Docker, on a random port. Skips with a message when neither is available. Set
//! `MANTA_S3_TEST_REQUIRE=1` to turn a skip into a failure.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{Auth, ConnectionConfig, Protocol, S3Options, SharedVfs, Vfs, VfsError, VfsPath};
use manta_ops::{ConflictPolicy, Job, JobKind, JobState, TransferManager};
use manta_vfs::{LocalFs, S3Fs};

const USER: &str = "manta";
const SECRET: &str = "manta-secret-key";

enum Server {
    Docker(String),
    Binary {
        child: Child,
        _data: tempfile::TempDir,
    },
}

impl Drop for Server {
    fn drop(&mut self) {
        match self {
            Server::Docker(id) => {
                let _ = Command::new("docker")
                    .args(["rm", "-f", id])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            Server::Binary { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_minio() -> Option<(Server, u16)> {
    let port = free_port();
    let env = [("MINIO_ROOT_USER", USER), ("MINIO_ROOT_PASSWORD", SECRET)];
    // A local binary is fastest.
    if Command::new("minio")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        let dir = tempfile::tempdir().ok()?;
        let child = Command::new("minio")
            .args([
                "server",
                dir.path().to_str()?,
                "--address",
                &format!("127.0.0.1:{port}"),
            ])
            .envs(env)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        return Some((Server::Binary { child, _data: dir }, port));
    }
    if !containers_opted_in() {
        return None;
    }
    match Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
    {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "docker unavailable: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return None;
        }
        Err(e) => {
            eprintln!("docker unavailable: {e}");
            return None;
        }
    }
    // MinIO's Docker Hub images are no longer public everywhere, so fall
    // back to RustFS, a MinIO-compatible server.
    type Candidate<'a> = (&'a str, &'a [&'a str], [(&'a str, &'a str); 2]);
    let candidates: [Candidate; 2] = [
        ("minio/minio", &["server", "/data"], env),
        (
            "rustfs/rustfs",
            &[],
            [("RUSTFS_ACCESS_KEY", USER), ("RUSTFS_SECRET_KEY", SECRET)],
        ),
    ];
    for (image, args, env) in candidates {
        let mut cmd = Command::new("docker");
        cmd.args(["run", "-d", "--rm", "-p", &format!("127.0.0.1:{port}:9000")]);
        for (k, v) in env {
            cmd.args(["-e", &format!("{k}={v}")]);
        }
        let out = match cmd.arg(image).args(args).output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("docker run failed: {e}");
                return None;
            }
        };
        if out.status.success() {
            let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
            eprintln!("S3 test server: {image} on port {port}");
            return Some((Server::Docker(id), port));
        }
        eprintln!(
            "docker run {image} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    None
}

fn cfg(port: u16, secret: &str, bucket: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: String::new(),
        protocol: Protocol::S3,
        host: "127.0.0.1".into(),
        port,
        username: USER.into(),
        auth: Auth::Password(secret.into()),
        initial_path: None,
        s3: Some(Box::new(S3Options {
            provider: "minio".into(),
            endpoint: format!("http://127.0.0.1:{port}"),
            region: "us-east-1".into(),
            bucket: bucket.into(),
            path_style: true,
            ..Default::default()
        })),
        container: None,
        ftp: None,
        webdav: None,
        smb: None,
    }
}

fn p(s: &str) -> VfsPath {
    VfsPath::new(s)
}

fn put(fs: &dyn Vfs, path: &str, data: &[u8]) {
    let mut w = fs.open_write(&p(path)).unwrap();
    for chunk in data.chunks(256 * 1024) {
        w.write_all(chunk).unwrap();
    }
    w.flush().unwrap();
}

fn get(fs: &dyn Vfs, path: &str) -> Vec<u8> {
    let mut out = Vec::new();
    fs.open_read(&p(path))
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn names(fs: &dyn Vfs, dir: &str) -> Vec<String> {
    let mut n: Vec<String> = fs
        .list(&p(dir))
        .unwrap()
        .into_iter()
        .map(|e| {
            if e.is_dir() {
                format!("{}/", e.name)
            } else {
                e.name
            }
        })
        .collect();
    n.sort();
    n
}

fn run_job(tm: &TransferManager, job: Job) {
    tm.clear_finished();
    tm.enqueue(job);
    let start = Instant::now();
    while tm.is_busy() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "transfer timeout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    for st in tm.snapshot() {
        assert_eq!(st.state, JobState::Done, "{st:?}");
        assert!(st.errors.is_empty(), "{:?}", st.errors);
    }
}

/// Deterministic pseudo-random bytes (xorshift), so corruption shows up.
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
fn s3_backend_against_minio() {
    let Some((_server, port)) = start_minio() else {
        assert!(
            std::env::var_os("MANTA_S3_TEST_REQUIRE").is_none(),
            "MinIO required but unavailable"
        );
        eprintln!("skipping: neither docker nor a minio binary is available");
        return;
    };

    // Wait for the server to come up.
    let start = Instant::now();
    let s3 = loop {
        match S3Fs::connect(&cfg(port, SECRET, "")) {
            Ok(fs) => break fs,
            Err(e) if start.elapsed() < Duration::from_secs(60) => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => panic!("MinIO did not start: {e}"),
        }
    };
    let s3: SharedVfs = Arc::new(s3);

    // Wrong secret -> authentication error.
    match S3Fs::connect(&cfg(port, "wrong-secret", "")) {
        Err(VfsError::Auth(m)) => assert!(m.contains("secret"), "{m}"),
        Err(other) => panic!("expected Auth, got {other:?}"),
        Ok(_) => panic!("connected with a wrong secret"),
    }

    // Buckets are the top-level folders.
    assert_eq!(s3.home().unwrap(), VfsPath::root());
    assert!(names(&*s3, "/").is_empty());
    s3.create_dir(&p("/data")).unwrap();
    assert_eq!(names(&*s3, "/"), ["data/"]);
    assert!(s3.stat(&p("/data")).unwrap().is_dir());
    assert!(matches!(s3.stat(&p("/nope")), Err(VfsError::NotFound(_))));

    // Folders and small files.
    s3.create_dir(&p("/data/docs")).unwrap();
    assert!(matches!(
        s3.create_dir(&p("/data/docs")),
        Err(VfsError::AlreadyExists(_))
    ));
    put(&*s3, "/data/docs/hello world.txt", b"hello S3");
    put(&*s3, "/data/docs/sub/ü & <x>.md", b"# special");
    put(&*s3, "/data/empty.bin", b"");
    assert_eq!(names(&*s3, "/data"), ["docs/", "empty.bin"]);
    assert_eq!(names(&*s3, "/data/docs"), ["hello world.txt", "sub/"]);
    assert_eq!(get(&*s3, "/data/docs/sub/ü & <x>.md"), b"# special");
    let st = s3.stat(&p("/data/docs/hello world.txt")).unwrap();
    assert_eq!((st.size, st.is_dir()), (8, false));
    assert!(st.modified.is_some());
    assert!(s3.stat(&p("/data/docs/sub")).unwrap().is_dir()); // implicit folder, no marker
    assert_eq!(s3.stat(&p("/data/empty.bin")).unwrap().size, 0);
    assert!(matches!(
        s3.list(&p("/data/empty.bin")),
        Err(VfsError::NotADirectory(_))
    ));
    assert!(matches!(
        s3.open_read(&p("/data/missing")),
        Err(VfsError::NotFound(_))
    ));
    assert!(matches!(
        s3.set_modified(&p("/data/empty.bin"), chrono::Utc::now()),
        Err(VfsError::Unsupported(_))
    ));

    // Multipart upload (20 MB > 16 MiB part size) round trip.
    let big = noise(20 * 1000 * 1000, 7);
    put(&*s3, "/data/big.bin", &big);
    assert_eq!(s3.stat(&p("/data/big.bin")).unwrap().size, big.len() as u64);
    assert!(
        get(&*s3, "/data/big.bin") == big,
        "multipart content differs"
    );

    // Dropping a writer without flush() uploads nothing (and aborts multipart).
    {
        let mut w = s3.open_write(&p("/data/aborted.bin")).unwrap();
        w.write_all(&big).unwrap();
    }
    assert!(matches!(
        s3.stat(&p("/data/aborted.bin")),
        Err(VfsError::NotFound(_))
    ));

    // Rename a file and a folder (copy + delete).
    s3.rename(
        &p("/data/docs/hello world.txt"),
        &p("/data/docs/renamed.txt"),
    )
    .unwrap();
    assert_eq!(names(&*s3, "/data/docs"), ["renamed.txt", "sub/"]);
    s3.rename(&p("/data/docs"), &p("/data/papers")).unwrap();
    assert_eq!(names(&*s3, "/data"), ["big.bin", "empty.bin", "papers/"]);
    assert_eq!(get(&*s3, "/data/papers/sub/ü & <x>.md"), b"# special");
    assert!(matches!(
        s3.stat(&p("/data/docs")),
        Err(VfsError::NotFound(_))
    ));

    // remove_dir only removes empty folders.
    assert!(matches!(
        s3.remove_dir(&p("/data/papers")),
        Err(VfsError::DirectoryNotEmpty(_))
    ));
    s3.create_dir(&p("/data/tmp")).unwrap();
    s3.remove_dir(&p("/data/tmp")).unwrap();
    assert!(matches!(
        s3.stat(&p("/data/tmp")),
        Err(VfsError::NotFound(_))
    ));

    // Delete a folder with more than one listing page (1000 keys) at once.
    let s3_ref = &s3;
    std::thread::scope(|scope| {
        for t in 0..8 {
            scope.spawn(move || {
                for i in (t..1005).step_by(8) {
                    put(&**s3_ref, &format!("/data/many/f{i:04}"), b"x");
                }
            });
        }
    });
    assert_eq!(s3.list(&p("/data/many")).unwrap().len(), 1005);
    let gone = S3Fs::delete_prefix(
        s3.as_any().unwrap().downcast_ref::<S3Fs>().unwrap(),
        &p("/data/many"),
    )
    .unwrap();
    assert_eq!(gone, 1006); // + the (absent) folder marker key
    assert!(matches!(
        s3.stat(&p("/data/many")),
        Err(VfsError::NotFound(_))
    ));

    // TransferManager: LocalFs -> S3 -> LocalFs, then delete on S3.
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let lp = |s: &str| VfsPath::new(root.join(s).to_str().unwrap());
    std::fs::create_dir_all(root.join("project/src/empty")).unwrap();
    std::fs::write(root.join("project/README.md"), b"readme").unwrap();
    std::fs::write(root.join("project/src/main.rs"), noise(300_000, 3)).unwrap();
    let local: SharedVfs = Arc::new(LocalFs::new());
    let tm = TransferManager::new(2);
    run_job(
        &tm,
        Job {
            kind: JobKind::Copy,
            src_fs: local.clone(),
            sources: vec![lp("project")],
            dst_fs: Some(s3.clone()),
            dst_dir: Some(p("/data")),
            conflict: ConflictPolicy::Overwrite,
        },
    );
    assert_eq!(names(&*s3, "/data/project"), ["README.md", "src/"]);
    assert_eq!(names(&*s3, "/data/project/src"), ["empty/", "main.rs"]);
    run_job(
        &tm,
        Job {
            kind: JobKind::Copy,
            src_fs: s3.clone(),
            sources: vec![p("/data/project")],
            dst_fs: Some(local.clone()),
            dst_dir: Some(lp("back")),
            conflict: ConflictPolicy::Overwrite,
        },
    );
    assert_eq!(
        std::fs::read(root.join("back/project/README.md")).unwrap(),
        b"readme"
    );
    assert_eq!(
        std::fs::read(root.join("back/project/src/main.rs")).unwrap(),
        noise(300_000, 3)
    );
    assert!(root.join("back/project/src/empty").is_dir());
    run_job(
        &tm,
        Job {
            kind: JobKind::Delete,
            src_fs: s3.clone(),
            sources: vec![p("/data/project")],
            dst_fs: None,
            dst_dir: None,
            conflict: ConflictPolicy::Overwrite,
        },
    );
    assert!(matches!(
        s3.stat(&p("/data/project")),
        Err(VfsError::NotFound(_))
    ));

    // A connection limited to one bucket starts there.
    let one = S3Fs::connect(&cfg(port, SECRET, "data")).unwrap();
    assert_eq!(one.home().unwrap(), p("/data"));
    assert_eq!(names(&one, "/"), ["data/"]);
    assert!(names(&one, "/data").contains(&"big.bin".to_string()));
    assert!(manta_vfs::connect(&cfg(port, SECRET, "missing-bucket")).is_err());
}

/// Upload `data[..upto]` through a resumable upload and drop it without
/// `flush()`, like a network drop; returns the last checkpointed session.
fn interrupted_upload(
    s3: &SharedVfs,
    path: &str,
    data: &[u8],
    upto: usize,
) -> manta_core::UploadSession {
    use std::sync::Mutex;
    let last: Arc<Mutex<Option<manta_core::UploadSession>>> = Arc::default();
    let l = last.clone();
    let up = s3
        .open_upload(
            &p(path),
            data.len() as u64,
            None,
            Arc::new(move |s| *l.lock().unwrap() = Some(s.clone())),
        )
        .unwrap();
    assert_eq!(up.offset, 0);
    let mut w = up.writer;
    for chunk in data[..upto].chunks(1024 * 1024) {
        w.write_all(chunk).unwrap();
    }
    drop(w);
    let s = last.lock().unwrap().clone().expect("session checkpointed");
    s
}

#[test]
fn s3_resume_against_minio() {
    use manta_vfs::s3::PART_SIZE;
    let Some((_server, port)) = start_minio() else {
        assert!(
            std::env::var_os("MANTA_S3_TEST_REQUIRE").is_none(),
            "MinIO required but unavailable"
        );
        eprintln!("skipping: neither docker nor a minio binary is available");
        return;
    };
    let start = Instant::now();
    let s3 = loop {
        match S3Fs::connect(&cfg(port, SECRET, "")) {
            Ok(fs) => break fs,
            Err(_) if start.elapsed() < Duration::from_secs(60) => {
                std::thread::sleep(Duration::from_millis(250))
            }
            Err(e) => panic!("S3 test server did not start: {e}"),
        }
    };
    let s3: SharedVfs = Arc::new(s3);
    s3.create_dir(&p("/res")).unwrap();
    let caps = s3.supports_resume();
    assert!(caps.read_at && caps.upload_session && !caps.append);

    // Range GET.
    let small = noise(1_000_000, 3);
    put(&*s3, "/res/small.bin", &small);
    let mut tail = Vec::new();
    s3.open_read_at(&p("/res/small.bin"), 600_000)
        .unwrap()
        .read_to_end(&mut tail)
        .unwrap();
    assert!(tail == small[600_000..]);
    assert!(s3
        .content_tag(&p("/res/small.bin"))
        .is_some_and(|t| !t.is_empty()));

    // Multipart: interrupted after 2 parts (+ a partial third), continued
    // with ListParts at exactly 2 parts.
    let data = noise(2 * PART_SIZE + 9_000_000, 5);
    let session = interrupted_upload(&s3, "/res/big.bin", &data, 2 * PART_SIZE + 5_000_000);
    assert_eq!(session.committed, 2 * PART_SIZE as u64);
    assert!(
        matches!(s3.stat(&p("/res/big.bin")), Err(VfsError::NotFound(_))),
        "invisible until complete"
    );
    let up = s3
        .open_upload(
            &p("/res/big.bin"),
            data.len() as u64,
            Some(&session),
            Arc::new(|_| {}),
        )
        .unwrap();
    assert_eq!(up.offset, 2 * PART_SIZE as u64);
    let mut w = up.writer;
    w.write_all(&data[up.offset as usize..]).unwrap();
    w.flush().unwrap();
    drop(w);
    assert!(
        get(&*s3, "/res/big.bin") == data,
        "resumed multipart differs"
    );

    // An aborted session can't be continued: start over.
    let session = interrupted_upload(&s3, "/res/gone.bin", &data, PART_SIZE + 10);
    s3.abort_upload(&p("/res/gone.bin"), &session).unwrap();
    let up = s3
        .open_upload(&p("/res/gone.bin"), 1, Some(&session), Arc::new(|_| {}))
        .unwrap();
    assert_eq!(up.offset, 0);
    drop(up);

    // Through the transfer manager: a journal with the upload session
    // (as left by a quit or crash) continues at part 3.
    use manta_ops::journal::{Journal, JournalJob, JournalState};
    use manta_ops::resume::{Partial, Progress};
    use manta_ops::{Endpoint, TransferOptions};
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    std::fs::write(root.join("movie.bin"), &data).unwrap();
    let local: SharedVfs = Arc::new(LocalFs::new());
    let src = local
        .stat(&VfsPath::new(root.join("movie.bin").to_str().unwrap()))
        .unwrap();
    let session = interrupted_upload(&s3, "/res/movie.bin", &data, 2 * PART_SIZE + 1);
    let s3_cfg = cfg(port, SECRET, "");
    let journal = root.join("transfers.json");
    let job = JournalJob {
        id: 1,
        kind: JobKind::Copy,
        title: "upload".into(),
        state: JournalState::Waiting,
        conflict: ConflictPolicy::Overwrite,
        src: Endpoint::Local,
        dst: Some(Endpoint::remote(&s3_cfg)),
        sources: vec![src.path.to_string()],
        dst_dir: Some("/res".into()),
        progress: Progress {
            partial: Some(Partial {
                src: src.path.to_string(),
                dst: "/res/movie.bin".into(),
                part: None,
                size: src.size,
                mtime: src.modified,
                tag: None,
                session: Some(session),
                choice: None,
            }),
            ..Default::default()
        },
        bytes_done: 0,
        bytes_total: src.size,
        files_done: 0,
        files_total: 1,
        errors: Vec::new(),
        log: Vec::new(),
    };
    Journal::new(journal.clone()).save(&[job]).unwrap();
    assert!(
        !std::fs::read_to_string(&journal).unwrap().contains(SECRET),
        "no secret in the journal"
    );
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        journal: Some(journal),
        ..Default::default()
    });
    let ids = tm.restore_interrupted(&|ep: &Endpoint| match ep {
        Endpoint::Local => Some(local.clone()),
        _ => Some(s3.clone()),
    });
    let mut resumed = None;
    let start = Instant::now();
    let st = loop {
        let st = tm.snapshot().into_iter().find(|s| s.id == ids[0]).unwrap();
        resumed = resumed.or(st.resume.resumed_from);
        if st.state.is_finished() {
            break st;
        }
        assert!(start.elapsed() < Duration::from_secs(120), "timeout");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(resumed, Some(2 * PART_SIZE as u64));
    assert!(
        get(&*s3, "/res/movie.bin") == data,
        "resumed upload differs"
    );
}

/// Real `docker` / `kubectl` only with `MANTA_TEST_CONTAINERS=1`.
fn containers_opted_in() -> bool {
    let ok = std::env::var_os("MANTA_TEST_CONTAINERS").is_some_and(|v| v == "1");
    if !ok {
        eprintln!("skipped: set MANTA_TEST_CONTAINERS=1 to run tests against docker/kubectl");
    }
    ok
}
