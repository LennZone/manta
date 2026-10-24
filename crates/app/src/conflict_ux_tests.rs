//! The conflict question of `ConflictPolicy::Ask`: a job that stops at an
//! existing item opens the dialog, the answer goes to the transfer
//! manager, "apply to all" suppresses further questions, ↩ keeps both,
//! Esc cancels the job (keeping what is done), several asking jobs are
//! asked one after the other.
//!
//! The jobs run on in-memory backends. `render_conflict_dialog_*` write
//! German light/dark PNGs (file and folder case) to
//! `$MANTA_RENDER_DIR/conflict`.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use egui_kittest::kittest::Queryable;
use manta_core::SharedVfs;
use manta_ops::memfs::MemFs;
use manta_ops::{ConflictChoice, ConflictItem, ConflictPolicy, ConflictQuestion, Job, JobId};

use super::*;

fn t(secs: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_790_000_000 + secs, 0).unwrap()
}

fn behaviour(fx: &Fixture) -> H {
    let mut h = harness_with(fx, WIDE, Theme::Light, false);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    h.run_steps(2);
    h
}

/// Copy `names` from /s to /d where each already exists (older).
fn conflicting(names: &[&str]) -> (MemFs, MemFs) {
    let a = MemFs::new();
    let b = MemFs::new();
    for n in names {
        a.put(&format!("/s/{n}"), b"new", t(100));
        b.put(&format!("/d/{n}"), b"old", t(0));
    }
    (a, b)
}

fn enqueue(h: &mut H, src: &MemFs, names: &[&str], dst: &MemFs) -> JobId {
    let (sf, df): (SharedVfs, SharedVfs) = (Arc::new(src.clone()), Arc::new(dst.clone()));
    app(h).transfers.enqueue(Job {
        kind: JobKind::Copy,
        src_fs: sf,
        sources: names
            .iter()
            .map(|n| VfsPath::new(&format!("/s/{n}")))
            .collect(),
        dst_fs: Some(df),
        dst_dir: Some(VfsPath::new("/d")),
        conflict: ConflictPolicy::Ask,
    })
}

/// The open conflict dialog's (job, file name).
fn open_question(h: &mut H) -> Option<(JobId, String)> {
    match &app(h).dialog {
        Some(Dialog::Conflict(f)) => Some((f.job, f.question.dst.path.file_name().to_string())),
        _ => None,
    }
}

/// Step until the dialog is open (or fail).
fn wait_question(h: &mut H) -> (JobId, String) {
    let start = Instant::now();
    loop {
        h.step();
        if let Some(q) = open_question(h) {
            h.run_steps(2);
            return q;
        }
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "no conflict dialog"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Step until the job finished; asserts no question comes up meanwhile
/// when `quiet`.
fn wait_finished(h: &mut H, id: JobId, quiet: bool) -> JobStatus {
    let start = Instant::now();
    loop {
        h.step();
        if quiet {
            assert!(
                open_question(h).is_none(),
                "asked again: {:?}",
                open_question(h)
            );
        }
        if let Some(j) = app(h).transfers.snapshot().into_iter().find(|j| j.id == id) {
            if j.state.is_finished() {
                h.run_steps(2);
                return j;
            }
        }
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "job didn't finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_dialog_asks_and_replace_is_applied() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let (a, b) = conflicting(&["report.txt"]);
    let id = enqueue(&mut h, &a, &["report.txt"], &b);
    assert_eq!(wait_question(&mut h), (id, "report.txt".to_string()));
    // Both items, the newer one marked; the four answers and Cancel.
    for label in [
        "Being copied",
        "Already there",
        "Newer",
        "Replace",
        "Replace if newer",
        "Keep both",
        "Skip",
        "Cancel",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label} shown");
    }
    h.get_by_label("Replace").click();
    let st = wait_finished(&mut h, id, true);
    assert_eq!(st.state, JobState::Done);
    assert_eq!(b.read_all("/d/report.txt").unwrap(), b"new");
    assert!(app(&mut h).dialog.is_none());
}

#[test]
fn apply_to_all_suppresses_further_questions() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let names = ["a.txt", "b.txt", "c.txt"];
    let (a, b) = conflicting(&names);
    let id = enqueue(&mut h, &a, &names, &b);
    wait_question(&mut h);
    h.get_by_label("Apply to all further conflicts of this job")
        .click();
    h.run_steps(2);
    h.get_by_label("Skip").click();
    let st = wait_finished(&mut h, id, true);
    assert_eq!(st.state, JobState::Done);
    for n in names {
        assert_eq!(b.read_all(&format!("/d/{n}")).unwrap(), b"old");
    }
}

#[test]
fn enter_keeps_both_and_escape_cancels_the_job() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let (a, b) = conflicting(&["x.txt"]);
    let id = enqueue(&mut h, &a, &["x.txt"], &b);
    wait_question(&mut h);
    h.key_press(egui::Key::Enter);
    assert_eq!(wait_finished(&mut h, id, true).state, JobState::Done);
    assert_eq!(b.read_all("/d/x.txt").unwrap(), b"old");
    assert_eq!(b.read_all("/d/x 2.txt").unwrap(), b"new");

    // Esc: the job stops; what was copied before stays.
    let (a, b) = conflicting(&["y.txt"]);
    a.put("/s/first.txt", b"1", t(0));
    let id = enqueue(&mut h, &a, &["first.txt", "y.txt"], &b);
    wait_question(&mut h);
    h.key_press(egui::Key::Escape);
    assert_eq!(wait_finished(&mut h, id, true).state, JobState::Cancelled);
    assert_eq!(b.read_all("/d/first.txt").unwrap(), b"1");
    assert_eq!(b.read_all("/d/y.txt").unwrap(), b"old");
}

#[test]
fn several_jobs_are_asked_one_after_the_other() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let (a1, b1) = conflicting(&["one.txt"]);
    let (a2, b2) = conflicting(&["two.txt"]);
    let first = enqueue(&mut h, &a1, &["one.txt"], &b1);
    let second = enqueue(&mut h, &a2, &["two.txt"], &b2);
    // Both wait; the older one is asked first.
    let start = Instant::now();
    while app(&mut h)
        .transfers
        .snapshot()
        .iter()
        .filter(|j| j.state.question().is_some())
        .count()
        < 2
    {
        assert!(start.elapsed() < crate::test_support::wait());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(wait_question(&mut h), (first, "one.txt".to_string()));
    h.get_by_label("Skip").click();
    h.run_steps(2);
    let (job, name) = wait_question(&mut h);
    assert_eq!((job, name.as_str()), (second, "two.txt"));
    h.get_by_label("Replace").click();
    assert_eq!(wait_finished(&mut h, second, true).state, JobState::Done);
    assert_eq!(b1.read_all("/d/one.txt").unwrap(), b"old");
    assert_eq!(b2.read_all("/d/two.txt").unwrap(), b"new");
}

#[test]
fn stopping_the_job_in_the_panel_closes_its_question() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let (a, b) = conflicting(&["z.txt"]);
    let id = enqueue(&mut h, &a, &["z.txt"], &b);
    wait_question(&mut h);
    app(&mut h).transfers.cancel(id);
    assert_eq!(wait_finished(&mut h, id, false).state, JobState::Cancelled);
    h.run_steps(2);
    assert!(app(&mut h).dialog.is_none());
}

#[test]
fn new_settings_ask_by_default() {
    let fx = fixture();
    let h = behaviour(&fx);
    assert_eq!(h.state().prefs.conflict, crate::prefs::ConflictPref::Ask);
    assert_eq!(
        crate::prefs::ConflictPref::ALL[0],
        crate::prefs::ConflictPref::Ask
    );
}

// ------------------------------------------------------------------ renders

pub(super) fn waiting_job(id: u64, q: ConflictQuestion, title: &str) -> JobStatus {
    JobStatus {
        id,
        kind: JobKind::Copy,
        title: title.into(),
        state: JobState::WaitingForDecision(Box::new(q)),
        current_file: None,
        bytes_done: 4_200_000,
        bytes_total: 12_600_000,
        files_done: 3,
        files_total: 9,
        speed: 0.0,
        errors: Vec::new(),
        resume: Default::default(),
    }
}

pub(super) fn file_question() -> ConflictQuestion {
    ConflictQuestion {
        src: ConflictItem {
            path: VfsPath::new("/Users/anna/Projects/Design/Brand Guide.pdf"),
            is_dir: false,
            size: Some(6_200_000),
            modified: Some(t(3 * 86_400)),
        },
        dst: ConflictItem {
            path: VfsPath::new("/Volumes/Backup/Design/Brand Guide.pdf"),
            is_dir: false,
            size: Some(5_870_000),
            modified: Some(t(0)),
        },
        choices: vec![
            ConflictChoice::Replace,
            ConflictChoice::ReplaceIfNewer,
            ConflictChoice::KeepBoth,
            ConflictChoice::Skip,
        ],
    }
}

fn folder_question() -> ConflictQuestion {
    ConflictQuestion {
        src: ConflictItem {
            path: VfsPath::new("/Users/anna/Projects/photos"),
            is_dir: true,
            size: None,
            modified: Some(t(0)),
        },
        dst: ConflictItem {
            path: VfsPath::new("/Volumes/Backup/photos"),
            is_dir: true,
            size: None,
            modified: Some(t(5 * 86_400)),
        },
        choices: vec![
            ConflictChoice::Merge,
            ConflictChoice::KeepBoth,
            ConflictChoice::Skip,
        ],
    }
}

fn render_conflict(theme: Theme, tag: &str) {
    let fx = fixture();
    let mut h = harness_de(&fx, WIDE, theme);
    let dir = render_dir("conflict");
    app(&mut h).fake_jobs = Some(vec![waiting_job(
        1,
        file_question(),
        "Copying 9 items to /Volumes/Backup/Design",
    )]);
    h.run_steps(3);
    save_to(&mut h, &dir, &format!("conflict-file-{tag}"));
    // "Apply to all" ticked.
    if let Some(Dialog::Conflict(f)) = &mut app(&mut h).dialog {
        f.all = true;
    }
    save_to(&mut h, &dir, &format!("conflict-file-all-{tag}"));
    app(&mut h).dialog = None;
    app(&mut h).fake_jobs = Some(vec![waiting_job(
        2,
        folder_question(),
        "Copying \"photos\" to /Volumes/Backup",
    )]);
    h.run_steps(3);
    save_to(&mut h, &dir, &format!("conflict-folder-{tag}"));
    // The transfers panel while the job waits (question answered, so no
    // dialog in front).
    app(&mut h).dialog = None;
    app(&mut h).fake_jobs = Some(vec![waiting_job(
        3,
        file_question(),
        "Copying 9 items to /Volumes/Backup/Design",
    )]);
    app(&mut h).conflicts.answered = Some((3, file_question()));
    app(&mut h).show_transfers = true;
    save_to(&mut h, &dir, &format!("conflict-transfers-{tag}"));
}

#[test]
fn render_conflict_dialog_light_de() {
    require_gpu!();
    render_conflict(Theme::Light, "light-de");
}

#[test]
fn render_conflict_dialog_dark_de() {
    require_gpu!();
    render_conflict(Theme::Dark, "dark-de");
}
