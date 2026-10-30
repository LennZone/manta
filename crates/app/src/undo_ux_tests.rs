//! Undo and Redo like Finder: ⌘Z puts items moved to the Trash back,
//! restores the old name after a rename and removes a new folder or file;
//! ⇧⌘Z does it again. A focused text field keeps ⌘Z for its text.

use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, Theme};
use manta_core::{Vfs, VfsPath};

use super::trash_ux_tests::open_server;
use super::{app, fixture, harness, select, Fixture, H, WIDE};
use crate::app::Side;
use crate::dialogs::Dialog;

fn fake_trash(fx: &Fixture) -> std::path::PathBuf {
    fx.root.parent().unwrap().join("Trash")
}

fn press(h: &mut H, mods: Modifiers, k: Key) {
    h.key_press_modifiers(mods, k);
    h.run_steps(2);
}

fn undo(h: &mut H) {
    press(h, Modifiers::COMMAND, Key::Z);
}

fn redo(h: &mut H) {
    press(h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
}

/// Step until `f` holds (worker threads finish in the background).
fn wait_for(h: &mut H, what: &str, mut f: impl FnMut(&mut H) -> bool) {
    let start = Instant::now();
    while !f(h) {
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "timed out waiting for {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Wait until no Undo or Redo step runs any more.
fn wait_idle(h: &mut H) {
    wait_for(h, "the undo step", |h| !app(h).history.busy);
}

fn toast(h: &mut H, text: &str) -> bool {
    app(h).toasts.titles().iter().any(|t| t == text)
}

/// Name the item in the open name dialog and confirm.
fn enter_name(h: &mut H, name: &str) {
    let Some(Dialog::Name(f)) = &mut app(h).dialog else {
        panic!("the name dialog is open");
    };
    f.name = name.into();
    h.key_press(Key::Enter);
    h.run_steps(2);
}

#[test]
fn cmd_z_puts_trashed_items_back_and_shift_cmd_z_trashes_them_again() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    let trash = fake_trash(&fx);
    select(&mut h, Side::Left, &["README.md", "src"]);
    press(&mut h, Modifiers::COMMAND, Key::Backspace);
    wait_for(&mut h, "items in the Trash", |_| {
        trash.join("README.md").exists() && trash.join("src").exists()
    });
    wait_for(&mut h, "Undo enabled", |h| app(h).history.can_undo());

    undo(&mut h);
    wait_idle(&mut h);
    assert!(fx.root.join("README.md").is_file() && fx.root.join("src/main.rs").is_file());
    assert!(!trash.join("README.md").exists() && !trash.join("src").exists());
    assert!(toast(&mut h, "Put back 2 items"));

    redo(&mut h);
    wait_idle(&mut h);
    assert!(trash.join("README.md").exists() && trash.join("src").exists());
    assert!(!fx.root.join("README.md").exists());

    undo(&mut h);
    wait_idle(&mut h);
    assert!(fx.root.join("README.md").is_file() && fx.root.join("src").is_dir());
    // Nothing left: says so instead of doing anything.
    undo(&mut h);
    wait_idle(&mut h);
    assert!(toast(&mut h, "Nothing to undo"));
}

/// The original place is taken again: nothing is replaced, the step stays
/// and works once the place is free.
#[test]
fn put_back_never_replaces_an_item_and_can_be_tried_again() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    let trash = fake_trash(&fx);
    select(&mut h, Side::Left, &["README.md"]);
    press(&mut h, Modifiers::COMMAND, Key::Backspace);
    wait_for(&mut h, "README.md in the Trash", |h| {
        trash.join("README.md").exists() && app(h).history.can_undo()
    });
    std::fs::write(fx.root.join("README.md"), "new").unwrap();

    undo(&mut h);
    wait_idle(&mut h);
    assert!(toast(&mut h, "Couldn't put back “README.md”"));
    assert_eq!(
        std::fs::read_to_string(fx.root.join("README.md")).unwrap(),
        "new"
    );
    assert!(trash.join("README.md").exists());
    assert!(app(&mut h).history.can_undo(), "the step stays");

    std::fs::remove_file(fx.root.join("README.md")).unwrap();
    undo(&mut h);
    wait_idle(&mut h);
    assert!(fx.root.join("README.md").is_file() && !trash.join("README.md").exists());
}

#[test]
fn cmd_z_restores_the_old_name() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    select(&mut h, Side::Left, &["README.md"]);
    h.key_press(Key::F2);
    h.run_steps(2);
    enter_name(&mut h, "LIESMICH.md");
    wait_for(&mut h, "the rename", |h| {
        fx.root.join("LIESMICH.md").exists() && app(h).history.can_undo()
    });

    undo(&mut h);
    wait_idle(&mut h);
    assert!(fx.root.join("README.md").exists() && !fx.root.join("LIESMICH.md").exists());
    assert!(toast(&mut h, "Renamed “LIESMICH.md” to “README.md”"));

    redo(&mut h);
    wait_idle(&mut h);
    assert!(fx.root.join("LIESMICH.md").exists() && !fx.root.join("README.md").exists());
}

/// A new local folder goes to the Trash on Undo (Finder), and comes back
/// with Redo, with whatever was put into it.
#[test]
fn cmd_z_moves_a_new_folder_to_the_trash() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    let trash = fake_trash(&fx);
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    enter_name(&mut h, "Drafts");
    wait_for(&mut h, "the new folder", |h| {
        fx.root.join("Drafts").is_dir() && app(h).history.can_undo()
    });
    std::fs::write(fx.root.join("Drafts/note.txt"), "keep me").unwrap();

    undo(&mut h);
    wait_idle(&mut h);
    assert!(!fx.root.join("Drafts").exists());
    assert!(
        trash.join("Drafts/note.txt").exists(),
        "trashed, not deleted"
    );

    redo(&mut h);
    wait_idle(&mut h);
    assert!(fx.root.join("Drafts/note.txt").exists());
}

/// Servers have no Trash: Undo deletes a new folder only while it is
/// still empty.
#[test]
fn on_a_server_undo_deletes_a_new_folder_only_while_empty() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    let mem = open_server(&mut h);
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    enter_name(&mut h, "build");
    let build = VfsPath::new("/srv/build");
    wait_for(&mut h, "the new folder", |h| {
        mem.stat(&build).is_ok() && app(h).history.can_undo()
    });

    undo(&mut h);
    wait_idle(&mut h);
    assert!(mem.stat(&build).is_err(), "deleted");
    redo(&mut h);
    wait_idle(&mut h);
    assert!(mem.stat(&build).is_ok(), "made again");

    mem.put("/srv/build/app.js", b"x", chrono::Utc::now());
    undo(&mut h);
    wait_idle(&mut h);
    assert!(toast(&mut h, "Couldn't delete “build”"));
    assert!(mem.stat(&VfsPath::new("/srv/build/app.js")).is_ok(), "kept");
    assert!(app(&mut h).history.can_undo());
}

/// In the filter field ⌘Z edits the text; files stay where they are.
#[test]
fn cmd_z_in_a_text_field_is_left_to_the_field() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    let trash = fake_trash(&fx);
    select(&mut h, Side::Left, &["README.md"]);
    press(&mut h, Modifiers::COMMAND, Key::Backspace);
    wait_for(&mut h, "README.md in the Trash", |h| {
        trash.join("README.md").exists() && app(h).history.can_undo()
    });
    press(&mut h, Modifiers::COMMAND, Key::F);
    h.event(egui::Event::Text("src".into()));
    h.run_steps(2);

    undo(&mut h);
    h.run_steps(5);
    assert!(!app(&mut h).history.busy);
    assert!(trash.join("README.md").exists(), "not put back");
    assert!(app(&mut h).history.can_undo());
}

/// A change of case only ("README.md" → "readme.md") is undone too, although
/// the Mac's disk finds the old name for the new one.
#[test]
fn cmd_z_undoes_a_change_of_case() {
    let fx = fixture();
    let mut h = harness(&fx, WIDE, Theme::Light);
    let names = |fx: &Fixture| -> Vec<String> {
        std::fs::read_dir(&fx.root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.eq_ignore_ascii_case("readme.md"))
            .collect()
    };
    select(&mut h, Side::Left, &["README.md"]);
    h.key_press(Key::F2);
    h.run_steps(2);
    enter_name(&mut h, "readme.md");
    wait_for(&mut h, "the rename", |h| {
        names(&fx) == ["readme.md"] && app(h).history.can_undo()
    });

    undo(&mut h);
    wait_idle(&mut h);
    assert_eq!(names(&fx), ["README.md"]);
}
