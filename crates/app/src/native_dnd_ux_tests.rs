//! Drag & drop with other apps: files dropped onto a pane or onto a folder
//! in every view (fake winit drops through `native_drag::test_hooks`),
//! the drop dialog (Copy preselected, ⌘ preselects Move, the "Always
//! copy / move" settings skip it; also between panes), an internal
//! drag leaving the window becoming a native session, and what the app does
//! with the native sessions' reports.
//!
//! Behaviour tests step the real app without a GPU; `render_external_drop_*`
//! need one and write PNGs to `$MANTA_RENDER_DIR/nativedrag`.

use super::*;
use crate::app::Action;
use crate::native_drag::{test_hooks, DragEvent, DropOp, PasteboardEntry};
use crate::pane::DragPayload;
use crate::views::ViewMode;
use egui::Modifiers;
use egui_kittest::kittest::Queryable;

fn behaviour(fx: &Fixture) -> H {
    harness_with(fx, WIDE, Theme::Light, false)
}

fn pane_rect(h: &mut H, side: Side) -> egui::Rect {
    app(h)
        .pane_rects
        .iter()
        .find(|(s, _)| *s == side)
        .map(|(_, r)| *r)
        .expect("pane drawn")
}

/// A folder next to the fixture (same volume), with `names` in it: what
/// another app drags in.
pub(super) fn outside(fx: &Fixture, names: &[&str]) -> Vec<PathBuf> {
    let dir = fx.root.parent().unwrap().join("Outside");
    std::fs::create_dir_all(&dir).unwrap();
    names
        .iter()
        .map(|n| {
            let p = dir.join(n);
            std::fs::write(&p, n.as_bytes()).unwrap();
            p
        })
        .collect()
}

/// Files from another app hover at `pos` (one frame).
fn hover_files(h: &mut H, pos: egui::Pos2, files: &[PathBuf]) {
    h.hover_at(pos);
    h.input_mut().hovered_files = files.iter().map(|f| test_hooks::hovered(f)).collect();
    h.step();
}

/// Hover, then drop `files` at `pos` with `mods` held; returns the
/// actions the drop produced (not yet handled).
pub(super) fn drop_files(h: &mut H, pos: egui::Pos2, files: &[PathBuf], mods: Modifiers) {
    h.event(egui::Event::ModifiersChanged(mods));
    hover_files(h, pos, files);
    h.step();
    h.input_mut().hovered_files.clear();
    h.input_mut().dropped_files = files.iter().map(|f| test_hooks::dropped(f)).collect();
    h.step();
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
}

/// An empty spot of the right pane (Design: three rows at the top).
pub(super) fn empty_spot(h: &mut H, side: Side) -> egui::Pos2 {
    let r = pane_rect(h, side);
    egui::pos2(r.center().x, r.bottom() - 120.0)
}

/// The drop dialog that is open: (Copy / Move offered, Move preselected).
fn drop_dialog(h: &mut H) -> Option<(bool, bool)> {
    match &app(h).dialog {
        Some(Dialog::Transfer(f)) => Some((f.choose, f.mv)),
        _ => None,
    }
}

#[test]
fn drop_from_another_app_asks_with_copy_preselected_and_enter_copies() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let files = outside(&fx, &["copied.txt"]);
    let design = fx.root.join("Design");

    // Default, also on the same volume: the dialog with Copy preselected.
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files, Modifiers::NONE);
    assert_eq!(
        drop_dialog(&mut h),
        Some((true, false)),
        "Copy / Move offered, Copy preselected"
    );
    h.run_steps(2);
    assert!(h.query_by_label("Move").is_some(), "Move can be chosen");
    assert_eq!(
        active_side(&mut h),
        Side::Right,
        "the drop target becomes active"
    );
    // ↩ confirms the preselection.
    h.key_press(egui::Key::Enter);
    wait_until(&mut h, "the copy", |_| design.join("copied.txt").exists());
    assert!(files[0].exists(), "copied: the source stays");
    assert!(app(&mut h).dialog.is_none());
    assert!(
        !app(&mut h).jobs.is_empty(),
        "it ran on the transfer manager"
    );
    wait_until(&mut h, "the pane to refresh", |h| {
        h.state()
            .pane(Side::Right)
            .tab()
            .entries
            .iter()
            .any(|e| e.name == "copied.txt")
    });
}

#[test]
fn choosing_move_in_the_drop_dialog_moves() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let files = outside(&fx, &["moved.txt"]);
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files, Modifiers::NONE);
    assert_eq!(drop_dialog(&mut h), Some((true, false)));
    h.run_steps(2);
    h.get_by_label("Move").click();
    h.run_steps(2);
    assert_eq!(drop_dialog(&mut h), Some((true, true)), "Move chosen");
    h.key_press(egui::Key::Enter);
    let dst = fx.root.join("Design/moved.txt");
    wait_until(&mut h, "the move", |_| dst.exists() && !files[0].exists());
}

#[test]
fn command_drop_preselects_move_and_option_copy() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let files = outside(&fx, &["a.txt"]);
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files, Modifiers::COMMAND);
    assert_eq!(
        drop_dialog(&mut h),
        Some((true, true)),
        "⌘: Move preselected"
    );
    app(&mut h).dialog = None;
    drop_files(&mut h, pos, &files, Modifiers::ALT);
    assert_eq!(
        drop_dialog(&mut h),
        Some((true, false)),
        "⌥: Copy preselected"
    );
}

#[test]
fn always_copy_setting_skips_the_dialog_and_command_still_moves() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).prefs.drop_action = crate::prefs::DropAction::Copy;
    let files = outside(&fx, &["copied.txt", "forced.txt"]);
    let design = fx.root.join("Design");
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files[..1], Modifiers::NONE);
    assert!(app(&mut h).dialog.is_none(), "no dialog with “Always copy”");
    wait_until(&mut h, "the copy", |_| design.join("copied.txt").exists());
    assert!(files[0].exists(), "copied: the source stays");

    settle(&mut h);
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files[1..], Modifiers::COMMAND);
    assert!(app(&mut h).dialog.is_none());
    wait_until(&mut h, "the move", |_| {
        design.join("forced.txt").exists() && !files[1].exists()
    });
}

#[test]
fn always_move_setting_moves_without_asking_and_option_copies() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).prefs.drop_action = crate::prefs::DropAction::Move;
    let files = outside(&fx, &["moved.txt", "kept.txt"]);
    let design = fx.root.join("Design");
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files[..1], Modifiers::NONE);
    assert!(app(&mut h).dialog.is_none());
    wait_until(&mut h, "the move", |_| {
        design.join("moved.txt").exists() && !files[0].exists()
    });

    settle(&mut h);
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files[1..], Modifiers::ALT);
    wait_until(&mut h, "the copy", |_| design.join("kept.txt").exists());
    assert!(files[1].exists(), "⌥ copies");
}

#[test]
fn dropping_into_the_same_folder_is_a_no_op() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let dst = fx.root.join("Design/notes-here.txt");
    std::fs::write(&dst, b"x").unwrap();
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, std::slice::from_ref(&dst), Modifiers::NONE);
    settle(&mut h);
    assert!(dst.exists());
    assert!(app(&mut h).dialog.is_none(), "nothing to ask");
    assert!(
        app(&mut h)
            .toasts
            .titles()
            .iter()
            .any(|t| t == t!("status.same_folder")),
        "{:?}",
        app(&mut h).toasts.titles()
    );
}

fn active_side(h: &mut H) -> Side {
    app(h).active_pane()
}

/// Every view registers its folders: a drop onto one goes into it, a drop
/// next to them into the pane's folder.
#[test]
fn drop_onto_a_folder_goes_into_that_folder_in_every_view() {
    for (i, mode) in [
        ViewMode::List,
        ViewMode::Icons,
        ViewMode::Columns,
        ViewMode::Gallery,
    ]
    .into_iter()
    .enumerate()
    {
        let fx = fixture();
        let mut h = behaviour(&fx);
        app(&mut h).prefs.drop_action = crate::prefs::DropAction::Copy;
        app(&mut h).pane_mut(Side::Left).view = mode;
        settle(&mut h);
        let name = format!("into-{i}.txt");
        let files = outside(&fx, &[name.as_str()]);
        // Any hover makes the views register their folders.
        let r = pane_rect(&mut h, Side::Left);
        hover_files(&mut h, r.center(), &files);
        h.step();
        let ctx = h.ctx.clone();
        let targets = crate::pane::folder_targets(&ctx, Side::Left);
        let photos = VfsPath::new(&fx.root.join("photos").to_string_lossy());
        let Some((rect, _)) = targets.iter().find(|(_, p)| *p == photos).cloned() else {
            panic!("{mode:?}: photos not registered as a drop target: {targets:?}")
        };
        assert!(r.contains(rect.center()), "{mode:?}: inside the pane");
        drop_files(&mut h, rect.center(), &files, Modifiers::ALT);
        let dst = fx.root.join("photos").join(&name);
        wait_until(&mut h, &format!("{mode:?}: copy into photos"), |_| {
            dst.exists()
        });
        assert!(
            !fx.root.join(&name).exists(),
            "{mode:?}: not into the pane's folder"
        );
    }
}

#[test]
fn read_only_panes_refuse_drops_from_other_apps() {
    let fx = fixture();
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(fx.root.join("bundle.zip")).unwrap());
        z.start_file("inside.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut z, b"x").unwrap();
        z.finish().unwrap();
    }
    let mut h = behaviour(&fx);
    let files = outside(&fx, &["nope.txt"]);
    app(&mut h).activate(Side::Left);
    select(&mut h, Side::Left, &["bundle.zip"]);
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, Action::OpenSelected(Side::Left));
    wait_until(&mut h, "the archive to open", |h| {
        let t = h.state().pane(Side::Left).tab();
        t.loc.archive.is_some() && t.opening.is_none() && !t.loading
    });
    let pos = empty_spot(&mut h, Side::Left);
    drop_files(&mut h, pos, &files, Modifiers::NONE);
    settle(&mut h);
    assert!(files[0].exists(), "nothing moved");
    let titles = app(&mut h).toasts.titles();
    assert!(
        titles.contains(&t!("archive.cant_paste", name = "bundle.zip")),
        "{titles:?}"
    );
    // A pane that is gone by the time the action runs: ignored.
    let dir = VfsPath::new(&fx.root.to_string_lossy());
    app(&mut h).handle_action(
        &ctx,
        Action::DropExternal {
            to: PaneId(42),
            dir,
            paths: files,
            mv: false,
            from_self: false,
        },
    );
    settle(&mut h);
}

/// Between panes the same rules: copy by default, ⌥ copies, ⌘ moves.
#[test]
fn internal_drops_copy_unless_command_is_held() {
    for (mods, want_move) in [
        (Modifiers::NONE, false),
        (Modifiers::ALT, false),
        (Modifiers::COMMAND, true),
    ] {
        let fx = fixture();
        let mut h = behaviour(&fx);
        app(&mut h).activate(Side::Left);
        let fs = app(&mut h).pane(Side::Left).tab().loc.fs.clone();
        let paths = vec![VfsPath::new(&fx.root.join("notes.txt").to_string_lossy())];
        let pos = empty_spot(&mut h, Side::Right);
        let ctx = h.ctx.clone();
        h.event(egui::Event::ModifiersChanged(mods));
        h.hover_at(pos);
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: mods,
        });
        h.step();
        egui::DragAndDrop::set_payload(
            &ctx,
            DragPayload::files(Side::Left, fs.clone(), paths.clone()),
        );
        h.step();
        egui::DragAndDrop::set_payload(&ctx, DragPayload::files(Side::Left, fs, paths));
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: mods,
        });
        h.run_steps(3);
        h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        let Some(Dialog::Transfer(f)) = &app(&mut h).dialog else {
            panic!("{mods:?}: drop opens the copy dialog")
        };
        assert_eq!(f.mv, want_move, "{mods:?}");
        assert!(f.choose, "{mods:?}: Copy / Move offered");
    }
}

/// An internal drag that leaves the window becomes a native session with
/// the dragged files; inside the window nothing happens.
#[test]
fn dragging_out_of_the_window_starts_a_native_session() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    test_hooks::take_begun();
    app(&mut h).activate(Side::Left);
    let fs = app(&mut h).pane(Side::Left).tab().loc.fs.clone();
    let paths = vec![VfsPath::new(&fx.root.join("notes.txt").to_string_lossy())];
    let ctx = h.ctx.clone();
    let start = pane_rect(&mut h, Side::Left).center();
    h.hover_at(start);
    h.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    egui::DragAndDrop::set_payload(&ctx, DragPayload::files(Side::Left, fs, paths));
    // Across the panes: still an internal drag.
    for x in [start.x + 100.0, start.x + 500.0] {
        h.hover_at(egui::pos2(x, start.y));
        h.step();
    }
    assert!(
        test_hooks::take_begun().is_empty(),
        "inside the window: no native drag"
    );
    assert!(egui::DragAndDrop::has_any_payload(&ctx));
    // Past the right edge.
    h.hover_at(egui::pos2(WIDE[0] + 40.0, start.y));
    h.step();
    // The press landed on a row, so egui started a real row drag: its
    // (selected) item is what flies out.
    let selected: Vec<PasteboardEntry> = app(&mut h)
        .pane(Side::Left)
        .tab()
        .selected_entries()
        .into_iter()
        .map(|e| PasteboardEntry::FileUrl(PathBuf::from(e.path.as_str())))
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(test_hooks::take_begun(), vec![selected]);
    assert!(
        !egui::DragAndDrop::has_any_payload(&ctx),
        "AppKit owns the drag now"
    );
    h.event(egui::Event::PointerButton {
        pos: egui::pos2(WIDE[0] + 40.0, start.y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    assert!(app(&mut h).dialog.is_none(), "no drop inside Manta");
}

#[test]
fn native_session_reports_refresh_and_toast() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    // The destination moved notes.txt away: the pane lists it no more.
    let notes = fx.root.join("notes.txt");
    assert!(app(&mut h)
        .pane(Side::Left)
        .tab()
        .entries
        .iter()
        .any(|e| e.name == "notes.txt"));
    std::fs::remove_file(&notes).unwrap();
    app(&mut h).native_drag_event(DragEvent::Ended {
        op: DropOp::Move,
        local: vec![notes.clone()],
    });
    settle(&mut h);
    assert!(!app(&mut h)
        .pane(Side::Left)
        .tab()
        .entries
        .iter()
        .any(|e| e.name == "notes.txt"));

    // A copy changes nothing at the source.
    app(&mut h).native_drag_event(DragEvent::Ended {
        op: DropOp::Copy,
        local: vec![notes.clone()],
    });
    // Promises: progress, success, failure.
    let dest = fx.root.join("from-server.txt");
    std::fs::write(&dest, b"x").unwrap();
    app(&mut h).native_drag_event(DragEvent::PromiseWriting {
        name: "from-server.txt".into(),
        backend: "me@host (SFTP)".into(),
        dest: dest.clone(),
        size: Some(1),
    });
    app(&mut h).native_drag_event(DragEvent::PromiseDone {
        name: "from-server.txt".into(),
        dest: dest.clone(),
        result: Ok(()),
    });
    settle(&mut h);
    assert!(
        app(&mut h)
            .pane(Side::Left)
            .tab()
            .entries
            .iter()
            .any(|e| e.name == "from-server.txt"),
        "the folder it landed in is refreshed"
    );
    app(&mut h).native_drag_event(DragEvent::PromiseDone {
        name: "big.iso".into(),
        dest: fx.root.join("big.iso"),
        result: Err("connection reset".into()),
    });
    app(&mut h).native_drag_event(DragEvent::Failed("no window".into()));
    let titles = app(&mut h).toasts.titles();
    for want in [
        t!("pane.drag.downloaded", name = "from-server.txt"),
        t!("pane.drag.download_failed", name = "big.iso"),
        t!("pane.drag.failed").to_string(),
    ] {
        assert!(titles.contains(&want), "{want:?} in {titles:?}");
    }
    assert!(
        titles
            .iter()
            .any(|t| t.contains("from-server.txt") && t.contains("me@host")),
        "{titles:?}"
    );
}

// ------------------------------------------------------------------ renders

fn render_external_drop(theme: Theme, tag: &str) {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, theme);
    crate::i18n::set_locale(crate::i18n::Locale::De);
    h.run_steps(3);
    let dir = render_dir("nativedrag");
    let files = outside(&fx, &["Rechnung.pdf", "Foto.jpg"]);
    // Over the empty part of the right pane: the whole pane is the target.
    let pos = empty_spot(&mut h, Side::Right);
    hover_files(&mut h, pos, &files);
    h.run_steps(2);
    save_to(&mut h, &dir, &format!("external-hover-pane-{tag}"));
    // Over a folder row in the left pane: that folder is the target.
    let r = pane_rect(&mut h, Side::Left);
    hover_files(&mut h, r.center(), &files);
    let ctx = h.ctx.clone();
    let photos = VfsPath::new(&fx.root.join("photos").to_string_lossy());
    if let Some((rect, _)) = crate::pane::folder_targets(&ctx, Side::Left)
        .into_iter()
        .find(|(_, p)| *p == photos)
    {
        hover_files(&mut h, rect.center(), &files);
        h.run_steps(2);
        save_to(&mut h, &dir, &format!("external-hover-folder-{tag}"));
        // ⌘ held: move.
        h.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
        hover_files(&mut h, rect.center(), &files);
        h.run_steps(2);
        save_to(&mut h, &dir, &format!("external-hover-folder-cmd-{tag}"));
        h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    }
    // Icons view.
    app(&mut h).pane_mut(Side::Left).view = ViewMode::Icons;
    settle(&mut h);
    hover_files(&mut h, r.center(), &files);
    if let Some((rect, _)) = crate::pane::folder_targets(&ctx, Side::Left)
        .into_iter()
        .find(|(_, p)| *p == photos)
    {
        hover_files(&mut h, rect.center(), &files);
        h.run_steps(2);
        save_to(&mut h, &dir, &format!("external-hover-icons-{tag}"));
    }
    h.input_mut().hovered_files.clear();
    // The drop dialog: Copy preselected, Move one click away.
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files, Modifiers::NONE);
    h.run_steps(3);
    save_to(&mut h, &render_dir("ux022"), &format!("drop-dialog-{tag}"));
    app(&mut h).dialog = None;
    crate::i18n::set_locale(crate::i18n::Locale::En);
}

#[test]
fn render_external_drop_light_de() {
    require_gpu!();
    render_external_drop(Theme::Light, "light-de");
}

#[test]
fn render_external_drop_dark_de() {
    require_gpu!();
    render_external_drop(Theme::Dark, "dark-de");
}

// ------------------------------------------------------------------ conflicts

/// Settings' "If an item already exists" is preselected in the drop dialog.
#[test]
fn conflict_setting_is_preselected_in_the_drop_dialog() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).prefs.conflict = crate::prefs::ConflictPref::OverwriteIfNewer;
    let files = outside(&fx, &["c.txt"]);
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files, Modifiers::NONE);
    let Some(Dialog::Transfer(f)) = &app(&mut h).dialog else {
        panic!("the drop dialog")
    };
    assert_eq!(f.conflict, manta_ops::ConflictPolicy::OverwriteIfNewer);
}

/// "Always copy" + "Replace if newer": applied without asking.
#[test]
fn always_copy_applies_the_conflict_setting_without_asking() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).prefs.drop_action = crate::prefs::DropAction::Copy;
    app(&mut h).prefs.conflict = crate::prefs::ConflictPref::OverwriteIfNewer;
    let design = fx.root.join("Design");
    // An older, different copy is already there.
    let old = design.join("report.txt");
    std::fs::write(&old, b"old").unwrap();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(past)
        .unwrap();
    let files = outside(&fx, &["report.txt"]);
    let pos = empty_spot(&mut h, Side::Right);
    drop_files(&mut h, pos, &files, Modifiers::NONE);
    assert!(app(&mut h).dialog.is_none(), "no dialog");
    wait_until(&mut h, "the newer file to replace the old one", |_| {
        std::fs::read(&old).is_ok_and(|b| b == b"report.txt")
    });
    assert!(files[0].exists(), "copied: the source stays");
}
