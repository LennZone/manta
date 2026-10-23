//! Space → Quick Look in the app, against a recording backend (no AppKit):
//! the setting, follow-the-cursor, Esc, folders, remote fallback, and a
//! render of the "Space opens" setting.

use eframe::egui::{self, Theme};

use super::{app, harness, harness_with, save, select, settle, wgpu_available, Fixture, H, WIDE};
use crate::app::Side;
use crate::prefs::SpaceAction;
use crate::quicklook::tests::Recorder;
use crate::quicklook::QuickLook;

fn with_recorder(h: &mut H, space: SpaceAction) -> Recorder {
    let rec = Recorder::default();
    let a = app(h);
    a.quicklook = QuickLook::new(Some(Box::new(rec.clone())));
    a.prefs.space_action = space;
    rec
}

fn calls(rec: &Recorder) -> Vec<String> {
    rec.calls.borrow().clone()
}

fn space(h: &mut H) {
    h.key_press(egui::Key::Space);
    h.run_steps(2);
}

fn cursor_name(h: &mut H) -> String {
    app(h)
        .pane(Side::Left)
        .tab()
        .cursor_entry()
        .unwrap()
        .name
        .clone()
}

fn fx() -> Fixture {
    super::fixture()
}

#[test]
fn space_with_builtin_setting_opens_egui_preview() {
    let fx = fx();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let rec = with_recorder(&mut h, SpaceAction::BuiltIn);
    select(&mut h, Side::Left, &["notes.txt"]);
    space(&mut h);
    assert!(app(&mut h).preview.is_some(), "built-in preview opened");
    assert!(
        calls(&rec).is_empty(),
        "Quick Look untouched: {:?}",
        calls(&rec)
    );
    space(&mut h);
    assert!(app(&mut h).preview.is_none(), "Space toggles it closed");
}

#[test]
fn space_opens_quick_look_follows_cursor_and_closes() {
    let fx = fx();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let rec = with_recorder(&mut h, SpaceAction::QuickLook);
    select(&mut h, Side::Left, &["notes.txt"]);
    space(&mut h);
    assert!(
        app(&mut h).preview.is_none(),
        "no egui preview with Quick Look"
    );
    assert_eq!(calls(&rec), ["show notes.txt @0"]);
    assert!(app(&mut h).quicklook.is_open());

    // ↑ moves the cursor; the panel follows.
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    let now = cursor_name(&mut h);
    assert_ne!(now, "notes.txt");
    assert_eq!(calls(&rec).last().unwrap(), &format!("show {now} @0"));

    // Space closes.
    space(&mut h);
    assert_eq!(calls(&rec).last().unwrap(), "close");
    assert!(!app(&mut h).quicklook.is_open());

    // Esc closes too.
    space(&mut h);
    assert!(app(&mut h).quicklook.is_open());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!app(&mut h).quicklook.is_open());
    assert_eq!(calls(&rec).last().unwrap(), "close");
}

#[test]
fn quick_look_multi_selection_and_folders() {
    let fx = fx();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let rec = with_recorder(&mut h, SpaceAction::QuickLook);
    // Two files: both shown in list order, the cursor's one current.
    select(&mut h, Side::Left, &["report-q3.pdf", "notes.txt"]);
    space(&mut h);
    let last = calls(&rec).last().unwrap().clone();
    assert!(
        last == "show notes.txt,report-q3.pdf @0" || last == "show report-q3.pdf,notes.txt @1",
        "{last}"
    );
    space(&mut h);
    // A folder previews like in Finder.
    select(&mut h, Side::Left, &["src"]);
    space(&mut h);
    assert_eq!(calls(&rec).last().unwrap(), "show src @0");
    // Navigating into it closes the panel.
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(!app(&mut h).quicklook.is_open());
    assert_eq!(calls(&rec).last().unwrap(), "close");
}

#[test]
fn panel_closed_by_user_then_space_reopens() {
    let fx = fx();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let rec = with_recorder(&mut h, SpaceAction::QuickLook);
    select(&mut h, Side::Left, &["notes.txt"]);
    space(&mut h);
    *rec.visible.borrow_mut() = false; // the panel's own close button
    h.run_steps(2);
    space(&mut h);
    assert_eq!(calls(&rec), ["show notes.txt @0", "show notes.txt @0"]);
}

#[test]
fn without_backend_space_falls_back_to_builtin() {
    let fx = fx();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    app(&mut h).quicklook = QuickLook::new(None);
    app(&mut h).prefs.space_action = SpaceAction::QuickLook;
    select(&mut h, Side::Left, &["notes.txt"]);
    space(&mut h);
    assert!(app(&mut h).preview.is_some());
}

/// Settings → General with the "Space opens" row, light and dark.
#[test]
fn render_space_setting() {
    if !wgpu_available() {
        return;
    }
    let fx = fx();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, WIDE, theme);
        let a = app(&mut h);
        a.settings.show(Some(crate::settings::SettingsTab::General));
        settle(&mut h);
        save(&mut h, &format!("quicklook_settings_general_{tname}"));
    }
}
