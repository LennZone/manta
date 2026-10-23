//! Batch rename on a real `LocalFs` in a temp dir: swaps, cycles, case-only
//! renames (case-insensitive APFS or not) and refusals.

use std::path::Path;

use manta_core::VfsPath;
use manta_ops::rename::{is_case_insensitive, rename_many};
use manta_vfs::LocalFs;

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn swaps_cycles_and_case_only_renames() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let dir = VfsPath::new(&root.to_string_lossy());
    let fs = LocalFs::new();
    for (n, body) in [
        ("a", "A"),
        ("b", "B"),
        ("x", "X"),
        ("y", "Y"),
        ("z", "Z"),
        ("readme", "R"),
    ] {
        write(&root, n, body);
    }
    let out = rename_many(
        &fs,
        &dir,
        &pairs(&[
            ("a", "b"),
            ("b", "a"),
            ("x", "y"),
            ("y", "z"),
            ("z", "x"),
            ("readme", "README"),
        ]),
    );
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert_eq!(out.ok.len(), 6);
    assert_eq!(read(&root, "b"), "A");
    assert_eq!(read(&root, "a"), "B");
    assert_eq!(read(&root, "y"), "X");
    assert_eq!(read(&root, "z"), "Y");
    assert_eq!(read(&root, "x"), "Z");
    assert_eq!(read(&root, "README"), "R");
    assert_eq!(
        names(&root),
        ["README", "a", "b", "x", "y", "z"],
        "no temp names left behind"
    );

    // Undo = the ok pairs reversed.
    let undo: Vec<(String, String)> = out.ok.iter().map(|(o, n)| (n.clone(), o.clone())).collect();
    let back = rename_many(&fs, &dir, &undo);
    assert!(back.failed.is_empty(), "{:?}", back.failed);
    assert_eq!(read(&root, "a"), "A");
    assert_eq!(read(&root, "x"), "X");
    assert_eq!(names(&root), ["a", "b", "readme", "x", "y", "z"]);
}

#[test]
fn refuses_collisions_without_touching_anything() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let dir = VfsPath::new(&root.to_string_lossy());
    let fs = LocalFs::new();
    for n in ["one.txt", "two.txt", "keep.txt"] {
        write(&root, n, n);
    }
    let out = rename_many(
        &fs,
        &dir,
        &pairs(&[("one.txt", "1.txt"), ("two.txt", "keep.txt")]),
    );
    assert!(out.ok.is_empty());
    assert_eq!(out.failed.len(), 2);
    assert!(
        out.failed[0].1.contains("already exists"),
        "{:?}",
        out.failed
    );
    assert_eq!(names(&root), ["keep.txt", "one.txt", "two.txt"]);
    assert_eq!(read(&root, "keep.txt"), "keep.txt", "never overwritten");
}

#[test]
fn case_sensitivity_probe_matches_the_volume() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().canonicalize().unwrap();
    let dir = VfsPath::new(&root.to_string_lossy());
    write(&root, "Probe.txt", "");
    let fs = LocalFs::new();
    let ci = is_case_insensitive(&fs, &dir, &["Probe.txt".to_string()]);
    assert_eq!(ci, root.join("PROBE.TXT").exists());
    // "Notes" → "notes" works either way.
    write(&root, "Notes", "n");
    let out = rename_many(&fs, &dir, &pairs(&[("Notes", "notes")]));
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert!(names(&root).contains(&"notes".to_string()));
}
