//! Archives end to end: compress a local tree in every format, reopen the
//! result with `ArchiveFs` and compare; extract trees and single files out of
//! archives with the transfer manager.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::{EntryKind, SharedVfs, Vfs, VfsPath};
use manta_ops::{
    CompressFormat, CompressJob, ConflictPolicy, Job, JobId, JobKind, JobState, JobStatus,
    TransferManager,
};
use manta_vfs::archive::{ArchiveFs, OpenOptions};
use manta_vfs::LocalFs;

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
            start.elapsed() < Duration::from_secs(60),
            "job did not finish: {st:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn vp(p: &Path) -> VfsPath {
    VfsPath::new(p.to_str().unwrap())
}

/// rel path -> (kind, content or link target, mode)
type Tree = BTreeMap<String, (EntryKind, Vec<u8>, Option<u32>)>;

fn snapshot(fs: &dyn Vfs, root: &VfsPath) -> Tree {
    let mut out = Tree::new();
    walk(fs, root, root, &mut out);
    out
}

fn walk(fs: &dyn Vfs, root: &VfsPath, dir: &VfsPath, out: &mut Tree) {
    for e in fs.list(dir).unwrap() {
        let rel = e.path.strip_prefix(root).unwrap();
        if let Some(t) = &e.link_target {
            out.insert(rel, (EntryKind::Symlink, t.as_bytes().to_vec(), None));
        } else if e.is_dir() {
            out.insert(rel, (EntryKind::Directory, Vec::new(), e.permissions));
            walk(fs, root, &e.path, out);
        } else {
            let mut data = Vec::new();
            fs.open_read(&e.path)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            out.insert(rel, (EntryKind::File, data, e.permissions));
        }
    }
}

fn make_source(root: &Path) -> std::path::PathBuf {
    let site = root.join("site");
    std::fs::create_dir_all(site.join("assets/img")).unwrap();
    std::fs::create_dir_all(site.join("empty")).unwrap();
    std::fs::create_dir_all(site.join("__MACOSX")).unwrap();
    std::fs::write(site.join("index.html"), b"<h1>Hi</h1>").unwrap();
    std::fs::write(site.join("assets/app.js"), b"console.log(1)".repeat(10_000)).unwrap();
    std::fs::write(site.join("assets/img/Grüße.txt"), "ünïcödé".as_bytes()).unwrap();
    std::fs::write(site.join("deploy.sh"), b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(
        site.join("deploy.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(site.join(".DS_Store"), b"junk").unwrap();
    std::fs::write(site.join("assets/.DS_Store"), b"junk").unwrap();
    std::fs::write(site.join("__MACOSX/._index.html"), b"junk").unwrap();
    std::os::unix::fs::symlink("index.html", site.join("home.html")).unwrap();
    site
}

fn without_junk(mut t: Tree) -> Tree {
    t.retain(|k, _| !k.ends_with(".DS_Store") && !k.starts_with("__MACOSX"));
    t
}

#[test]
fn compress_every_format_then_reopen_and_compare() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let site = make_source(&root);
    let local: SharedVfs = Arc::new(LocalFs::new());
    let original = without_junk(snapshot(&*local, &vp(&site)));
    assert!(original.contains_key("home.html"));
    let tm = TransferManager::new(2);

    for format in CompressFormat::ALL {
        let dst = root.join(format!("site{}", format.ext()));
        let id = tm.enqueue_compress(CompressJob {
            src_fs: local.clone(),
            sources: vec![vp(&site)],
            dst_fs: local.clone(),
            dst_path: vp(&dst),
            format,
            level: 6,
            exclude_junk: true,
        });
        let st = wait(&tm, id);
        assert_eq!(st.state, JobState::Done, "{format:?}: {st:?}");
        assert!(st.errors.is_empty(), "{format:?}: {:?}", st.errors);
        assert_eq!(st.kind, JobKind::Compress);
        assert_eq!(st.files_done, st.files_total);
        assert!(st.bytes_done >= 140_000, "{format:?}: {st:?}");

        let ar = ArchiveFs::open(&dst, &OpenOptions::default()).unwrap();
        assert!(ar.rejected().is_empty());
        let names: Vec<String> = ar
            .list(&VfsPath::root())
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["site"]);
        let packed = snapshot(&ar, &VfsPath::new("/site"));
        assert_eq!(
            packed.keys().collect::<Vec<_>>(),
            original.keys().collect::<Vec<_>>(),
            "{format:?}"
        );
        for (k, (kind, data, mode)) in &original {
            let (pk, pdata, pmode) = &packed[k];
            assert_eq!(kind, pk, "{format:?} {k}");
            assert_eq!(data, pdata, "{format:?} {k}");
            if *kind == EntryKind::File {
                assert_eq!(mode, pmode, "{format:?} {k}");
            }
        }
        // The mtime survives (zip: 2 s resolution).
        let a = local
            .stat(&vp(&site.join("index.html")))
            .unwrap()
            .modified
            .unwrap();
        let b = ar
            .stat(&VfsPath::new("/site/index.html"))
            .unwrap()
            .modified
            .unwrap();
        assert!((a - b).num_seconds().abs() <= 2, "{format:?}: {a} vs {b}");
    }
    // No temp files were left behind.
    let partial = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with(".partial"));
    assert!(!partial);
}

#[test]
fn compress_keeps_junk_when_asked_and_never_overwrites() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let site = make_source(&root);
    let local: SharedVfs = Arc::new(LocalFs::new());
    let tm = TransferManager::new(1);
    let dst = root.join("site.zip");
    std::fs::write(&dst, b"existing").unwrap();
    let id = tm.enqueue_compress(CompressJob {
        src_fs: local.clone(),
        sources: vec![vp(&site.join("index.html")), vp(&site.join(".DS_Store"))],
        dst_fs: local.clone(),
        dst_path: vp(&dst),
        format: CompressFormat::Zip,
        level: 1,
        exclude_junk: false,
    });
    let st = wait(&tm, id);
    assert_eq!(st.state, JobState::Done, "{st:?}");
    assert!(st.title.contains("site 2.zip"), "{}", st.title);
    assert_eq!(std::fs::read(&dst).unwrap(), b"existing");
    let ar = ArchiveFs::open(&root.join("site 2.zip"), &OpenOptions::default()).unwrap();
    let mut names: Vec<String> = ar
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    assert_eq!(names, vec![".DS_Store", "index.html"]);
}

#[test]
fn compress_into_an_archive_is_refused() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let site = make_source(&root);
    let local: SharedVfs = Arc::new(LocalFs::new());
    let tm = TransferManager::new(1);
    let zip = root.join("a.zip");
    wait(
        &tm,
        tm.enqueue_compress(CompressJob {
            src_fs: local.clone(),
            sources: vec![vp(&site.join("index.html"))],
            dst_fs: local.clone(),
            dst_path: vp(&zip),
            format: CompressFormat::Zip,
            level: 6,
            exclude_junk: true,
        }),
    );
    let ar: SharedVfs = Arc::new(ArchiveFs::open(&zip, &OpenOptions::default()).unwrap());
    let st = wait(
        &tm,
        tm.enqueue_compress(CompressJob {
            src_fs: local.clone(),
            sources: vec![vp(&site.join("index.html"))],
            dst_fs: ar.clone(),
            dst_path: VfsPath::new("/x.zip"),
            format: CompressFormat::Zip,
            level: 6,
            exclude_junk: true,
        }),
    );
    assert!(
        matches!(st.state, JobState::Failed(ref m) if m.contains("read-only")),
        "{st:?}"
    );
    // Copying into an archive fails too.
    let st = wait(
        &tm,
        tm.enqueue(Job {
            kind: JobKind::Copy,
            src_fs: local.clone(),
            sources: vec![vp(&site.join("index.html"))],
            dst_fs: Some(ar),
            dst_dir: Some(VfsPath::root()),
            conflict: ConflictPolicy::Overwrite,
        }),
    );
    assert!(matches!(st.state, JobState::Done | JobState::Failed(_)));
    assert!(!st.errors.is_empty() || matches!(st.state, JobState::Failed(_)));
}

#[test]
fn extract_tree_and_single_file_with_the_transfer_manager() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let site = make_source(&root);
    let local: SharedVfs = Arc::new(LocalFs::new());
    let tm = TransferManager::new(2);
    for format in [CompressFormat::Zip, CompressFormat::TarZst] {
        let archive = root.join(format!("pack{}", format.ext()));
        let st = wait(
            &tm,
            tm.enqueue_compress(CompressJob {
                src_fs: local.clone(),
                sources: vec![vp(&site)],
                dst_fs: local.clone(),
                dst_path: vp(&archive),
                format,
                level: 3,
                exclude_junk: true,
            }),
        );
        assert_eq!(st.state, JobState::Done);
        let ar: SharedVfs = Arc::new(ArchiveFs::open(&archive, &OpenOptions::default()).unwrap());

        // Whole tree.
        let out = root.join(format!("out-{}", format.label()));
        let st = wait(
            &tm,
            tm.enqueue(Job {
                kind: JobKind::Extract,
                src_fs: ar.clone(),
                sources: vec![VfsPath::new("/site")],
                dst_fs: Some(local.clone()),
                dst_dir: Some(vp(&out)),
                conflict: ConflictPolicy::Skip,
            }),
        );
        assert_eq!(st.state, JobState::Done, "{st:?}");
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.title.starts_with("Extracting"), "{}", st.title);
        let mut expected = without_junk(snapshot(&*local, &vp(&site)));
        let mut got = snapshot(&*local, &vp(&out.join("site")));
        // Symlinks are extracted as copies of their target.
        expected.remove("home.html");
        assert_eq!(
            got.remove("home.html").map(|v| v.1),
            Some(b"<h1>Hi</h1>".to_vec())
        );
        assert_eq!(expected, got, "{format:?}");
        let exec = std::fs::metadata(out.join("site/deploy.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(exec, 0o755);

        // One file into another folder.
        let one = root.join(format!("one-{}", format.label()));
        let st = wait(
            &tm,
            tm.enqueue(Job {
                kind: JobKind::Copy,
                src_fs: ar.clone(),
                sources: vec![VfsPath::new("/site/assets/img/Grüße.txt")],
                dst_fs: Some(local.clone()),
                dst_dir: Some(vp(&one)),
                conflict: ConflictPolicy::Overwrite,
            }),
        );
        assert_eq!(st.state, JobState::Done);
        assert_eq!(
            std::fs::read(one.join("Grüße.txt")).unwrap(),
            "ünïcödé".as_bytes()
        );

        // Moving out of an archive copies but can't delete the source.
        let moved = root.join(format!("moved-{}", format.label()));
        let st = wait(
            &tm,
            tm.enqueue(Job {
                kind: JobKind::Move,
                src_fs: ar.clone(),
                sources: vec![VfsPath::new("/site/index.html")],
                dst_fs: Some(local.clone()),
                dst_dir: Some(vp(&moved)),
                conflict: ConflictPolicy::Overwrite,
            }),
        );
        assert!(st.errors.iter().any(|e| e.contains("read-only")), "{st:?}");
        assert!(ar.stat(&VfsPath::new("/site/index.html")).is_ok());
    }
}
