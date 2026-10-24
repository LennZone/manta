//! `render_manual_images`: the user manual's screenshots
//! (`docs/manual/images/<lang>/NAME.png`, see `images/NEEDED.md`), in
//! all seven UI languages (`manual_shots::LANGS`), light theme, from the
//! synthetic fixtures only (temp folders, `example.com` servers, the name
//! "anna"). Opt-in because it writes into the source tree:
//!
//! `MANTA_MANUAL_IMAGES=1 cargo test -p manta render_manual_images -- --test-threads=1`
//!
//! Child of `ux_tests`.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Rect, Theme};
use manta_core::VfsPath;
use manta_ops::{CompareStatus, JobKind, JobState};

use super::*;
use crate::app::Action;
use crate::aux_window::AuxKind;
use crate::dialogs::{AuthKind, ConnectForm, Dialog};
use crate::i18n::{set_locale, Locale};
use crate::manual_shots::{self, Crop};

/// The standard fixture, at `…/Users/anna/Projects`.
fn demo() -> Fixture {
    fixture_at("Users/anna/Projects")
}

fn shot(h: &mut H, lang: &str, name: &str, crop: Crop) {
    // The harness may have set up its fonts before the language was known.
    manual_shots::use_fonts(&h.ctx);
    // The app was built with English preferences: plugin names,
    // descriptions and panel / sheet titles follow the test's language.
    app(h).plugins.relocalize();
    h.run_steps(4);
    let img = h.render().expect("render");
    let rect = manual_shots::crop_rect(&h.ctx, crop);
    manual_shots::save(&img, 2.0, rect, lang, name);
}

fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    settle(h);
}

/// A light harness in `locale` with the standard fixture.
fn light(fx: &Fixture, locale: Locale) -> H {
    set_locale(locale);
    let mut h = harness(fx, WIDE, Theme::Light);
    set_locale(locale);
    h.run_steps(3);
    h
}

fn pts(x0: f32, y0: f32, x1: f32, y1: f32) -> Crop {
    Crop::Rect(Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)))
}

macro_rules! manual_only {
    () => {
        if !manual_shots::enabled() || !wgpu_available() {
            return;
        }
    };
}

/// The main window, panes, palette, dialogs, transfers, Compare & Sync,
/// Settings and the built-in preview.
#[test]
fn render_manual_images() {
    manual_only!();
    let fx = demo();
    for (locale, lang) in manual_shots::langs() {
        // 01: overview, sidebar close-up (07).
        let mut h = light(&fx, locale);
        select(&mut h, Side::Left, &["README.md", "report-q3.pdf"]);
        shot(&mut h, lang, "overview-two-panes", Crop::Full);
        shot(&mut h, lang, "sidebar", pts(0.0, 54.0, 214.0, 506.0));

        // 01: command palette with the start of "New Folder…" typed.
        app(&mut h).palette.show();
        h.run_steps(2);
        let typed = match lang {
            "de" => "neuer or",
            "zh-Hans" => "新建文",
            "es" => "nueva car",
            "fr" => "nouveau do",
            "it" => "nuova car",
            "ru" => "новая па",
            _ => "new fo",
        };
        h.event(egui::Event::Text(typed.into()));
        shot(
            &mut h,
            lang,
            "command-palette",
            pts(240.0, 0.0, 1040.0, 520.0),
        );
        app(&mut h).palette.open = false;

        // 02: three panes, the middle one active, a selection in it.
        let mut h = light(&fx, locale);
        act(&mut h, Action::SplitPane(Side::Left));
        let order = app(&mut h).pane_order();
        let middle = order[1];
        app(&mut h).activate(middle);
        let src = VfsPath::new(&fx.root.join("src").to_string_lossy());
        app(&mut h).navigate_path(order[2], src);
        settle(&mut h);
        app(&mut h).activate(middle);
        h.run_steps(2);
        shot(&mut h, lang, "panes-three", Crop::Full);
        select(&mut h, middle, &["README.md", "report-q3.pdf"]);
        shot(&mut h, lang, "panes-target-badge", Crop::Full);

        // 05: the copy sheet for three items with three panes.
        let mut h = light(&fx, locale);
        act(&mut h, Action::SplitPane(Side::Left));
        let left = app(&mut h).pane_order()[0];
        app(&mut h).activate(left);
        let middle = app(&mut h).pane_order()[1];
        let photos = VfsPath::new(&fx.root.join("photos").to_string_lossy());
        app(&mut h).navigate_path(middle, photos);
        settle(&mut h);
        app(&mut h).activate(left);
        select(&mut h, left, &["README.md", "report-q3.pdf", "src"]);
        act(&mut h, Action::CopyToOther(left, false));
        shot(&mut h, lang, "copy-dialog", Crop::Modal);

        // 05: transfers panel, one running copy and one finished job.
        let mut h = light(&fx, locale);
        let a = app(&mut h);
        a.fake_jobs = Some(vec![
            job(
                1,
                JobKind::Copy,
                "Copy 3 items to anna@files.example.com:/srv/share",
                JobState::Running,
                31_200_000,
                52_000_000,
                (14, 22),
            ),
            job(
                2,
                JobKind::Copy,
                "Copy backup-2026-10-01.zip to /Volumes/Backup Drive",
                JobState::Done,
                48_300_000,
                48_300_000,
                (1, 1),
            ),
        ]);
        a.show_transfers = true;
        h.run_steps(3);
        shot(
            &mut h,
            lang,
            "transfers-panel",
            pts(0.0, 540.0, 1280.0, 800.0),
        );

        // 08: Connect sheet, SFTP with a key file, added to Favorites.
        let mut h = light(&fx, locale);
        let mut form = ConnectForm::new(Side::Left);
        form.host = "files.example.com".into();
        form.user = "anna".into();
        form.auth_kind = AuthKind::KeyFile;
        form.key_path = "/Users/anna/.ssh/id_ed25519".into();
        form.save_favorite = true;
        form.name = "Files".into();
        app(&mut h).dialog = Some(Dialog::Connect(form));
        shot(&mut h, lang, "connect-sftp", Crop::Modal);

        // 08: FTPS data channel.
        app(&mut h).dialog = Some(Dialog::Connect(super::ftps_ux::ftps_form(
            manta_core::FtpDataChannel::Auto,
        )));
        shot(&mut h, lang, "connect-ftps-data-channel", Crop::Modal);

        // 08: S3 with the Cloudflare R2 preset.
        let mut form = ConnectForm::new(Side::Left);
        form.protocol = manta_core::Protocol::S3;
        form.auth_kind = AuthKind::Password;
        form.s3.set_provider("r2");
        form.s3.account = "8f2c1e9a4b7d".into();
        form.s3.set_provider("r2");
        form.user = "4c1f8e0d2b9a7e65".into();
        form.password = "secret".into();
        form.s3.bucket = "media-assets".into();
        app(&mut h).dialog = Some(Dialog::Connect(form));
        shot(&mut h, lang, "connect-s3", Crop::Modal);

        // 08: WebDAV with the Nextcloud preset, SMB with the share list.
        let mut form = ConnectForm::new(Side::Left);
        form.set_protocol_for_tests(manta_core::Protocol::WebDavs);
        form.host = "cloud.example.com".into();
        form.user = "anna".into();
        form.dav.set_preset("nextcloud", "anna");
        app(&mut h).dialog = Some(Dialog::Connect(form));
        shot(&mut h, lang, "connect-webdav", Crop::Modal);
        app(&mut h).dialog = Some(Dialog::Connect(super::smb_webdav_ux::smb_form(true)));
        shot(&mut h, lang, "connect-smb", Crop::Modal);
        app(&mut h).dialog = None;

        // 13: Compare & Sync.
        let mut h = light(&fx, locale);
        let a = app(&mut h);
        a.sync.open = true;
        a.sync.left_label = "Local ~/Projects".into();
        a.sync.right_label = "anna@files.example.com /srv/share".into();
        a.sync.items = Some(vec![
            item("src", true, CompareStatus::Identical, Some(0), Some(0)),
            item(
                "src/main.rs",
                false,
                CompareStatus::LeftNewer,
                Some(1_204),
                Some(1_180),
            ),
            item(
                "src/lib.rs",
                false,
                CompareStatus::Identical,
                Some(13),
                Some(13),
            ),
            item("Design", true, CompareStatus::OnlyLeft, Some(0), None),
            item(
                "Design/hero-banner.png",
                false,
                CompareStatus::OnlyLeft,
                Some(2_480_000),
                None,
            ),
            item(
                "README.md",
                false,
                CompareStatus::RightNewer,
                Some(220),
                Some(310),
            ),
            item(
                "report-q3.pdf",
                false,
                CompareStatus::Different,
                Some(1_840_000),
                Some(1_790_000),
            ),
            item("cache", true, CompareStatus::OnlyRight, None, Some(0)),
            item(
                "notes.txt",
                false,
                CompareStatus::Identical,
                Some(9),
                Some(9),
            ),
        ]);
        shot(&mut h, lang, "compare-sync", Crop::Window(AuxKind::Sync));

        // 06: the built-in preview window with a text file.
        let mut h = light(&fx, locale);
        let path = VfsPath::new(&fx.root.join("README.md").to_string_lossy());
        let entry = manta_core::Entry {
            name: "README.md".into(),
            path,
            kind: manta_core::EntryKind::File,
            size: README.len() as u64,
            modified: None,
            permissions: Some(0o644),
            link_target: None,
            hidden: false,
        };
        let mut p = crate::preview::Preview::new(999, entry);
        p.loading = false;
        p.content = Some(crate::worker::PreviewContent::Text {
            text: README.into(),
            truncated: false,
        });
        app(&mut h).preview = Some(p);
        shot(
            &mut h,
            lang,
            "preview-builtin",
            Crop::Window(AuxKind::Preview),
        );

        // 16 / 14 / 15: Settings with the real built-in plugins.
        let mut h = plugin_harness(&fx, Theme::Light, true);
        set_locale(locale);
        app(&mut h)
            .settings
            .show(Some(crate::settings::SettingsTab::General));
        h.run_steps(4);
        shot(
            &mut h,
            lang,
            "settings-general",
            Crop::Window(AuxKind::Settings),
        );
        app(&mut h).settings.tab = crate::settings::SettingsTab::Plugins;
        h.run_steps(4);
        shot(
            &mut h,
            lang,
            "settings-plugins",
            Crop::Window(AuxKind::Settings),
        );
    }
    set_locale(Locale::En);
}

/// 15: the install sheet and the trust sheet of an external plugin (a
/// test of its own, so they can be rendered without the rest:
/// `MANTA_MANUAL_IMAGES=1 cargo test -p manta render_manual_plugin_sheets`).
#[test]
fn render_manual_plugin_sheets() {
    manual_only!();
    let fx = demo();
    for (locale, lang) in manual_shots::langs() {
        let mut h = plugin_harness(&fx, Theme::Light, true);
        set_locale(locale);
        app(&mut h)
            .settings
            .show(Some(crate::settings::SettingsTab::Plugins));
        app(&mut h).settings.confirm = Some(crate::settings::Confirm::Enable {
            id: "cloud-sync".into(),
            review: Box::new(super::sample_review(
                "cloud-sync",
                "Cloud Sync",
                "node index.js",
                false,
            )),
        });
        shot(
            &mut h,
            lang,
            "plugin-confirm",
            Crop::Window(AuxKind::Settings),
        );
        app(&mut h).settings.confirm = Some(crate::settings::Confirm::Install(Box::new(
            super::sample_review("cloud-sync", "Cloud Sync", "node index.js", true),
        )));
        shot(
            &mut h,
            lang,
            "plugin-install",
            Crop::Window(AuxKind::Settings),
        );
    }
    set_locale(Locale::En);
}

/// Views (03), archives (10), batch rename (14), containers (08), the
/// default-file-manager prompt (01), cloud folders (09), Software Update (17).
#[test]
fn render_manual_images_more() {
    manual_only!();
    for (locale, lang) in manual_shots::langs() {
        // 03: List / Columns / Gallery of a photo folder.
        let fx = demo();
        let dir = super::views_ux_tests::photos(&fx);
        let mut h = light(&fx, locale);
        super::views_ux_tests::goto(&mut h, Side::Left, &dir);
        app(&mut h).activate(Side::Left);
        act(
            &mut h,
            Action::SetView(Side::Left, crate::views::ViewMode::List),
        );
        let tab = app(&mut h).pane_mut(Side::Left).tab_mut();
        tab.sort = crate::util::SortSpec {
            column: crate::util::SortColumn::Size,
            ascending: false,
        };
        tab.rebuild_view();
        super::views_ux_tests::settle_thumbs(&mut h);
        shot(&mut h, lang, "view-list", pts(206.0, 48.0, 1280.0, 520.0));

        // Narrower window: the long temp path collapses into "…".
        set_locale(locale);
        let mut h = harness(&fx, [1120.0, 720.0], Theme::Light);
        set_locale(locale);
        super::views_ux_tests::goto(&mut h, Side::Left, &dir);
        act(&mut h, Action::ClosePane(Side::RIGHT));
        app(&mut h).activate(Side::LEFT);
        act(
            &mut h,
            Action::SetView(Side::LEFT, crate::views::ViewMode::Columns),
        );
        let enter = |h: &mut H, name: &str, open: bool| {
            if let Some(i) = app(h).pane(Side::LEFT).tab().view_index_of(name) {
                app(h).pane_mut(Side::LEFT).tab_mut().select_only(i);
                if open {
                    act(h, Action::ColumnsEnter(Side::LEFT));
                }
            }
            settle(h);
        };
        enter(&mut h, "Reisen", true);
        enter(&mut h, "2026", true);
        enter(&mut h, "Island", true);
        enter(&mut h, "Geysir.jpg", false);
        app(&mut h)
            .pane_mut(Side::LEFT)
            .tab_mut()
            .view_state
            .cols
            .wait_loaded(std::time::Duration::from_secs(5));
        super::views_ux_tests::settle_thumbs(&mut h);
        shot(&mut h, lang, "view-columns", Crop::Full);

        act(&mut h, Action::SplitPane(Side::LEFT));
        app(&mut h).activate(Side::LEFT);
        super::views_ux_tests::goto(&mut h, Side::LEFT, &dir);
        act(
            &mut h,
            Action::SetView(Side::LEFT, crate::views::ViewMode::Gallery),
        );
        if let Some(i) = app(&mut h)
            .pane(Side::LEFT)
            .tab()
            .view_index_of("Hafen.jpg")
        {
            app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
        }
        super::views_ux_tests::settle_thumbs(&mut h);
        shot(&mut h, lang, "view-gallery", Crop::Full);

        // 10: a ZIP as a folder (one pane, a window narrow enough for the
        // temp path to collapse into "…"), and the Compress sheet.
        let fx = demo();
        super::archive::make_zip(&fx.root.join("site-backup.zip"));
        set_locale(locale);
        let mut h = harness(&fx, [960.0, 600.0], Theme::Light);
        set_locale(locale);
        act(&mut h, Action::ClosePane(Side::Right));
        app(&mut h).activate(Side::Left);
        select(&mut h, Side::Left, &["site-backup.zip"]);
        h.key_press(egui::Key::Enter);
        super::archive::settle_open(&mut h, Side::Left);
        select(&mut h, Side::Left, &["docs"]);
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        shot(
            &mut h,
            lang,
            "archive-as-folder",
            pts(206.0, 48.0, 960.0, 300.0),
        );
        let mut h = light(&fx, locale);
        app(&mut h).active = Side::Left;
        select(&mut h, Side::Left, &["src", "Design", "README.md"]);
        act(&mut h, Action::Compress(Side::Left));
        shot(&mut h, lang, "compress-sheet", Crop::Modal);

        // 14: Batch Rename.
        let (_fx, mut h, _plugin) = rename_sheet(Theme::Light);
        set_locale(locale);
        h.run_steps(4);
        shot(&mut h, lang, "batch-rename", Crop::Modal);

        // 08: Connect sheet, Container tab (Docker).
        let fx = demo();
        let mut h = light(&fx, locale);
        super::containers_ux::with_containers(&mut h);
        let mut form = ConnectForm::new(Side::Left);
        form.set_protocol_for_tests(manta_core::Protocol::Docker);
        let d = super::containers_ux::discovered();
        form.ctr.set_found(crate::dialogs::Found {
            docker: d.docker.clone(),
            ..Default::default()
        });
        if let Some(Ok(list)) = &d.docker {
            if let Some(c) = list.first() {
                form.ctr.container = c.name.clone();
            }
        }
        app(&mut h).dialog = Some(Dialog::Connect(form));
        shot(&mut h, lang, "connect-container", Crop::Modal);

        // 01: "Use Manta as your default file manager?"
        let mut h = light(&fx, locale);
        let a = app(&mut h);
        a.viewer = crate::default_viewer::DefaultViewer::with_status(
            super::default_viewer_ux::status(Ok(()), false),
        );
        a.viewer.prompt_open = true;
        shot(&mut h, lang, "default-file-manager-prompt", Crop::Modal);

        // 09: Cloud section and sync badges (neutral accounts).
        let home = CloudDemo::new();
        let mut h =
            super::cloud_ux_tests::harness_cloud(&fx, Theme::Light, true, home.cloud(), None);
        set_locale(locale);
        super::cloud_ux_tests::open_local(&mut h, Side::Left, &home.drive());
        select(&mut h, Side::Left, &["Holiday.mov"]);
        shot(&mut h, lang, "cloud-sidebar-badges", Crop::Full);
    }
    set_locale(Locale::En);
}

/// 14: Project Info for a Laravel demo project.
#[test]
fn render_manual_images_project_info() {
    manual_only!();
    let fx = projects_fixture();
    for (locale, lang) in manual_shots::langs() {
        set_locale(locale);
        let config = tempfile::tempdir().unwrap();
        let mut h = project_info_harness(&fx, Theme::Light, config.path());
        set_locale(locale);
        app(&mut h)
            .plugins
            .set_panel_open(PI, "project", None, true);
        wait_until(&mut h, "laravel panel", |h| {
            pi_panel_has(h, "\"acme-shop\"")
        });
        h.run_steps(20);
        settle(&mut h);
        shot(&mut h, lang, "project-info", Crop::Full);
    }
    set_locale(Locale::En);
}

/// 17: the Software Update window ("0.2.0 is available").
#[test]
fn render_manual_images_update() {
    manual_only!();
    for (locale, lang) in manual_shots::langs() {
        let (mut u, _) = super::update_ux::fake_updates(manta_updater::UpdateState::default());
        u.show_state(
            crate::updates::View::Available,
            Some(crate::updates::tests::release("0.2.0")),
        );
        let mut h = super::update_ux::content_harness(Theme::Light, crate::update_window::SIZE, u);
        set_locale(locale);
        manual_shots::use_fonts(&h.ctx);
        for _ in 0..6 {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let img = h.render().expect("render");
        manual_shots::save(&img, 2.0, None, lang, "software-update");
    }
    set_locale(Locale::En);
}

/// A fake home with iCloud Drive, Google Drive and OneDrive folders (no
/// real account names) and a few files in "My Drive".
struct CloudDemo {
    _dir: tempfile::TempDir,
    home: PathBuf,
}

impl CloudDemo {
    fn new() -> CloudDemo {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("anna");
        std::fs::create_dir_all(home.join("Library/Mobile Documents/com~apple~CloudDocs")).unwrap();
        let cs = home.join("Library/CloudStorage");
        std::fs::create_dir_all(cs.join("OneDrive-Personal")).unwrap();
        let my = cs.join("GoogleDrive-anna@example.com/My Drive");
        std::fs::create_dir_all(my.join("Photos 2025")).unwrap();
        for (name, len) in [
            ("Budget 2026.xlsx", 48_000usize),
            ("Holiday.mov", 9_000),
            ("notes.txt", 1_200),
            ("Report Q3.pdf", 2_400),
        ] {
            std::fs::write(my.join(name), vec![b'x'; len]).unwrap();
        }
        CloudDemo { _dir: dir, home }
    }

    fn drive(&self) -> PathBuf {
        self.home
            .join("Library/CloudStorage/GoogleDrive-anna@example.com/My Drive")
    }

    fn cloud(&self) -> crate::cloud::Cloud {
        let probe = Arc::new(crate::cloud::FakeProbe::default());
        probe.set_dataless(&self.drive().join("Holiday.mov"), true);
        probe.set_dataless(&self.drive().join("Report Q3.pdf"), true);
        let home = self.home.clone();
        crate::cloud::Cloud::new(
            Box::new(move || manta_vfs::cloud_local::discover_in(&home)),
            probe,
            true,
        )
    }
}
