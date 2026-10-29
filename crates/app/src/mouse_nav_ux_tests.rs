//! The mouse's side buttons (thumb buttons 4 and 5) go back and forward in
//! the pane under the pointer; Settings swaps or turns them off.

use super::*;
use crate::prefs::MouseNav;
use egui::Modifiers;

fn behaviour(fx: &Fixture) -> H {
    harness_with(fx, WIDE, Theme::Light, false)
}

fn local_path(fx: &Fixture, name: &str) -> VfsPath {
    VfsPath::new(&fx.root.join(name).to_string_lossy())
}

fn path(h: &mut H, side: Side) -> VfsPath {
    app(h).pane(side).tab().loc.path.clone()
}

/// A click of a side button with the pointer over the middle of `side`.
fn side_click(h: &mut H, side: Side, button: egui::PointerButton) {
    let rect = app(h)
        .pane_rects
        .iter()
        .find(|(s, _)| *s == side)
        .map(|(_, r)| *r)
        .expect("the pane was drawn");
    let pos = rect.center();
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    settle(h);
}

#[test]
fn side_buttons_go_back_and_forward_in_the_pane_under_the_pointer() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    settle(&mut h);
    let (left, right) = (Side::Left, Side::Right);
    app(&mut h).navigate_path(left, local_path(&fx, "src"));
    app(&mut h).navigate_path(right, local_path(&fx, "photos"));
    settle(&mut h);
    app(&mut h).activate(left);
    h.run_steps(2);

    // Back over the right pane: only it goes back, and it becomes active.
    side_click(&mut h, right, egui::PointerButton::Extra1);
    assert_eq!(path(&mut h, right), local_path(&fx, "Design"));
    assert_eq!(path(&mut h, left), local_path(&fx, "src"));
    assert_eq!(app(&mut h).active, right);
    // Forward again.
    side_click(&mut h, right, egui::PointerButton::Extra2);
    assert_eq!(path(&mut h, right), local_path(&fx, "photos"));

    // Swapped: button 5 goes back, button 4 forward.
    app(&mut h).prefs.mouse_nav = MouseNav::Swapped;
    side_click(&mut h, left, egui::PointerButton::Extra2);
    assert_eq!(path(&mut h, left), local_path(&fx, ""));
    assert_eq!(app(&mut h).active, left);
    side_click(&mut h, left, egui::PointerButton::Extra1);
    assert_eq!(path(&mut h, left), local_path(&fx, "src"));

    // Off: nothing happens.
    app(&mut h).prefs.mouse_nav = MouseNav::Off;
    side_click(&mut h, left, egui::PointerButton::Extra2);
    side_click(&mut h, left, egui::PointerButton::Extra1);
    assert_eq!(path(&mut h, left), local_path(&fx, "src"));
}

#[test]
fn side_buttons_leave_the_panes_alone_while_the_palette_is_open() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    settle(&mut h);
    app(&mut h).navigate_path(Side::Left, local_path(&fx, "src"));
    settle(&mut h);
    app(&mut h).palette.open = true;
    h.run_steps(2);
    side_click(&mut h, Side::Left, egui::PointerButton::Extra1);
    assert_eq!(path(&mut h, Side::Left), local_path(&fx, "src"));
}
