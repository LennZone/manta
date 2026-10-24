//! The Help window: opening it (⌘?, the palette, "?" buttons), search,
//! links between chapters with Back / Forward, and the language fallback.
//! `render_help` writes German light/dark PNGs to `$MANTA_RENDER_DIR/help`.

use eframe::egui::{self, Key, Modifiers, Theme};
use egui_kittest::kittest::Queryable;
use manta_plugin_markdown::links::{self, Target};
use manta_plugin_markdown::render::Action as MdAction;

use super::{app, fixture, harness, harness_with, render_dir, save_to, wgpu_available, H, WIDE};
use crate::app::Action;
use crate::help::{manual, HelpOutcome, Page};
use crate::i18n::{set_locale, Locale};

fn page(h: &mut H) -> Page {
    app(h).help.loc.page.clone()
}

fn chapter(file: &str) -> Page {
    Page::Chapter(file.into())
}

fn english(h: &mut H) {
    set_locale(Locale::En);
    h.run_steps(2);
}

#[test]
fn cmd_question_mark_opens_help() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    english(&mut h);
    assert!(!app(&mut h).help.open);
    // ⇧⌘/ on a US keyboard, ⇧⌘ß on a German one: egui reports "?".
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Questionmark);
    h.run_steps(3);
    assert!(app(&mut h).help.open, "⌘? opens the manual");
    assert_eq!(
        page(&mut h),
        chapter("01-erste-schritte.md"),
        "starts at the first chapter"
    );
    assert!(
        // (The contents entry and the page's own heading.)
        h.query_all_by_label("Getting Started").next().is_some(),
        "TOC in English"
    );

    // ⌘W closes it; ⇧⌘/ (layouts that report the physical key) reopens.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(3);
    assert!(!app(&mut h).help.open, "⌘W closes the help window");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Slash);
    h.run_steps(3);
    assert!(app(&mut h).help.open);
}

#[test]
fn palette_opens_the_manual() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    set_locale(Locale::De);
    h.run_steps(2);
    app(&mut h).palette.show();
    h.run_steps(2);
    h.event(egui::Event::Text("handbuch".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(app(&mut h).help.open, "palette command opens the manual");
    // German UI: the German manual.
    assert!(h.query_all_by_label("Erste Schritte").next().is_some());
    assert!(h.query_all_by_label("Getting Started").next().is_none());
    set_locale(Locale::En);
}

/// Type `query` into the palette and run the best match.
fn run_palette(h: &mut H, query: &str) {
    app(h).palette.show();
    h.run_steps(2);
    h.event(egui::Event::Text(query.into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

/// "Manta-Website & Support …" hands the website to the browser
/// (recorded in tests, nothing is opened) and nothing else.
#[test]
fn palette_support_opens_the_website() {
    for query in ["support", "kontakt"] {
        let fx = fixture();
        let mut h = harness_with(&fx, WIDE, Theme::Light, false);
        set_locale(Locale::De);
        h.run_steps(2);
        run_palette(&mut h, query);
        let a = app(&mut h);
        assert_eq!(
            a.opened_links,
            vec![crate::help::WEBSITE_URL.to_string()],
            "{query}"
        );
        assert_eq!(crate::help::WEBSITE_URL, "https://manta-filemanager.com/");
        assert!(!a.help.open);
        assert!(!a.palette.open);
    }
    set_locale(Locale::En);
}

/// "Quellcode auf GitHub …" / "Source Code on GitHub…" opens the repository.
#[test]
fn palette_source_code_opens_the_repository() {
    for (locale, query) in [(Locale::En, "github"), (Locale::De, "quellcode")] {
        let fx = fixture();
        let mut h = harness_with(&fx, WIDE, Theme::Light, false);
        set_locale(locale);
        h.run_steps(2);
        run_palette(&mut h, query);
        let a = app(&mut h);
        assert_eq!(
            a.opened_links,
            vec![crate::help::SOURCE_URL.to_string()],
            "{query}"
        );
        assert!(crate::help::SOURCE_URL.starts_with("https://github.com/"));
        assert!(!a.palette.open);
    }
    set_locale(Locale::En);
}

/// "Fehler melden …" / "Report an Issue…" opens GitHub's bug report form,
/// prefilled with the Manta version, the macOS version and the chip
/// (recorded in tests, no browser).
#[test]
fn palette_report_issue_opens_the_prefilled_bug_form() {
    for (locale, query) in [
        (Locale::En, "report an issue"),
        (Locale::En, "bug"),
        (Locale::De, "fehler melden"),
        (Locale::De, "problem"),
    ] {
        let fx = fixture();
        let mut h = harness_with(&fx, WIDE, Theme::Light, false);
        set_locale(locale);
        h.run_steps(2);
        run_palette(&mut h, query);
        let a = app(&mut h);
        assert_eq!(a.opened_links.len(), 1, "{query}: {:?}", a.opened_links);
        let url = &a.opened_links[0];
        assert!(
            url.starts_with(
                "https://github.com/web-workers-com/manta/issues/new?template=bug_report.yml&"
            ),
            "{query}: {url}"
        );
        let version = format!("&manta-version={}&", env!("CARGO_PKG_VERSION"));
        assert!(url.contains(&version), "{url}");
        assert!(url.contains("&macos-version="), "{url}");
        assert!(url.contains("&mac-model="), "{url}");
        assert!(!url.contains(' '), "encoded: {url}");
        assert_eq!(
            url,
            &crate::feedback::current_issue_url(crate::feedback::Kind::Bug)
        );
        assert!(!a.palette.open);
        assert!(!a.help.open);
    }
    set_locale(Locale::En);
}

/// "Idee vorschlagen …" / "Suggest an Idea…" opens the feature request form.
#[test]
fn palette_suggest_idea_opens_the_feature_form() {
    for (locale, query) in [(Locale::En, "idea"), (Locale::De, "idee")] {
        let fx = fixture();
        let mut h = harness_with(&fx, WIDE, Theme::Light, false);
        set_locale(locale);
        h.run_steps(2);
        run_palette(&mut h, query);
        assert_eq!(
            app(&mut h).opened_links,
            vec![
                "https://github.com/web-workers-com/manta/issues/new?template=feature_request.yml"
                    .to_string()
            ],
            "{query}"
        );
    }
    set_locale(Locale::En);
}

/// The Help menu lists both (and the log file); the menu bar runs them
/// like the palette.
#[test]
fn help_menu_feedback_items_run_as_commands() {
    use crate::commands::Cmd;
    assert!(crate::app_menu::ELSEWHERE.contains(&Cmd::ReportIssue));
    assert!(crate::app_menu::ELSEWHERE.contains(&Cmd::SuggestIdea));
    set_locale(Locale::De);
    assert_eq!(Cmd::ReportIssue.label(), "Fehler melden …");
    assert_eq!(Cmd::SuggestIdea.label(), "Idee vorschlagen …");
    set_locale(Locale::En);
    assert_eq!(Cmd::ReportIssue.label(), "Report an Issue…");
    assert_eq!(Cmd::SuggestIdea.label(), "Suggest an Idea…");
    for c in [Cmd::ReportIssue, Cmd::SuggestIdea] {
        assert_eq!(c.shortcut(), "");
        for kw in ["feedback", "github"] {
            assert!(c.keywords().contains(kw), "{c:?}: {kw}");
        }
    }
}

#[test]
fn palette_opens_the_third_party_licences() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    english(&mut h);
    run_palette(&mut h, "third-party");
    let a = app(&mut h);
    assert!(a.help.open);
    assert_eq!(
        a.help.loc.page,
        Page::Dev(crate::help::THIRD_PARTY_LICENSES.into())
    );
    assert!(a.opened_links.is_empty());
    assert_eq!(
        manual::dev_title(crate::help::THIRD_PARTY_LICENSES),
        "Third-Party Licences"
    );
}

/// Settings → General → About Manta: the website link and the licences.
#[test]
fn settings_about_links() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    set_locale(Locale::De);
    h.run_steps(2);
    app(&mut h)
        .settings
        .show(Some(crate::settings::SettingsTab::General));
    h.run_steps(3);
    assert!(h.query_by_label("Website & Support").is_some());
    h.get_by_label(crate::help::WEBSITE_LABEL).scroll_to_me();
    h.run_steps(3);
    h.get_by_label(crate::help::WEBSITE_LABEL).click();
    h.run_steps(3);
    assert_eq!(
        app(&mut h).opened_links,
        vec![crate::help::WEBSITE_URL.to_string()]
    );
    // Source code and licence (MIT) on GitHub.
    assert!(h.query_by_label("Quellcode").is_some());
    assert!(h.query_by_label("MIT-Lizenz").is_some());
    h.get_by_label(crate::help::SOURCE_LABEL).click_accesskit();
    h.run_steps(3);
    h.get_by_label("Lesen").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        app(&mut h).opened_links,
        vec![
            crate::help::WEBSITE_URL.to_string(),
            crate::help::SOURCE_URL.to_string(),
            crate::help::LICENSE_URL.to_string()
        ]
    );
    h.get_by_label("Anzeigen").click_accesskit();
    h.run_steps(3);
    let help = &app(&mut h).help;
    assert!(help.open);
    assert_eq!(
        help.loc.page,
        Page::Dev(crate::help::THIRD_PARTY_LICENSES.into())
    );
    set_locale(Locale::En);
}

#[test]
fn search_finds_a_keyword_and_enter_opens_it() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    english(&mut h);
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, Action::ShowHelp("08-verbindungen.md".into()));
    h.run_steps(3);
    assert_eq!(page(&mut h), chapter("08-verbindungen.md"));
    // ⌘F in the help window focuses its search field.
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    // "gatekeeper" is a keyword of chapter 1 (index.json).
    h.event(egui::Event::Text("gatekeeper".into()));
    h.run_steps(3);
    assert_eq!(app(&mut h).help.query, "gatekeeper");
    let hits = {
        let m = crate::help::current_manual();
        app(&mut h).help.hits(&m)
    };
    assert!(!hits.is_empty(), "keyword found");
    assert!(
        h.query_all_by_label_contains("Getting Started")
            .next()
            .is_some(),
        "results name the chapter"
    );
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(
        page(&mut h),
        chapter("01-erste-schritte.md"),
        "Enter opens the first result"
    );
    assert_eq!(
        app(&mut h).help.back.len(),
        1,
        "the search result is a new page in the history"
    );

    // A full-text match in a section opens at that section.
    let m = crate::help::current_manual();
    let hits = manual::search(&m, "known_hosts");
    let first = hits.first().expect("known_hosts is in chapter 8");
    assert_eq!(m.chapters[first.chapter].file, "08-verbindungen.md");
    app(&mut h).help.open_hit(&m, first);
    assert_eq!(app(&mut h).help.loc.anchor, first.anchor);
}

#[test]
fn links_navigate_inside_the_help_window_and_back() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    english(&mut h);
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, Action::ShowHelp("05-dateien.md".into()));
    h.run_steps(3);
    // The link the chapter actually contains: 02-bereiche-tabs.md#zielbereich.
    let m = crate::help::current_manual();
    let href = m
        .chapter("05-dateien.md")
        .unwrap()
        .doc
        .links
        .iter()
        .find(|l| l.starts_with("02-bereiche-tabs.md#"))
        .cloned()
        .expect("chapter 5 links to chapter 2");
    let Target::Markdown { path, anchor } = links::classify(&href, &manual::chapter_dir(m.lang))
    else {
        panic!("{href}")
    };
    let mut out = HelpOutcome::default();
    app(&mut h).help.follow(
        MdAction::Markdown {
            path,
            anchor: anchor.clone(),
        },
        &mut out,
    );
    h.run_steps(3);
    assert_eq!(page(&mut h), chapter("02-bereiche-tabs.md"));
    assert_eq!(app(&mut h).help.loc.anchor, anchor);
    assert!(out.open_url.is_none());

    // Back / Forward buttons in the toolbar.
    // (The main window's toolbar has a "Back" too; the help window's comes last.)
    h.query_all_by_label("Back").last().unwrap().click();
    h.run_steps(3);
    assert_eq!(page(&mut h), chapter("05-dateien.md"));
    h.query_all_by_label("Forward").last().unwrap().click();
    h.run_steps(3);
    assert_eq!(page(&mut h), chapter("02-bereiche-tabs.md"));
    // ⌘[ goes back too.
    h.key_press_modifiers(Modifiers::COMMAND, Key::OpenBracket);
    h.run_steps(3);
    assert_eq!(page(&mut h), chapter("05-dateien.md"));

    // Developer docs open in the window; web links go to the browser.
    let Target::Markdown { path, anchor } =
        links::classify("../../plugins.md", &manual::chapter_dir(m.lang))
    else {
        panic!()
    };
    app(&mut h)
        .help
        .follow(MdAction::Markdown { path, anchor }, &mut out);
    h.run_steps(3);
    assert_eq!(page(&mut h), Page::Dev("plugins.md".into()));
    app(&mut h)
        .help
        .follow(MdAction::OpenUrl("https://example.com".into()), &mut out);
    assert_eq!(out.open_url.as_deref(), Some("https://example.com"));

    // A TOC entry is a click away.
    h.get_by_label("Views").click();
    h.run_steps(3);
    assert_eq!(page(&mut h), chapter("03-ansichten.md"));
}

#[test]
fn context_help_buttons_open_the_right_chapter() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    english(&mut h);
    let ctx = h.ctx.clone();
    let _ = ctx;
    app(&mut h)
        .settings
        .show(Some(crate::settings::SettingsTab::General));
    h.run_steps(3);
    // The first "?" is the one in the tab strip (General).
    h.get_all_by_label("Help")
        .next()
        .expect("help button in Settings")
        .click();
    h.run_steps(3);
    let help = &app(&mut h).help;
    assert!(help.open);
    assert_eq!(help.loc.page, chapter("16-einstellungen.md"));
    assert_eq!(help.loc.anchor.as_deref(), Some("allgemein"));
}

#[test]
fn fallback_language_is_english() {
    assert_eq!(manual::get("pt").lang, "en");
    assert_eq!(manual::get("fr").lang, "fr");
    assert_eq!(manual::get("de").lang, "de");
    let en = manual::get("en");
    assert_eq!(en.chapters[0].title, "Getting Started");
}

macro_rules! require_gpu {
    () => {
        // Screenshots are opt-in: they need a GPU and are slow, so a plain
        // `cargo test` skips them. Set MANTA_RENDER_DIR to produce them.
        if std::env::var_os("MANTA_RENDER_DIR").is_none() {
            return;
        }
        if !wgpu_available() {
            return;
        }
    };
}

#[test]
fn render_help() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("help");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, WIDE, theme);
        set_locale(Locale::De);
        h.run_steps(2);
        let ctx = h.ctx.clone();
        app(&mut h).handle_action(&ctx, Action::ShowHelp("05-dateien.md".into()));
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("help_chapter_de_{tname}"));

        app(&mut h).handle_action(
            &ctx,
            Action::ShowHelp("08-verbindungen.md#server-schluessel".into()),
        );
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("help_anchor_de_{tname}"));

        app(&mut h).help.query = "papierkorb".into();
        h.run_steps(3);
        save_to(&mut h, &dir, &format!("help_search_de_{tname}"));
        app(&mut h).help.query.clear();

        app(&mut h).handle_action(&ctx, Action::ShowHelp("01-erste-schritte.md".into()));
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("help_images_de_{tname}"));

        app(&mut h).handle_action(
            &ctx,
            Action::ShowHelp(crate::help::THIRD_PARTY_LICENSES.into()),
        );
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("help_licenses_de_{tname}"));

        app(&mut h).help.open = false;
        app(&mut h)
            .settings
            .show(Some(crate::settings::SettingsTab::General));
        h.run_steps(3);
        h.get_by_label(crate::help::WEBSITE_LABEL).scroll_to_me();
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("settings_about_de_{tname}"));
        set_locale(Locale::En);
    }
}

/// Settings → General → About Manta → Feedback: the prefilled bug form.
#[test]
fn settings_about_feedback_row() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    set_locale(Locale::De);
    h.run_steps(2);
    app(&mut h)
        .settings
        .show(Some(crate::settings::SettingsTab::General));
    h.run_steps(3);
    assert!(h.query_by_label("Rückmeldung").is_some());
    h.get_by_label("Fehler melden").click_accesskit();
    h.run_steps(3);
    let links = app(&mut h).opened_links.clone();
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(links[0].contains("template=bug_report.yml&manta-version="));
    set_locale(Locale::En);
}
