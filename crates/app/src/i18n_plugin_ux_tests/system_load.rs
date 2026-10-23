//! The System Load plugin's status bar items in the app: values from a
//! fake sampler (never the real machine), hiding when the plugin is
//! switched off, narrow windows, the click, and German / Russian renders
//! (see mod.rs).

use eframe::egui::{self, Theme};
use egui_kittest::kittest::Queryable;
use manta_plugin_sysstats::{FakeSampler, MemInfo, Pressure, SysStatsPlugin, PLUGIN_ID};

use super::super::*;
use crate::i18n::{self, Locale};

const G: u64 = 1 << 30;

fn plugin(cpu: f32, gpu: Option<f32>) -> Box<dyn FnOnce(&egui::Context) -> PluginHost> {
    Box::new(move |ctx: &egui::Context| {
        let mem = MemInfo {
            total: 16 * G,
            app: 124 * G / 10 - 3 * G,
            wired: 2 * G,
            compressed: G,
            swap_used: Some(G / 2),
            pressure: Some(Pressure::Normal),
        };
        PluginHost::new(
            ctx.clone(),
            vec![Box::new(SysStatsPlugin::with_sampler(
                Box::new(move || Box::new(FakeSampler::new(cpu, gpu, Some(mem)))),
                Duration::from_millis(40),
            ))],
            None,
        )
    })
}

/// Step until the plugin's items arrived (and a few frames more).
fn wait_for_items(h: &mut H) {
    let start = Instant::now();
    while app(h).plugins.status_items().is_empty() && start.elapsed() < Duration::from_secs(5) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(4);
}

/// The plugin was started before the test switched the language: start it
/// again so its thread formats in that language.
fn restart(h: &mut H) {
    app(h).plugins.set_enabled(PLUGIN_ID, false);
    app(h).plugins.set_enabled(PLUGIN_ID, true);
    // The first items of the new thread replace the old ones.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(300) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    wait_for_items(h);
}

fn shown(h: &H, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

#[test]
fn status_bar_shows_fake_loads_and_hides_when_switched_off() {
    let fx = fixture();
    let mut h = harness_full(&fx, WIDE, Theme::Light, false, plugin(0.42, Some(0.03)));
    wait_for_items(&mut h);
    for label in [
        "CPU 42%",
        "GPU 3%",
        "RAM 12.4 / 16 GB",
        "⇧⌘P  Command Palette",
    ] {
        assert!(shown(&h, label), "{label} in the status bar");
    }
    // A click opens Activity Monitor (recorded in tests, not run).
    h.get_by_label("CPU 42%").click();
    h.run_steps(2);
    assert_eq!(
        app(&mut h).launched.last().cloned(),
        Some(vec!["-b".to_string(), "com.apple.ActivityMonitor".into()])
    );
    // Switched off in Settings › Plugins: gone, the hint stays.
    app(&mut h).plugins.set_enabled(PLUGIN_ID, false);
    h.run_steps(4);
    assert!(!shown(&h, "CPU 42%") && !shown(&h, "RAM 12.4 / 16 GB"));
    assert!(shown(&h, "⇧⌘P  Command Palette"));
    // On again: back.
    app(&mut h).plugins.set_enabled(PLUGIN_ID, true);
    wait_for_items(&mut h);
    assert!(shown(&h, "CPU 42%"));
}

#[test]
fn narrow_windows_drop_the_gpu_then_shrink() {
    let fx = fixture();
    let mut h = harness_full(&fx, WIDE, Theme::Light, false, plugin(0.42, Some(0.03)));
    wait_for_items(&mut h);
    assert!(shown(&h, "GPU 3%"));
    // Shrink the window step by step and note the width at which each
    // part goes: the palette hint first (shortcut only, then gone), then
    // the GPU; CPU and RAM stay (as icon and meter, labelled with the full
    // text for the tooltip and accessibility) down to a very narrow window.
    let mut hint_gone = None;
    let mut gpu_gone = None;
    let mut w = 1280.0;
    while w >= 330.0 {
        h.set_size(egui::vec2(w, 600.0));
        h.run_steps(3);
        let hint = shown(&h, "⇧⌘P  Command Palette") || shown(&h, "⇧⌘P");
        if !hint && hint_gone.is_none() {
            hint_gone = Some(w);
        }
        if !shown(&h, "GPU 3%") && gpu_gone.is_none() {
            gpu_gone = Some(w);
            assert!(
                shown(&h, "CPU 42%") && shown(&h, "RAM 12.4 / 16 GB"),
                "CPU and RAM stay at {w}"
            );
        }
        w -= 10.0;
    }
    let (hint_gone, gpu_gone) = (hint_gone.expect("hint goes"), gpu_gone.expect("GPU goes"));
    assert!(
        hint_gone > gpu_gone,
        "hint at {hint_gone}, GPU at {gpu_gone}"
    );
    assert!(shown(&h, "CPU 42%"), "CPU stays as icon and meter at 330");
}

#[test]
fn render_system_load_status_bar() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("sysstats");
    let size = [1100.0, 420.0];
    for (locale, lname, theme, tname) in [
        (Locale::De, "de", Theme::Light, "light"),
        (Locale::De, "de", Theme::Dark, "dark"),
        (Locale::Ru, "ru", Theme::Light, "light"),
    ] {
        let mut h = harness_full(&fx, size, theme, true, plugin(0.23, Some(0.91)));
        i18n::set_locale(locale);
        restart(&mut h);
        h.run_steps(3);
        let img = h.render().expect("render");
        img.save(dir.join(format!("status_{lname}_{tname}.png")))
            .expect("save");
        // The status bar alone (24 pt at 2×).
        let bar = image::imageops::crop_imm(&img, 0, img.height() - 64, img.width(), 64).to_image();
        bar.save(dir.join(format!("status_bar_{lname}_{tname}.png")))
            .expect("save");
        if lname == "de" && tname == "light" {
            // Narrow: GPU and hint gone; then icons only.
            for (w, name) in [(640.0, "narrow"), (380.0, "narrowest")] {
                h.set_size(egui::vec2(w, size[1]));
                h.run_steps(4);
                let img = h.render().expect("render");
                let bar = image::imageops::crop_imm(&img, 0, img.height() - 64, img.width(), 64)
                    .to_image();
                bar.save(dir.join(format!("status_bar_de_{name}.png")))
                    .expect("save");
            }
            // The tooltip.
            h.set_size(egui::vec2(size[0], size[1]));
            h.run_steps(4);
            h.get_by_label("RAM 12,4 / 16 GB").hover();
            for _ in 0..40 {
                h.step();
                std::thread::sleep(Duration::from_millis(15));
            }
            save_to(&mut h, &dir, "status_tooltip_de_light");
        }
        i18n::set_thread_locale(None);
    }
}
