//! German renders: Quick Actions (see mod.rs). The built-in plugin in
//! German: the context menu with the actions submenu, the panel with runs,
//! the toasts, the confirm sheet and the replaced Python copy in Settings.

use eframe::egui::{self, Theme};

use super::super::*;
use crate::i18n::{self, Locale};

#[test]
fn render_quick_actions_german() {
    use egui_kittest::kittest::Queryable;
    require_gpu!();
    let fx = fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let root = tempfile::tempdir().unwrap();
        let mut h = qa_harness(&fx, theme, root.path());
        // The app starts in its preferred language (English under test):
        // switch this harness to German and tell the plugin, as the
        // Language setting does (without touching the process-wide locale
        // other tests read).
        i18n::set_locale(Locale::De);
        app(&mut h)
            .plugins
            .deliver(QA, manta_plugin::PluginEvent::LocaleChanged("de".into()));
        app(&mut h).plugins.refresh_infos();
        wait_until(&mut h, "German titles", |h| {
            h.state()
                .plugins
                .commands()
                .iter()
                .any(|c| c.spec.title == "Zeilen zählen")
        });
        add_demo_actions(root.path(), &mut h);
        app(&mut h).toasts.clear();
        let readme = app(&mut h).pane_state(Side::Left);

        // Context menu on README.md with the Quick Actions submenu open.
        let pos = egui::pos2(330.0, readme_row_y(&mut h));
        h.hover_at(pos);
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(3);
        // The submenu (its label is the plugin's German name).
        h.get_by_label(&format!("{}   Schnellaktionen", crate::icons::PUZZLE_PIECE))
            .hover();
        h.run_steps(4);
        save(&mut h, &format!("quick_actions_menu_de_{tname}"));
        h.key_press(egui::Key::Escape);
        h.run_steps(3);

        // Panel with runs: done, failed, running.
        app(&mut h)
            .plugins
            .set_panel_open(QA, "actions", None, true);
        for id in ["count-lines", "sha256", "lint", "backup"] {
            app(&mut h)
                .plugins
                .run_command(&format!("{QA}:run.{id}"), &readme);
            std::thread::sleep(Duration::from_millis(150));
            h.step();
        }
        wait_until(&mut h, "runs in the panel", |h| {
            qa_panel_has(h, "Zeilen zählen")
                && qa_panel_has(h, "fehlgeschlagen")
                && qa_panel_has(h, "läuft")
        });
        app(&mut h)
            .plugins
            .panel_action(QA, "actions", "show:2", &readme);
        wait_until(&mut h, "sha output", |h| qa_panel_has(h, "$ shasum"));
        std::thread::sleep(Duration::from_millis(1200));
        h.run_steps(6);
        // The toasts the runs raised get their own render; keep the panel clear.
        let toasts = std::mem::take(&mut app(&mut h).toasts);
        h.run_steps(2);
        save(&mut h, &format!("quick_actions_panel_de_{tname}"));
        app(&mut h).toasts = toasts;
        h.run_steps(2);
        save(&mut h, &format!("quick_actions_toasts_de_{tname}"));

        // The confirmation sheet of the rsync action.
        app(&mut h).toasts.clear();
        app(&mut h)
            .plugins
            .run_command(&format!("{QA}:run.rsync-other"), &readme);
        wait_until(&mut h, "confirm sheet", |h| {
            h.state().plugins.is_panel_open(QA, "confirm", None)
        });
        h.run_steps(6);
        save(&mut h, &format!("quick_actions_confirm_de_{tname}"));
        app(&mut h)
            .plugins
            .set_panel_open(QA, "confirm", None, false);

        // Settings with the replaced Python copy.
        let a = app(&mut h);
        a.plugins.set_panel_open(QA, "actions", None, false);
        a.settings.show(Some(crate::settings::SettingsTab::Plugins));
        a.settings.scroll_to = Some(1000.0);
        h.run_steps(6);
        save(&mut h, &format!("quick_actions_settings_de_{tname}"));
        app(&mut h).plugins.shutdown(Duration::from_secs(1));
        i18n::set_thread_locale(None);
    }
}
