//! Seeing what copies and uploads do while the transfers panel is hidden:
//! the status bar's progress pill ("Kopiere 3 Objekte … 42 %", "Fertig",
//! red until seen after an error; click: the transfers), and the "Google
//! Drive lädt … hoch" pill for what a cloud provider still uploads after
//! the copy.
//!
//! Real copies run on the transfer manager into a fake Google Drive folder
//! with simulated upload states ([`FakeProbe`]); the user's real cloud
//! files are never touched. `render_progress_pill` writes German light /
//! dark PNGs to `$MANTA_RENDER_DIR/progress`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::Theme;
use egui_kittest::kittest::Queryable;
use manta_ops::{JobKind, JobState, JobStatus};
use manta_vfs::cloud_local::{self, SyncInfo, SyncState};

use super::cloud_ux_tests::harness_cloud;
use super::{
    app, fixture, render_dir, save_to, select, settle, wait_until, wgpu_available, Fixture, H,
};
use crate::app::{Action, Dialog, Side};
use crate::cloud::{Cloud, FakeProbe};

fn job(id: u64, kind: JobKind, title: &str, state: JobState, done: u64, total: u64) -> JobStatus {
    JobStatus {
        id,
        kind,
        title: title.into(),
        state,
        current_file: Some("Budget 2026.xlsx".into()),
        bytes_done: done,
        bytes_total: total,
        files_done: 1,
        files_total: 3,
        speed: 2_000_000.0,
        errors: vec![],
        resume: manta_ops::ResumeInfo {
            can_pause: true,
            ..Default::default()
        },
    }
}

/// A fake Google Drive for desktop folder with three files.
struct Drive {
    _dir: tempfile::TempDir,
    my: PathBuf,
    probe: Arc<FakeProbe>,
}

fn harness(fx: &Fixture, theme: Theme, gpu: bool) -> (H, Drive) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let my = home.join("Library/CloudStorage/GoogleDrive-anna@gmail.com/My Drive");
    std::fs::create_dir_all(&my).unwrap();
    for n in ["Angebot.pdf", "Fotos.zip", "Notizen.txt"] {
        std::fs::write(my.join(n), b"x").unwrap();
    }
    let probe = Arc::new(FakeProbe::default());
    let h2 = home.clone();
    let cloud = Cloud::new(
        Box::new(move || cloud_local::discover_in(&h2)),
        probe.clone(),
        true,
    );
    let h = harness_cloud(fx, theme, gpu, cloud, None);
    (
        h,
        Drive {
            _dir: dir,
            my,
            probe,
        },
    )
}

fn open_drive(h: &mut H, d: &Drive) {
    super::cloud_ux_tests::open_local(h, Side::Left, &d.my);
}

fn reread(h: &mut H, d: &Drive) {
    app(h).cloud.invalidate(&d.my);
    h.run_steps(3);
}

fn has(h: &H, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

#[test]
fn running_job_pill_opens_the_transfers_and_hides_with_the_panel() {
    let fx = fixture();
    let (mut h, _d) = harness(&fx, Theme::Light, false);
    app(&mut h).fake_jobs = Some(vec![job(
        1,
        JobKind::Copy,
        "Copying 3 items to /Drive",
        JobState::Running,
        420,
        1000,
    )]);
    h.run_steps(3);
    let label = "Copying 3 items… 42%";
    assert!(has(&h, label), "the pill names kind, objects and percent");
    h.get_by_label(label).click();
    h.run_steps(3);
    assert!(app(&mut h).show_transfers, "a click shows the transfers");
    assert!(!has(&h, label), "the panel shows it: no pill");

    // Paused: says so instead of a percentage.
    app(&mut h).show_transfers = false;
    app(&mut h).fake_jobs = Some(vec![job(
        1,
        JobKind::Move,
        "Moving \"a.iso\" to /x",
        JobState::Paused,
        420,
        1000,
    )]);
    h.run_steps(3);
    assert!(has(&h, "Moving \"a.iso\" · paused"));
}

/// A real copy into Google Drive with the panel hidden: progress, "Done"
/// for a moment, then what Google Drive still uploads.
#[test]
fn copy_into_google_drive_shows_done_then_the_upload() {
    let fx = fixture();
    let (mut h, d) = harness(&fx, Theme::Light, false);
    open_drive(&mut h, &d);
    // Right pane: the fixture's Design folder.
    select(&mut h, Side::Right, &["Brand Guide.pdf"]);
    d.probe.set_upload(
        &d.my.join("Brand Guide.pdf"),
        Some(SyncInfo::new(SyncState::Uploading)),
    );
    // "Done" lingers on the wall clock; keep it until the test lets it go, so a
    // slow machine (CI) can't miss it.
    app(&mut h).transfer_pill.linger = Duration::from_secs(3600);
    start_copy_panel_hidden(&mut h);
    wait_until(&mut h, "the copy", |_| {
        d.my.join("Brand Guide.pdf").exists()
    });
    wait_until(&mut h, "Done", |h| h.query_by_label("Done").is_some());

    // A few seconds later "Done" goes; the cloud upload stays visible.
    app(&mut h).transfer_pill.linger = Duration::ZERO;
    reread(&mut h, &d);
    assert!(!has(&h, "Done"));
    assert!(
        has(&h, "Google Drive is uploading 1 item"),
        "the provider's upload"
    );
    let st = app(&mut h)
        .pane(Side::Left)
        .tab()
        .cloud
        .as_ref()
        .and_then(|c| c.get("Brand Guide.pdf").map(|i| i.state));
    assert_eq!(st, Some(SyncState::Uploading), "badge in the list");

    // The upload fails: red pill; done: nothing.
    d.probe.set_upload(
        &d.my.join("Brand Guide.pdf"),
        Some(SyncInfo {
            state: SyncState::UploadError,
            upload_error: Some("Storage full".into()),
            ..SyncInfo::default()
        }),
    );
    reread(&mut h, &d);
    assert!(has(&h, "Google Drive: 1 upload error"));
    d.probe.set_upload(&d.my.join("Brand Guide.pdf"), None);
    reread(&mut h, &d);
    assert!(
        h.query_by_label_contains("is uploading").is_none()
            && h.query_by_label_contains("upload error").is_none()
    );
}

/// F5 and confirming the dialog (what Enter does), then the user hides the
/// transfers panel the copy opened, before any frame sees the job.
///
/// Not Enter plus a frame: that frame confirms the dialog in its first pass
/// and egui runs a second pass (the dialog closing requests a discard), whose
/// job snapshot may already show the tiny copy finished while the panel is
/// still open. The panel showing it counts as seen: no "Done" pill afterwards
/// (correct for the app, a race for the test: it timed out on CI).
fn start_copy_panel_hidden(h: &mut H) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, Action::CopyToOther(Side::Right, false));
    assert!(
        matches!(app(h).dialog, Some(Dialog::Transfer(_))),
        "the copy dialog"
    );
    app(h).submit_dialog();
    assert!(app(h).show_transfers, "a new job opens the panel");
    app(h).show_transfers = false;
}

/// Wait (without frames) until the transfer manager reports every job done.
fn wait_jobs_done(h: &mut H) {
    let start = std::time::Instant::now();
    loop {
        let jobs = app(h).transfers.snapshot();
        if !jobs.is_empty() && jobs.iter().all(|j| j.state == JobState::Done) {
            return;
        }
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "timed out waiting for the copy job: {jobs:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The CI race made deterministic: the copy is already finished when the app
/// first looks at the jobs. With the panel hidden that look still shows
/// "Done".
#[test]
fn copy_finished_before_the_first_frame_still_shows_done() {
    let fx = fixture();
    let (mut h, d) = harness(&fx, Theme::Light, false);
    open_drive(&mut h, &d);
    select(&mut h, Side::Right, &["Brand Guide.pdf"]);
    app(&mut h).transfer_pill.linger = Duration::from_secs(3600);
    start_copy_panel_hidden(&mut h);
    wait_jobs_done(&mut h);
    assert!(d.my.join("Brand Guide.pdf").exists());
    h.run_steps(2);
    assert!(has(&h, "Done"), "a job that finished unseen shows Done");

    // A job that finished while the panel was open was seen there: hiding
    // the panel afterwards shows no "Done" (why Enter + a frame raced).
    app(&mut h).transfers.clear_finished();
    h.run_steps(2);
    select(&mut h, Side::Right, &["app-icon.svg"]);
    start_copy_panel_hidden(&mut h);
    app(&mut h).show_transfers = true;
    wait_jobs_done(&mut h);
    h.run_steps(1);
    app(&mut h).show_transfers = false;
    h.run_steps(2);
    assert!(!has(&h, "Done"), "seen in the panel: no pill");
}

#[test]
fn errors_stay_until_clicked() {
    let fx = fixture();
    let (mut h, _d) = harness(&fx, Theme::Light, false);
    app(&mut h).transfer_pill.linger = Duration::ZERO;
    let mut j = job(
        7,
        JobKind::Copy,
        "Copying 2 items to /var/www",
        JobState::Running,
        0,
        10,
    );
    app(&mut h).fake_jobs = Some(vec![j.clone()]);
    h.run_steps(2);
    j.state = JobState::Failed("permission denied: /var/www".into());
    app(&mut h).fake_jobs = Some(vec![j]);
    h.run_steps(3);
    std::thread::sleep(Duration::from_millis(30));
    h.run_steps(3);
    assert!(has(&h, "Transfer failed"), "stays after the job ended");
    h.get_by_label("Transfer failed").click();
    h.run_steps(3);
    assert!(app(&mut h).show_transfers);
    app(&mut h).show_transfers = false;
    h.run_steps(3);
    assert!(!has(&h, "Transfer failed"), "seen: gone");
}

// ------------------------------------------------------------------ renders

fn shot(h: &mut H, dir: &std::path::Path, name: &str) {
    save_to(h, dir, name);
}

#[test]
fn render_progress_pill() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("progress");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let (mut h, d) = harness(&fx, theme, true);
        crate::i18n::set_locale(crate::i18n::Locale::De);
        open_drive(&mut h, &d);
        settle(&mut h);
        app(&mut h).show_transfers = false;
        app(&mut h).transfer_pill.linger = Duration::from_secs(3600);
        let running = job(
            1,
            JobKind::Copy,
            "Copying 3 items to /Drive",
            JobState::Running,
            420,
            1000,
        );
        app(&mut h).fake_jobs = Some(vec![running.clone()]);
        h.run_steps(3);
        shot(&mut h, &dir, &format!("pill_running_de_{tname}"));

        let second = job(
            2,
            JobKind::Move,
            "Moving 2 items to /x",
            JobState::Running,
            320,
            1000,
        );
        app(&mut h).fake_jobs = Some(vec![running.clone(), second]);
        h.run_steps(3);
        shot(&mut h, &dir, &format!("pill_two_de_{tname}"));

        let mut done = running.clone();
        done.state = JobState::Done;
        app(&mut h).fake_jobs = Some(vec![done.clone()]);
        h.run_steps(3);
        shot(&mut h, &dir, &format!("pill_done_de_{tname}"));

        let mut failed = job(
            3,
            JobKind::Copy,
            "Copying 2 items to /var/www",
            JobState::Running,
            0,
            10,
        );
        app(&mut h).fake_jobs = Some(vec![done.clone(), failed.clone()]);
        h.run_steps(2);
        failed.state = JobState::Failed("permission denied".into());
        app(&mut h).fake_jobs = Some(vec![done, failed]);
        h.run_steps(3);
        shot(&mut h, &dir, &format!("pill_error_de_{tname}"));

        // No jobs: what Google Drive still uploads.
        app(&mut h).transfer_pill.acknowledge();
        app(&mut h).fake_jobs = Some(vec![]);
        d.probe.set_upload(
            &d.my.join("Angebot.pdf"),
            Some(SyncInfo::new(SyncState::Uploading)),
        );
        d.probe.set_upload(
            &d.my.join("Fotos.zip"),
            Some(SyncInfo {
                upload_waiting: true,
                ..SyncInfo::new(SyncState::Uploading)
            }),
        );
        reread(&mut h, &d);
        shot(&mut h, &dir, &format!("pill_cloud_uploading_de_{tname}"));
        app(&mut h).fake_jobs = None;
    }
    crate::i18n::set_thread_locale(None);
}
