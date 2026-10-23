//! ArchiveFs on generated fixtures: zip (nested, implicit folders, unicode,
//! stored + deflated, encrypted, symlink, large entry), tar.gz / tar.xz /
//! tar.zst / tar.bz2 / tar with symlinks and hard links, single compressed
//! files, and malicious entries.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use manta_core::{EntryKind, Vfs, VfsError, VfsPath};
use manta_vfs::archive::{ArchiveFormat, ArchiveFs, OpenOptions};
use zip::write::SimpleFileOptions;

fn p(s: &str) -> VfsPath {
    VfsPath::new(s)
}

fn read(fs: &dyn Vfs, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    fs.open_read(&p(path))
        .unwrap_or_else(|e| panic!("open {path}: {e}"))
        .read_to_end(&mut buf)
        .unwrap();
    buf
}

fn names(fs: &dyn Vfs, dir: &str) -> Vec<String> {
    let mut v: Vec<String> = fs
        .list(&p(dir))
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    v.sort();
    v
}

fn big_data() -> Vec<u8> {
    (0..5_000_000u32)
        .map(|i| (i % 251) as u8 ^ (i / 7919) as u8)
        .collect()
}

const MTIME: u64 = 1_700_000_000;

fn make_zip(path: &Path) {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let deflated = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o640)
        .last_modified_time(zip::DateTime::from_date_and_time(2024, 3, 5, 10, 20, 30).unwrap());
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // Implicit folders: no "docs/" or "docs/guide/" entries.
    z.start_file("docs/guide/intro.md", deflated).unwrap();
    z.write_all(b"# Intro\nHello archive\n").unwrap();
    z.start_file("stored.txt", stored).unwrap();
    z.write_all(b"stored, not compressed").unwrap();
    z.start_file("übersicht/Größe – 日本.txt", deflated)
        .unwrap();
    z.write_all("ünïcödé".as_bytes()).unwrap();
    z.add_directory("empty/", SimpleFileOptions::default())
        .unwrap();
    z.start_file("big.bin", deflated).unwrap();
    z.write_all(&big_data()).unwrap();
    z.add_symlink(
        "link.md",
        "docs/guide/intro.md",
        SimpleFileOptions::default(),
    )
    .unwrap();
    z.add_symlink("docs-link", "docs", SimpleFileOptions::default())
        .unwrap();
    // Windows-style separators.
    z.start_file("win\\path\\file.txt", deflated).unwrap();
    z.write_all(b"backslashes").unwrap();
    let secret = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .with_aes_encryption(zip::AesMode::Aes256, "hunter2");
    z.start_file("secret.txt", secret).unwrap();
    z.write_all(b"top secret").unwrap();
    z.finish().unwrap();
}

fn zip_fixture() -> (tempfile::TempDir, PathBuf) {
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("backup.zip");
    make_zip(&path);
    (td, path)
}

#[test]
fn zip_listing_stat_and_read() {
    let (_td, path) = zip_fixture();
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    assert_eq!(fs.format(), ArchiveFormat::Zip);
    assert_eq!(fs.label(), "backup.zip");
    assert!(fs.is_read_only());
    assert!(fs.rejected().is_empty(), "{:?}", fs.rejected());
    assert_eq!(
        names(&fs, "/"),
        vec![
            "big.bin",
            "docs",
            "docs-link",
            "empty",
            "link.md",
            "secret.txt",
            "stored.txt",
            "win",
            "übersicht"
        ]
    );
    // Implicit folders were synthesised.
    assert_eq!(names(&fs, "/docs"), vec!["guide"]);
    assert!(fs.stat(&p("/docs")).unwrap().is_dir());
    assert_eq!(names(&fs, "/docs/guide"), vec!["intro.md"]);
    assert_eq!(names(&fs, "/win/path"), vec!["file.txt"]);
    assert!(names(&fs, "/empty").is_empty());

    let intro = fs.stat(&p("/docs/guide/intro.md")).unwrap();
    assert_eq!(intro.kind, EntryKind::File);
    assert_eq!(intro.size, 22);
    assert_eq!(intro.permissions, Some(0o640));
    let m = intro.modified.expect("mtime").with_timezone(&chrono::Local);
    assert_eq!(
        m.format("%Y-%m-%d %H:%M:%S").to_string(),
        "2024-03-05 10:20:30"
    );
    assert_eq!(
        read(&fs, "/docs/guide/intro.md"),
        b"# Intro\nHello archive\n"
    );
    assert_eq!(read(&fs, "/stored.txt"), b"stored, not compressed");
    assert_eq!(
        read(&fs, "/übersicht/Größe – 日本.txt"),
        "ünïcödé".as_bytes()
    );
    assert_eq!(read(&fs, "/win/path/file.txt"), b"backslashes");
    // Large entries are streamed by a helper thread.
    assert_eq!(read(&fs, "/big.bin"), big_data());
    assert_eq!(fs.stat(&p("/big.bin")).unwrap().size, 5_000_000);
    assert!(fs.total_size() > 5_000_000);

    // A reader dropped half-way doesn't hang.
    let mut r = fs.open_read(&p("/big.bin")).unwrap();
    let mut buf = [0u8; 1000];
    r.read_exact(&mut buf).unwrap();
    drop(r);
}

#[test]
fn zip_symlinks_and_encryption() {
    let (_td, path) = zip_fixture();
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    let link = fs.stat(&p("/link.md")).unwrap();
    assert_eq!(link.kind, EntryKind::Symlink);
    assert_eq!(link.link_target.as_deref(), Some("docs/guide/intro.md"));
    assert_eq!(read(&fs, "/link.md"), b"# Intro\nHello archive\n");
    // A link to a folder can be listed through.
    assert_eq!(names(&fs, "/docs-link/guide"), vec!["intro.md"]);
    assert!(fs.open_read(&p("/docs-link")).is_err());

    assert!(fs.is_encrypted(&p("/secret.txt")));
    assert!(!fs.is_encrypted(&p("/stored.txt")));
    match fs.open_read(&p("/secret.txt")) {
        Err(VfsError::PermissionDenied(m)) => assert!(m.contains("encrypted"), "{m}"),
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("encrypted entry was readable"),
    }
}

#[test]
fn writes_are_refused() {
    let (_td, path) = zip_fixture();
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    let ro = |r: Result<(), VfsError>| matches!(r, Err(VfsError::Unsupported(m)) if m == "archive is read-only");
    assert!(ro(fs.create_dir(&p("/new"))));
    assert!(ro(fs.remove_file(&p("/stored.txt"))));
    assert!(ro(fs.remove_dir(&p("/empty"))));
    assert!(ro(fs.rename(&p("/stored.txt"), &p("/x"))));
    assert!(matches!(
        fs.open_write(&p("/x")),
        Err(VfsError::Unsupported(_))
    ));
    assert!(matches!(
        fs.stat(&p("/missing")),
        Err(VfsError::NotFound(_))
    ));
    assert!(matches!(
        fs.list(&p("/stored.txt")),
        Err(VfsError::NotADirectory(_))
    ));
    // Resume: read from an offset only (the source side of an extraction).
    let caps = fs.supports_resume();
    assert!(!caps.append && !caps.upload_session);
    assert!(matches!(
        fs.open_append(&p("/stored.txt")),
        Err(VfsError::Unsupported(_))
    ));
}

#[test]
fn archive_entries_read_from_an_offset() {
    let (_td, path) = zip_fixture();
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    let mut tail = Vec::new();
    fs.open_read_at(&p("/big.bin"), 3_333_333)
        .unwrap()
        .read_to_end(&mut tail)
        .unwrap();
    assert!(tail == big_data()[3_333_333..]);
    assert!(fs.open_read_at(&p("/big.bin"), 5_000_001).is_err());
}

// ------------------------------------------------------------------ tar

enum Codec {
    None,
    Gz,
    Bz2,
    Xz,
    Zst,
}

fn tar_bytes() -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let mut dir = tar::Header::new_gnu();
    dir.set_entry_type(tar::EntryType::Directory);
    dir.set_mode(0o750);
    dir.set_mtime(MTIME);
    dir.set_size(0);
    b.append_data(&mut dir, "./project/", std::io::empty())
        .unwrap();
    let mut add = |name: &str, data: &[u8], mode: u32| {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(mode);
        h.set_mtime(MTIME);
        h.set_size(data.len() as u64);
        b.append_data(&mut h, name, data).unwrap();
    };
    add("./project/src/main.rs", b"fn main() {}\n", 0o644);
    add("./project/run.sh", b"#!/bin/sh\necho hi\n", 0o755);
    let long = format!("./project/{}/deep.txt", "a-very-long-folder-name".repeat(6));
    add(&long, b"long names work", 0o644);
    add("./project/big.bin", &big_data(), 0o600);
    let mut link = |name: &str, target: &str, kind: tar::EntryType| {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(kind);
        h.set_mode(0o777);
        h.set_mtime(MTIME);
        h.set_size(0);
        b.append_link(&mut h, name, target).unwrap();
    };
    link("./project/latest", "src/main.rs", tar::EntryType::Symlink);
    link("./project/srcdir", "src", tar::EntryType::Symlink);
    link("./project/hard.sh", "project/run.sh", tar::EntryType::Link);
    link(
        "./project/escape",
        "../../etc/passwd",
        tar::EntryType::Symlink,
    );
    link("./project/abs", "/etc/passwd", tar::EntryType::Symlink);
    b.into_inner().unwrap()
}

fn compress(data: &[u8], codec: &Codec) -> Vec<u8> {
    match codec {
        Codec::None => data.to_vec(),
        Codec::Gz => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
        Codec::Bz2 => {
            let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
        Codec::Xz => {
            let mut e =
                lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(3))
                    .unwrap();
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
        Codec::Zst => zstd::encode_all(data, 3).unwrap(),
    }
}

fn check_tar(fs: &ArchiveFs) {
    assert_eq!(names(fs, "/"), vec!["project"]);
    let proj = fs.stat(&p("/project")).unwrap();
    assert!(proj.is_dir());
    assert_eq!(proj.permissions, Some(0o750));
    let mut root = names(fs, "/project");
    root.retain(|n| !n.starts_with("a-very"));
    assert_eq!(
        root,
        vec!["abs", "big.bin", "escape", "hard.sh", "latest", "run.sh", "src", "srcdir"]
    );
    assert_eq!(read(fs, "/project/src/main.rs"), b"fn main() {}\n");
    let run = fs.stat(&p("/project/run.sh")).unwrap();
    assert_eq!(run.permissions, Some(0o755));
    assert_eq!(run.modified.unwrap().timestamp() as u64, MTIME);
    assert_eq!(
        read(
            fs,
            &format!("/project/{}/deep.txt", "a-very-long-folder-name".repeat(6))
        ),
        b"long names work"
    );
    assert_eq!(read(fs, "/project/big.bin"), big_data());
    // Symlinks stay symlinks and resolve inside the archive.
    let latest = fs.stat(&p("/project/latest")).unwrap();
    assert_eq!(latest.kind, EntryKind::Symlink);
    assert_eq!(latest.link_target.as_deref(), Some("src/main.rs"));
    assert_eq!(read(fs, "/project/latest"), b"fn main() {}\n");
    assert_eq!(names(fs, "/project/srcdir"), vec!["main.rs"]);
    // Hard links share the target's data.
    assert_eq!(read(fs, "/project/hard.sh"), b"#!/bin/sh\necho hi\n");
    // Links out of the archive can't be read.
    assert!(fs.open_read(&p("/project/escape")).is_err());
    assert!(fs.open_read(&p("/project/abs")).is_err());
}

fn tar_fixture(name: &str, codec: Codec) -> (tempfile::TempDir, PathBuf) {
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join(name);
    std::fs::write(&path, compress(&tar_bytes(), &codec)).unwrap();
    (td, path)
}

#[test]
fn tar_family_with_symlinks() {
    for (name, codec, format) in [
        ("a.tar", Codec::None, ArchiveFormat::Tar),
        ("a.tar.gz", Codec::Gz, ArchiveFormat::TarGz),
        ("a.tgz", Codec::Gz, ArchiveFormat::TarGz),
        ("a.tar.bz2", Codec::Bz2, ArchiveFormat::TarBz2),
        ("a.tar.xz", Codec::Xz, ArchiveFormat::TarXz),
        ("a.tar.zst", Codec::Zst, ArchiveFormat::TarZst),
    ] {
        let (_td, path) = tar_fixture(name, codec);
        let progress = Arc::new(AtomicU64::new(0));
        let opts = OpenOptions {
            progress: Some(progress.clone()),
            ..Default::default()
        };
        let fs = ArchiveFs::open(&path, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(fs.format(), format, "{name}");
        assert!(!fs.is_streaming(), "{name}: should be spooled");
        check_tar(&fs);
        assert_eq!(
            progress.load(std::sync::atomic::Ordering::Relaxed),
            std::fs::metadata(&path).unwrap().len()
        );
    }
}

#[test]
fn compressed_tar_without_spool_streams_from_the_start() {
    let (_td, path) = tar_fixture("a.tar.zst", Codec::Zst);
    let opts = OpenOptions {
        spool_limit: 0,
        ..Default::default()
    };
    let fs = ArchiveFs::open(&path, &opts).unwrap();
    assert!(fs.is_streaming());
    check_tar(&fs);
    // A spool limit smaller than the archive also falls back to streaming.
    let opts = OpenOptions {
        spool_limit: 1000,
        ..Default::default()
    };
    let fs = ArchiveFs::open(&path, &opts).unwrap();
    assert!(fs.is_streaming());
    assert_eq!(read(&fs, "/project/run.sh"), b"#!/bin/sh\necho hi\n");
}

#[test]
fn spool_is_removed_on_drop() {
    let (_td, path) = tar_fixture("a.tar.gz", Codec::Gz);
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    let temps = fs.temp_files();
    assert_eq!(temps.len(), 1);
    assert!(temps[0].exists());
    assert!(temps[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(manta_vfs::archive::TEMP_PREFIX));
    drop(fs);
    assert!(!temps[0].exists());
}

#[test]
fn cancel_stops_indexing() {
    let (_td, path) = tar_fixture("a.tar.xz", Codec::Xz);
    let opts = OpenOptions {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..Default::default()
    };
    assert!(matches!(
        ArchiveFs::open(&path, &opts),
        Err(VfsError::Cancelled)
    ));
}

#[test]
fn single_compressed_files() {
    let td = tempfile::tempdir().unwrap();
    let text = b"just one file\n".repeat(1000);
    for (name, codec, inner) in [
        ("notes.txt.gz", Codec::Gz, "notes.txt"),
        ("notes.txt.bz2", Codec::Bz2, "notes.txt"),
        ("notes.txt.xz", Codec::Xz, "notes.txt"),
        ("notes.txt.zst", Codec::Zst, "notes.txt"),
    ] {
        let path = td.path().join(name);
        std::fs::write(&path, compress(&text, &codec)).unwrap();
        let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
        assert_eq!(names(&fs, "/"), vec![inner], "{name}");
        assert_eq!(
            fs.stat(&p(&format!("/{inner}"))).unwrap().size,
            text.len() as u64
        );
        assert_eq!(read(&fs, &format!("/{inner}")), text);
    }
    // A ".gz" that is really a tar is shown as the tar.
    let path = td.path().join("bundle.gz");
    std::fs::write(&path, compress(&tar_bytes(), &Codec::Gz)).unwrap();
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    assert_eq!(fs.format(), ArchiveFormat::TarGz);
    check_tar(&fs);
}

#[test]
fn malicious_entries_are_refused() {
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("evil.zip");
    let f = std::fs::File::create(&path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let o = SimpleFileOptions::default();
    for name in [
        "../evil",
        "/abs",
        "ok/../../evil2",
        "..\\win-evil",
        "C:\\drive.txt",
        "good/ok.txt",
    ] {
        z.start_file(name, o).unwrap();
        z.write_all(b"x").unwrap();
    }
    z.finish().unwrap();
    let fs = ArchiveFs::open(&path, &OpenOptions::default()).unwrap();
    assert_eq!(names(&fs, "/"), vec!["good"]);
    assert_eq!(names(&fs, "/good"), vec!["ok.txt"]);
    assert_eq!(fs.rejected().len(), 5, "{:?}", fs.rejected());
    // Nothing can be reached outside the root.
    assert!(fs.stat(&p("/../evil")).is_err());
    assert!(fs.stat(&p("/evil")).is_err());
    assert!(fs.stat(&p("/abs")).is_err());
}

#[test]
fn not_an_archive() {
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("fake.zip");
    std::fs::write(&path, b"this is not a zip").unwrap();
    assert!(ArchiveFs::open(&path, &OpenOptions::default()).is_err());
    let path = td.path().join("fake.tar.gz");
    std::fs::write(&path, b"not gzip either").unwrap();
    assert!(ArchiveFs::open(&path, &OpenOptions::default()).is_err());
    let path = td.path().join("plain.txt");
    std::fs::write(&path, b"x").unwrap();
    assert!(matches!(
        ArchiveFs::open(&path, &OpenOptions::default()),
        Err(VfsError::Unsupported(_))
    ));
}

#[test]
fn downcast_from_shared_vfs() {
    let (_td, path) = zip_fixture();
    let fs: manta_core::SharedVfs =
        Arc::new(ArchiveFs::open(&path, &OpenOptions::default()).unwrap());
    let a = manta_vfs::archive::as_archive(&*fs).expect("downcast");
    assert_eq!(a.name(), "backup.zip");
    let local: manta_core::SharedVfs = Arc::new(manta_vfs::LocalFs::new());
    assert!(manta_vfs::archive::as_archive(&*local).is_none());
    assert!(!local.is_read_only());
}
