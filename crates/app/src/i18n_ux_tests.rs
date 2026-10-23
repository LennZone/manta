//! Renders for languages, appearance and accent colours (German and
//! English, light and dark). Child of `ux_tests`, so it reuses the fixture
//! and harness. Written to `$MANTA_RENDER_DIR` like the other renders.

use eframe::egui::{self, Theme};
use egui_kittest::kittest::Queryable;
use manta_core::VfsPath;
use manta_ops::{JobKind, JobState};

use super::*;
use crate::i18n::{self, Locale};
use crate::theme::Accent;

const LOCALES: [(Locale, &str); 2] = [(Locale::De, "de"), (Locale::En, "en")];
const THEMES: [(Theme, &str); 2] = [(Theme::Light, "light"), (Theme::Dark, "dark")];

/// A harness in `locale` (this thread only; see `i18n::set_locale`).
fn localized(fx: &Fixture, size: [f32; 2], theme: Theme, locale: Locale) -> H {
    let mut h = harness(fx, size, theme);
    i18n::set_locale(locale);
    h.run_steps(3);
    h
}

#[test]
fn render_i18n_main_settings_dialogs() {
    require_gpu!();
    let fx = fixture();
    for (locale, lname) in LOCALES {
        for (theme, tname) in THEMES {
            let tag = format!("{lname}_{tname}");
            let mut h = localized(&fx, WIDE, theme, locale);
            select(&mut h, Side::Left, &["README.md", "report-q3.pdf"]);
            save(&mut h, &format!("i18n_main_{tag}"));

            // Settings → General with Appearance and Language.
            app(&mut h)
                .settings
                .show(Some(crate::settings::SettingsTab::General));
            h.run_steps(4);
            save(&mut h, &format!("i18n_settings_general_{tag}"));
            app(&mut h).settings.open = false;

            app(&mut h).dialog = Some(Dialog::Connect(ConnectForm::new(Side::Left)));
            save(&mut h, &format!("i18n_connect_{tag}"));
            app(&mut h).dialog = None;

            let a = app(&mut h);
            let fs = a.local.clone();
            let paths: Vec<VfsPath> = a
                .pane(Side::Left)
                .tab()
                .selected_entries()
                .into_iter()
                .map(|e| e.path)
                .collect();
            let folders = vec![VfsPath::new(&fx.root.join("src").to_string_lossy())];
            a.dialog = Some(Dialog::Delete(DeleteForm {
                side: Side::Left,
                fs,
                paths,
                folders,
                no_trash: false,
                in_trash: false,
            }));
            save(&mut h, &format!("i18n_delete_{tag}"));
            app(&mut h).dialog = None;

            app(&mut h).palette.show();
            h.run_steps(2);
            h.event(egui::Event::Text(
                if locale == Locale::De { "ord" } else { "fol" }.into(),
            ));
            save(&mut h, &format!("i18n_palette_{tag}"));
            app(&mut h).palette = Default::default();
            h.run_steps(2);

            let de = locale == Locale::De;
            let mut failed = job(
                4,
                JobKind::Copy,
                if de {
                    "2 Objekte nach /var/www kopieren"
                } else {
                    "Copy 2 items to /var/www"
                },
                JobState::Failed("permission denied: /var/www".into()),
                0,
                0,
                (0, 2),
            );
            failed.errors = vec!["permission denied: /var/www/index.html".into()];
            let a = app(&mut h);
            a.fake_jobs = Some(vec![
                job(
                    1,
                    JobKind::Copy,
                    if de {
                        "3 Objekte nach deploy@web-01.example.com:/var/www kopieren"
                    } else {
                        "Copy 3 items to deploy@web-01.example.com:/var/www"
                    },
                    JobState::Running,
                    31_200_000,
                    52_000_000,
                    (14, 22),
                ),
                job(
                    2,
                    JobKind::Move,
                    if de {
                        "12 Objekte nach /Users/anna/photos bewegen"
                    } else {
                        "Move 12 items to /Users/anna/photos"
                    },
                    JobState::Queued,
                    0,
                    48_300_000,
                    (0, 12),
                ),
                job(
                    3,
                    JobKind::Delete,
                    if de {
                        "4 Objekte löschen"
                    } else {
                        "Delete 4 items"
                    },
                    JobState::Done,
                    0,
                    0,
                    (4, 4),
                ),
                failed,
            ]);
            a.show_transfers = true;
            save(&mut h, &format!("i18n_transfers_{tag}"));
        }
    }
}

/// One strip per accent colour: the top of the main window with a selection
/// (focused pane), stacked into one image per theme.
#[test]
fn render_accent_strips() {
    require_gpu!();
    let fx = fixture();
    for (theme, tname) in THEMES {
        let mut strips = Vec::new();
        for accent in Accent::ALL {
            let mut h = localized(&fx, [1100.0, 330.0], theme, Locale::De);
            crate::theme::apply_accent(&h.ctx, accent);
            select(&mut h, Side::Left, &["Design", "photos"]);
            h.run_steps(3);
            let img = h.render().expect("render");
            strips.push(img);
        }
        let w = strips[0].width();
        let hgt: u32 = strips.iter().map(|s| s.height()).sum();
        let mut out = image::RgbaImage::new(w, hgt);
        let mut y = 0;
        for s in &strips {
            image::imageops::replace(&mut out, s, 0, y as i64);
            y += s.height();
        }
        let path = out_dir().join(format!("i18n_accents_{tname}.png"));
        out.save(&path).expect("save png");
        eprintln!("ux_tests: wrote {}", path.display());
    }
}

/// Settings with each accent selected, so the swatch row is visible too.
#[test]
fn render_accent_swatches() {
    require_gpu!();
    let fx = fixture();
    for (theme, tname) in THEMES {
        let mut h = localized(&fx, [900.0, 640.0], theme, Locale::De);
        crate::theme::apply_accent(&h.ctx, Accent::Purple);
        app(&mut h).prefs.accent = Accent::Purple;
        app(&mut h)
            .settings
            .show(Some(crate::settings::SettingsTab::General));
        h.run_steps(4);
        save(&mut h, &format!("i18n_settings_purple_{tname}"));
    }
}

/// Key screens in the five translated languages (light): main window,
/// Settings › General, the drop dialog (Copy / Move), the conflict dialog,
/// the delete dialog, the transfers panel and the command palette.
#[test]
fn render_i18n_new_languages() {
    require_gpu!();
    use super::conflict_ux_tests::{file_question, waiting_job};
    use super::native_dnd_ux_tests::{drop_files, empty_spot, outside};
    let fx = fixture();
    let langs = [
        (Locale::Zh, "zh"),
        (Locale::Es, "es"),
        (Locale::Fr, "fr"),
        (Locale::It, "it"),
        (Locale::Ru, "ru"),
    ];
    for (locale, lname) in langs {
        let mut h = localized(&fx, WIDE, Theme::Light, locale);
        // Like `MantaApp::apply_language`: Chinese adds its font.
        h.ctx.set_fonts(crate::theme::font_definitions());
        select(&mut h, Side::Left, &["README.md", "report-q3.pdf"]);
        save(&mut h, &format!("lang_{lname}_main"));

        app(&mut h)
            .settings
            .show(Some(crate::settings::SettingsTab::General));
        h.run_steps(4);
        save(&mut h, &format!("lang_{lname}_settings_general"));
        app(&mut h).settings.open = false;

        let files = outside(&fx, &["Rechnung.pdf", "Foto.jpg"]);
        let pos = empty_spot(&mut h, Side::Right);
        drop_files(&mut h, pos, &files, egui::Modifiers::NONE);
        h.run_steps(3);
        save(&mut h, &format!("lang_{lname}_drop_dialog"));
        app(&mut h).dialog = None;

        app(&mut h).fake_jobs = Some(vec![waiting_job(
            1,
            file_question(),
            "Copying 9 items to /Volumes/Backup/Design",
        )]);
        h.run_steps(3);
        save(&mut h, &format!("lang_{lname}_conflict"));
        app(&mut h).dialog = None;
        app(&mut h).fake_jobs = None;
        h.run_steps(2);

        let a = app(&mut h);
        let fs = a.local.clone();
        let paths: Vec<VfsPath> = a
            .pane(Side::Left)
            .tab()
            .selected_entries()
            .into_iter()
            .map(|e| e.path)
            .collect();
        let folders = vec![VfsPath::new(&fx.root.join("src").to_string_lossy())];
        a.dialog = Some(Dialog::Delete(DeleteForm {
            side: Side::Left,
            fs,
            paths,
            folders,
            no_trash: false,
            in_trash: false,
        }));
        save(&mut h, &format!("lang_{lname}_delete"));
        app(&mut h).dialog = None;

        app(&mut h).palette.show();
        h.run_steps(2);
        save(&mut h, &format!("lang_{lname}_palette"));
        app(&mut h).palette = Default::default();
        h.run_steps(2);

        let mut failed = job(
            4,
            JobKind::Copy,
            "Copying 2 items to /var/www",
            JobState::Failed("permission denied: /var/www".into()),
            0,
            0,
            (0, 2),
        );
        failed.errors = vec!["permission denied: /var/www/index.html".into()];
        let a = app(&mut h);
        a.fake_jobs = Some(vec![
            job(
                1,
                JobKind::Copy,
                "Copying 3 items to deploy@web-01.example.com:/var/www",
                JobState::Running,
                31_200_000,
                52_000_000,
                (14, 22),
            ),
            job(
                2,
                JobKind::Move,
                "Moving 12 items to /Users/anna/photos",
                JobState::Queued,
                0,
                48_300_000,
                (0, 12),
            ),
            job(
                3,
                JobKind::Delete,
                "Deleting 4 items",
                JobState::Done,
                0,
                0,
                (4, 4),
            ),
            failed,
        ]);
        a.show_transfers = true;
        save(&mut h, &format!("lang_{lname}_transfers"));
        i18n::set_locale(Locale::En);
    }
}

/// Settings › About Manta (website link), the "What's new" notice and a
/// delete dialog for exactly one file (its name, not "1 item"), light.
#[test]
fn render_i18n_about_whats_new_single() {
    require_gpu!();
    let fx = fixture();
    let langs = [
        (Locale::De, "de"),
        (Locale::En, "en"),
        (Locale::Zh, "zh"),
        (Locale::Fr, "fr"),
        (Locale::Ru, "ru"),
    ];
    for (locale, lname) in langs {
        let mut h = localized(&fx, WIDE, Theme::Light, locale);
        h.ctx.set_fonts(crate::theme::font_definitions());
        h.run_steps(2);

        if locale != Locale::En {
            app(&mut h)
                .settings
                .show(Some(crate::settings::SettingsTab::General));
            h.run_steps(4);
            h.get_by_label(crate::help::WEBSITE_LABEL).scroll_to_me();
            h.run_steps(4);
            save(&mut h, &format!("about_{lname}"));
            app(&mut h).settings.open = false;
            h.run_steps(2);
        }

        app(&mut h).dialog = Some(Dialog::WhatsNew(crate::dialogs::WhatsNewForm));
        save(&mut h, &format!("whats_new_{lname}"));
        app(&mut h).dialog = None;

        if matches!(locale, Locale::Zh | Locale::Fr | Locale::Ru) {
            select(&mut h, Side::Left, &["report-q3.pdf"]);
            let a = app(&mut h);
            let fs = a.local.clone();
            let paths: Vec<VfsPath> = a
                .pane(Side::Left)
                .tab()
                .selected_entries()
                .into_iter()
                .map(|e| e.path)
                .collect();
            assert_eq!(paths.len(), 1);
            a.dialog = Some(Dialog::Delete(DeleteForm {
                side: Side::Left,
                fs,
                paths,
                folders: Vec::new(),
                no_trash: true,
                in_trash: false,
            }));
            save(&mut h, &format!("delete_single_{lname}"));
            app(&mut h).dialog = None;
        }
        i18n::set_locale(Locale::En);
    }
}
