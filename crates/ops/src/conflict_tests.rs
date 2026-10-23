//! `ConflictPolicy::Ask`: the job stops at an existing item until the user
//! answers ([`TransferManager::resolve_conflict`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use manta_core::{SharedVfs, VfsPath};

use crate::memfs::MemFs;
use crate::resume::Endpoint;
use crate::*;

fn t(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn p(s: &str) -> VfsPath {
    VfsPath::new(s)
}

fn shared(m: &MemFs) -> SharedVfs {
    Arc::new(m.clone())
}

fn job(kind: JobKind, src: &MemFs, sources: &[&str], dst: &MemFs) -> Job {
    Job {
        kind,
        src_fs: shared(src),
        sources: sources.iter().map(|s| p(s)).collect(),
        dst_fs: Some(shared(dst)),
        dst_dir: Some(p("/d")),
        conflict: ConflictPolicy::Ask,
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
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}: {st:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn question(tm: &TransferManager, id: JobId) -> ConflictQuestion {
    let st = wait_for(tm, id, "a question", |s| {
        s.state.question().is_some() || s.state.is_finished()
    });
    st.state
        .question()
        .cloned()
        .unwrap_or_else(|| panic!("no question: {st:?}"))
}

fn finished(tm: &TransferManager, id: JobId) -> JobStatus {
    wait_for(tm, id, "the end", |s| s.state.is_finished())
}

/// Source: a.txt (newer, conflicts) and b.txt (new). Destination: an older a.txt.
fn setup() -> (MemFs, MemFs) {
    let a = MemFs::new();
    a.put("/s/a.txt", b"new content", t(100));
    a.put("/s/b.txt", b"b", t(0));
    let b = MemFs::new();
    b.put("/d/a.txt", b"old", t(0));
    (a, b)
}

#[test]
fn asks_and_replaces() {
    let (a, b) = setup();
    let tm = TransferManager::new(1);
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/a.txt", "/s/b.txt"], &b));
    let q = question(&tm, id);
    assert_eq!(q.src.path, p("/s/a.txt"));
    assert_eq!(q.dst.path, p("/d/a.txt"));
    assert_eq!((q.src.size, q.dst.size), (Some(11), Some(3)));
    assert!(!q.src.is_dir && !q.dst.is_dir);
    assert_eq!(q.source_newer(), Some(true));
    assert_eq!(
        q.choices,
        vec![
            ConflictChoice::Replace,
            ConflictChoice::ReplaceIfNewer,
            ConflictChoice::KeepBoth,
            ConflictChoice::Skip
        ]
    );
    assert!(status(&tm, id).state.is_suspended());
    assert!(
        tm.is_busy(),
        "a job waiting for a decision keeps the app busy"
    );
    // Not paused: the transfers panel offers no resume.
    tm.resolve_conflict(id, ConflictChoice::Replace, false);
    let st = finished(&tm, id);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"new content");
    assert_eq!(b.read_all("/d/b.txt").unwrap(), b"b");
    assert_eq!(st.files_done, 2);
    assert_eq!(st.bytes_done, st.bytes_total);
}

#[test]
fn keep_both_and_skip() {
    let (a, b) = setup();
    let tm = TransferManager::new(1);
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/a.txt"], &b));
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::KeepBoth, false);
    assert_eq!(finished(&tm, id).state, JobState::Done);
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"old");
    assert_eq!(b.read_all("/d/a 2.txt").unwrap(), b"new content");

    let (a, b) = setup();
    let id = tm.enqueue(job(JobKind::Move, &a, &["/s/a.txt", "/s/b.txt"], &b));
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::Skip, false);
    let st = finished(&tm, id);
    assert_eq!(st.state, JobState::Done);
    assert_eq!(st.files_done, 2);
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"old");
    assert!(
        a.exists("/s/a.txt"),
        "a skipped item stays at the source of a move"
    );
    assert!(!a.exists("/s/b.txt"), "the rest was moved");
    assert!(
        st.errors.is_empty(),
        "skipping is no error: {:?}",
        st.errors
    );
}

#[test]
fn replace_if_newer_keeps_a_newer_destination() {
    let a = MemFs::new();
    a.put("/s/a.txt", b"older source", t(0));
    let b = MemFs::new();
    b.put("/d/a.txt", b"newer", t(100));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/a.txt"], &b));
    assert_eq!(question(&tm, id).source_newer(), Some(false));
    tm.resolve_conflict(id, ConflictChoice::ReplaceIfNewer, false);
    assert_eq!(finished(&tm, id).state, JobState::Done);
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"newer");
}

#[test]
fn every_conflict_asks_unless_applied_to_all() {
    let a = MemFs::new();
    let b = MemFs::new();
    for n in ["x", "y", "z"] {
        a.put(&format!("/s/{n}.txt"), b"src", t(10));
        b.put(&format!("/d/{n}.txt"), b"dst", t(0));
    }
    let srcs = ["/s/x.txt", "/s/y.txt", "/s/z.txt"];
    let tm = TransferManager::new(1);

    // One by one.
    let id = tm.enqueue(job(JobKind::Copy, &a, &srcs, &b));
    for (n, choice) in [
        ("x", ConflictChoice::Replace),
        ("y", ConflictChoice::Skip),
        ("z", ConflictChoice::KeepBoth),
    ] {
        let q = question(&tm, id);
        assert_eq!(q.src.path, p(&format!("/s/{n}.txt")));
        tm.resolve_conflict(id, choice, false);
        wait_for(&tm, id, "the next item", |s| {
            s.state
                .question()
                .is_none_or(|q| q.src.path != p(&format!("/s/{n}.txt")))
        });
    }
    assert_eq!(finished(&tm, id).state, JobState::Done);
    assert_eq!(b.read_all("/d/x.txt").unwrap(), b"src");
    assert_eq!(b.read_all("/d/y.txt").unwrap(), b"dst");
    assert_eq!(b.read_all("/d/z 2.txt").unwrap(), b"src");

    // "Apply to all" on the first question: no further questions.
    let b = MemFs::new();
    for n in ["x", "y", "z"] {
        b.put(&format!("/d/{n}.txt"), b"dst", t(0));
    }
    let id = tm.enqueue(job(JobKind::Copy, &a, &srcs, &b));
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::Replace, true);
    let st = finished(&tm, id);
    assert_eq!(st.state, JobState::Done);
    for n in ["x", "y", "z"] {
        assert_eq!(b.read_all(&format!("/d/{n}.txt")).unwrap(), b"src");
    }
}

#[test]
fn cancel_while_waiting_keeps_what_is_done() {
    // Cancel from the panel (stop button).
    let (a, b) = setup();
    let tm = TransferManager::new(1);
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/b.txt", "/s/a.txt"], &b));
    question(&tm, id);
    tm.cancel(id);
    assert_eq!(finished(&tm, id).state, JobState::Cancelled);
    assert_eq!(b.read_all("/d/b.txt").unwrap(), b"b", "done items stay");
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"old");

    // "Cancel" answered in the dialog.
    let (a, b) = setup();
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/b.txt", "/s/a.txt"], &b));
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::Cancel, false);
    assert_eq!(finished(&tm, id).state, JobState::Cancelled);
    assert_eq!(b.read_all("/d/b.txt").unwrap(), b"b");
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"old");
    assert!(
        !b.paths().iter().any(|p| p.contains("manta-part")),
        "{:?}",
        b.paths()
    );
}

#[test]
fn other_jobs_keep_running_while_one_waits() {
    let (a, b) = setup();
    let tm = TransferManager::new(1);
    let asking = tm.enqueue(job(JobKind::Copy, &a, &["/s/a.txt"], &b));
    question(&tm, asking);
    let other = MemFs::new();
    let mut j = job(JobKind::Copy, &a, &["/s/b.txt"], &other);
    j.dst_dir = Some(p("/elsewhere"));
    let id = tm.enqueue(j);
    assert_eq!(
        finished(&tm, id).state,
        JobState::Done,
        "the only worker isn't blocked by the question"
    );
    assert!(status(&tm, asking).state.question().is_some());
    tm.resolve_conflict(asking, ConflictChoice::Skip, false);
    assert_eq!(finished(&tm, asking).state, JobState::Done);
}

#[test]
fn pause_while_waiting_asks_again_on_resume() {
    let (a, b) = setup();
    let tm = TransferManager::new(1);
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/a.txt"], &b));
    question(&tm, id);
    tm.pause(id);
    wait_for(&tm, id, "paused", |s| s.state == JobState::Paused);
    tm.resume(id);
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::Replace, false);
    assert_eq!(finished(&tm, id).state, JobState::Done);
    assert_eq!(b.read_all("/d/a.txt").unwrap(), b"new content");
}

#[test]
fn folders_merge_keep_both_or_skip() {
    let setup = || {
        let a = MemFs::new();
        a.put("/s/photos/x.jpg", b"new x", t(10));
        a.put("/s/photos/y.jpg", b"y", t(10));
        let b = MemFs::new();
        b.put("/d/photos/x.jpg", b"old x", t(0));
        b.put("/d/photos/keep.jpg", b"k", t(0));
        (a, b)
    };
    let tm = TransferManager::new(1);

    // Merge: the folder question, then one for the file inside.
    let (a, b) = setup();
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/photos"], &b));
    let q = question(&tm, id);
    assert!(q.src.is_dir && q.dst.is_dir);
    assert_eq!(q.src.size, None);
    assert_eq!(
        q.choices,
        vec![
            ConflictChoice::Merge,
            ConflictChoice::KeepBoth,
            ConflictChoice::Skip
        ]
    );
    tm.resolve_conflict(id, ConflictChoice::Merge, false);
    let q = wait_for(&tm, id, "file question", |s| {
        s.state.question().is_some_and(|q| !q.src.is_dir)
    });
    assert_eq!(q.state.question().unwrap().dst.path, p("/d/photos/x.jpg"));
    tm.resolve_conflict(id, ConflictChoice::Replace, false);
    assert_eq!(finished(&tm, id).state, JobState::Done);
    assert_eq!(b.read_all("/d/photos/x.jpg").unwrap(), b"new x");
    assert_eq!(b.read_all("/d/photos/y.jpg").unwrap(), b"y");
    assert_eq!(
        b.read_all("/d/photos/keep.jpg").unwrap(),
        b"k",
        "merge keeps the other files"
    );

    // Keep both: "photos 2".
    let (a, b) = setup();
    let id = tm.enqueue(job(JobKind::Copy, &a, &["/s/photos"], &b));
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::KeepBoth, false);
    assert_eq!(finished(&tm, id).state, JobState::Done);
    assert_eq!(b.read_all("/d/photos 2/x.jpg").unwrap(), b"new x");
    assert_eq!(b.read_all("/d/photos/x.jpg").unwrap(), b"old x");

    // Skip, applied to all: the second folder isn't asked about either.
    let (a, b) = setup();
    a.put("/s/music/m.mp3", b"m", t(0));
    b.put("/d/music/m.mp3", b"old m", t(0));
    let id = tm.enqueue(job(JobKind::Move, &a, &["/s/photos", "/s/music"], &b));
    question(&tm, id);
    tm.resolve_conflict(id, ConflictChoice::Skip, true);
    let st = finished(&tm, id);
    assert_eq!(st.state, JobState::Done);
    assert!(!b.exists("/d/photos/y.jpg"));
    assert_eq!(b.read_all("/d/music/m.mp3").unwrap(), b"old m");
    assert!(
        a.exists("/s/photos/x.jpg") && a.exists("/s/music/m.mp3"),
        "skipped folders stay"
    );
}

#[test]
fn restart_while_waiting_asks_again() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    // One backend for both sides, so the restore can reopen it as "local".
    let fs = MemFs::new();
    fs.put("/s/a.txt", b"new content", t(100));
    fs.put("/s/b.txt", b"b", t(0));
    fs.put("/s/c.txt", b"c", t(0));
    fs.put("/d/a.txt", b"old", t(0));
    fs.put("/d/c.txt", b"old c", t(0));
    let opts = || TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        ..Default::default()
    };
    {
        let tm = TransferManager::with_options(opts());
        let id = tm.enqueue_with_endpoints(
            job(
                JobKind::Copy,
                &fs,
                &["/s/b.txt", "/s/a.txt", "/s/c.txt"],
                &fs,
            ),
            Endpoint::Local,
            Some(Endpoint::Local),
        );
        assert_eq!(question(&tm, id).src.path, p("/s/a.txt"));
        // The journal is written asynchronously; wait (bounded) under load.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            text = std::fs::read_to_string(&journal).unwrap_or_default();
            if text.contains("\"deciding\"") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(text.contains("\"deciding\""), "{text}");
        // Quit while the question is open.
    }
    std::thread::sleep(Duration::from_millis(100));
    let tm = TransferManager::with_options(opts());
    let pending = tm.interrupted();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].conflict, ConflictPolicy::Ask);
    let backend = shared(&fs);
    let ids = tm.restore_interrupted(&|_: &Endpoint| Some(backend.clone()));
    let q = question(&tm, ids[0]);
    assert_eq!(q.src.path, p("/s/a.txt"), "asks again about the same item");
    // Apply to all survives a further restart: persisted with the job.
    tm.resolve_conflict(ids[0], ConflictChoice::KeepBoth, true);
    let st = finished(&tm, ids[0]);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(fs.read_all("/d/b.txt").unwrap(), b"b");
    assert_eq!(fs.read_all("/d/a 2.txt").unwrap(), b"new content");
    assert_eq!(
        fs.read_all("/d/c 2.txt").unwrap(),
        b"c",
        "no second question"
    );
    assert_eq!(fs.read_all("/d/a.txt").unwrap(), b"old");
}

#[test]
fn apply_to_all_is_journaled() {
    let td = tempfile::tempdir().unwrap();
    let journal = td.path().join("transfers.json");
    let fs = MemFs::new();
    fs.set_read_throttle(1024, Some(Duration::from_millis(5)));
    fs.put("/s/a.txt", b"a", t(10));
    fs.put("/s/big.bin", &vec![1u8; 400_000], t(0));
    fs.put("/s/c.txt", b"c", t(10));
    fs.put("/d/a.txt", b"old a", t(0));
    fs.put("/d/c.txt", b"old c", t(0));
    let opts = || TransferOptions {
        max_parallel: 1,
        journal: Some(journal.clone()),
        ..Default::default()
    };
    {
        let tm = TransferManager::with_options(opts());
        let id = tm.enqueue_with_endpoints(
            job(
                JobKind::Copy,
                &fs,
                &["/s/a.txt", "/s/big.bin", "/s/c.txt"],
                &fs,
            ),
            Endpoint::Local,
            Some(Endpoint::Local),
        );
        question(&tm, id);
        tm.resolve_conflict(id, ConflictChoice::Skip, true);
        wait_for(&tm, id, "copying big.bin", |s| s.bytes_done > 2048);
    }
    std::thread::sleep(Duration::from_millis(100));
    let tm = TransferManager::with_options(opts());
    assert_eq!(tm.interrupted()[0].conflict, ConflictPolicy::Skip);
    fs.set_read_throttle(0, None);
    let backend = shared(&fs);
    let ids = tm.restore_interrupted(&|_: &Endpoint| Some(backend.clone()));
    assert_eq!(finished(&tm, ids[0]).state, JobState::Done);
    assert_eq!(fs.read_all("/d/c.txt").unwrap(), b"old c");
    assert_eq!(fs.read_all("/d/big.bin").unwrap().len(), 400_000);
}
