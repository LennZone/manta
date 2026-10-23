//! German renders: the Batch Rename sheet (see mod.rs). English renders
//! live in `ux_tests.rs` (`render_batch_rename*`).

use eframe::egui::{self, Theme};
use manta_plugin_rename::rules::{RuleKind, RuleType};

use super::super::*;
use crate::i18n::{self, Locale};

const THEMES: [(Theme, &str); 2] = [(Theme::Light, "light"), (Theme::Dark, "dark")];

/// Every rule type once, so each card's German labels show up.
fn all_rule_cards() -> Vec<manta_plugin_rename::rules::Rule> {
    RuleType::ALL
        .iter()
        .map(|t| {
            let mut r = t.new_rule();
            if let RuleKind::Insert { text, .. } = &mut r.kind {
                *text = "Island ".into();
            }
            r
        })
        .collect()
}

/// Switch the harness's thread to German after the app set its own
/// language, and tell the plugin (the host would on a real switch).
fn german((fx, mut h, plugin): (Fixture, H, SharedRename)) -> (Fixture, H, SharedRename) {
    i18n::set_locale(Locale::De);
    plugin
        .lock()
        .unwrap()
        .on_event(&manta_plugin::PluginEvent::LocaleChanged("de".into()));
    h.run_steps(3);
    (fx, h, plugin)
}

#[test]
fn render_batch_rename_german() {
    require_gpu!();
    for (theme, tname) in THEMES {
        let (fx, mut h, plugin) = german(rename_sheet(theme));
        save(&mut h, &format!("rename_sheet_de_{tname}"));
        // The command title, as the palette shows it.
        let labels: Vec<String> = app(&mut h)
            .palette_entries()
            .iter()
            .map(|e| e.label.to_string())
            .collect();
        assert!(
            labels.iter().any(|l| l == "Stapelweise umbenennen …"),
            "{labels:?}"
        );

        // Every rule card: the first four, then the last four (the list scrolls).
        let cards = all_rule_cards();
        for (part, range) in [("a", 0..4), ("b", 4..8)] {
            plugin.lock().unwrap().rules = cards[range].to_vec();
            h.run_steps(3);
            save(&mut h, &format!("rename_rules_{part}_de_{tname}"));
        }

        // Presets menu.
        {
            let mut p = plugin.lock().unwrap();
            p.presets.items.clear();
            p.presets.upsert("Kamera-Importe", photo_rules());
            p.presets.upsert(
                "Scans mit Datum",
                vec![RuleType::Date.new_rule(), RuleType::Clean.new_rule()],
            );
            p.ui.open_presets = true;
            p.ui.preset_name = "Kamera-Importe".into();
        }
        h.run_steps(4);
        save(&mut h, &format!("rename_presets_de_{tname}"));

        drop(h);
        drop(fx);

        // Apply (conflict resolved) → German toast with "Widerrufen".
        let (fx, mut h, plugin) = german(rename_sheet(theme));
        std::fs::remove_file(fx.root.join("Iceland-42.jpg")).unwrap();
        {
            let mut p = plugin.lock().unwrap();
            let s = p.session.as_mut().unwrap();
            s.all_names.retain(|n| n != "Iceland-42.jpg");
            let pane = s.pane;
            let dir = s.dir.clone();
            let entries = s
                .listing
                .clone()
                .unwrap()
                .unwrap()
                .into_iter()
                .filter(|e| e.name != "Iceland-42.jpg")
                .collect();
            p.on_listing(&manta_plugin::DirListing {
                pane,
                dir,
                entries,
                error: None,
            });
        }
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        wait_until(&mut h, "the toast", |h| {
            h.state()
                .toasts
                .actions()
                .iter()
                .any(|(l, _)| l == "Widerrufen")
        });
        settle(&mut h);
        save(&mut h, &format!("rename_done_de_{tname}"));
        i18n::set_thread_locale(None);
    }
}

/// French and Russian (light): the sheet and every rule card, to check the
/// longer labels fit.
#[test]
fn render_batch_rename_fr_ru() {
    require_gpu!();
    for (locale, code) in [(Locale::Fr, "fr"), (Locale::Ru, "ru")] {
        let (_fx, mut h, plugin) = rename_sheet(Theme::Light);
        i18n::set_locale(locale);
        plugin
            .lock()
            .unwrap()
            .on_event(&manta_plugin::PluginEvent::LocaleChanged(code.into()));
        h.run_steps(3);
        save(&mut h, &format!("rename_sheet_{code}"));
        let cards = all_rule_cards();
        for (part, range) in [("a", 0..4), ("b", 4..8)] {
            plugin.lock().unwrap().rules = cards[range].to_vec();
            h.run_steps(3);
            save(&mut h, &format!("rename_rules_{part}_{code}"));
        }
        i18n::set_thread_locale(None);
    }
}
