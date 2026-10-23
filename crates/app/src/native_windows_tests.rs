//! Settings, Compare & Sync and the preview as native windows.
//!
//! The test harness can't create native windows (`ctx.embed_viewports()` is
//! true there), so these run the same code in its fallback mode, where the
//! window is an in-window egui window and keyboard focus is emulated (see
//! `aux_window`). The native window *content* is also rendered on its own,
//! in a harness the size of the window. Child of `ux_tests`.

use eframe::egui::{self, Key, Modifiers, Theme};
use egui_kittest::kittest::Queryable;

use super::*;
use crate::aux_window::{embedded_key, AuxKind};
use crate::i18n::{self, Locale};
use crate::prefs::{Appearance, Prefs};
use crate::settings::{SettingsOutcome, SettingsTab, SettingsView};

fn behaviour_harness(fx: &Fixture) -> H {
    let mut h = harness_with(fx, WIDE, Theme::Light, false);
    i18n::set_locale(Locale::En);
    h.run_steps(2);
    h
}

fn open_settings(h: &mut H) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, super::super::Action::ShowSettings);
    h.run_steps(3);
}

/// Click somewhere in the main window outside the Settings window (the
/// status bar), which gives the main window the keyboard back.
fn click_main(h: &mut H) {
    let pos = egui::pos2(40.0, WIDE[1] - 10.0);
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

#[test]
fn settings_opens_as_key_window_and_cmd_w_closes_only_it() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    // A second tab, so ⌘W *could* close one.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    let tabs = app(&mut h).pane(Side::Left).tabs.len();
    assert_eq!(tabs, 2);

    open_settings(&mut h);
    assert!(app(&mut h).settings.open);
    assert_eq!(
        embedded_key(&h.ctx),
        Some(AuxKind::Settings),
        "Settings is the key window"
    );

    // Esc does not close it (macOS windows close with ⌘W, not Esc).
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(app(&mut h).settings.open, "Esc keeps Settings open");

    // ⌘W closes Settings, not the tab; the app keeps running.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert!(!app(&mut h).settings.open, "⌘W closes Settings");
    assert_eq!(
        app(&mut h).pane(Side::Left).tabs.len(),
        tabs,
        "the tab stays"
    );
    assert_eq!(embedded_key(&h.ctx), None);

    // Back in the main window, ⌘W closes the tab as before.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert_eq!(
        app(&mut h).pane(Side::Left).tabs.len(),
        tabs - 1,
        "⌘W closes the tab"
    );
}

#[test]
fn cmd_comma_focuses_open_settings() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(3);
    assert!(app(&mut h).settings.open, "⌘, opens Settings");
    assert_eq!(embedded_key(&h.ctx), Some(AuxKind::Settings));

    click_main(&mut h);
    assert!(
        app(&mut h).settings.open,
        "clicking the main window doesn't close it"
    );
    assert_eq!(
        embedded_key(&h.ctx),
        None,
        "the main window has the keyboard"
    );

    // ⌘, again: brought to the front (still one window, now key).
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(3);
    assert!(app(&mut h).settings.open);
    assert_eq!(
        embedded_key(&h.ctx),
        Some(AuxKind::Settings),
        "⌘, focuses Settings"
    );
}

#[test]
fn shortcuts_stay_in_their_window() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    open_settings(&mut h);
    let transfers = app(&mut h).show_transfers;
    let active = app(&mut h).active;
    let tabs = app(&mut h).pane(active).tabs.len();
    // Main-window shortcuts typed into Settings do nothing there.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::L);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.key_press(Key::Tab);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(2);
    let a = app(&mut h);
    assert_eq!(a.show_transfers, transfers, "⌥⌘L stays in Settings");
    assert_eq!(a.pane(active).tabs.len(), tabs, "⌘T stays in Settings");
    assert_eq!(a.active, active, "Tab doesn't switch panes");
    assert!(!a.sync.open, "⇧⌘S doesn't open Compare & Sync");

    // In the main window they work again, and ⌘W there doesn't close Settings.
    click_main(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::L);
    h.run_steps(2);
    assert_ne!(
        app(&mut h).show_transfers,
        transfers,
        "⌥⌘L works in the main window"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert!(
        app(&mut h).settings.open,
        "⌘W in the main window leaves Settings open"
    );
}

#[test]
fn settings_changes_apply_to_the_main_window() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    app(&mut h).settings.show(Some(SettingsTab::General));
    h.run_steps(3);
    assert_eq!(h.ctx.theme(), Theme::Light);

    h.get_by_label("Dark").click();
    h.run_steps(3);
    assert_eq!(app(&mut h).prefs.appearance, Appearance::Dark);
    assert_eq!(
        h.ctx.theme(),
        Theme::Dark,
        "the whole app (all windows) turns dark"
    );

    // Language: pick Deutsch in the combo box.
    h.get_by_value("System").click();
    h.run_steps(2);
    h.get_by_label("Deutsch").click();
    h.run_steps(3);
    assert_eq!(app(&mut h).prefs.language, crate::i18n::Language::German);
    assert_eq!(i18n::locale(), Locale::De);
    assert_eq!(
        t!("settings.title"),
        "Einstellungen",
        "window title follows the language"
    );
    i18n::set_locale(Locale::En);
}

#[test]
fn compare_sync_and_preview_close_with_cmd_w() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(3);
    assert!(app(&mut h).sync.open);
    assert_eq!(embedded_key(&h.ctx), Some(AuxKind::Sync));
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(app(&mut h).sync.open, "Esc keeps Compare & Sync open");
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert!(!app(&mut h).sync.open, "⌘W closes Compare & Sync");

    // The preview opens without taking the keyboard (like Quick Look):
    // Space in the list closes it again; ⌘W once it is key.
    let entry = app(&mut h)
        .pane(Side::Left)
        .tab()
        .entries
        .iter()
        .find(|e| e.name == "README.md")
        .cloned()
        .expect("README");
    app(&mut h).preview = Some(Preview::new(1, entry.clone()));
    h.run_steps(2);
    assert_eq!(
        embedded_key(&h.ctx),
        None,
        "the file list keeps the keyboard"
    );
    app(&mut h).preview_window.request_focus();
    h.run_steps(2);
    assert_eq!(embedded_key(&h.ctx), Some(AuxKind::Preview));
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert!(app(&mut h).preview.is_none(), "⌘W closes the preview");
}

/// The Settings window content as the native window shows it (no egui
/// title bar: macOS draws the title and traffic lights), in a harness the
/// size of the window.
#[test]
fn render_settings_native_content() {
    require_gpu!();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for (tab, tab_name) in [
            (SettingsTab::General, "general"),
            (SettingsTab::Plugins, "plugins"),
        ] {
            struct State {
                view: SettingsView,
                host: Option<PluginHost>,
                prefs: Prefs,
            }
            let mut view = SettingsView::default();
            view.show(Some(tab));
            let mut h = Harness::builder()
                .with_size(egui::vec2(
                    crate::settings::SIZE[0],
                    crate::settings::SIZE[1],
                ))
                .with_pixels_per_point(2.0)
                .with_theme(theme)
                .allow_missing_glyphs()
                // Unlabelled combo boxes / switches in the existing page.
                .with_accessibility_check(false)
                .with_context_setup(|ctx| {
                    ctx.add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                    crate::theme::install(ctx);
                })
                .wgpu()
                .build_ui_state(
                    |ui, s: &mut State| {
                        let host = s
                            .host
                            .get_or_insert_with(|| plugin_host(ui.ctx(), Vec::new()));
                        let p = crate::theme::pal(ui);
                        // The whole window is the content (no harness margin).
                        ui.painter()
                            .rect_filled(ui.ctx().content_rect(), 0.0, p.elevated);
                        egui::CentralPanel::default()
                            .frame(egui::Frame::new().fill(p.elevated))
                            .show(ui, |ui| {
                                let mut out = SettingsOutcome::default();
                                s.view.contents(ui, host, &mut s.prefs, &mut out);
                            });
                    },
                    State {
                        view,
                        host: None,
                        prefs: Prefs::default(),
                    },
                );
            i18n::set_locale(Locale::En);
            h.run_steps(4);
            let img = h.render().expect("render");
            let path = out_dir().join(format!("native_settings_{tab_name}_{tname}.png"));
            img.save(&path).expect("save png");
            eprintln!("ux_tests: wrote {}", path.display());
        }
    }
}
