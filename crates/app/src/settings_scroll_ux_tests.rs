//! Settings: the toolbar tabs stay a fixed header while the page scrolls
//! (the scroll area's clip and its top edge fade start below the header's
//! separator). `render_settings_scroll` writes light/dark PNGs, scrolled and
//! not, to `$MANTA_RENDER_DIR/settings-scroll`.

use eframe::egui::{self, Theme};

use super::*;
use crate::i18n::Locale;
use crate::settings::SettingsTab;

const TABS: [(SettingsTab, &str); 2] = [
    (SettingsTab::General, "general"),
    (SettingsTab::Plugins, "plugins"),
];
const LOCALES: [(Locale, &str); 4] = [
    (Locale::De, "de"),
    (Locale::En, "en"),
    (Locale::Fr, "fr"),
    (Locale::Ru, "ru"),
];

/// Open Settings on `tab`, scrolled to `y`; the tab bar and the scroll
/// area's visible (clip / fade) rect.
fn layout(h: &mut H, tab: SettingsTab, y: f32) -> (egui::Rect, egui::Rect) {
    app(h).settings.show(Some(tab));
    app(h).settings.scroll_to = Some(y);
    h.run_steps(4);
    app(h).settings.last_layout.expect("settings drawn")
}

#[test]
fn scrolled_page_never_overlaps_the_tab_bar() {
    let fx = fixture();
    for (locale, lname) in LOCALES {
        let mut h = harness_with(&fx, WIDE, Theme::Light, false);
        crate::i18n::set_locale(locale);
        for (tab, tname) in TABS {
            for y in [0.0, 300.0, 10_000.0] {
                let (bar, page) = layout(&mut h, tab, y);
                // The scroll area clips its content to `page` and paints its
                // top fade from `page.top()` down: both below the separator.
                assert!(
                    page.top() >= bar.bottom(),
                    "{lname}/{tname} scrolled {y}: page {page:?} reaches into tab bar {bar:?}"
                );
                assert!(
                    page.height() > 100.0,
                    "{lname}/{tname}: page has room ({page:?})"
                );
            }
        }
    }
    crate::i18n::set_thread_locale(None);
}

/// With a GPU: the tab bar's pixels are the same scrolled or not.
#[test]
fn render_settings_scroll() {
    require_gpu!();
    let fx = fixture();
    let out = render_dir("settings-scroll");
    for (locale, lname) in LOCALES {
        for (theme, thname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            let mut h = harness(&fx, WIDE, theme);
            crate::i18n::set_locale(locale);
            for (tab, tname) in TABS {
                let (bar, _) = layout(&mut h, tab, 0.0);
                h.run_steps(3);
                let top = h.render().expect("render");
                top.save(out.join(format!("settings_{tname}_top_{lname}_{thname}.png")))
                    .unwrap();
                layout(&mut h, tab, 300.0);
                h.run_steps(3);
                let scrolled = h.render().expect("render");
                scrolled
                    .save(out.join(format!("settings_{tname}_scrolled_{lname}_{thname}.png")))
                    .unwrap();
                // Tab icons and labels (the bar above its separator).
                let ppp = 2.0;
                let (x0, x1) = ((bar.left() * ppp) as u32, (bar.right() * ppp) as u32);
                let (y0, y1) = (
                    (bar.top() * ppp) as u32,
                    ((bar.bottom() - 1.0) * ppp) as u32,
                );
                let differing = (y0..y1)
                    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                    .filter(|&(x, y)| top.get_pixel(x, y) != scrolled.get_pixel(x, y))
                    .count();
                assert_eq!(
                    differing, 0,
                    "{lname}/{tname}/{thname}: scrolling changed {differing} tab bar pixels"
                );
            }
            app(&mut h).settings.open = false;
        }
    }
    crate::i18n::set_thread_locale(None);
}
