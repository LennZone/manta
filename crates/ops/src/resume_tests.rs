//! Engine tests for pause / resume, network drops and the journal, with
//! [`MemFs`] and a "flaky" wrapper that loses its connection.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use manta_core::{
    Auth, ConnectionConfig, Entry, Protocol, ResumeCaps, SharedVfs, Vfs, VfsError, VfsPath,
    VfsResult,
};
use sha2::{Digest, Sha256};

use crate::memfs::MemFs;
use crate::network::{NetworkMonitor, SimulatedNetwork};
use crate::resume::Endpoint;
use crate::*;

fn t(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn p(s: &str) -> VfsPath {
    VfsPath::new(s)
}

fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 31 + i / 7) as u8).wrapping_add(seed))
        .collect()
}

fn sha(d: &[u8]) -> Vec<u8> {
    Sha256::digest(d).to_vec()
}

fn copy_job(
    src: &SharedVfs,
    sources: &[&str],
    dst: &SharedVfs,
    dst_dir: &str,
    conflict: ConflictPolicy,
) -> Job {
    Job {
        kind: JobKind::Copy,
        src_fs: src.clone(),
        sources: sources.iter().map(|s| p(s)).collect(),
        dst_fs: Some(dst.clone()),
        dst_dir: Some(p(dst_dir)),
        conflict,
    }
}

fn status(tm: &TransferManager, id: JobId) -> JobStatus {
    tm.snapshot()
        .into_iter()
        .find(|s| s.id == id)
        .expect("job present")
}

fn wait_for(
    tm: &TransferManager,
    id: JobId,
    what: &str,
    f: impl Fn(&JobStatus) -> bool,
) -> JobStatus {
    let start = Instant::now();
    loop {
        let st = status(tm, id);
        if f(&st) {
            return st;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timed out waiting for {what}: {st:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_done(tm: &TransferManager, id: JobId) -> JobStatus {
    let st = wait_for(tm, id, "finish", |s| s.state.is_finished());
    assert_eq!(st.state, JobState::Done, "{st:?}");
    st
}

fn fast_backoff() -> Backoff {
    Backoff {
        base: Duration::from_millis(20),
        max: Duration::from_millis(80),
    }
}

// --------------------------------------------------------------- flaky fs

/// A remote-looking [`MemFs`] whose connection drops after `budget` bytes
/// were read, and that refuses `reconnect` while the simulated network is
/// down.
struct FlakyFs {
    mem: MemFs,
    net: Arc<SimulatedNetwork>,
    connected: Arc<AtomicBool>,
    /// Bytes readers may still deliver before the connection drops.
    budget: Arc<AtomicU64>,
    /// Take the network down when the connection drops.
    drop_network: bool,
    /// Offsets passed to `open_read_at`.
    read_offsets: Mutex<Vec<u64>>,
    reconnects: AtomicU64,
}

impl FlakyFs {
    fn new(mem: MemFs, net: Arc<SimulatedNetwork>, budget: u64) -> Arc<FlakyFs> {
        Arc::new(FlakyFs {
            mem,
            net,
            connected: Arc::new(AtomicBool::new(true)),
            budget: Arc::new(AtomicU64::new(budget)),
            drop_network: true,
            read_offsets: Mutex::new(Vec::new()),
            reconnects: AtomicU64::new(0),
        })
    }

    fn up(&self) -> VfsResult<()> {
        if self.connected.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(VfsError::Connection("not connected".into()))
        }
    }
}

struct FlakyReader {
    inner: Box<dyn Read + Send>,
    connected: Arc<AtomicBool>,
    budget: Arc<AtomicU64>,
    net: Arc<SimulatedNetwork>,
    drop_network: bool,
}

impl Read for FlakyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::ConnectionReset));
        }
        let left = self.budget.load(Ordering::SeqCst);
        if left == 0 {
            self.connected.store(false, Ordering::SeqCst);
            if self.drop_network {
                self.net.set_online(false);
            }
            return Err(io::Error::from(io::ErrorKind::ConnectionReset));
        }
        let max = buf.len().min(left as usize);
        let n = self.inner.read(&mut buf[..max])?;
        self.budget.fetch_sub(n as u64, Ordering::SeqCst);
        Ok(n)
    }
}

impl Vfs for FlakyFs {
    fn label(&self) -> String {
        "flaky (SFTP)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Sftp
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.up()?;
        self.mem.list(dir)
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        self.up()?;
        self.mem.stat(path)
    }
    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.up()?;
        self.mem.create_dir(path)
    }
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.up()?;
        self.mem.remove_file(path)
    }
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.up()?;
        self.mem.remove_dir(path)
    }
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.up()?;
        self.mem.rename(from, to)
    }
    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        self.open_read_at(path, 0)
    }
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        self.up()?;
        self.read_offsets.lock().unwrap().push(offset);
        Ok(Box::new(FlakyReader {
            inner: self.mem.open_read_at(path, offset)?,
            connected: self.connected.clone(),
            budget: self.budget.clone(),
            net: self.net.clone(),
            drop_network: self.drop_network,
        }))
    }
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.up()?;
        self.mem.open_write(path)
    }
    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.up()?;
        self.mem.open_append(path)
    }
    fn supports_resume(&self) -> ResumeCaps {
        self.mem.supports_resume()
    }
    fn reconnect(&self) -> VfsResult<()> {
        self.reconnects.fetch_add(1, Ordering::SeqCst);
        if self.net.is_online() {
            self.connected.store(true, Ordering::SeqCst);
            Ok(())
        } else {
            Err(VfsError::Connection("network is unreachable".into()))
        }
    }
}

// ------------------------------------------------------------------ tests

#[test]
fn network_drop_waits_and_resumes_at_the_exact_offset() {
    let net = SimulatedNetwork::new(true);
    let remote = MemFs::new();
    let content = data(3 * 1024 * 1024 + 4321, 1);
    remote.put("/pub/big.iso", &content, t(0));
    let drop_at = 1024 * 1024 + 12_345;
    let flaky = FlakyFs::new(remote, net.clone(), drop_at);
    let local = MemFs::new();
    local.mkdir_p("/dl");
    let (src, dst): (SharedVfs, SharedVfs) = (flaky.clone(), Arc::new(local.clone()));

    // A long backoff: only the "network is back" notification can make the
    // retry happen quickly.
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        network: Some(net.clone()),
        backoff: Backoff {
            base: Duration::from_secs(30),
            max: Duration::from_secs(60),
        },
        ..Default::default()
    });
    let id = tm.enqueue(copy_job(
        &src,
        &["/pub/big.iso"],
        &dst,
        "/dl",
        ConflictPolicy::Overwrite,
    ));
    let st = wait_for(&tm, id, "waiting", |s| {
        matches!(s.state, JobState::WaitingForNetwork { .. })
    });
    let JobState::WaitingForNetwork {
        attempt,
        next_retry,
        error,
    } = &st.state
    else {
        unreachable!()
    };
    assert_eq!(*attempt, 1);
    assert!(next_retry.is_some());
    assert!(!error.is_empty());
    assert!(st.state.retry_in_secs().unwrap() > 20, "{st:?}");
    assert!(!tm.is_online());
    assert!(tm.is_busy(), "a waiting job keeps the queue busy");
    // The partial file holds exactly what arrived; the final name doesn't exist.
    assert_eq!(
        local.read_all("/dl/.big.iso.manta-part").unwrap(),
        content[..drop_at as usize]
    );
    assert!(!local.exists("/dl/big.iso"));
    assert!(!st.state.is_finished(), "no Finished event while waiting");

    // Network back: retried at once, reconnects, continues at the offset.
    flaky.budget.store(u64::MAX, Ordering::SeqCst);
    net.set_online(true);
    let st = wait_done(&tm, id);
    assert_eq!(sha(&local.read_all("/dl/big.iso").unwrap()), sha(&content));
    assert!(!local.exists("/dl/.big.iso.manta-part"));
    // The source is read again 64 KiB before the partial's size: those
    // bytes are compared with the end of the partial file, then the copy
    // goes on at exactly the partial's size.
    assert_eq!(
        *flaky.read_offsets.lock().unwrap(),
        vec![0, drop_at - 64 * 1024]
    );
    assert!(flaky.reconnects.load(Ordering::SeqCst) >= 1);
    assert_eq!(st.bytes_done, content.len() as u64);
    assert_eq!(st.files_done, 1);
}

#[test]
fn reconnect_is_retried_with_backoff_until_the_network_returns() {
    let net = SimulatedNetwork::new(true);
    let remote = MemFs::new();
    let content = data(600_000, 2);
    remote.put("/a.bin", &content, t(0));
    let flaky = FlakyFs::new(remote, net.clone(), 100_000);
    let local = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (flaky.clone(), Arc::new(local.clone()));
    // No network monitor: the manager only has the backoff (and the
    // flaky reconnect keeps failing while the simulated network is down).
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        backoff: fast_backoff(),
        ..Default::default()
    });
    let id = tm.enqueue(copy_job(
        &src,
        &["/a.bin"],
        &dst,
        "/in",
        ConflictPolicy::Overwrite,
    ));
    let st = wait_for(
        &tm,
        id,
        "third attempt",
        |s| matches!(s.state, JobState::WaitingForNetwork { attempt, .. } if attempt >= 3),
    );
    assert!(st.state.retry_in_secs().is_some());
    flaky.budget.store(u64::MAX, Ordering::SeqCst);
    net.set_online(true);
    wait_done(&tm, id);
    assert_eq!(sha(&local.read_all("/in/a.bin").unwrap()), sha(&content));
    assert!(flaky.reconnects.load(Ordering::SeqCst) >= 3);
    assert_eq!(
        *flaky.read_offsets.lock().unwrap(),
        vec![0, 100_000 - 64 * 1024]
    );
}

#[test]
fn permanent_errors_do_not_wait() {
    let src_mem = MemFs::new();
    src_mem.put("/x/ok.txt", b"ok", t(0));
    src_mem.put("/x/secret.txt", b"no", t(0));
    src_mem.set_unreadable("/x/secret.txt");
    let net = SimulatedNetwork::new(true);
    let flaky = FlakyFs::new(src_mem, net, u64::MAX);
    let dst = MemFs::new();
    let tm = TransferManager::new(1);
    let src: SharedVfs = flaky;
    let id = tm.enqueue(copy_job(
        &src,
        &["/x"],
        &(Arc::new(dst.clone()) as SharedVfs),
        "/d",
        ConflictPolicy::Overwrite,
    ));
    let st = wait_done(&tm, id);
    assert_eq!(st.errors.len(), 1, "{st:?}");
    assert!(dst.exists("/d/x/ok.txt"));
    assert!(!dst.exists("/d/x/.secret.txt.manta-part"));
}

/// Throttled source so a job can be paused half-way.
fn slow_source(len: usize) -> (MemFs, Vec<u8>) {
    let m = MemFs::new();
    let content = data(len, 3);
    m.put("/s/movie.mov", &content, t(0));
    m.set_read_throttle(32 * 1024, Some(Duration::from_millis(3)));
    (m, content)
}

fn pause_midway(tm: &TransferManager, id: JobId) -> JobStatus {
    wait_for(tm, id, "progress", |s| s.bytes_done > 200_000);
    tm.pause(id);
    wait_for(tm, id, "paused", |s| s.state == JobState::Paused)
}

#[test]
fn pause_and_resume_continue_the_partial_file() {
    let (a, content) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    let st = pause_midway(&tm, id);
    assert!(st.resume.can_pause);
    assert!(!tm.is_busy(), "paused jobs don't keep the queue busy");
    let part = b
        .read_all("/d/.movie.mov.manta-part")
        .expect("partial file kept");
    assert!(!part.is_empty() && part.len() < content.len());
    assert_eq!(part, content[..part.len()]);
    assert!(!b.exists("/d/movie.mov"));
    // Nothing more is written while paused.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        b.read_all("/d/.movie.mov.manta-part").unwrap().len(),
        part.len()
    );

    tm.resume(id);
    let st = wait_for(&tm, id, "resumed", |s| {
        s.resume.resumed_from.is_some() || s.state.is_finished()
    });
    assert_eq!(st.resume.resumed_from, Some(part.len() as u64));
    let st = wait_done(&tm, id);
    assert_eq!(st.bytes_done, content.len() as u64);
    assert_eq!(sha(&b.read_all("/d/movie.mov").unwrap()), sha(&content));
    assert!(!b.exists("/d/.movie.mov.manta-part"));
}

#[test]
fn pause_all_and_resume_all() {
    let (a, content) = slow_source(1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    let first = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/one",
        ConflictPolicy::Overwrite,
    ));
    let second = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/two",
        ConflictPolicy::Overwrite,
    ));
    wait_for(&tm, first, "progress", |s| s.bytes_done > 100_000);
    tm.pause_all();
    wait_for(&tm, first, "paused", |s| s.state == JobState::Paused);
    assert_eq!(
        status(&tm, second).state,
        JobState::Paused,
        "queued jobs pause at once"
    );
    tm.resume_all();
    wait_done(&tm, first);
    wait_done(&tm, second);
    assert_eq!(b.read_all("/one/movie.mov").unwrap(), content);
    assert_eq!(b.read_all("/two/movie.mov").unwrap(), content);
}

#[test]
fn cancel_removes_the_partial_file() {
    let (a, _) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    // Cancel while paused (the job isn't running: cleanup in the background).
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    pause_midway(&tm, id);
    assert!(b.exists("/d/.movie.mov.manta-part"));
    tm.cancel(id);
    assert_eq!(status(&tm, id).state, JobState::Cancelled);
    let start = Instant::now();
    while b.exists("/d/.movie.mov.manta-part") {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "partial file not removed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // Cancel while running.
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/e",
        ConflictPolicy::Overwrite,
    ));
    wait_for(&tm, id, "progress", |s| s.bytes_done > 100_000);
    tm.cancel(id);
    let st = wait_for(&tm, id, "cancelled", |s| s.state.is_finished());
    assert_eq!(st.state, JobState::Cancelled);
    assert!(!b.exists("/e/.movie.mov.manta-part"));
    assert!(!b.exists("/e/movie.mov"));
}

#[test]
fn changed_source_restarts_from_zero() {
    let (a, _) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    pause_midway(&tm, id);
    let new_content = data(1024 * 1024 + 99, 9);
    a.put("/s/movie.mov", &new_content, t(500));
    tm.resume(id);
    let st = wait_done(&tm, id);
    assert_eq!(sha(&b.read_all("/d/movie.mov").unwrap()), sha(&new_content));
    assert!(
        st.resume.log.iter().any(|l| l.contains("source changed")),
        "{:?}",
        st.resume.log
    );
    assert_eq!(st.resume.resumed_from, None);
}

#[test]
fn destination_without_append_restarts_from_zero() {
    let (a, content) = slow_source(1024 * 1024);
    let b = MemFs::new();
    b.disable_append();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    pause_midway(&tm, id);
    tm.resume(id);
    let st = wait_done(&tm, id);
    assert_eq!(b.read_all("/d/movie.mov").unwrap(), content);
    assert!(
        st.resume.log.iter().any(|l| l.contains("can't continue")),
        "{:?}",
        st.resume.log
    );
}

#[test]
fn conflict_policy_applies_at_the_final_rename() {
    for (policy, expect_final, expect_other) in [
        (ConflictPolicy::Overwrite, Some(true), None),
        (ConflictPolicy::Skip, Some(false), None),
        (ConflictPolicy::OverwriteIfNewer, Some(true), None),
        (ConflictPolicy::Rename, Some(false), Some("/d/movie 2.mov")),
    ] {
        let (a, content) = slow_source(1024 * 1024);
        let b = MemFs::new();
        let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
        let tm = TransferManager::new(1);
        let id = tm.enqueue(copy_job(&src, &["/s/movie.mov"], &dst, "/d", policy));
        pause_midway(&tm, id);
        // Someone else creates the target meanwhile (older than the source).
        b.put("/d/movie.mov", b"theirs", t(-1000));
        tm.resume(id);
        wait_done(&tm, id);
        let fin = b.read_all("/d/movie.mov").unwrap();
        assert_eq!(fin == content, expect_final.unwrap(), "{policy:?}");
        if !expect_final.unwrap() {
            assert_eq!(fin, b"theirs", "{policy:?}");
        }
        if let Some(other) = expect_other {
            assert_eq!(b.read_all(other).unwrap(), content, "{policy:?}");
        }
        assert!(!b.exists("/d/.movie.mov.manta-part"), "{policy:?}");
    }
}

#[test]
fn rerun_skips_finished_files_and_keeps_both_names() {
    let a = MemFs::new();
    a.put("/tree/1.bin", &data(300_000, 4), t(0));
    a.put("/tree/2.bin", &data(300_000, 5), t(0));
    a.put("/tree/3.bin", &data(300_000, 6), t(0));
    a.set_read_throttle(32 * 1024, Some(Duration::from_millis(2)));
    let b = MemFs::new();
    b.put("/d/tree/old.txt", b"existing folder", t(0));
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(copy_job(
        &src,
        &["/tree"],
        &dst,
        "/d",
        ConflictPolicy::Rename,
    ));
    wait_for(&tm, id, "second file", |s| {
        s.files_done >= 1 && s.bytes_done > 400_000
    });
    tm.pause(id);
    wait_for(&tm, id, "paused", |s| s.state == JobState::Paused);
    tm.resume(id);
    let st = wait_done(&tm, id);
    // Still "tree 2", not "tree 3", after the re-run.
    assert!(!b.exists("/d/tree 3"));
    for n in ["1", "2", "3"] {
        assert_eq!(
            b.read_all(&format!("/d/tree 2/{n}.bin")).unwrap(),
            a.read_all(&format!("/tree/{n}.bin")).unwrap()
        );
    }
    assert_eq!(st.files_done, 3);
    assert_eq!(st.files_total, 3);
}

#[test]
fn move_resumes_and_removes_sources() {
    let net = SimulatedNetwork::new(true);
    let remote = MemFs::new();
    remote.put("/m/a.bin", &data(200_000, 7), t(0));
    remote.put("/m/b.bin", &data(200_000, 8), t(0));
    let flaky = FlakyFs::new(remote.clone(), net.clone(), 300_000);
    let local = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (flaky.clone(), Arc::new(local.clone()));
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        network: Some(net.clone()),
        backoff: fast_backoff(),
        ..Default::default()
    });
    let mut job = copy_job(&src, &["/m"], &dst, "/in", ConflictPolicy::Overwrite);
    job.kind = JobKind::Move;
    let id = tm.enqueue(job);
    wait_for(&tm, id, "waiting", |s| {
        matches!(s.state, JobState::WaitingForNetwork { .. })
    });
    flaky.budget.store(u64::MAX, Ordering::SeqCst);
    net.set_online(true);
    wait_done(&tm, id);
    assert_eq!(local.read_all("/in/m/a.bin").unwrap(), data(200_000, 7));
    assert_eq!(local.read_all("/in/m/b.bin").unwrap(), data(200_000, 8));
    assert!(
        !remote.exists("/m"),
        "sources removed after the move: {:?}",
        remote.paths()
    );
}

#[test]
fn journal_restores_jobs_after_a_restart() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    let (a, content) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let server = ConnectionConfig {
        name: "Media".into(),
        protocol: Protocol::Sftp,
        host: "media.example".into(),
        port: 22,
        username: "me".into(),
        auth: Auth::Password("pw".into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: None,
        smb: None,
    };
    let src_ep = Endpoint::remote(&server);
    let opts = || TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        ..Default::default()
    };

    // First run: the app quits in the middle of the copy.
    {
        let tm = TransferManager::with_options(opts());
        assert!(tm.interrupted().is_empty());
        let id = tm.enqueue_with_endpoints(
            copy_job(
                &src,
                &["/s/movie.mov"],
                &dst,
                "/d",
                ConflictPolicy::Overwrite,
            ),
            src_ep.clone(),
            Some(Endpoint::Local),
        );
        wait_for(&tm, id, "progress", |s| s.bytes_done > 300_000);
        // An unrestorable job (unknown backend) is not journaled.
        tm.enqueue(copy_job(
            &src,
            &["/s/movie.mov"],
            &dst,
            "/other",
            ConflictPolicy::Overwrite,
        ));
    }
    // Dropping the manager wrote the journal one last time.
    let text = std::fs::read_to_string(&journal).expect("journal written");
    assert!(!text.contains("\"pw\""), "no secrets in the journal");
    let part_len = b
        .read_all("/d/.movie.mov.manta-part")
        .expect("partial kept at quit")
        .len() as u64;
    assert!(part_len > 0);

    // Second run: offered, nothing starts by itself.
    let tm = TransferManager::with_options(opts());
    let pending = tm.interrupted();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert!(tm.snapshot().is_empty());
    // The pending job stays in the journal until the user decides.
    drop(tm);
    let tm = TransferManager::with_options(opts());
    assert_eq!(tm.interrupted().len(), 1);

    // Restore with only the local side open: waits for the server.
    let local_only = |ep: &Endpoint| matches!(ep, Endpoint::Local).then(|| dst.clone());
    let ids = tm.restore_interrupted(&local_only);
    assert_eq!(ids.len(), 1);
    let st = status(&tm, ids[0]);
    assert_eq!(st.state, JobState::NeedsConnection("Media (SFTP)".into()));
    assert!(st.resume.restored);
    assert_eq!(st.resume.needs.len(), 1);
    assert!(tm.interrupted().is_empty());

    // The user reconnects: the job continues at the partial file's size.
    tm.provide_connection(&Endpoint::remote(&server), src.clone());
    let st = wait_for(&tm, ids[0], "resumed", |s| {
        s.resume.resumed_from.is_some() || s.state.is_finished()
    });
    assert!(st.resume.resumed_from.unwrap_or(0) >= part_len, "{st:?}");
    wait_done(&tm, ids[0]);
    assert_eq!(sha(&b.read_all("/d/movie.mov").unwrap()), sha(&content));
    drop(tm);
    assert!(!journal.exists(), "an empty queue removes the journal");
}

#[test]
fn discarding_interrupted_jobs_removes_partials() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    let (a, _) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let opts = || TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        ..Default::default()
    };
    {
        let tm = TransferManager::with_options(opts());
        let id = tm.enqueue_with_endpoints(
            copy_job(
                &src,
                &["/s/movie.mov"],
                &dst,
                "/d",
                ConflictPolicy::Overwrite,
            ),
            Endpoint::Local,
            Some(Endpoint::Local),
        );
        pause_midway(&tm, id);
    }
    assert!(b.exists("/d/.movie.mov.manta-part"));
    let tm = TransferManager::with_options(opts());
    assert_eq!(tm.interrupted().len(), 1);
    tm.discard_interrupted(&|_: &Endpoint| Some(dst.clone()));
    assert!(tm.interrupted().is_empty());
    let start = Instant::now();
    while b.exists("/d/.movie.mov.manta-part") {
        assert!(start.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(tm);
    assert!(!journal.exists());
}

#[test]
fn journal_is_throttled_but_follows_state_changes() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    let (a, _) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        journal_interval: Duration::from_millis(200),
        ..Default::default()
    });
    let id = tm.enqueue_with_endpoints(
        copy_job(
            &src,
            &["/s/movie.mov"],
            &dst,
            "/d",
            ConflictPolicy::Overwrite,
        ),
        Endpoint::Local,
        Some(Endpoint::Local),
    );
    pause_midway(&tm, id);
    // A state change is written right away (by the writer thread): wait for
    // it rather than for a time.
    let load = || crate::journal::Journal::new(journal.clone()).load();
    let start = Instant::now();
    while !load()
        .first()
        .is_some_and(|j| j.state == JournalState::Paused)
    {
        assert!(start.elapsed() < Duration::from_secs(60), "{:?}", load());
        std::thread::sleep(Duration::from_millis(10));
    }
    let jobs = load();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, JournalState::Paused);
    let partial = jobs[0]
        .progress
        .partial
        .as_ref()
        .expect("file in flight recorded");
    assert_eq!(partial.part.as_deref(), Some("/d/.movie.mov.manta-part"));
    assert_eq!(partial.size, 2 * 1024 * 1024);
    tm.resume(id);
    wait_done(&tm, id);
    let start = Instant::now();
    while journal.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "finished jobs leave the journal"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ------------------------------------------- quit while a chunk is written

/// The restored job's resolver: `Local` is `dst`, anything else `src`.
fn resolver<'a>(
    src: &'a SharedVfs,
    dst: &'a SharedVfs,
) -> impl Fn(&Endpoint) -> Option<SharedVfs> + 'a {
    move |ep: &Endpoint| {
        Some(if matches!(ep, Endpoint::Local) {
            dst.clone()
        } else {
            src.clone()
        })
    }
}

/// Starts a journaled copy of `/s/movie.mov` and freezes its worker in the
/// middle of writing a chunk (the write gate is closed).
fn copy_frozen_mid_chunk(
    opts: TransferOptions,
    src: &SharedVfs,
    dst: &SharedVfs,
    b: &MemFs,
) -> (TransferManager, Arc<crate::memfs::WriteGate>) {
    let tm = TransferManager::with_options(opts);
    let id = tm.enqueue_with_endpoints(
        copy_job(src, &["/s/movie.mov"], dst, "/d", ConflictPolicy::Overwrite),
        media_endpoint(),
        Some(Endpoint::Local),
    );
    wait_for(&tm, id, "progress", |s| s.bytes_done > 300_000);
    let gate = crate::memfs::WriteGate::closed();
    b.set_write_gate(Some(gate.clone()));
    assert!(gate.wait_for_writers(1, Duration::from_secs(60)));
    (tm, gate)
}

/// The CI failure of `journal_restores_jobs_after_a_restart`: the worker
/// of the quit manager wrote one more chunk into the part file after the
/// restored job had taken the part file's size as its offset, so that
/// chunk ended up twice in the file — and the job still reported success.
#[test]
fn resume_never_appends_while_an_old_worker_still_writes() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    let (a, content) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let opts = || TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        // The old worker stays stuck in its write past the quit (like a
        // network write that takes longer than the shutdown wait).
        shutdown_timeout: Duration::ZERO,
        ..Default::default()
    };
    let (tm, gate) = copy_frozen_mid_chunk(opts(), &src, &dst, &b);
    drop(tm);
    let tm = TransferManager::with_options(opts());
    let ids = tm.restore_interrupted(&resolver(&src, &dst));
    // The restored job must not continue while the old worker can still
    // write: it waits for it (before the fix it continued at once).
    let st = wait_for(&tm, ids[0], "waiting or resumed", |s| {
        s.resume.resumed_from.is_some()
            || s.resume.log.iter().any(|l| l.contains("earlier transfer"))
    });
    assert_eq!(st.resume.resumed_from, None, "{st:?}");
    gate.open();
    // Once the old worker stopped, the job continues the partial file.
    let st = wait_for(&tm, ids[0], "resumed", |s| {
        s.resume.resumed_from.is_some() || s.state.is_finished()
    });
    assert!(st.resume.resumed_from.is_some(), "{st:?}");
    let st = wait_done(&tm, ids[0]);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    let got = b.read_all("/d/movie.mov").unwrap();
    assert_eq!(got.len(), content.len());
    assert_eq!(sha(&got), sha(&content));
}

/// Quitting waits for the workers to stop writing, so the partial file is
/// final when the manager is gone and the last journal write follows it.
#[test]
fn quit_waits_for_workers_before_the_last_journal_write() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    let (a, content) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let opts = TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        shutdown_timeout: Duration::from_secs(600),
        ..Default::default()
    };
    let (tm, gate) = copy_frozen_mid_chunk(opts, &src, &dst, &b);
    let shared = tm.shared_for_tests();
    let quit = std::thread::spawn(move || drop(tm));
    // The quit waits for the frozen write: release it.
    gate.open();
    quit.join().unwrap();
    assert_eq!(shared.running_threads(), 0, "all transfer threads joined");
    let part = b.read_all("/d/.movie.mov.manta-part").unwrap();
    assert!(!part.is_empty() && part.len() < content.len());
    assert_eq!(part, content[..part.len()], "the partial file is a prefix");
    let jobs = crate::journal::Journal::new(journal.clone()).load();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, JournalState::Paused, "{:?}", jobs[0]);
    assert_eq!(
        jobs[0].bytes_done,
        part.len() as u64,
        "journal matches the file"
    );
    // Resuming continues exactly there.
    let tm = TransferManager::with_options(TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        ..Default::default()
    });
    let ids = tm.restore_interrupted(&resolver(&src, &dst));
    let st = wait_for(&tm, ids[0], "resumed", |s| {
        s.resume.resumed_from.is_some() || s.state.is_finished()
    });
    assert_eq!(st.resume.resumed_from, Some(part.len() as u64));
    wait_done(&tm, ids[0]);
    assert_eq!(sha(&b.read_all("/d/movie.mov").unwrap()), sha(&content));
}

#[test]
fn a_partial_file_that_does_not_match_the_source_is_not_continued() {
    let (a, content) = slow_source(2 * 1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    pause_midway(&tm, id);
    // Something else changed the end of the partial file.
    let mut part = b.read_all("/d/.movie.mov.manta-part").unwrap();
    let n = part.len();
    part[n - 10..].copy_from_slice(&[0xAA; 10]);
    b.put("/d/.movie.mov.manta-part", &part, t(0));
    tm.resume(id);
    let st = wait_done(&tm, id);
    assert_eq!(st.state, JobState::Done);
    assert_eq!(st.resume.resumed_from, None, "{st:?}");
    assert!(
        st.resume.log.iter().any(|l| l.contains("doesn't match")),
        "{st:?}"
    );
    assert_eq!(sha(&b.read_all("/d/movie.mov").unwrap()), sha(&content));
}

#[test]
fn a_copy_of_the_wrong_size_is_redone_and_never_reported_as_done() {
    let content = data(600_000, 5);
    let a = MemFs::new();
    a.put("/s/f.bin", &content, t(0));
    a.set_read_throttle(32 * 1024, None);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(1);
    // One chunk is lost: the copy is redone from the start.
    b.lose_writes(1);
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/f.bin"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    let st = wait_done(&tm, id);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert!(
        st.resume.log.iter().any(|l| l.contains("instead of")),
        "{st:?}"
    );
    assert_eq!(st.bytes_done, content.len() as u64);
    assert_eq!(sha(&b.read_all("/d/f.bin").unwrap()), sha(&content));
    // Losing data again on the second try: an error, not a bad file.
    b.lose_writes(usize::MAX);
    let id = tm.enqueue(copy_job(
        &src,
        &["/s/f.bin"],
        &dst,
        "/e",
        ConflictPolicy::Overwrite,
    ));
    let st = wait_done(&tm, id);
    assert!(!st.errors.is_empty(), "{st:?}");
    assert!(!b.exists("/e/f.bin"), "{:?}", b.paths());
    assert!(!b.exists("/e/.f.bin.manta-part"), "{:?}", b.paths());
}

#[test]
fn the_same_file_copied_twice_at_once_uses_separate_partial_files() {
    let (a, content) = slow_source(1024 * 1024);
    let b = MemFs::new();
    let (src, dst): (SharedVfs, SharedVfs) = (Arc::new(a.clone()), Arc::new(b.clone()));
    let tm = TransferManager::new(2);
    let gate = crate::memfs::WriteGate::closed();
    b.set_write_gate(Some(gate.clone()));
    let one = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    let two = tm.enqueue(copy_job(
        &src,
        &["/s/movie.mov"],
        &dst,
        "/d",
        ConflictPolicy::Overwrite,
    ));
    // Both are writing at the same time.
    assert!(gate.wait_for_writers(2, Duration::from_secs(60)));
    let parts: Vec<String> = b
        .paths()
        .into_iter()
        .filter(|p| p.ends_with(".manta-part"))
        .collect();
    assert_eq!(
        parts,
        vec!["/d/.movie.mov.manta-part", "/d/.movie.mov~2.manta-part"]
    );
    gate.open();
    for id in [one, two] {
        let st = wait_done(&tm, id);
        assert_eq!(st.state, JobState::Done, "{st:?}");
        assert!(st.errors.is_empty(), "{st:?}");
    }
    assert_eq!(sha(&b.read_all("/d/movie.mov").unwrap()), sha(&content));
    assert_eq!(b.paths(), vec!["/d", "/d/movie.mov"]);
}

fn media_endpoint() -> Endpoint {
    Endpoint::remote(&ConnectionConfig {
        name: "Media".into(),
        protocol: Protocol::Sftp,
        host: "media.example".into(),
        port: 22,
        username: "me".into(),
        auth: Auth::None,
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: None,
        smb: None,
    })
}
