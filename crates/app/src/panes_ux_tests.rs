//! Any number of panes: split / close (at least one), widths, focus, the
//! copy target (most recently active pane), the copy dialog's target
//! picker, drag & drop onto any pane, Compare & Sync's A / B panes,
//! commands that need two panes, plugin events and the saved session.
//!
//! Behaviour tests step the real app without a GPU; the renders at the end
//! (`render_n_panes_*`) need one and write PNGs to `$MANTA_RENDER_DIR`.

use super::*;
use crate::app::Action;
use crate::commands::Cmd;
use crate::pane::SyncSlot;
use eframe::App as _;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

const BIG: [f32; 2] = [1600.0, 900.0];

fn behaviour(fx: &Fixture, size: [f32; 2]) -> H {
    harness_with(fx, size, Theme::Light, false)
}

fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    settle(h);
}

fn order(h: &mut H) -> Vec<Side> {
    app(h).pane_order()
}

fn active(h: &mut H) -> Side {
    app(h).active_pane()
}

fn path_of(h: &mut H, side: Side) -> String {
    app(h).pane(side).tab().loc.path.to_string()
}

fn sub(fx: &Fixture, name: &str) -> VfsPath {
    VfsPath::new(&fx.root.join(name).to_string_lossy())
}

/// Four panes: [Projects, src, photos, Design] with ids [0, 2, 3, 1].
fn four_panes(fx: &Fixture) -> H {
    four_panes_sized(fx, BIG)
}

fn four_panes_sized(fx: &Fixture, size: [f32; 2]) -> H {
    let mut h = behaviour(fx, size);
    act(&mut h, Action::SplitPane(Side::LEFT));
    act(&mut h, Action::SplitPane(PaneId(2)));
    assert_eq!(
        order(&mut h),
        vec![Side::LEFT, PaneId(2), PaneId(3), Side::RIGHT]
    );
    app(&mut h).navigate_path(PaneId(2), sub(fx, "src"));
    app(&mut h).navigate_path(PaneId(3), sub(fx, "photos"));
    settle(&mut h);
    h
}

fn weights(h: &mut H) -> Vec<f32> {
    app(h).panes.iter().map(|p| p.weight).collect()
}

#[test]
fn split_and_close_keep_at_least_one_pane() {
    let fx = fixture();
    let mut h = behaviour(&fx, BIG);
    assert_eq!(order(&mut h), vec![Side::LEFT, Side::RIGHT]);
    app(&mut h).activate(Side::LEFT);
    let left = path_of(&mut h, Side::LEFT);

    // ⌘D: a new pane right of the active one, same folder, focused.
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    settle(&mut h);
    assert_eq!(order(&mut h), vec![Side::LEFT, PaneId(2), Side::RIGHT]);
    assert_eq!(active(&mut h), PaneId(2));
    assert_eq!(path_of(&mut h, PaneId(2)), left);
    assert_eq!(
        weights(&mut h),
        vec![0.25, 0.25, 0.5],
        "the split pane shares its width"
    );
    assert!(
        !app(&mut h).pane(PaneId(2)).tab().entries.is_empty(),
        "the new pane lists its folder"
    );

    // ⇧⌘D: closes it; the focus goes back to the pane used before.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    settle(&mut h);
    assert_eq!(order(&mut h), vec![Side::LEFT, Side::RIGHT]);
    assert_eq!(active(&mut h), Side::LEFT);
    assert_eq!(weights(&mut h), vec![0.5, 0.5]);

    // Down to one pane; the last one can't be closed.
    act(&mut h, Action::ClosePane(Side::RIGHT));
    assert_eq!(order(&mut h), vec![Side::LEFT]);
    assert_eq!(weights(&mut h), vec![1.0]);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    settle(&mut h);
    assert_eq!(order(&mut h), vec![Side::LEFT], "minimum is one pane");
    assert!(app(&mut h)
        .toasts
        .titles()
        .iter()
        .any(|t| t == t!("panes.last_pane")));
    assert!(!app(&mut h).cmd_enabled(Cmd::ClosePane));
    assert_eq!(
        app(&mut h).cmd_disabled_reason(Cmd::ClosePane),
        Some(t!("panes.last_pane"))
    );

    // The freed id comes back first.
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    settle(&mut h);
    assert_eq!(order(&mut h), vec![Side::LEFT, Side::RIGHT]);
}

#[test]
fn a_narrow_window_refuses_another_pane() {
    let fx = fixture();
    let mut h = behaviour(&fx, NARROW);
    let w = app(&mut h).panes_width;
    assert!(
        w > 0.0 && w < 3.0 * crate::pane_set::MIN_PANE_W,
        "pane area {w}"
    );
    act(&mut h, Action::SplitPane(Side::LEFT));
    assert_eq!(order(&mut h).len(), 2, "no third pane");
    assert!(app(&mut h)
        .toasts
        .titles()
        .iter()
        .any(|t| t == t!("panes.no_room")));
    // Every pane keeps its minimum width.
    let rects = app(&mut h).pane_rects.clone();
    assert!(
        rects
            .iter()
            .all(|(_, r)| r.width() >= crate::pane_set::MIN_PANE_W - 0.5),
        "{rects:?}"
    );
}

#[test]
fn dividers_resize_neighbours_and_reset() {
    let fx = fixture();
    // Wide enough that no pane sits at its minimum width.
    let mut h = four_panes_sized(&fx, [2600.0, 900.0]);
    for p in app(&mut h).panes.iter_mut() {
        p.weight = 0.25;
    }
    h.run_steps(2);
    let rects = app(&mut h).pane_rects.clone();
    assert_eq!(rects.len(), 4);
    // Drag the divider between panes 2 and 3 by 60 points.
    let x = (rects[1].1.right() + rects[2].1.left()) / 2.0;
    let y = rects[1].1.center().y;
    h.hover_at(egui::pos2(x, y));
    h.step();
    h.drag_at(egui::pos2(x, y));
    h.run_steps(2);
    for i in 1..=10 {
        h.hover_at(egui::pos2(x + 6.0 * i as f32, y));
        h.step();
    }
    h.drop_at(egui::pos2(x + 60.0, y));
    h.run_steps(2);
    let after = app(&mut h).pane_rects.clone();
    let grow = after[1].1.width() - rects[1].1.width();
    assert!((grow - 60.0).abs() < 3.0, "pane 2 grew by {grow}");
    assert!((after[2].1.width() - (rects[2].1.width() - 60.0)).abs() < 3.0);
    assert!(
        (after[0].1.width() - rects[0].1.width()).abs() < 1.0,
        "pane 1 unchanged"
    );
    assert!(
        (after[3].1.width() - rects[3].1.width()).abs() < 1.0,
        "pane 4 unchanged"
    );
    assert!((weights(&mut h)[1] - 0.25).abs() > 0.01);
    // Double-click: all equal.
    let x = (after[1].1.right() + after[2].1.left()) / 2.0;
    h.hover_at(egui::pos2(x, y));
    h.step();
    // A harness step is 0.25 s, a double-click needs both clicks within
    // 0.3 s: give these frames explicit times 50 ms apart.
    let t0 = h.ctx.input(|i| i.time) + 1.0;
    for (k, pressed) in [true, false, true, false].into_iter().enumerate() {
        let input = h.input_mut();
        input.time = Some(t0 + 0.05 * k as f64);
        input.events.push(egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
    assert!(
        weights(&mut h).iter().all(|w| (w - 0.25).abs() < 1e-4),
        "{:?}",
        weights(&mut h)
    );
}

#[test]
fn tab_and_option_command_arrows_move_the_focus() {
    let fx = fixture();
    let mut h = behaviour(&fx, BIG);
    act(&mut h, Action::SplitPane(Side::LEFT));
    assert_eq!(order(&mut h), vec![Side::LEFT, PaneId(2), Side::RIGHT]);
    assert_eq!(active(&mut h), PaneId(2));
    // Tab: the next pane, wrapping. (The harness doesn't run eframe's
    // raw-input hook, where the app takes Tab; feed it like eframe does.)
    let tab = |h: &mut H| {
        let ctx = h.ctx.clone();
        let mut raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        app(h).raw_input_hook(&ctx, &mut raw);
        assert!(raw.events.is_empty(), "the app takes Tab");
        h.run_steps(2);
    };
    tab(&mut h);
    assert_eq!(active(&mut h), Side::RIGHT);
    tab(&mut h);
    assert_eq!(active(&mut h), Side::LEFT, "Tab wraps around");
    // ⌥⌘→ / ⌥⌘←: neighbours, stopping at the edges.
    let alt_cmd = Modifiers::COMMAND | Modifiers::ALT;
    h.key_press_modifiers(alt_cmd, Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(active(&mut h), PaneId(2));
    h.key_press_modifiers(alt_cmd, Key::ArrowRight);
    h.key_press_modifiers(alt_cmd, Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(active(&mut h), Side::RIGHT, "stops at the right edge");
    h.key_press_modifiers(alt_cmd, Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(active(&mut h), PaneId(2));
    // ⌥⌘← didn't go Back in the pane's history.
    assert!(app(&mut h).pane(PaneId(2)).tab().forward.is_empty());
}

#[test]
fn copy_target_is_the_most_recently_active_pane() {
    let fx = fixture();
    // One pane: no target, F5 does nothing.
    let mut h = behaviour(&fx, BIG);
    act(&mut h, Action::ClosePane(Side::RIGHT));
    select(&mut h, Side::LEFT, &["notes.txt"]);
    assert_eq!(app(&mut h).target_of(Side::LEFT), None);
    h.key_press(Key::F5);
    settle(&mut h);
    assert!(app(&mut h).dialog.is_none(), "F5 is off with one pane");

    // Two panes: the other one, no picker.
    let mut h = behaviour(&fx, BIG);
    app(&mut h).activate(Side::LEFT);
    select(&mut h, Side::LEFT, &["notes.txt"]);
    h.key_press(Key::F5);
    settle(&mut h);
    match &app(&mut h).dialog {
        Some(Dialog::Transfer(f)) => {
            assert_eq!(f.dst_side, Side::RIGHT);
            assert!(f.targets.is_empty(), "no picker with two panes");
        }
        _ => panic!("copy dialog expected"),
    }
    assert_eq!(
        app(&mut h).target_highlight(),
        None,
        "nothing to point at with two panes"
    );

    // Four panes: the most recently active other pane.
    let mut h = four_panes(&fx);
    app(&mut h).activate(Side::RIGHT);
    app(&mut h).activate(Side::LEFT);
    assert_eq!(app(&mut h).target_of(Side::LEFT), Some(Side::RIGHT));
    assert_eq!(
        app(&mut h).target_highlight(),
        None,
        "no selection, no highlight"
    );
    select(&mut h, Side::LEFT, &["notes.txt"]);
    assert_eq!(app(&mut h).target_highlight(), Some(Side::RIGHT));
    app(&mut h).activate(PaneId(3));
    app(&mut h).activate(Side::LEFT);
    assert_eq!(app(&mut h).target_of(Side::LEFT), Some(PaneId(3)));
    assert_eq!(app(&mut h).target_highlight(), Some(PaneId(3)));

    // F5: the dialog targets pane 3 and offers every other pane.
    h.key_press(Key::F5);
    settle(&mut h);
    let src = sub(&fx, "src");
    let Some(Dialog::Transfer(f)) = &mut app(&mut h).dialog else {
        panic!("copy dialog expected")
    };
    assert_eq!(f.dst_side, PaneId(3));
    assert_eq!(f.dst_dir, sub(&fx, "photos"));
    let offered: Vec<(Side, usize)> = f.targets.iter().map(|t| (t.side, t.number)).collect();
    assert_eq!(
        offered,
        vec![(PaneId(2), 2), (PaneId(3), 3), (Side::RIGHT, 4)]
    );
    assert!(f.targets.iter().all(|t| t.problem.is_none()));
    // Picking another pane points the dialog there.
    f.set_target(PaneId(2));
    assert_eq!((f.dst_side, f.dst_dir.clone()), (PaneId(2), src));
}

#[test]
fn the_copy_picker_marks_targets_that_cant_take_the_items() {
    let fx = fixture();
    let mut h = four_panes(&fx);
    // Pane 2 shows the same folder as the source: picked by default, but
    // the dialog opens because other panes can take the items.
    app(&mut h).navigate_path(PaneId(2), VfsPath::new(&fx.root.to_string_lossy()));
    settle(&mut h);
    app(&mut h).activate(PaneId(2));
    app(&mut h).activate(Side::LEFT);
    select(&mut h, Side::LEFT, &["notes.txt"]);
    act(&mut h, Action::CopyToOther(Side::LEFT, false));
    let Some(Dialog::Transfer(f)) = &mut app(&mut h).dialog else {
        panic!("copy dialog expected")
    };
    assert_eq!(f.dst_side, PaneId(2));
    let same = f.targets.iter().find(|t| t.side == PaneId(2)).unwrap();
    assert_eq!(same.problem.as_deref(), Some(t!("status.same_folder")));
    f.set_target(PaneId(3));
    assert!(f
        .targets
        .iter()
        .find(|t| t.side == PaneId(3))
        .unwrap()
        .problem
        .is_none());
}

#[test]
fn drag_and_drop_onto_the_third_pane() {
    let fx = fixture();
    let mut h = four_panes(&fx);
    app(&mut h).activate(Side::LEFT);
    let fs = app(&mut h).pane(Side::LEFT).tab().loc.fs.clone();
    let paths = vec![VfsPath::new(&fx.root.join("notes.txt").to_string_lossy())];
    let target = app(&mut h).pane_rects[2];
    assert_eq!(target.0, PaneId(3));
    // A drag from pane 1 is in flight; the pointer is released over pane 3.
    let ctx = h.ctx.clone();
    egui::DragAndDrop::set_payload(
        &ctx,
        crate::pane::DragPayload::files(Side::LEFT, fs, paths.clone()),
    );
    let pos = target.1.center();
    h.hover_at(pos);
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    egui::DragAndDrop::set_payload(
        &ctx,
        crate::pane::DragPayload::files(
            Side::LEFT,
            app(&mut h).pane(Side::LEFT).tab().loc.fs.clone(),
            paths,
        ),
    );
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    let Some(Dialog::Transfer(f)) = &app(&mut h).dialog else {
        panic!("drop opens the copy dialog")
    };
    assert_eq!(f.src_side, Side::LEFT);
    assert_eq!(f.dst_side, PaneId(3));
    assert_eq!(f.dst_dir, sub(&fx, "photos"));
    assert_eq!(active(&mut h), PaneId(3), "the drop target becomes active");
}

#[test]
fn two_pane_features_follow_the_pane_count() {
    let fx = fixture();
    let two_pane = [
        Cmd::CopyToOther,
        Cmd::MoveToOther,
        Cmd::CompareSync,
        Cmd::SwitchPane,
        Cmd::ClosePane,
    ];
    let mut h = behaviour(&fx, BIG);
    app(&mut h).activate(Side::LEFT);
    select(&mut h, Side::LEFT, &["notes.txt"]);
    for n in [2, 3, 1] {
        while order(&mut h).len() < n {
            let a = active(&mut h);
            act(&mut h, Action::SplitPane(a));
        }
        while order(&mut h).len() > n {
            let last = *order(&mut h).last().unwrap();
            act(&mut h, Action::ClosePane(last));
        }
        app(&mut h).activate(Side::LEFT);
        select(&mut h, Side::LEFT, &["notes.txt"]);
        for c in two_pane {
            let on = app(&mut h).cmd_enabled(c);
            assert_eq!(on, n > 1, "{c:?} with {n} pane(s)");
            let why = app(&mut h).cmd_disabled_reason(c);
            assert_eq!(why.is_some(), n == 1, "{c:?} explains itself with one pane");
        }
        // The palette greys them out and says why.
        let entries = app(&mut h).palette_entries();
        let sync = entries
            .iter()
            .find(|e| e.id == crate::palette::PaletteId::Cmd(Cmd::CompareSync))
            .unwrap();
        assert_eq!(sync.enabled, n > 1);
        if n == 1 {
            assert_eq!(sync.category, t!("panes.needs_two"));
        }
        // ⇧⌘S does nothing with one pane.
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        settle(&mut h);
        assert_eq!(
            app(&mut h).sync.open,
            n > 1,
            "Compare & Sync with {n} pane(s)"
        );
        app(&mut h).sync.open = false;
        // F6 with one pane does nothing either.
        h.key_press(Key::F6);
        settle(&mut h);
        assert_eq!(app(&mut h).dialog.is_some(), n > 1, "F6 with {n} pane(s)");
        app(&mut h).dialog = None;
    }
    // Split works with one pane, of course.
    assert!(app(&mut h).cmd_enabled(Cmd::SplitPane));
}

#[test]
fn compare_and_sync_picks_two_panes() {
    let fx = fixture();
    // Two panes: left and right, whatever is active.
    let mut h = behaviour(&fx, BIG);
    app(&mut h).activate(Side::RIGHT);
    act(&mut h, Action::ShowSync);
    assert_eq!(app(&mut h).sync_pair(), Some((Side::LEFT, Side::RIGHT)));
    assert!(!app(&mut h).sync.picker, "no picker with two panes");

    // Four panes: the active pane (A) and its target (B).
    let mut h = four_panes(&fx);
    app(&mut h).activate(PaneId(3));
    app(&mut h).activate(PaneId(2));
    act(&mut h, Action::ShowSync);
    assert!(app(&mut h).sync.picker);
    assert_eq!(app(&mut h).sync.pick, Some((PaneId(2), PaneId(3))));
    assert_eq!(
        app(&mut h).sync.left.as_ref().map(|l| l.path.clone()),
        Some(sub(&fx, "src"))
    );
    assert_eq!(app(&mut h).sync.choices.len(), 4);
    // Reassign B; A stays.
    act(&mut h, Action::SetSyncPane(Side::RIGHT, SyncSlot::B));
    assert_eq!(app(&mut h).sync.pick, Some((PaneId(2), Side::RIGHT)));
    assert_eq!(
        app(&mut h).sync.right.as_ref().map(|l| l.path.clone()),
        Some(sub(&fx, "Design"))
    );
    // A and B differ: picking B's pane as A swaps them.
    act(&mut h, Action::SetSyncPane(Side::RIGHT, SyncSlot::A));
    assert_eq!(app(&mut h).sync.pick, Some((Side::RIGHT, PaneId(2))));
    act(&mut h, Action::SetSyncPane(Side::RIGHT, SyncSlot::B));
    let (a, b) = app(&mut h).sync.pick.unwrap();
    assert_ne!(a, b);
    // Closing a chosen pane falls back to the defaults.
    act(&mut h, Action::ClosePane(a));
    let (a2, b2) = app(&mut h).sync.pick.unwrap();
    assert!(a2 != b2 && app(&mut h).has_pane(a2) && app(&mut h).has_pane(b2));
    // A fresh window starts from the active pane again.
    app(&mut h).sync.open = false;
    app(&mut h).activate(Side::LEFT);
    act(&mut h, Action::ShowSync);
    assert_eq!(app(&mut h).sync.pick.map(|p| p.0), Some(Side::LEFT));
}

/// A stand-in for the Terminal: records pane events.
struct PaneEvents(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl Plugin for PaneEvents {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "manta.terminal".into(),
            name: "Terminal".into(),
            version: "1".into(),
            description: String::new(),
            author: String::new(),
            builtin: true,
        }
    }
    fn enable(&mut self, _handle: PluginHandle) {}
    fn disable(&mut self) {}
    fn on_event(&mut self, ev: &PluginEvent) {
        let line = match ev {
            PluginEvent::PaneOpened(p) => format!("opened {p}"),
            PluginEvent::PaneClosed(p) => format!("closed {p}"),
            PluginEvent::DirectoryChanged(s) => format!("dir {}", s.pane),
            _ => return,
        };
        self.0.lock().unwrap().push(line);
    }
    fn panels(&self) -> Vec<manta_plugin::PanelSpec> {
        vec![manta_plugin::PanelSpec {
            id: "terminal".into(),
            title: "Terminal".into(),
            placement: manta_plugin::PanelPlacement::PaneBottom,
            default_open: false,
        }]
    }
    fn panel_ui(&mut self, _panel: &str, _pane: &manta_plugin::PaneState, ui: &mut egui::Ui) {
        ui.label("$");
    }
}

#[test]
fn plugins_hear_panes_come_and_go() {
    let fx = fixture();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let l = log.clone();
    let mut h = harness_full(
        &fx,
        BIG,
        Theme::Light,
        false,
        Box::new(move |ctx: &egui::Context| {
            PluginHost::new(ctx.clone(), vec![Box::new(PaneEvents(l))], None)
        }),
    );
    h.run_steps(2);
    let take = |log: &std::sync::Arc<std::sync::Mutex<Vec<String>>>| {
        std::mem::take(&mut *log.lock().unwrap())
    };
    let start = take(&log);
    assert!(
        start.starts_with(&["opened left".to_string()])
            || start.contains(&"opened left".to_string()),
        "{start:?}"
    );
    assert!(start.contains(&"opened right".to_string()));
    // The terminal below the right pane; split it: the new pane gets one too.
    app(&mut h)
        .plugins
        .set_panel_open("manta.terminal", "terminal", Some(Side::RIGHT), true);
    act(&mut h, Action::SplitPane(Side::RIGHT));
    let new = PaneId(2);
    assert_eq!(order(&mut h), vec![Side::LEFT, Side::RIGHT, new]);
    let ev = take(&log);
    assert_eq!(ev.first().map(String::as_str), Some("opened 2"), "{ev:?}");
    assert!(ev.contains(&"dir 2".to_string()));
    assert!(app(&mut h)
        .plugins
        .is_panel_open("manta.terminal", "terminal", Some(new)));
    // Closing it: PaneClosed (the Terminal hangs up), its panel instance goes.
    act(&mut h, Action::ClosePane(new));
    assert_eq!(take(&log), vec!["closed 2".to_string()]);
    assert!(!app(&mut h)
        .plugins
        .is_panel_open("manta.terminal", "terminal", Some(new)));
    assert!(app(&mut h)
        .plugins
        .is_panel_open("manta.terminal", "terminal", Some(Side::RIGHT)));
    assert_eq!(
        app(&mut h).plugins.pane_ids(),
        vec![Side::LEFT, Side::RIGHT]
    );
}

#[test]
fn panes_are_saved_and_restored() {
    let fx = fixture();
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");
    let mk = |fx: &Fixture, session: &Path| {
        let init = AppInit {
            start_dir: Some(fx.root.clone()),
            favorites: Some(favorites(&fx.root)),
            places: Some(places(&fx.root)),
            plugins: Some(Box::new(|ctx: &egui::Context| {
                PluginHost::new(ctx.clone(), Vec::new(), None)
            })),
            prefs: Some(crate::prefs::Prefs::default()),
            session_path: Some(session.to_path_buf()),
            ..Default::default()
        };
        let mut h = Harness::builder()
            .with_size(egui::vec2(BIG[0], BIG[1]))
            .with_max_steps(10_000)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::install)
            .build_eframe(move |cc| MantaApp::with_init(&cc.egui_ctx, init));
        settle(&mut h);
        h
    };
    // No session yet (first launch, or the two-pane version): two panes.
    let mut h = mk(&fx, &session);
    assert_eq!(order(&mut h), vec![Side::LEFT, Side::RIGHT]);
    assert_eq!(weights(&mut h), vec![0.5, 0.5]);
    // Three panes, a second tab, custom widths, a focus history.
    app(&mut h).navigate_path(Side::RIGHT, sub(&fx, "Design"));
    act(&mut h, Action::SplitPane(Side::LEFT));
    app(&mut h).navigate_path(PaneId(2), sub(&fx, "src"));
    act(&mut h, Action::NewTab(PaneId(2)));
    app(&mut h).navigate_path(PaneId(2), sub(&fx, "photos"));
    settle(&mut h);
    for (p, w) in app(&mut h).panes.iter_mut().zip([0.5, 0.2, 0.3]) {
        p.weight = w;
    }
    app(&mut h).activate(Side::RIGHT);
    app(&mut h).activate(PaneId(2));
    app(&mut h).on_exit();
    let saved = app(&mut h).session();
    drop(h);

    let mut h = mk(&fx, &session);
    assert_eq!(
        app(&mut h).session(),
        saved,
        "same panes, tabs, widths, focus and MRU"
    );
    assert_eq!(order(&mut h), vec![Side::LEFT, PaneId(2), Side::RIGHT]);
    assert_eq!(active(&mut h), PaneId(2));
    assert_eq!(
        app(&mut h).target_of(PaneId(2)),
        Some(Side::RIGHT),
        "MRU restored"
    );
    let tabs: Vec<String> = app(&mut h)
        .pane(PaneId(2))
        .tabs
        .iter()
        .map(|t| t.loc.path.to_string())
        .collect();
    assert_eq!(
        tabs,
        vec![sub(&fx, "src").to_string(), sub(&fx, "photos").to_string()]
    );
    assert_eq!(app(&mut h).pane(PaneId(2)).active, 1);
    assert_eq!(path_of(&mut h, Side::RIGHT), sub(&fx, "Design").to_string());
    assert_eq!(weights(&mut h), vec![0.5, 0.2, 0.3]);
}

#[test]
fn remote_tabs_are_saved_as_their_last_local_folder() {
    let fx = fixture();
    let mut h = behaviour(&fx, BIG);
    // A tab on a "server": the session keeps its last local folder.
    let remote: manta_core::SharedVfs = std::sync::Arc::new(manta_vfs::LocalFs::new());
    let tab = app(&mut h).pane_mut(Side::RIGHT).tab_mut();
    tab.back.push(tab.loc.clone());
    tab.loc = crate::pane::Location::new(remote, Some(99), VfsPath::new("/var/www"));
    let s = app(&mut h).session();
    let right = s.panes.iter().find(|p| p.id == Side::RIGHT).unwrap();
    assert_eq!(right.tabs[0].path, sub(&fx, "Design").to_string());
}

// ------------------------------------------------------------------ renders

/// Renders of this phase go to `$MANTA_RENDER_DIR` (see `save`).
fn render_set(theme: Theme, locale: crate::i18n::Locale, tag: &str) {
    let fx = fixture();
    let mut h = harness(&fx, [1680.0, 860.0], theme);
    crate::i18n::set_locale(locale);
    h.run_steps(2);
    // 1 pane.
    act(&mut h, Action::ClosePane(Side::RIGHT));
    select(&mut h, Side::LEFT, &["notes.txt"]);
    save(&mut h, &format!("n-panes-1-{tag}"));
    // The disabled toolbar (Copy / Move / Compare) says why.
    let compare = app(&mut h)
        .toolbar_rects
        .iter()
        .find(|(c, _)| *c == Cmd::CompareSync)
        .map(|(_, r)| *r);
    if let Some(r) = compare {
        h.hover_at(r.center());
        h.run_steps(30);
        save(&mut h, &format!("n-panes-1-disabled-tip-{tag}"));
        h.hover_at(egui::pos2(900.0, 500.0));
    }
    // 2 panes.
    act(&mut h, Action::SplitPane(Side::LEFT));
    app(&mut h).navigate_path(Side::RIGHT, sub(&fx, "Design"));
    settle(&mut h);
    app(&mut h).activate(Side::LEFT);
    save(&mut h, &format!("n-panes-2-{tag}"));
    // 3 panes, a selection in the active one: the target is marked.
    act(&mut h, Action::SplitPane(Side::LEFT));
    app(&mut h).navigate_path(PaneId(2), sub(&fx, "src"));
    settle(&mut h);
    app(&mut h).activate(Side::RIGHT);
    app(&mut h).activate(Side::LEFT);
    select(&mut h, Side::LEFT, &["notes.txt", "report-q3.pdf"]);
    save(&mut h, &format!("n-panes-3-target-{tag}"));
    // 4 panes.
    act(&mut h, Action::SplitPane(PaneId(2)));
    app(&mut h).navigate_path(PaneId(3), sub(&fx, "photos"));
    settle(&mut h);
    app(&mut h).activate(Side::LEFT);
    select(&mut h, Side::LEFT, &["notes.txt"]);
    save(&mut h, &format!("n-panes-4-{tag}"));
    // The copy dialog with the target picker, open.
    act(&mut h, Action::CopyToOther(Side::LEFT, false));
    save(&mut h, &format!("n-panes-copy-picker-{tag}"));
    h.run_steps(1);
    if let Some(combo) = h.query_by_label_contains("4 · ") {
        combo.click();
        h.run_steps(3);
        save(&mut h, &format!("n-panes-copy-picker-open-{tag}"));
    }
    app(&mut h).dialog = None;
    h.run_steps(2);
    // Compare & Sync with four panes: the A / B picker and badges.
    app(&mut h).activate(Side::RIGHT);
    app(&mut h).activate(Side::LEFT);
    act(&mut h, Action::ShowSync);
    app(&mut h).sync.window.request_focus();
    h.run_steps(4);
    save(&mut h, &format!("n-panes-sync-picker-{tag}"));
    app(&mut h).sync.open = false;
    h.run_steps(2);
    // A narrow window refuses a fifth pane (toast).
    h.set_size(egui::vec2(1180.0, 760.0));
    h.run_steps(3);
    act(&mut h, Action::SplitPane(Side::LEFT));
    h.run_steps(3);
    save(&mut h, &format!("n-panes-no-room-{tag}"));
    crate::i18n::set_locale(crate::i18n::Locale::En);
}

#[test]
fn render_n_panes_light_en() {
    require_gpu!();
    render_set(Theme::Light, crate::i18n::Locale::En, "light-en");
}

#[test]
fn render_n_panes_dark_de() {
    require_gpu!();
    render_set(Theme::Dark, crate::i18n::Locale::De, "dark-de");
}

#[test]
fn render_n_panes_dark_en() {
    require_gpu!();
    render_set(Theme::Dark, crate::i18n::Locale::En, "dark-en");
}

#[test]
fn render_n_panes_light_de() {
    require_gpu!();
    render_set(Theme::Light, crate::i18n::Locale::De, "light-de");
}
