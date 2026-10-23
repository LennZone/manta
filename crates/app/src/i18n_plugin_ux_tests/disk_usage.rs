//! German renders: the Disk Usage panel of the Folder Sizes plugin in the
//! app (see mod.rs).

use eframe::egui::{self, Theme};

use super::super::*;
use crate::i18n::{self, Locale};

#[test]
fn render_disk_usage_de_in_app() {
    require_gpu!();
    let fx = fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness_full(
            &fx,
            WIDE,
            theme,
            true,
            Box::new(|ctx: &egui::Context| {
                PluginHost::new(
                    ctx.clone(),
                    vec![Box::new(manta_plugin_foldersize::FolderSizePlugin::new())],
                    None,
                )
            }),
        );
        i18n::set_locale(Locale::De);
        // Panel titles were taken in English while the harness started.
        app(&mut h).plugins.set_enabled("manta.foldersize", false);
        app(&mut h).plugins.set_enabled("manta.foldersize", true);
        app(&mut h).plugins.set_panel_open(
            "manta.foldersize",
            manta_plugin_foldersize::PANEL_ID,
            None,
            true,
        );
        for _ in 0..80 {
            h.step();
            std::thread::sleep(Duration::from_millis(15));
        }
        save(&mut h, &format!("disk_usage_de_app_{tname}"));
        i18n::set_thread_locale(None);
    }
}
