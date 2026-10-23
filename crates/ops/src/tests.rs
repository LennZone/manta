use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use manta_core::{SharedVfs, Vfs, VfsPath};

use crate::memfs::MemFs;
use crate::*;

fn t(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn shared(m: &MemFs) -> SharedVfs {
    Arc::new(m.clone())
}

fn p(s: &str) -> VfsPath {
    VfsPath::new(s)
}

fn job(
    kind: JobKind,
    src: &SharedVfs,
    sources: &[&str],
    dst: Option<(&SharedVfs, &str)>,
    conflict: ConflictPolicy,
) -> Job {
    Job {
        kind,
        src_fs: src.clone(),
        sources: sources.iter().map(|s| p(s)).collect(),
        dst_fs: dst.map(|(f, _)| f.clone()),
        dst_dir: dst.map(|(_, d)| p(d)),
        conflict,
    }
}

fn wait(tm: &TransferManager, id: JobId) -> JobStatus {
    let start = Instant::now();
    loop {
        let st = tm
            .snapshot()
            .into_iter()
            .find(|s| s.id == id)
            .expect("job present");
        if st.state.is_finished() {
            return st;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "job did not finish: {st:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run(j: Job) -> JobStatus {
    let tm = TransferManager::new(2);
    let id = tm.enqueue(j);
    wait(&tm, id)
}

fn sample_tree(m: &MemFs) {
    m.put("/src/a.txt", b"hello", t(0));
    m.put("/src/dir/b.bin", &vec![7u8; 600_000], t(0));
    m.put("/src/dir/sub/c.txt", b"c", t(0));
    m.mkdir_p("/src/dir/empty");
    m.mkdir_p("/dst");
}

#[test]
fn copy_tree_cross_fs() {
    let a = MemFs::new();
    let b = MemFs::new();
    sample_tree(&a);
    b.mkdir_p("/dst");
    let (fa, fb) = (shared(&a), shared(&b));
    let st = run(job(
        JobKind::Copy,
        &fa,
        &["/src/a.txt", "/src/dir"],
        Some((&fb, "/dst")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert!(st.errors.is_empty());
    assert_eq!(st.files_total, 3);
    assert_eq!(st.files_done, 3);
    assert_eq!(st.bytes_total, 600_006);
    assert_eq!(st.bytes_done, 600_006);
    assert_eq!(st.title, "Copying 2 items to /dst");
    assert_eq!(b.read_all("/dst/a.txt").unwrap(), b"hello");
    assert_eq!(b.read_all("/dst/dir/b.bin").unwrap().len(), 600_000);
    assert_eq!(b.read_all("/dst/dir/sub/c.txt").unwrap(), b"c");
    assert!(b.is_dir("/dst/dir/empty"));
    assert!(a.exists("/src/dir/b.bin"));
}

#[test]
fn copy_preserves_permissions_and_creates_dst_dir() {
    let a = MemFs::new();
    a.put("/x.sh", b"#!", t(0));
    a.set_permissions(&p("/x.sh"), 0o755).unwrap();
    let fa = shared(&a);
    let st = run(job(
        JobKind::Copy,
        &fa,
        &["/x.sh"],
        Some((&fa, "/new/deep")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done);
    assert_eq!(a.permissions_of("/new/deep/x.sh"), Some(0o755));
}

#[test]
fn copy_into_itself_is_refused() {
    let a = MemFs::new();
    sample_tree(&a);
    let fa = shared(&a);
    let st = run(job(
        JobKind::Copy,
        &fa,
        &["/src/dir"],
        Some((&fa, "/src/dir/sub")),
        ConflictPolicy::Overwrite,
    ));
    assert!(matches!(st.state, JobState::Failed(_)), "{st:?}");
    assert!(!a.exists("/src/dir/sub/dir"));
}

#[test]
fn duplicate_in_place_with_rename() {
    let a = MemFs::new();
    sample_tree(&a);
    let fa = shared(&a);
    let st = run(job(
        JobKind::Copy,
        &fa,
        &["/src/dir", "/src/a.txt"],
        Some((&fa, "/src")),
        ConflictPolicy::Rename,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert!(a.exists("/src/dir 2/sub/c.txt"));
    assert_eq!(a.read_all("/src/a 2.txt").unwrap(), b"hello");
    // Without Rename: error, source untouched.
    let st = run(job(
        JobKind::Copy,
        &fa,
        &["/src/a.txt"],
        Some((&fa, "/src")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.errors.len(), 1);
    assert_eq!(a.read_all("/src/a.txt").unwrap(), b"hello");
}

#[test]
fn conflict_policies() {
    let setup = || {
        let a = MemFs::new();
        a.put("/s/new.txt", b"newer", t(100));
        a.put("/s/old.txt", b"older", t(0));
        a.put("/s/nomt.txt", b"x", t(0));
        let b = MemFs::new();
        b.put("/d/new.txt", b"OLD", t(0));
        b.put("/d/old.txt", b"NEWER", t(100));
        b.put("/d/nomt.txt", b"y", t(0));
        (a, b)
    };
    let srcs = ["/s/new.txt", "/s/old.txt"];

    let (a, b) = setup();
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &srcs,
        Some((&shared(&b), "/d")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done);
    assert_eq!(b.read_all("/d/new.txt").unwrap(), b"newer");
    assert_eq!(b.read_all("/d/old.txt").unwrap(), b"older");

    let (a, b) = setup();
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &srcs,
        Some((&shared(&b), "/d")),
        ConflictPolicy::Skip,
    ));
    assert_eq!(st.state, JobState::Done);
    assert_eq!(st.files_done, 2);
    assert_eq!(st.bytes_done, st.bytes_total);
    assert_eq!(b.read_all("/d/new.txt").unwrap(), b"OLD");
    assert_eq!(b.read_all("/d/old.txt").unwrap(), b"NEWER");

    let (a, b) = setup();
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &srcs,
        Some((&shared(&b), "/d")),
        ConflictPolicy::OverwriteIfNewer,
    ));
    assert_eq!(st.state, JobState::Done);
    assert_eq!(b.read_all("/d/new.txt").unwrap(), b"newer");
    assert_eq!(b.read_all("/d/old.txt").unwrap(), b"NEWER");

    let (a, b) = setup();
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &srcs,
        Some((&shared(&b), "/d")),
        ConflictPolicy::Rename,
    ));
    assert_eq!(st.state, JobState::Done);
    assert_eq!(b.read_all("/d/new.txt").unwrap(), b"OLD");
    assert_eq!(b.read_all("/d/new 2.txt").unwrap(), b"newer");
    assert_eq!(b.read_all("/d/old 2.txt").unwrap(), b"older");
    b.put("/d/new 3.txt", b"", t(0));
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &["/s/new.txt"],
        Some((&shared(&b), "/d")),
        ConflictPolicy::Rename,
    ));
    assert_eq!(st.state, JobState::Done);
    assert_eq!(b.read_all("/d/new 4.txt").unwrap(), b"newer");
}

#[test]
fn unique_name_formats() {
    let a = MemFs::new();
    a.put("/d/x.tar.gz", b"", t(0));
    a.put("/d/.bashrc", b"", t(0));
    a.mkdir_p("/d/my.dir");
    assert_eq!(
        unique_name(&a, &p("/d/x.tar.gz"), false).as_str(),
        "/d/x.tar 2.gz"
    );
    assert_eq!(
        unique_name(&a, &p("/d/.bashrc"), false).as_str(),
        "/d/.bashrc 2"
    );
    assert_eq!(
        unique_name(&a, &p("/d/my.dir"), true).as_str(),
        "/d/my.dir 2"
    );
}

#[test]
fn move_same_fs_uses_rename() {
    let a = MemFs::new();
    sample_tree(&a);
    let fa = shared(&a);
    let st = run(job(
        JobKind::Move,
        &fa,
        &["/src/dir", "/src/a.txt"],
        Some((&fa, "/dst")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(st.files_total, 2); // two renames, no scan
    assert!(a.exists("/dst/dir/sub/c.txt"));
    assert!(a.exists("/dst/a.txt"));
    assert!(!a.exists("/src/dir"));
    assert!(!a.exists("/src/a.txt"));
}

#[test]
fn move_same_fs_falls_back_when_rename_fails() {
    let a = MemFs::new();
    sample_tree(&a);
    a.disable_rename();
    let fa = shared(&a);
    let st = run(job(
        JobKind::Move,
        &fa,
        &["/src/dir"],
        Some((&fa, "/dst")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert!(a.exists("/dst/dir/sub/c.txt"));
    assert!(a.is_dir("/dst/dir/empty"));
    assert!(!a.exists("/src/dir"));
}

#[test]
fn move_cross_fs_and_partial_skip_keeps_source() {
    let a = MemFs::new();
    let b = MemFs::new();
    sample_tree(&a);
    b.put("/dst/dir/sub/c.txt", b"existing", t(0));
    let st = run(job(
        JobKind::Move,
        &shared(&a),
        &["/src/dir", "/src/a.txt"],
        Some((&shared(&b), "/dst")),
        ConflictPolicy::Skip,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert!(!a.exists("/src/a.txt"));
    assert!(!a.exists("/src/dir/b.bin"));
    assert!(!a.exists("/src/dir/empty"));
    // c.txt was skipped -> it and its parents stay on the source.
    assert!(a.exists("/src/dir/sub/c.txt"));
    assert_eq!(b.read_all("/dst/dir/sub/c.txt").unwrap(), b"existing");
    assert_eq!(b.read_all("/dst/a.txt").unwrap(), b"hello");
}

#[test]
fn delete_tree() {
    let a = MemFs::new();
    sample_tree(&a);
    let fa = shared(&a);
    let st = run(job(
        JobKind::Delete,
        &fa,
        &["/src/dir", "/src/a.txt"],
        None,
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(st.files_total, 6);
    assert_eq!(st.files_done, 6);
    assert_eq!(a.paths(), vec!["/dst", "/src"]);
}

#[test]
fn per_file_errors_do_not_fail_job() {
    let a = MemFs::new();
    let b = MemFs::new();
    sample_tree(&a);
    a.set_unreadable("/src/a.txt");
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &["/src/a.txt", "/src/dir", "/missing"],
        Some((&shared(&b), "/dst")),
        ConflictPolicy::Overwrite,
    ));
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert_eq!(st.errors.len(), 2, "{:?}", st.errors);
    assert!(!b.exists("/dst/a.txt"));
    assert!(b.exists("/dst/dir/b.bin"));
    assert_eq!(st.files_done, st.files_total);

    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &["/missing"],
        Some((&shared(&b), "/dst")),
        ConflictPolicy::Overwrite,
    ));
    assert!(matches!(st.state, JobState::Failed(_)));
    // Destination is a file -> fatal.
    let st = run(job(
        JobKind::Copy,
        &shared(&a),
        &["/src/dir"],
        Some((&shared(&a), "/src/a.txt")),
        ConflictPolicy::Overwrite,
    ));
    assert!(matches!(st.state, JobState::Failed(_)));
}

#[test]
fn cancel_running_and_queued() {
    let a = MemFs::new();
    let b = MemFs::new();
    a.put("/big.bin", &vec![1u8; 4 * 1024 * 1024], t(0));
    a.set_read_throttle(64 * 1024, Some(Duration::from_millis(20)));
    b.mkdir_p("/dst");
    let (fa, fb) = (shared(&a), shared(&b));
    let tm = TransferManager::new(1);
    let running = tm.enqueue(job(
        JobKind::Copy,
        &fa,
        &["/big.bin"],
        Some((&fb, "/dst")),
        ConflictPolicy::Overwrite,
    ));
    let queued = tm.enqueue(job(
        JobKind::Copy,
        &fa,
        &["/big.bin"],
        Some((&fb, "/dst2")),
        ConflictPolicy::Overwrite,
    ));
    // Wait until some bytes are transferred.
    let start = Instant::now();
    loop {
        let s = tm.snapshot();
        if s[0].bytes_done > 0 {
            assert_eq!(s[0].state, JobState::Running);
            assert_eq!(s[1].state, JobState::Queued);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(tm.is_busy());
    tm.cancel(queued);
    assert_eq!(tm.snapshot()[1].state, JobState::Cancelled);
    tm.cancel(running);
    let st = wait(&tm, running);
    assert_eq!(st.state, JobState::Cancelled);
    assert!(st.bytes_done < st.bytes_total);
    assert!(!b.exists("/dst/big.bin"), "partial file must be removed");
    assert!(!b.exists("/dst2"));
    assert!(!tm.is_busy());
    tm.clear_finished();
    assert!(tm.snapshot().is_empty());
}

#[test]
fn speed_is_reported() {
    let a = MemFs::new();
    let b = MemFs::new();
    a.put("/f.bin", &vec![1u8; 2 * 1024 * 1024], t(0));
    a.set_read_throttle(64 * 1024, Some(Duration::from_millis(25)));
    let tm = TransferManager::new(1);
    let id = tm.enqueue(job(
        JobKind::Copy,
        &shared(&a),
        &["/f.bin"],
        Some((&shared(&b), "/")),
        ConflictPolicy::Overwrite,
    ));
    let mut max_speed: f64 = 0.0;
    loop {
        let s = tm.snapshot().into_iter().find(|s| s.id == id).unwrap();
        max_speed = max_speed.max(s.speed);
        if s.state.is_finished() {
            assert_eq!(s.state, JobState::Done);
            assert_eq!(s.speed, 0.0);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(max_speed > 100_000.0, "speed {max_speed}");
}

#[test]
fn parallel_jobs() {
    let a = MemFs::new();
    let b = MemFs::new();
    for i in 0..8 {
        a.put(&format!("/s{i}/f"), &vec![0u8; 1000], t(0));
    }
    let (fa, fb) = (shared(&a), shared(&b));
    let tm = TransferManager::new(3);
    let ids: Vec<_> = (0..8)
        .map(|i| {
            tm.enqueue(job(
                JobKind::Copy,
                &fa,
                &[&format!("/s{i}")],
                Some((&fb, "/d")),
                ConflictPolicy::Overwrite,
            ))
        })
        .collect();
    for id in ids {
        assert_eq!(wait(&tm, id).state, JobState::Done);
    }
    for i in 0..8 {
        assert!(b.exists(&format!("/d/s{i}/f")));
    }
}

// ------------------------------------------------------------ compare / sync

fn compare_fixture() -> (MemFs, MemFs) {
    let l = MemFs::new();
    let r = MemFs::new();
    l.put("/L/same.txt", b"same", t(0));
    r.put("/R/same.txt", b"same", t(1)); // within tolerance
    l.put("/L/lnew.txt", b"l", t(100));
    r.put("/R/lnew.txt", b"r", t(0));
    l.put("/L/rnew.txt", b"l", t(0));
    r.put("/R/rnew.txt", b"r", t(100));
    l.put("/L/diff.txt", b"long", t(0));
    r.put("/R/diff.txt", b"s", t(0));
    l.put("/L/onlyl.txt", b"x", t(0));
    r.put("/R/onlyr.txt", b"x", t(0));
    l.put("/L/ldir/deep/f.txt", b"f", t(0));
    r.put("/R/rdir/g.txt", b"g", t(0));
    l.put("/L/both/inner.txt", b"i", t(50));
    r.put("/R/both/inner.txt", b"i", t(0));
    l.put("/L/both/ok.txt", b"ok", t(0));
    r.put("/R/both/ok.txt", b"ok", t(0));
    l.put("/L/samedir/x", b"x", t(0));
    r.put("/R/samedir/x", b"x", t(0));
    l.put("/L/.hidden", b"h", t(0));
    l.put("/L/mixed", b"file", t(0));
    r.mkdir_p("/R/mixed");
    (l, r)
}

fn status_of<'a>(items: &'a [CompareItem], rel: &str) -> &'a CompareItem {
    items
        .iter()
        .find(|i| i.rel_path == rel)
        .unwrap_or_else(|| panic!("{rel} missing"))
}

#[test]
fn compare_statuses() {
    let (l, r) = compare_fixture();
    let items = compare_dirs(
        &shared(&l),
        &p("/L"),
        &shared(&r),
        &p("/R"),
        &CompareOptions::default(),
    )
    .unwrap();
    use CompareStatus::*;
    assert_eq!(status_of(&items, "same.txt").status, Identical);
    assert_eq!(status_of(&items, "lnew.txt").status, LeftNewer);
    assert_eq!(status_of(&items, "rnew.txt").status, RightNewer);
    assert_eq!(status_of(&items, "diff.txt").status, Different);
    assert_eq!(status_of(&items, "diff.txt").left_size, Some(4));
    assert_eq!(status_of(&items, "onlyl.txt").status, OnlyLeft);
    assert_eq!(status_of(&items, "onlyr.txt").status, OnlyRight);
    assert_eq!(status_of(&items, "ldir").status, OnlyLeft);
    assert!(status_of(&items, "ldir").is_dir);
    assert!(items.iter().all(|i| !i.rel_path.starts_with("ldir/")));
    assert_eq!(status_of(&items, "rdir").status, OnlyRight);
    assert_eq!(status_of(&items, "both").status, Different);
    assert_eq!(status_of(&items, "both/inner.txt").status, LeftNewer);
    assert_eq!(status_of(&items, "both/ok.txt").status, Identical);
    assert_eq!(status_of(&items, "samedir").status, Identical);
    assert_eq!(status_of(&items, "mixed").status, Different);
    assert!(items.iter().all(|i| i.rel_path != ".hidden"));
    // Sorted with children right after their parent.
    let rels: Vec<_> = items.iter().map(|i| i.rel_path.as_str()).collect();
    let pos = |s: &str| rels.iter().position(|r| *r == s).unwrap();
    assert_eq!(pos("both/inner.txt"), pos("both") + 1);

    let opts = CompareOptions {
        include_hidden: true,
        recursive: false,
        ..Default::default()
    };
    let items = compare_dirs(&shared(&l), &p("/L"), &shared(&r), &p("/R"), &opts).unwrap();
    assert_eq!(status_of(&items, ".hidden").status, OnlyLeft);
    assert_eq!(status_of(&items, "both").status, Identical);
    assert!(items.iter().all(|i| !i.rel_path.contains('/')));

    assert!(compare_dirs(
        &shared(&l),
        &p("/nope"),
        &shared(&r),
        &p("/R"),
        &CompareOptions::default()
    )
    .is_err());
}

#[test]
fn sync_is_idempotent() {
    // Copies keep the source mtime, so a second compare finds nothing to do.
    let (l, r) = compare_fixture();
    let (fl, fr) = (shared(&l), shared(&r));
    let items = compare_dirs(&fl, &p("/L"), &fr, &p("/R"), &CompareOptions::default()).unwrap();
    apply(plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::Bidirectional,
        false,
    ));
    let items = compare_dirs(&fl, &p("/L"), &fr, &p("/R"), &CompareOptions::default()).unwrap();
    let again = plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::Bidirectional,
        false,
    );
    assert!(again.is_empty(), "{:?}", items);
}

fn apply(jobs: Vec<Job>) {
    let tm = TransferManager::new(2);
    let ids: Vec<_> = jobs.into_iter().map(|j| tm.enqueue(j)).collect();
    for id in ids {
        let st = wait(&tm, id);
        assert_eq!(st.state, JobState::Done, "{st:?}");
        assert!(st.errors.is_empty(), "{:?}", st.errors);
    }
}

#[test]
fn plan_sync_left_to_right() {
    let (l, r) = compare_fixture();
    let (fl, fr) = (shared(&l), shared(&r));
    let items = compare_dirs(&fl, &p("/L"), &fr, &p("/R"), &CompareOptions::default()).unwrap();
    let jobs = plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::LeftToRight,
        true,
    );
    // One copy job for root, one for "both", one delete job.
    assert_eq!(jobs.len(), 3);
    let root_job = jobs.iter().find(|j| j.dst_dir == Some(p("/R"))).unwrap();
    let mut srcs: Vec<_> = root_job.sources.iter().map(|s| s.to_string()).collect();
    srcs.sort();
    assert_eq!(
        srcs,
        vec!["/L/diff.txt", "/L/ldir", "/L/lnew.txt", "/L/onlyl.txt"]
    );
    let del = jobs.iter().find(|j| j.kind == JobKind::Delete).unwrap();
    assert!(Arc::ptr_eq(&del.src_fs, &fr));
    apply(jobs);
    assert_eq!(r.read_all("/R/diff.txt").unwrap(), b"long");
    assert_eq!(r.read_all("/R/lnew.txt").unwrap(), b"l");
    assert_eq!(r.read_all("/R/rnew.txt").unwrap(), b"r"); // newer target untouched
    assert_eq!(r.read_all("/R/ldir/deep/f.txt").unwrap(), b"f");
    assert_eq!(r.read_all("/R/both/inner.txt").unwrap(), b"i");
    assert!(!r.exists("/R/onlyr.txt"));
    assert!(!r.exists("/R/rdir"));
    assert!(r.is_dir("/R/mixed")); // type mismatch skipped
    assert!(l.exists("/L/onlyl.txt"));

    // Without delete_extra no delete job.
    let (l, r) = compare_fixture();
    let (fl, fr) = (shared(&l), shared(&r));
    let items = compare_dirs(&fl, &p("/L"), &fr, &p("/R"), &CompareOptions::default()).unwrap();
    let jobs = plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::LeftToRight,
        false,
    );
    assert!(jobs.iter().all(|j| j.kind == JobKind::Copy));
}

#[test]
fn plan_sync_right_to_left() {
    let (l, r) = compare_fixture();
    let (fl, fr) = (shared(&l), shared(&r));
    let items = compare_dirs(&fl, &p("/L"), &fr, &p("/R"), &CompareOptions::default()).unwrap();
    let jobs = plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::RightToLeft,
        true,
    );
    apply(jobs);
    assert_eq!(l.read_all("/L/rnew.txt").unwrap(), b"r");
    assert_eq!(l.read_all("/L/diff.txt").unwrap(), b"s");
    assert_eq!(l.read_all("/L/lnew.txt").unwrap(), b"l"); // newer target untouched
    assert_eq!(l.read_all("/L/onlyr.txt").unwrap(), b"x");
    assert_eq!(l.read_all("/L/rdir/g.txt").unwrap(), b"g");
    assert!(!l.exists("/L/onlyl.txt"));
    assert!(!l.exists("/L/ldir"));
    assert!(r.exists("/R/onlyr.txt"));
}

#[test]
fn plan_sync_bidirectional() {
    let (l, r) = compare_fixture();
    let (fl, fr) = (shared(&l), shared(&r));
    let items = compare_dirs(&fl, &p("/L"), &fr, &p("/R"), &CompareOptions::default()).unwrap();
    let jobs = plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::Bidirectional,
        true,
    );
    assert!(jobs.iter().all(|j| j.kind == JobKind::Copy));
    apply(jobs);
    assert_eq!(r.read_all("/R/lnew.txt").unwrap(), b"l");
    assert_eq!(l.read_all("/L/rnew.txt").unwrap(), b"r");
    assert_eq!(r.read_all("/R/onlyl.txt").unwrap(), b"x");
    assert_eq!(l.read_all("/L/onlyr.txt").unwrap(), b"x");
    assert_eq!(r.read_all("/R/ldir/deep/f.txt").unwrap(), b"f");
    assert_eq!(l.read_all("/L/rdir/g.txt").unwrap(), b"g");
    assert_eq!(r.read_all("/R/both/inner.txt").unwrap(), b"i");
    // Ambiguous "Different" left alone on both sides.
    assert_eq!(l.read_all("/L/diff.txt").unwrap(), b"long");
    assert_eq!(r.read_all("/R/diff.txt").unwrap(), b"s");
}

#[test]
fn plan_sync_skips_descendants_of_whole_dirs() {
    let l = MemFs::new();
    let r = MemFs::new();
    let (fl, fr) = (shared(&l), shared(&r));
    let item = |rel: &str, is_dir| CompareItem {
        rel_path: rel.into(),
        is_dir,
        status: CompareStatus::OnlyLeft,
        left_size: None,
        right_size: None,
    };
    let items = vec![
        item("a/b/c.txt", false),
        item("a", true),
        item("a/b", true),
        item("ab.txt", false),
    ];
    let jobs = plan_sync(
        &items,
        &fl,
        &p("/L"),
        &fr,
        &p("/R"),
        SyncDirection::LeftToRight,
        false,
    );
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].sources, vec![p("/L/a"), p("/L/ab.txt")]);
}

// ------------------------------------------------------------- compress

fn compress_job(
    src: &SharedVfs,
    dst: &SharedVfs,
    sources: &[&str],
    dst_path: &str,
    format: CompressFormat,
) -> CompressJob {
    CompressJob {
        src_fs: src.clone(),
        sources: sources.iter().map(|s| p(s)).collect(),
        dst_fs: dst.clone(),
        dst_path: p(dst_path),
        format,
        level: 5,
        exclude_junk: true,
    }
}

#[test]
fn compress_to_a_remote_destination_uploads_the_archive() {
    use std::io::Read;
    let a = MemFs::new();
    sample_tree(&a);
    a.put("/src/dir/.DS_Store", b"junk", t(0));
    let b = MemFs::new();
    b.mkdir_p("/remote");
    let tm = TransferManager::new(1);
    for format in CompressFormat::ALL {
        let dst = format!("/remote/pack{}", format.ext());
        let id = tm.enqueue_compress(compress_job(
            &shared(&a),
            &shared(&b),
            &["/src/a.txt", "/src/dir"],
            &dst,
            format,
        ));
        let st = wait(&tm, id);
        assert_eq!(st.state, JobState::Done, "{format:?}: {st:?}");
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let bytes = b.read_all(&dst).expect("uploaded");
        // Bytes = sources + upload.
        assert_eq!(st.bytes_done, 5 + 600_001 + bytes.len() as u64);
        let td = tempfile::tempdir().unwrap();
        let local = td.path().join(format!("x{}", format.ext()));
        std::fs::write(&local, &bytes).unwrap();
        let ar = manta_vfs::ArchiveFs::open(&local, &Default::default()).unwrap();
        let mut root: Vec<String> = ar
            .list(&VfsPath::root())
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        root.sort();
        assert_eq!(root, vec!["a.txt", "dir"]);
        let mut sub: Vec<String> = ar
            .list(&p("/dir"))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        sub.sort();
        assert_eq!(sub, vec!["b.bin", "empty", "sub"], "junk is left out");
        let mut data = Vec::new();
        ar.open_read(&p("/dir/b.bin"))
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        assert_eq!(data, vec![7u8; 600_000]);
    }
}

#[test]
fn cancelled_compress_leaves_nothing_behind() {
    let a = MemFs::new();
    a.put("/src/big.bin", &vec![1u8; 4_000_000], t(0));
    a.set_read_throttle(64 * 1024, Some(Duration::from_millis(20)));
    let b = MemFs::new();
    b.mkdir_p("/remote");
    let tm = TransferManager::new(1);
    let id = tm.enqueue_compress(compress_job(
        &shared(&a),
        &shared(&b),
        &["/src/big.bin"],
        "/remote/big.tar.gz",
        CompressFormat::TarGz,
    ));
    let start = Instant::now();
    while tm.snapshot()[0].bytes_done == 0 && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(5));
    }
    tm.cancel(id);
    let st = wait(&tm, id);
    assert_eq!(st.state, JobState::Cancelled);
    assert!(!b.exists("/remote/big.tar.gz"));
}

#[test]
fn compress_kind_on_a_plain_job_fails() {
    let a = MemFs::new();
    sample_tree(&a);
    let fa = shared(&a);
    let st = run(job(
        JobKind::Compress,
        &fa,
        &["/src/a.txt"],
        Some((&fa, "/dst")),
        ConflictPolicy::Skip,
    ));
    assert!(matches!(st.state, JobState::Failed(_)));
}
