//! Modality (see `modal.rs`): Settings and Software Update block the main
//! window, in-window dialogs block everything behind them, and a click
//! outside a popover only closes it. Child of `ux_tests`.
//!
//! The harness can't create native windows, so Settings runs in its
//! fallback mode (an egui window inside the main window). The native path
//! (`raw_input_hook` taking the main window's input) is driven by calling
//! the hook directly with `embed_viewports` switched off for that call.

use eframe::App as _;
use egui::{Key, Modifiers, PointerButton};

use super::*;
use crate::app::Action;
use crate::aux_window::{embedded_key, AuxKind};
use crate::commands::Cmd;
use crate::dialogs::WhatsNewForm;

fn behaviour(fx: &Fixture) -> H {
    let mut h = harness_with(fx, WIDE, Theme::Light, false);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    h.run_steps(2);
    h
}

/// Row `i` of the left file list, left of where Settings opens.
fn row(i: usize) -> egui::Pos2 {
    egui::pos2(260.0, 108.0 + 24.0 * i as f32)
}

/// One frame whose input first goes through `raw_input_hook`, as eframe
/// does it (the harness itself skips the hook).
fn hooked_event(h: &mut H, e: egui::Event) {
    h.input_mut().events.push(e);
    let ctx = h.ctx.clone();
    let mut raw = h.input_mut().clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

fn click(h: &mut H, pos: egui::Pos2, button: PointerButton) {
    hooked_event(h, egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        hooked_event(
            h,
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            },
        );
    }
    h.run_steps(2);
}

fn cursor(h: &mut H) -> Option<String> {
    app(h)
        .pane(Side::Left)
        .tab()
        .cursor_entry()
        .map(|e| e.name.clone())
}

fn clear_selection(h: &mut H) {
    let tab = app(h).pane_mut(Side::Left).tab_mut();
    tab.selected.clear();
    tab.cursor = None;
    h.run_steps(2);
}

/// Shortcuts that would change the main window: ⌥⌘L (transfers), ⌘T (new
/// tab), ⇧⌘S (Compare & Sync), ⌘K (Connect).
fn press_main_shortcuts(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::L);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(2);
}

fn assert_main_unchanged(h: &mut H, transfers: bool, tabs: usize, what: &str) {
    let a = app(h);
    assert_eq!(a.show_transfers, transfers, "{what}: ⌥⌘L ignored");
    assert_eq!(a.pane(Side::Left).tabs.len(), tabs, "{what}: ⌘T ignored");
    assert!(!a.sync.open, "{what}: ⇧⌘S ignored");
    assert!(
        !matches!(a.dialog, Some(crate::dialogs::Dialog::Connect(_))),
        "{what}: ⌘K ignored"
    );
}

#[test]
fn clicking_a_row_selects_it_without_a_modal() {
    // Control for the tests below: `row` hits the file list.
    let fx = fixture();
    let mut h = behaviour(&fx);
    clear_selection(&mut h);
    click(&mut h, row(1), PointerButton::Primary);
    assert!(cursor(&mut h).is_some(), "the click selects a row");
}

#[test]
fn settings_blocks_the_main_window() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    clear_selection(&mut h);
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, Action::ShowSettings);
    h.run_steps(3);
    assert!(app(&mut h).modal_open());
    assert_eq!(app(&mut h).modal_window(), Some(AuxKind::Settings));

    // A click into the file list selects nothing; Settings comes back to
    // the front (stays key) with the alert sound.
    let beeps = crate::modal::beeps();
    click(&mut h, row(1), PointerButton::Primary);
    assert_eq!(cursor(&mut h), None, "the click doesn't reach the list");
    assert!(crate::modal::beeps() > beeps, "the alert sound");
    assert_eq!(embedded_key(&h.ctx), Some(AuxKind::Settings));
    assert!(app(&mut h).settings.open);

    // Main-window shortcuts stay off.
    let transfers = app(&mut h).show_transfers;
    let tabs = app(&mut h).pane(Side::Left).tabs.len();
    press_main_shortcuts(&mut h);
    assert_main_unchanged(&mut h, transfers, tabs, "Settings open");

    // Menu bar commands for the main window are greyed out and ignored;
    // Help and the web links still work.
    let a = app(&mut h);
    assert!(!a.menu_cmd_enabled(Cmd::NewTab, true));
    assert!(!a.menu_cmd_enabled(Cmd::Refresh, true));
    assert!(a.menu_cmd_enabled(Cmd::Help, true));
    assert!(a.menu_cmd_enabled(Cmd::NewTab, false));
    let mut actions = Vec::new();
    a.menu_commands(
        vec![Cmd::NewTab, Cmd::Transfers, Cmd::Connect],
        &mut actions,
    );
    assert!(actions.is_empty(), "menu commands ignored");
    a.menu_commands(vec![Cmd::Help], &mut actions);
    assert_eq!(actions.len(), 1, "Help still opens");

    // ⌘W closes Settings; then the main window works again.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(3);
    assert!(!app(&mut h).settings.open);
    assert!(!app(&mut h).modal_open());
    click(&mut h, row(1), PointerButton::Primary);
    assert!(cursor(&mut h).is_some(), "the list works again");
}

#[test]
fn native_settings_window_takes_the_main_windows_input() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).settings.show(None);
    h.run_steps(3);
    assert!(!app(&mut h).settings.focus_requested());

    // As eframe calls it for the main window while Settings is a native
    // window: every click, key, scroll and file drop is taken out.
    let ctx = h.ctx.clone();
    ctx.set_embed_viewports(false);
    let press = |pressed| egui::Event::PointerButton {
        pos: row(1),
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let mut raw = egui::RawInput {
        events: vec![
            egui::Event::PointerMoved(row(1)),
            press(true),
            press(false),
            egui::Event::Key {
                key: Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            },
            egui::Event::Text("x".into()),
        ],
        dropped_files: vec![crate::native_drag::test_hooks::dropped(
            std::path::Path::new("/tmp/x"),
        )],
        ..Default::default()
    };
    let beeps = crate::modal::beeps();
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    ctx.set_embed_viewports(true);
    assert_eq!(raw.events, vec![egui::Event::PointerGone]);
    assert!(raw.dropped_files.is_empty());
    assert!(
        app(&mut h).settings.focus_requested(),
        "Settings is brought to the front (ViewportCommand::Focus)"
    );
    assert_eq!(crate::modal::beeps(), beeps + 1, "one alert sound");

    // Hovering alone neither beeps nor focuses.
    h.run_steps(2);
    let mut raw = egui::RawInput {
        events: vec![egui::Event::PointerMoved(row(2))],
        ..Default::default()
    };
    ctx.set_embed_viewports(false);
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    ctx.set_embed_viewports(true);
    assert_eq!(raw.events, vec![egui::Event::PointerGone]);
    assert_eq!(crate::modal::beeps(), beeps + 1);
    assert!(!app(&mut h).settings.focus_requested());

    // Help stays non-modal: with only Help open the input passes.
    app(&mut h).settings.open = false;
    app(&mut h).help.show("");
    h.run_steps(3);
    assert!(!app(&mut h).modal_open(), "Help is not modal");
    let mut raw = egui::RawInput {
        events: vec![press(true)],
        ..Default::default()
    };
    ctx.set_embed_viewports(false);
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    ctx.set_embed_viewports(true);
    assert_eq!(raw.events.len(), 1, "the click reaches the main window");
}

#[test]
fn software_update_window_is_modal_too() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).updates.show();
    h.run_steps(3);
    assert_eq!(app(&mut h).modal_window(), Some(AuxKind::Update));
    let transfers = app(&mut h).show_transfers;
    let tabs = app(&mut h).pane(Side::Left).tabs.len();
    press_main_shortcuts(&mut h);
    assert_main_unchanged(&mut h, transfers, tabs, "Software Update open");
}

#[test]
fn in_window_dialog_blocks_panes_shortcuts_and_drops() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    clear_selection(&mut h);
    app(&mut h).dialog = Some(crate::dialogs::Dialog::WhatsNew(WhatsNewForm));
    h.run_steps(3);
    assert!(app(&mut h).modal_open());

    let transfers = app(&mut h).show_transfers;
    let tabs = app(&mut h).pane(Side::Left).tabs.len();
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::L);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.run_steps(2);
    assert_eq!(app(&mut h).show_transfers, transfers, "⌥⌘L ignored");
    assert_eq!(app(&mut h).pane(Side::Left).tabs.len(), tabs, "⌘T ignored");
    let mut actions = Vec::new();
    app(&mut h).menu_commands(vec![Cmd::NewTab], &mut actions);
    assert!(actions.is_empty(), "menu commands ignored");

    // No file drops behind the dialog.
    let ctx = h.ctx.clone();
    let mut raw = egui::RawInput {
        hovered_files: vec![egui::HoveredFile::default()],
        dropped_files: vec![crate::native_drag::test_hooks::dropped(
            std::path::Path::new("/tmp/x"),
        )],
        ..Default::default()
    };
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    assert!(raw.hovered_files.is_empty() && raw.dropped_files.is_empty());

    // A click on the dimmed backdrop over the list selects nothing.
    click(&mut h, row(1), PointerButton::Primary);
    assert_eq!(cursor(&mut h), None, "the click doesn't reach the list");

    // Esc closes the dialog.
    if app(&mut h).dialog.is_some() {
        h.key_press(Key::Escape);
        h.run_steps(3);
    }
    assert!(app(&mut h).dialog.is_none(), "Esc closes it");
    assert!(!app(&mut h).modal_open());
}

#[test]
fn outside_click_closes_a_context_menu_without_selecting_the_row_under_it() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    clear_selection(&mut h);
    click(&mut h, row(0), PointerButton::Secondary);
    h.run_steps(2);
    assert!(egui::Popup::is_any_open(&h.ctx), "the context menu is open");
    let before = cursor(&mut h);

    // Below the menu (it opens at the pointer, to the right): row 8.
    click(&mut h, egui::pos2(220.0, row(8).y), PointerButton::Primary);
    assert!(!egui::Popup::is_any_open(&h.ctx), "the menu closed");
    assert_eq!(
        cursor(&mut h),
        before,
        "the click only closed the menu, it selected nothing"
    );

    // The next click works as usual.
    click(&mut h, egui::pos2(220.0, row(8).y), PointerButton::Primary);
    assert_ne!(cursor(&mut h), before, "the next click selects");
}

/// German, light: the main window dimmed behind Settings (and behind
/// Software Update). `$MANTA_RENDER_DIR/modal/`.
#[test]
fn render_settings_modal() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("modal");
    let mut h = harness_de(&fx, WIDE, Theme::Light);
    select(&mut h, Side::Left, &["README.md"]);
    app(&mut h)
        .settings
        .show(Some(crate::settings::SettingsTab::General));
    h.run_steps(4);
    save_to(&mut h, &dir, "settings_modal_de_light");
    // A click into the dimmed main window: nothing happens there.
    click(&mut h, row(3), PointerButton::Primary);
    save_to(&mut h, &dir, "settings_modal_after_click_de_light");
    app(&mut h).settings.open = false;
    app(&mut h).updates.show();
    h.run_steps(4);
    save_to(&mut h, &dir, "update_modal_de_light");
    crate::i18n::set_locale(crate::i18n::Locale::En);
}
