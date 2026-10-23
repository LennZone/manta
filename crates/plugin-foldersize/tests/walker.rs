//! The size walker on real temp trees.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use manta_plugin_foldersize::walker::{measure, Category, Totals};

fn file(p: &Path, len: usize) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, vec![b'x'; len]).unwrap();
}

fn run(p: &Path) -> Totals {
    measure(p, &AtomicBool::new(false), |_| {})
}

#[test]
fn nested_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    file(&r.join("a.txt"), 100);
    file(&r.join("sub/b.png"), 2000);
    file(&r.join("sub/deeper/c.rs"), 30);
    file(&r.join("sub/deeper/deepest/d.mov"), 4000);
    fs::create_dir_all(r.join("empty")).unwrap();
    let t = run(r);
    assert_eq!(t.logical, 6130);
    assert_eq!(t.files, 4);
    assert_eq!(t.dirs, 4);
    assert!(t.complete() && t.done && !t.cancelled);
    assert!(t.on_disk >= 4 * 512, "{}", t.on_disk);
    assert_eq!(t.by_cat[Category::Images.index()], 2000);
    assert_eq!(t.by_cat[Category::Video.index()], 4000);
    assert_eq!(t.by_cat[Category::Code.index()], 30);
    assert_eq!(t.by_cat[Category::Documents.index()], 100);
    assert_eq!(t.dominant(), Category::Video);
    // A single file measures as itself.
    let f = run(&r.join("a.txt"));
    assert_eq!((f.logical, f.files), (100, 1));
}

#[test]
fn symlink_loops_are_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    file(&r.join("data/big.bin"), 10_000);
    std::os::unix::fs::symlink(".", r.join("data/self")).unwrap();
    std::os::unix::fs::symlink("..", r.join("data/up")).unwrap();
    std::os::unix::fs::symlink(r.join("data"), r.join("data/abs")).unwrap();
    let t = run(r);
    // The file once plus three small symlinks; no endless loop.
    assert_eq!(t.files, 4);
    assert!(t.logical >= 10_000 && t.logical < 11_000, "{}", t.logical);
    assert_eq!(t.dirs, 1);
    assert!(t.complete());
}

#[test]
fn hard_links_count_once() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    file(&r.join("one/original.bin"), 50_000);
    fs::create_dir_all(r.join("two")).unwrap();
    fs::hard_link(r.join("one/original.bin"), r.join("two/link.bin")).unwrap();
    fs::hard_link(r.join("one/original.bin"), r.join("one/again.bin")).unwrap();
    let t = run(r);
    assert_eq!(t.files, 1);
    assert_eq!(t.logical, 50_000);
    // Each folder alone still counts it.
    assert_eq!(run(&r.join("two")).logical, 50_000);
}

#[test]
fn sparse_files_differ_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    let f = fs::File::create(r.join("sparse.img")).unwrap();
    f.set_len(200_000_000).unwrap();
    drop(f);
    let t = run(r);
    assert_eq!(t.logical, 200_000_000);
    use std::os::unix::fs::MetadataExt;
    let blocks = fs::metadata(r.join("sparse.img")).unwrap().blocks() * 512;
    if blocks < 200_000_000 {
        // The file system supports sparse files (APFS does).
        assert!(
            t.on_disk < t.logical / 2,
            "on disk {} vs {}",
            t.on_disk,
            t.logical
        );
    }
}

#[test]
fn permission_denied_counts_what_is_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    file(&r.join("open/a.bin"), 1_000);
    file(&r.join("locked/secret.bin"), 9_999);
    fs::set_permissions(r.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(r.join("locked")).is_ok(); // root can read anyway
    let t = run(r);
    fs::set_permissions(r.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        return;
    }
    assert_eq!(t.logical, 1_000);
    assert_eq!(t.unreadable, 1);
    assert!(t.done && !t.complete(), "marked incomplete");
}

#[test]
fn cancellation_stops_the_walk() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    for i in 0..40 {
        for j in 0..50 {
            file(&r.join(format!("d{i}/f{j}")), 10);
        }
    }
    // Cancelled before the start: nothing counted.
    let t = measure(r, &AtomicBool::new(true), |_| {});
    assert!(t.cancelled && !t.done && t.files == 0);
    // Cancelled from the progress callback: stops early.
    let cancel = AtomicBool::new(false);
    let mut calls = 0;
    let t = measure(r, &cancel, |_| {
        calls += 1;
        cancel.store(true, Ordering::Relaxed);
    });
    if calls > 0 {
        assert!(t.cancelled && t.files < 2000);
    }
    // Uncancelled: everything.
    assert_eq!(run(r).files, 2000);
}

#[test]
fn files_in_app_bundles_count_as_apps() {
    let tmp = tempfile::tempdir().unwrap();
    file(&tmp.path().join("Apps/Figma.app/Contents/MacOS/Figma"), 700);
    file(&tmp.path().join("Apps/readme.txt"), 10);
    let t = run(&tmp.path().join("Apps"));
    assert_eq!(t.by_cat[Category::Apps.index()], 700);
    assert_eq!(t.dominant(), Category::Apps);
    let t = run(&tmp.path().join("Apps/Figma.app"));
    assert_eq!(t.by_cat[Category::Apps.index()], 700);
}

#[test]
fn categories() {
    assert_eq!(Category::of_name("photo.JPG"), Category::Images);
    assert_eq!(Category::of_name("Movie.mov"), Category::Video);
    assert_eq!(Category::of_name("Safari.app"), Category::Apps);
    assert_eq!(Category::of_name(".zshrc"), Category::Other);
    assert_eq!(Category::of_name("Makefile"), Category::Other);
    assert_eq!(Category::of_name("x.tar.gz"), Category::Archives);
}
