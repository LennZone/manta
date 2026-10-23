//! Archives in the app: open with Enter, breadcrumb, leave with ⌫, history,
//! read-only refusals, extraction; renders of an archive pane, the Compress
//! sheet and archive jobs in the transfers panel.

use std::io::Write;
use std::path::Path;

use eframe::egui::{self, Theme};
use manta_core::VfsPath;
use manta_ops::{JobKind, JobState};

use super::{
    app, harness, harness_with, job, save, select, settle, wait_until, wgpu_available, Fixture, H,
    WIDE,
};
use crate::app::Side;
use crate::dialogs::Dialog;
use crate::pane::{crumbs, CrumbTarget};

/// A zip with folders, an implicit folder, a symlink and an encrypted entry.
pub(super) fn make_zip(path: &Path) {
    use zip::write::SimpleFileOptions;
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let o = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
        .last_modified_time(zip::DateTime::from_date_and_time(2026, 9, 28, 14, 5, 0).unwrap());
    let d = SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::from_date_and_time(2026, 9, 28, 14, 5, 0).unwrap());
    z.add_directory("docs/", d).unwrap();
    z.start_file("docs/index.md", o).unwrap();
    z.write_all(b"# Docs\n\nWelcome to the site backup.\n")
        .unwrap();
    z.start_file("docs/guide/setup.md", o).unwrap();
    z.write_all(b"Install it.\n").unwrap();
    z.add_directory("assets/", d).unwrap();
    z.start_file("assets/logo.svg", o).unwrap();
    z.write_all(&b"<svg/>".repeat(800)).unwrap();
    z.start_file("assets/hero.png", o).unwrap();
    z.write_all(&vec![7u8; 380_000]).unwrap();
    z.start_file("index.html", o).unwrap();
    z.write_all(&b"<!doctype html><title>Site</title>".repeat(60))
        .unwrap();
    z.start_file("robots.txt", o).unwrap();
    z.write_all(b"User-agent: *\n").unwrap();
    z.add_symlink("latest.html", "index.html", d).unwrap();
    let secret = o.with_aes_encryption(zip::AesMode::Aes256, "pw");
    z.start_file("credentials.env", secret).unwrap();
    z.write_all(b"TOKEN=abc\n").unwrap();
    z.finish().unwrap();
}

pub(super) fn archive_fixture() -> Fixture {
    let fx = super::fixture();
    make_zip(&fx.root.join("site-backup.zip"));
    fx
}

fn labels(h: &mut H, side: Side) -> Vec<String> {
    crumbs(&app(h).pane(side).tab().loc)
        .into_iter()
        .map(|c| c.label)
        .collect()
}

pub(super) fn settle_open(h: &mut H, side: Side) {
    // Wait for the archive itself to be the location (not just "nothing is
    // loading": right after Enter the open may not have started yet).
    wait_until(h, "the archive to open", |h| {
        let t = h.state().pane(side).tab();
        t.loc.archive.is_some() && t.opening.is_none() && !t.loading && !t.entries.is_empty()
    });
    settle(h);
}

fn names(h: &mut H, side: Side) -> Vec<String> {
    let t = app(h).pane(side).tab();
    t.view
        .iter()
        .filter_map(|&i| t.entries.get(i))
        .map(|e| e.name.clone())
        .collect()
}

fn toast_titles(h: &mut H) -> Vec<String> {
    h.state().toasts.titles()
}

#[test]
fn open_archive_breadcrumb_up_history_and_read_only() {
    let fx = archive_fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    app(&mut h).active = Side::Left;
    select(&mut h, Side::Left, &["site-backup.zip"]);
    h.key_press(egui::Key::Enter);
    settle_open(&mut h, Side::Left);
    {
        let t = app(&mut h).pane(Side::Left).tab();
        assert!(
            t.loc.archive.is_some(),
            "Enter opens the archive in the pane"
        );
        assert!(t.loc.read_only());
        assert!(t.loc.path.is_root());
        assert_eq!(t.title(), "site-backup.zip");
        assert!(
            t.locked.contains("credentials.env"),
            "encrypted entry gets a lock badge"
        );
    }
    assert_eq!(
        names(&mut h, Side::Left),
        vec![
            "assets",
            "docs",
            "credentials.env",
            "index.html",
            "latest.html",
            "robots.txt"
        ]
    );
    let l = labels(&mut h, Side::Left);
    assert_eq!(&l[l.len() - 2..], ["Projects", "site-backup.zip"]);
    let c = crumbs(&app(&mut h).pane(Side::Left).tab().loc);
    assert!(c.last().unwrap().archive);
    // The folder crumb leads out of the archive and selects it.
    match &c[c.len() - 2].target {
        CrumbTarget::Outer(loc, select) => {
            assert!(loc.archive.is_none());
            assert_eq!(select.as_deref(), Some("site-backup.zip"));
        }
        CrumbTarget::Here(_) => panic!("the containing folder must be outside the archive"),
    }

    // Into a folder of the archive.
    select(&mut h, Side::Left, &["docs"]);
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    let l = labels(&mut h, Side::Left);
    assert_eq!(&l[l.len() - 3..], ["Projects", "site-backup.zip", "docs"]);
    assert_eq!(names(&mut h, Side::Left), vec!["guide", "index.md"]);

    // Up twice: archive root, then the containing folder with the archive selected.
    h.key_press(egui::Key::Backspace);
    settle(&mut h);
    assert!(app(&mut h).pane(Side::Left).tab().loc.path.is_root());
    assert!(app(&mut h).pane(Side::Left).tab().loc.archive.is_some());
    h.key_press(egui::Key::Backspace);
    settle(&mut h);
    {
        let t = app(&mut h).pane(Side::Left).tab();
        assert!(t.loc.archive.is_none());
        assert_eq!(t.loc.path, VfsPath::new(&fx.root.to_string_lossy()));
        assert!(t.selected.contains("site-backup.zip"), "{:?}", t.selected);
    }
    // History goes back into the archive and forward out again.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::OpenBracket);
    settle(&mut h);
    assert!(app(&mut h).pane(Side::Left).tab().loc.archive.is_some());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::CloseBracket);
    settle(&mut h);
    assert!(app(&mut h).pane(Side::Left).tab().loc.archive.is_none());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::OpenBracket);
    settle(&mut h);
    assert!(app(&mut h).pane(Side::Left).tab().loc.archive.is_some());

    // Read-only: no new folder, rename or delete dialogs.
    app(&mut h).toasts.clear();
    select(&mut h, Side::Left, &["robots.txt"]);
    h.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::N,
    );
    h.key_press(egui::Key::F2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Backspace);
    h.run_steps(2);
    assert!(app(&mut h).dialog.is_none());
    let t = toast_titles(&mut h);
    assert!(t.iter().any(|t| t.contains("create folders")), "{t:?}");
    assert!(t.iter().any(|t| t.contains("rename")), "{t:?}");
    assert!(t.iter().any(|t| t.contains("delete")), "{t:?}");

    // Pasting (copying) into the archive is refused.
    app(&mut h).toasts.clear();
    app(&mut h).active = Side::Right;
    select(&mut h, Side::Right, &["app-icon.svg"]);
    h.key_press(egui::Key::F5);
    h.run_steps(2);
    assert!(
        app(&mut h).dialog.is_none(),
        "no copy dialog into an archive"
    );
    assert!(
        toast_titles(&mut h).iter().any(|t| t.contains("paste")),
        "{:?}",
        toast_titles(&mut h)
    );

    // Copying out of the archive (F5) extracts into the other pane.
    app(&mut h).active = Side::Left;
    select(&mut h, Side::Left, &["robots.txt"]);
    h.key_press(egui::Key::F5);
    h.run_steps(2);
    match &app(&mut h).dialog {
        Some(Dialog::Transfer(f)) => {
            assert!(f.extract);
            assert!(f.archive.is_some());
        }
        _ => panic!("expected the extract dialog"),
    }
    h.key_press(egui::Key::Enter);
    let out = fx.root.join("Design/robots.txt");
    wait_until(&mut h, "the extracted file", |_| out.exists());
    assert_eq!(std::fs::read(&out).unwrap(), b"User-agent: *\n");

    // Extract Here on the archive file: a folder named after it.
    h.key_press(egui::Key::Backspace);
    settle(&mut h);
    select(&mut h, Side::Left, &["site-backup.zip"]);
    h.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        egui::Key::E,
    );
    let target = fx.root.join("site-backup/docs/guide/setup.md");
    wait_until(&mut h, "Extract Here", |_| target.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"Install it.\n");
}

#[test]
fn archives_open_with_app_when_switched_off() {
    let fx = archive_fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    app(&mut h).prefs.open_archives = false;
    select(&mut h, Side::Left, &["site-backup.zip"]);
    // Browse Archive still works on request.
    app(&mut h).handle_action(
        &egui::Context::default(),
        crate::app::Action::BrowseArchive(Side::Left),
    );
    settle_open(&mut h, Side::Left);
    assert!(app(&mut h).pane(Side::Left).tab().loc.archive.is_some());
}

#[test]
fn compress_sheet_creates_an_archive() {
    let fx = archive_fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    app(&mut h).active = Side::Left;
    select(&mut h, Side::Left, &["src", "README.md"]);
    h.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        egui::Key::C,
    );
    h.run_steps(2);
    match &mut app(&mut h).dialog {
        Some(Dialog::Compress(f)) => {
            assert_eq!(f.file_name(), "Archive.zip");
            f.format = manta_ops::CompressFormat::TarGz;
            f.stem = "sources".into();
        }
        _ => panic!("expected the Compress sheet"),
    }
    let d = app(&mut h).dialog.take();
    if let Some(Dialog::Compress(f)) = d {
        app(&mut h).submit_compress(f);
    }
    let out = fx.root.join("sources.tar.gz");
    wait_until(&mut h, "the archive", |h| {
        h.state()
            .transfers
            .snapshot()
            .iter()
            .all(|j| j.state.is_finished())
            && out.exists()
    });
    let ar = manta_vfs::ArchiveFs::open(&out, &Default::default()).unwrap();
    use manta_core::Vfs;
    let mut top: Vec<String> = ar
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    top.sort();
    assert_eq!(top, vec!["README.md", "src"]);
}

// ------------------------------------------------------------------ renders

fn dir() -> std::path::PathBuf {
    super::out_dir()
}

#[test]
fn render_archive_pane() {
    if !wgpu_available() {
        return;
    }
    let fx = archive_fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, WIDE, theme);
        app(&mut h).active = Side::Left;
        select(&mut h, Side::Left, &["site-backup.zip"]);
        h.key_press(egui::Key::Enter);
        settle_open(&mut h, Side::Left);
        select(&mut h, Side::Left, &["index.html"]);
        save(&mut h, &format!("archive_pane_{tname}"));
        // Inside a sub folder: the breadcrumb continues after the archive.
        select(&mut h, Side::Left, &["docs"]);
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        save(&mut h, &format!("archive_pane_docs_{tname}"));
        let _ = dir();
    }
}

#[test]
fn render_compress_sheet() {
    if !wgpu_available() {
        return;
    }
    let fx = archive_fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, WIDE, theme);
        app(&mut h).active = Side::Left;
        select(&mut h, Side::Left, &["src", "Design", "README.md"]);
        app(&mut h).handle_action(
            &egui::Context::default(),
            crate::app::Action::Compress(Side::Left),
        );
        h.run_steps(3);
        save(&mut h, &format!("compress_sheet_{tname}"));
    }
}

#[test]
fn render_archive_jobs_and_opening() {
    if !wgpu_available() {
        return;
    }
    let fx = archive_fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, WIDE, theme);
        let a = app(&mut h);
        a.fake_jobs = Some(vec![
            job(
                1,
                JobKind::Extract,
                "Extracting \"site-backup\" to /Users/anna/Projects",
                JobState::Running,
                18_600_000,
                41_000_000,
                (212, 530),
            ),
            job(
                2,
                JobKind::Compress,
                "Compressing 3 items to \"Archive.tar.zst\"",
                JobState::Queued,
                0,
                9_800_000,
                (0, 84),
            ),
            job(
                3,
                JobKind::Extract,
                "Extracting \"robots.txt\" to /tmp",
                JobState::Done,
                14,
                14,
                (1, 1),
            ),
        ]);
        a.show_transfers = true;
        // An archive being downloaded in the right pane.
        let tab = a.pane_mut(Side::Right).tab_mut();
        tab.opening = Some(crate::pane::ArchiveOpening {
            id: 99,
            name: "release-2026-10.tar.xz".into(),
            done: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(31_000_000)),
            total: 74_000_000,
            downloading: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            since: std::time::Instant::now() - std::time::Duration::from_secs(2),
        });
        save(&mut h, &format!("archive_jobs_{tname}"));
    }
}
