//! German renders: the Markdown preview's empty state in the app (see
//! mod.rs).

use eframe::egui::{self, Theme};

use super::super::*;
use crate::i18n::{self, Locale};

#[test]
fn render_markdown_empty_de_in_app() {
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
                    vec![Box::new(manta_plugin_markdown::MarkdownPlugin::new())],
                    None,
                )
            }),
        );
        i18n::set_locale(Locale::De);
        // Panel titles were taken in English while the harness started.
        app(&mut h)
            .plugins
            .set_enabled(manta_plugin_markdown::PLUGIN_ID, false);
        app(&mut h)
            .plugins
            .set_enabled(manta_plugin_markdown::PLUGIN_ID, true);
        select(&mut h, Side::Left, &["report-q3.pdf"]);
        app(&mut h).plugins.set_panel_open(
            manta_plugin_markdown::PLUGIN_ID,
            "markdown",
            None,
            true,
        );
        for _ in 0..40 {
            h.step();
            std::thread::sleep(Duration::from_millis(15));
        }
        save(&mut h, &format!("markdown_de_app_{tname}"));
        i18n::set_thread_locale(None);
    }
}
