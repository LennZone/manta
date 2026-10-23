//! End-to-end: real `LocalFs` through the transfer engine and sync.

use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{SharedVfs, VfsPath};
use manta_ops::{
    compare_dirs, plan_sync, CompareOptions, ConflictPolicy, Job, JobKind, JobState, SyncDirection,
    TransferManager,
};
use manta_vfs::LocalFs;

fn wait_idle(tm: &TransferManager) {
    let start = Instant::now();
    while tm.is_busy() {
        assert!(start.elapsed() < Duration::from_secs(20), "timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
    for st in tm.snapshot() {
        assert_eq!(st.state, JobState::Done, "{st:?}");
        assert!(st.errors.is_empty(), "{:?}", st.errors);
    }
}

#[test]
fn copy_move_delete_and_sync_on_local_fs() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let src = root.join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), b"alpha").unwrap();
    std::fs::write(src.join("sub/b.bin"), vec![42u8; 1_000_000]).unwrap();

    let fs: SharedVfs = Arc::new(LocalFs::new());
    let p = |s: &str| VfsPath::new(root.join(s).to_str().unwrap());
    let tm = TransferManager::new(2);

    // Copy src -> dst
    tm.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: fs.clone(),
        sources: vec![p("src")],
        dst_fs: Some(fs.clone()),
        dst_dir: Some(p("dst")),
        conflict: ConflictPolicy::Overwrite,
    });
    wait_idle(&tm);
    assert_eq!(std::fs::read(root.join("dst/src/a.txt")).unwrap(), b"alpha");
    assert_eq!(
        std::fs::read(root.join("dst/src/sub/b.bin")).unwrap().len(),
        1_000_000
    );

    // Copies keep mtime, so the trees compare identical and sync has nothing to do.
    let opts = CompareOptions::default();
    let items = compare_dirs(&fs, &p("src"), &fs, &p("dst/src"), &opts).unwrap();
    let jobs = plan_sync(
        &items,
        &fs,
        &p("src"),
        &fs,
        &p("dst/src"),
        SyncDirection::Bidirectional,
        false,
    );
    assert!(jobs.is_empty(), "{items:?}");

    // A new file on the left syncs to the right.
    std::fs::write(src.join("new.txt"), b"new").unwrap();
    let items = compare_dirs(&fs, &p("src"), &fs, &p("dst/src"), &opts).unwrap();
    for j in plan_sync(
        &items,
        &fs,
        &p("src"),
        &fs,
        &p("dst/src"),
        SyncDirection::LeftToRight,
        true,
    ) {
        tm.enqueue(j);
    }
    wait_idle(&tm);
    assert_eq!(std::fs::read(root.join("dst/src/new.txt")).unwrap(), b"new");

    // Move and delete.
    tm.clear_finished();
    tm.enqueue(Job {
        kind: JobKind::Move,
        src_fs: fs.clone(),
        sources: vec![p("dst/src/a.txt")],
        dst_fs: Some(fs.clone()),
        dst_dir: Some(p("moved")),
        conflict: ConflictPolicy::Overwrite,
    });
    wait_idle(&tm);
    assert!(root.join("moved/a.txt").exists());
    assert!(!root.join("dst/src/a.txt").exists());

    tm.enqueue(Job {
        kind: JobKind::Delete,
        src_fs: fs.clone(),
        sources: vec![p("dst")],
        dst_fs: None,
        dst_dir: None,
        conflict: ConflictPolicy::Overwrite,
    });
    wait_idle(&tm);
    assert!(!root.join("dst").exists());
}
