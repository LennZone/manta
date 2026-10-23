//! Docker backend against a real, throwaway container.
//!
//! SAFETY: this test only ever touches containers it creates itself
//! (`alpine`, labelled `manta-test`) and removes them by ID in a Drop
//! guard, also on panic. It never stops, changes or removes any other
//! container. Skipped when docker or its daemon is unavailable, or when
//! the alpine image can't be pulled.

use std::io::{Read, Write};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{EntryKind, SharedVfs, Vfs, VfsError, VfsPath};
use manta_ops::{ConflictPolicy, Job, JobKind, JobState, TransferManager};
use manta_vfs::container::{docker_config, find_program, ContainerFs, Mode};
use manta_vfs::LocalFs;

const IMAGE: &str = "alpine:3";

/// Removes the container it holds (ours only) when dropped.
struct Throwaway {
    id: String,
}

impl Drop for Throwaway {
    fn drop(&mut self) {
        let _ = Command::new(docker()).args(["rm", "-f", &self.id]).output();
    }
}

fn docker() -> std::path::PathBuf {
    find_program("docker").unwrap_or_else(|| "docker".into())
}

fn docker_ok(args: &[&str]) -> Option<String> {
    let out = Command::new(docker()).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Docker reachable and the image present (pulled if needed).
fn ready() -> bool {
    if !containers_opted_in() {
        return false;
    }
    if find_program("docker").is_none()
        || docker_ok(&["version", "--format", "{{.Server.Version}}"]).is_none()
    {
        eprintln!("docker test skipped: docker or its daemon is not available");
        return false;
    }
    if docker_ok(&["image", "inspect", IMAGE]).is_none() && docker_ok(&["pull", IMAGE]).is_none() {
        eprintln!("docker test skipped: cannot pull {IMAGE}");
        return false;
    }
    true
}

/// A new container from `args` (`run -d --rm …` or `create …`).
fn throwaway(args: &[&str]) -> Throwaway {
    let id = docker_ok(args).expect("create test container");
    assert!(!id.is_empty());
    Throwaway { id }
}

fn p(s: &str) -> VfsPath {
    VfsPath::new(s)
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
            start.elapsed() < Duration::from_secs(120),
            "transfer timeout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    for st in tm.snapshot() {
        assert_eq!(st.state, JobState::Done, "{st:?}");
        assert!(st.errors.is_empty(), "{:?}", st.errors);
    }
}

#[test]
fn docker_container_end_to_end() {
    if !ready() {
        return;
    }
    let ctr = throwaway(&[
        "run",
        "-d",
        "--rm",
        "--label",
        "manta-test",
        "-w",
        "/work",
        IMAGE,
        "sleep",
        "300",
    ]);
    let mut cfg = docker_config(&ctr.id);
    cfg.name = "manta test".into();
    let fs: SharedVfs = Arc::new(ContainerFs::connect(&cfg).expect("connect"));
    assert_eq!(fs.home().unwrap(), p("/work"), "WORKDIR is home");
    assert!(!fs.is_read_only());
    assert_eq!(fs.label(), "manta test (Docker)");

    // Listing the root of a busybox system.
    let root = names(&*fs, "/");
    for d in ["bin/", "etc/", "tmp/", "work/"] {
        assert!(root.contains(&d.to_string()), "{root:?}");
    }
    let etc = fs.list(&p("/etc")).unwrap();
    let os = etc
        .iter()
        .find(|e| e.name == "os-release")
        .expect("os-release");
    assert_eq!(os.kind, EntryKind::Symlink, "alpine links os-release");
    assert!(os.size > 0, "size of the link target");

    // mkdir, odd names, upload 5 MB, download, compare.
    fs.create_dir(&p("/work/dir")).unwrap();
    assert!(matches!(
        fs.create_dir(&p("/work/dir")),
        Err(VfsError::AlreadyExists(_))
    ));
    let odd = p("/work/dir/it's a \"quoted\" file.bin");
    let data = noise(5 * 1024 * 1024 + 17, 7);
    let mut w = fs.open_write(&odd).unwrap();
    for chunk in data.chunks(64 * 1024) {
        w.write_all(chunk).unwrap();
    }
    w.flush().unwrap();
    drop(w);
    assert_eq!(names(&*fs, "/work/dir"), ["it's a \"quoted\" file.bin"]);
    let st = fs.stat(&odd).unwrap();
    assert_eq!((st.size, st.kind), (data.len() as u64, EntryKind::File));
    let mut back = Vec::new();
    fs.open_read(&odd).unwrap().read_to_end(&mut back).unwrap();
    assert!(back == data, "download differs");

    // A name with a newline round-trips through the listing.
    let nl = p("/work/dir/line\nbreak");
    fs.open_write(&nl).unwrap().flush().unwrap();
    assert!(names(&*fs, "/work/dir").contains(&"line\nbreak".to_string()));

    // chmod, set_modified, rename, delete.
    fs.set_permissions(&odd, 0o600).unwrap();
    assert_eq!(fs.stat(&odd).unwrap().permissions, Some(0o600));
    let t = chrono::DateTime::from_timestamp(1_600_000_000, 0).unwrap();
    fs.set_modified(&odd, t).unwrap();
    assert_eq!(fs.stat(&odd).unwrap().modified, Some(t));
    let renamed = p("/work/dir/renamed.bin");
    fs.rename(&odd, &renamed).unwrap();
    assert!(matches!(fs.stat(&odd), Err(VfsError::NotFound(_))));
    assert!(
        matches!(
            fs.rename(&renamed, &p("/work")),
            Err(VfsError::AlreadyExists(_))
        ),
        "never moves into a folder"
    );
    fs.remove_file(&renamed).unwrap();
    fs.remove_file(&nl).unwrap();
    fs.remove_dir(&p("/work/dir")).unwrap();
    assert!(names(&*fs, "/work").is_empty());
    assert!(matches!(fs.list(&p("/nope")), Err(VfsError::NotFound(_))));
    assert!(matches!(
        fs.open_read(&p("/nope")),
        Err(VfsError::NotFound(_))
    ));

    // TransferManager: Local -> container -> Local.
    let td = tempfile::tempdir().unwrap();
    let rootdir = td.path().canonicalize().unwrap();
    let lp = |s: &str| VfsPath::new(rootdir.join(s).to_str().unwrap());
    std::fs::create_dir_all(rootdir.join("project/src/empty")).unwrap();
    std::fs::write(rootdir.join("project/READ ME.md"), b"readme").unwrap();
    std::fs::write(rootdir.join("project/src/main.rs"), noise(300_000, 3)).unwrap();
    let local: SharedVfs = Arc::new(LocalFs::new());
    let tm = TransferManager::new(2);
    run_job(
        &tm,
        Job {
            kind: JobKind::Copy,
            src_fs: local.clone(),
            sources: vec![lp("project")],
            dst_fs: Some(fs.clone()),
            dst_dir: Some(p("/work")),
            conflict: ConflictPolicy::Overwrite,
        },
    );
    assert_eq!(names(&*fs, "/work/project"), ["READ ME.md", "src/"]);
    assert_eq!(names(&*fs, "/work/project/src"), ["empty/", "main.rs"]);
    run_job(
        &tm,
        Job {
            kind: JobKind::Copy,
            src_fs: fs.clone(),
            sources: vec![p("/work/project")],
            dst_fs: Some(local.clone()),
            dst_dir: Some(lp("back")),
            conflict: ConflictPolicy::Overwrite,
        },
    );
    assert_eq!(
        std::fs::read(rootdir.join("back/project/READ ME.md")).unwrap(),
        b"readme"
    );
    assert_eq!(
        std::fs::read(rootdir.join("back/project/src/main.rs")).unwrap(),
        noise(300_000, 3)
    );
    assert!(rootdir.join("back/project/src/empty").is_dir());
    run_job(
        &tm,
        Job {
            kind: JobKind::Delete,
            src_fs: fs.clone(),
            sources: vec![p("/work/project")],
            dst_fs: None,
            dst_dir: None,
            conflict: ConflictPolicy::Overwrite,
        },
    );
    assert!(names(&*fs, "/work").is_empty());
}

#[test]
fn stopped_container_is_read_only() {
    if !ready() {
        return;
    }
    // Created, never started: `docker cp` works, `exec` doesn't.
    let ctr = throwaway(&["create", "--label", "manta-test", IMAGE, "true"]);
    let fs = ContainerFs::connect(&docker_config(&ctr.id)).expect("connect");
    assert_eq!(fs.mode(), Mode::Stopped);
    assert!(fs.is_read_only());
    let etc = names(&fs, "/etc");
    assert!(etc.contains(&"alpine-release".to_string()), "{etc:?}");
    let mut s = String::new();
    fs.open_read(&p("/etc/alpine-release"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s.starts_with('3'), "{s}");
    let st = fs.stat(&p("/etc/alpine-release")).unwrap();
    assert_eq!(st.size, s.len() as u64);
    assert!(fs.create_dir(&p("/x")).is_err());
    assert!(matches!(fs.stat(&p("/nope")), Err(VfsError::NotFound(_))));
}

#[test]
fn missing_container_is_a_clear_error() {
    if !ready() {
        return;
    }
    let err = ContainerFs::connect(&docker_config("manta-test-does-not-exist"))
        .err()
        .expect("no such container");
    assert!(
        matches!(err, VfsError::NotFound(ref m) if m.contains("not found")),
        "{err}"
    );
}

/// A journal with one interrupted copy of `src` (on `src_ep`) into
/// `dst_dir` (on `dst_ep`), whose partial file already holds `done` bytes.
fn interrupted_journal(
    path: &std::path::Path,
    src_ep: manta_ops::Endpoint,
    src: &manta_core::Entry,
    dst_ep: manta_ops::Endpoint,
    dst_dir: &VfsPath,
) {
    use manta_ops::journal::{Journal, JournalJob, JournalState};
    use manta_ops::resume::{part_path, Partial, Progress};
    let dst = dst_dir.join(&src.name);
    let progress = Progress {
        partial: Some(Partial {
            src: src.path.to_string(),
            dst: dst.to_string(),
            part: Some(part_path(&dst).to_string()),
            size: src.size,
            mtime: src.modified,
            tag: None,
            session: None,
            choice: None,
        }),
        ..Default::default()
    };
    let job = JournalJob {
        id: 1,
        kind: JobKind::Copy,
        title: "resume".into(),
        state: JournalState::Running,
        conflict: ConflictPolicy::Overwrite,
        src: src_ep,
        dst: Some(dst_ep),
        sources: vec![src.path.to_string()],
        dst_dir: Some(dst_dir.to_string()),
        progress,
        bytes_done: 0,
        bytes_total: src.size,
        files_done: 0,
        files_total: 1,
        errors: Vec::new(),
        log: Vec::new(),
    };
    Journal::new(path.to_path_buf()).save(&[job]).unwrap();
}

/// Restore the journal at `path` and run it to the end; returns the
/// offset the file was continued at.
fn resume_from_journal(path: &std::path::Path, fs: &SharedVfs, local: &SharedVfs) -> Option<u64> {
    use manta_ops::{Endpoint, TransferOptions};
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        journal: Some(path.to_path_buf()),
        ..Default::default()
    });
    assert_eq!(tm.interrupted().len(), 1);
    let ids = tm.restore_interrupted(&|ep: &Endpoint| match ep {
        Endpoint::Local => Some(local.clone()),
        _ => Some(fs.clone()),
    });
    let start = Instant::now();
    let mut resumed = None;
    loop {
        let st = tm.snapshot().into_iter().find(|s| s.id == ids[0]).unwrap();
        resumed = resumed.or(st.resume.resumed_from);
        if st.state.is_finished() {
            assert_eq!(st.state, JobState::Done, "{st:?}");
            return resumed;
        }
        assert!(start.elapsed() < Duration::from_secs(120), "resume timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn resume_in_a_throwaway_container() {
    if !ready() {
        return;
    }
    let ctr = throwaway(&[
        "run",
        "-d",
        "--rm",
        "--label",
        "manta-test",
        "-w",
        "/work",
        IMAGE,
        "sleep",
        "300",
    ]);
    let cfg = docker_config(&ctr.id);
    let fs: SharedVfs = Arc::new(ContainerFs::connect(&cfg).expect("connect"));
    let caps = fs.supports_resume();
    assert!(caps.read_at && caps.append);
    fs.reconnect().expect("container still answers");

    let data = noise(3 * 1024 * 1024 + 5, 11);
    let remote = p("/work/data.bin");
    let mut w = fs.open_write(&remote).unwrap();
    w.write_all(&data).unwrap();
    w.flush().unwrap();
    drop(w);

    // tail -c +N in busybox.
    let mut tail = Vec::new();
    fs.open_read_at(&remote, 1_234_567)
        .unwrap()
        .read_to_end(&mut tail)
        .unwrap();
    assert!(tail == data[1_234_567..], "read_at differs");
    // cat >> in busybox.
    let mut w = fs.open_append(&remote).unwrap();
    w.write_all(b"++").unwrap();
    w.flush().unwrap();
    drop(w);
    assert_eq!(fs.stat(&remote).unwrap().size, data.len() as u64 + 2);
    let mut w = fs.open_write(&remote).unwrap();
    w.write_all(&data).unwrap();
    w.flush().unwrap();
    drop(w);

    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let local: SharedVfs = Arc::new(LocalFs::new());
    let journal = root.join("transfers.json");
    let ep = manta_ops::Endpoint::remote(&cfg);

    // Download interrupted after 1 MB: continues at the part file's size.
    let down = VfsPath::new(root.join("down").to_str().unwrap());
    std::fs::create_dir_all(root.join("down")).unwrap();
    std::fs::write(root.join("down/.data.bin.manta-part"), &data[..1_000_000]).unwrap();
    let src_entry = fs.stat(&remote).unwrap();
    interrupted_journal(
        &journal,
        ep.clone(),
        &src_entry,
        manta_ops::Endpoint::Local,
        &down,
    );
    let at = resume_from_journal(&journal, &fs, &local);
    assert_eq!(at, Some(1_000_000));
    assert!(
        std::fs::read(root.join("down/data.bin")).unwrap() == data,
        "download differs"
    );
    assert!(!root.join("down/.data.bin.manta-part").exists());

    // Upload interrupted after 2 MB: continues with cat >>.
    std::fs::write(root.join("up.bin"), &data).unwrap();
    let local_entry = local
        .stat(&VfsPath::new(root.join("up.bin").to_str().unwrap()))
        .unwrap();
    let mut w = fs.open_write(&p("/work/.up.bin.manta-part")).unwrap();
    w.write_all(&data[..2_000_000]).unwrap();
    w.flush().unwrap();
    drop(w);
    interrupted_journal(
        &journal,
        manta_ops::Endpoint::Local,
        &local_entry,
        ep,
        &p("/work"),
    );
    let at = resume_from_journal(&journal, &fs, &local);
    assert_eq!(at, Some(2_000_000));
    let mut back = Vec::new();
    fs.open_read(&p("/work/up.bin"))
        .unwrap()
        .read_to_end(&mut back)
        .unwrap();
    assert!(back == data, "upload differs");
    assert!(matches!(
        fs.stat(&p("/work/.up.bin.manta-part")),
        Err(VfsError::NotFound(_))
    ));
}

/// Real `docker` / `kubectl` only with `MANTA_TEST_CONTAINERS=1`.
fn containers_opted_in() -> bool {
    let ok = std::env::var_os("MANTA_TEST_CONTAINERS").is_some_and(|v| v == "1");
    if !ok {
        eprintln!("skipped: set MANTA_TEST_CONTAINERS=1 to run tests against docker/kubectl");
    }
    ok
}
