//! Cloud folders (iCloud Drive, Google Drive for desktop, OneDrive,
//! Dropbox, ...): the sidebar's "Cloud" section, the "Connect Google
//! Drive …" hint, sync badges and "Download Now" / "Remove Download", and
//! Google Drive's own connection hidden unless the user set up an OAuth
//! client of their own.
//!
//! Everything runs on a fake home folder in a temp dir with simulated stat
//! flags ([`FakeProbe`]); the user's real cloud files are never touched.
//! `render_cloud` writes German light/dark PNGs to `$MANTA_RENDER_DIR/cloud`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Theme};
use egui_kittest::kittest::Queryable;
use manta_core::{Protocol, VfsPath};
use manta_plugin::host::PluginHost;
use manta_vfs::cloud_local::{self, SyncInfo, SyncState};

use super::{
    app, fixture, harness_init, render_dir, save_to, select, settle, wgpu_available, Fixture, H,
    WIDE,
};
use crate::app::{Action, Side};
use crate::cloud::{Cloud, CloudProbe, FakeProbe, GOOGLE_DRIVE_DOWNLOAD};
use crate::dialogs::Dialog;

/// A fake home with iCloud Drive and File Provider folders; `with_drive`:
/// also Google Drive for desktop (two accounts) with a few files.
struct CloudHome {
    _dir: tempfile::TempDir,
    home: PathBuf,
}

impl CloudHome {
    fn new(with_drive: bool) -> CloudHome {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let cs = home.join("Library/CloudStorage");
        std::fs::create_dir_all(home.join("Library/Mobile Documents/com~apple~CloudDocs/Notes"))
            .unwrap();
        for d in [
            "OneDrive-ContosoTravelGmbH",
            "OneDrive-SharedLibraries-OneDriveCloudTemp",
            "Dropbox",
        ] {
            std::fs::create_dir_all(cs.join(d)).unwrap();
        }
        if with_drive {
            let my = cs.join("GoogleDrive-anna@gmail.com/My Drive");
            std::fs::create_dir_all(my.join("Photos 2025")).unwrap();
            std::fs::create_dir_all(cs.join("GoogleDrive-anna@firma.de/My Drive")).unwrap();
            for (name, len) in [
                ("Budget 2026.xlsx", 48_000usize),
                ("Holiday.mov", 9_000),
                ("notes.txt", 1_200),
                ("Report Q3.pdf", 2_400),
            ] {
                std::fs::write(my.join(name), vec![b'x'; len]).unwrap();
            }
        }
        CloudHome { _dir: dir, home }
    }

    fn storage(&self) -> PathBuf {
        self.home.join("Library/CloudStorage")
    }

    fn my_drive(&self) -> PathBuf {
        self.storage().join("GoogleDrive-anna@gmail.com/My Drive")
    }

    /// A cloud model over this home (rediscovers from disk) and the probe.
    fn cloud(&self) -> (Cloud, Arc<FakeProbe>) {
        let probe = Arc::new(FakeProbe::default());
        let home = self.home.clone();
        let cloud = Cloud::new(
            Box::new(move || cloud_local::discover_in(&home)),
            probe.clone(),
            true,
        );
        (cloud, probe)
    }
}

pub(super) fn harness_cloud(
    fx: &Fixture,
    theme: Theme,
    gpu: bool,
    cloud: Cloud,
    client_file: Option<PathBuf>,
) -> H {
    harness_init(
        fx,
        WIDE,
        theme,
        gpu,
        Box::new(|ctx: &egui::Context| PluginHost::new(ctx.clone(), Vec::new(), None)),
        move |init| {
            init.cloud = Some(cloud);
            init.gdrive_client_file = client_file;
        },
    )
}

fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    h.run_steps(3);
}

fn has_label(h: &H, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

pub(super) fn open_local(h: &mut H, side: Side, dir: &Path) {
    let p = VfsPath::new(&dir.to_string_lossy());
    app(h).navigate_path(side, p);
    settle(h);
}

#[test]
fn sidebar_lists_providers_and_opens_them() {
    let fx = fixture();
    let ch = CloudHome::new(true);
    let (cloud, _) = ch.cloud();
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);
    h.run_steps(3);
    assert!(has_label(&h, "Cloud"), "section title");
    // Two Google accounts: each row names its account; one Dropbox: plain.
    assert!(has_label(&h, "Google Drive anna@gmail.com"));
    assert!(has_label(&h, "Google Drive anna@firma.de"));
    assert!(has_label(&h, "OneDrive Contoso Travel GmbH"));
    assert!(has_label(&h, "OneDrive Shared Libraries"));
    assert!(has_label(&h, "Dropbox"));
    assert!(has_label(&h, "iCloud Drive"));
    // Google Drive is there: no hint.
    assert!(!has_label(&h, "Connect Google Drive…"));

    h.get_by_label("Dropbox").click();
    settle(&mut h);
    let a = app(&mut h);
    let tab = a.pane(a.active).tab();
    assert_eq!(
        tab.loc.path.to_string(),
        ch.storage().join("Dropbox").to_string_lossy()
    );
    assert_eq!(
        tab.loc.fs.protocol(),
        Protocol::Local,
        "a plain local folder"
    );
}

#[test]
fn section_hidden_without_providers_and_rescanned() {
    let fx = fixture();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_path_buf();
    let probe = Arc::new(FakeProbe::default());
    let h2 = home.clone();
    let cloud = Cloud::new(Box::new(move || cloud_local::discover_in(&h2)), probe, true);
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);
    h.run_steps(3);
    assert!(!has_label(&h, "Cloud"), "no providers: no section");
    assert!(!has_label(&h, "Connect Google Drive…"));

    // Google Drive for desktop gets installed; the sidebar refresh finds it.
    std::fs::create_dir_all(home.join("Library/CloudStorage/GoogleDrive-new@gmail.com")).unwrap();
    act(&mut h, Action::RescanVolumes);
    assert!(has_label(&h, "Cloud"));
    assert!(
        has_label(&h, "Google Drive"),
        "one account: no account after the name"
    );
}

#[test]
fn google_drive_hint_explains_and_can_be_dismissed() {
    let fx = fixture();
    let ch = CloudHome::new(false);
    let (cloud, _) = ch.cloud();
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);
    h.run_steps(3);
    assert!(has_label(&h, "Connect Google Drive…"));
    h.get_by_label("Connect Google Drive…").click();
    h.run_steps(3);
    assert!(has_label(&h, "Don't Show Again"), "unfolded");
    h.get_by_label_contains("Get Google Drive for desktop")
        .click();
    h.run_steps(3);
    assert_eq!(
        app(&mut h).opened_links,
        vec![GOOGLE_DRIVE_DOWNLOAD.to_string()]
    );
    h.get_by_label("Don't Show Again").click();
    h.run_steps(3);
    assert!(app(&mut h).prefs.gdrive_hint_dismissed);
    assert!(!has_label(&h, "Connect Google Drive…"));
    assert!(has_label(&h, "Dropbox"), "the section stays");
}

#[test]
fn badges_and_download_actions_follow_the_flags() {
    let fx = fixture();
    let ch = CloudHome::new(true);
    let (cloud, probe) = ch.cloud();
    let my = ch.my_drive();
    probe.set_dataless(&my.join("Holiday.mov"), true);
    probe.set_dataless(&my.join("Report Q3.pdf"), true);
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);
    open_local(&mut h, Side::Left, &my);
    h.run_steps(3);
    let states = app(&mut h)
        .pane(Side::Left)
        .tab()
        .cloud
        .clone()
        .expect("cloud states");
    assert_eq!(
        states.get("Holiday.mov").map(|i| i.state),
        Some(SyncState::CloudOnly)
    );
    assert_eq!(
        states.get("notes.txt").map(|i| i.state),
        Some(SyncState::Current)
    );
    // A folder outside the cloud has none.
    assert!(app(&mut h).pane(Side::Right).tab().cloud.is_none());

    let offer = |h: &mut H| {
        crate::pane::MenuCtx::of(
            &egui::Context::default(),
            app(h).pane(Side::Left).tab(),
            false,
        )
        .cloud_offer()
    };
    select(&mut h, Side::Left, &["Holiday.mov"]);
    assert_eq!(
        offer(&mut h),
        crate::cloud::MenuOffer {
            download: true,
            evict: false
        }
    );
    select(&mut h, Side::Left, &["notes.txt"]);
    assert_eq!(
        offer(&mut h),
        crate::cloud::MenuOffer {
            download: false,
            evict: true
        }
    );

    select(&mut h, Side::Left, &["Holiday.mov", "Report Q3.pdf"]);
    act(&mut h, Action::CloudDownload(Side::Left));
    h.run_steps(3);
    let calls = probe.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            ("download", my.join("Holiday.mov")),
            ("download", my.join("Report Q3.pdf"))
        ]
    );
    let states = app(&mut h).pane(Side::Left).tab().cloud.clone().unwrap();
    assert_eq!(
        states.get("Holiday.mov").map(|i| i.state),
        Some(SyncState::Current),
        "re-read after the action"
    );

    select(&mut h, Side::Left, &["notes.txt"]);
    act(&mut h, Action::CloudEvict(Side::Left));
    h.run_steps(3);
    let states = app(&mut h).pane(Side::Left).tab().cloud.clone().unwrap();
    assert_eq!(
        states.get("notes.txt").map(|i| i.state),
        Some(SyncState::CloudOnly)
    );

    // Outside a cloud folder there is nothing to offer.
    select(&mut h, Side::Right, &["Brand Guide.pdf"]);
    let right = crate::pane::MenuCtx::of(
        &egui::Context::default(),
        app(&mut h).pane(Side::Right).tab(),
        false,
    )
    .cloud_offer();
    assert_eq!(right, crate::cloud::MenuOffer::default());
}

/// After a copy into a cloud folder the provider app uploads in its own
/// time: the new item shows "uploading" (or "waiting to upload") until the
/// provider is done; a failed upload shows the provider's message.
#[test]
fn upload_states_after_a_copy_into_the_cloud() {
    let fx = fixture();
    let ch = CloudHome::new(true);
    let (cloud, probe) = ch.cloud();
    let my = ch.my_drive();
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);
    open_local(&mut h, Side::Left, &my);
    // The provider will report the copied file as uploading (iCloud: 40 %).
    probe.set_upload(
        &my.join("Brand Guide.pdf"),
        Some(SyncInfo {
            upload_percent: Some(40.0),
            ..SyncInfo::new(SyncState::Uploading)
        }),
    );
    // Right pane: the fixture's Design folder.
    select(&mut h, Side::Right, &["Brand Guide.pdf"]);
    act(&mut h, Action::CopyToOther(Side::Right, false));
    if app(&mut h).dialog.is_some() {
        h.key_press(egui::Key::Enter);
    }
    super::wait_until(&mut h, "the copy", |_| my.join("Brand Guide.pdf").exists());
    super::wait_until(&mut h, "the listing", |h| {
        h.state()
            .pane(Side::Left)
            .tab()
            .cloud
            .as_ref()
            .is_some_and(|c| c.contains_key("Brand Guide.pdf"))
    });
    let states = app(&mut h)
        .pane(Side::Left)
        .tab()
        .cloud
        .clone()
        .expect("cloud states");
    let up = states
        .get("Brand Guide.pdf")
        .expect("re-read after the copy");
    assert_eq!(up.state, SyncState::Uploading);
    crate::i18n::with_locale(crate::i18n::Locale::De, || {
        assert_eq!(
            crate::cloud::tooltip(up).as_deref(),
            Some("Wird hochgeladen … 40 %")
        );
    });

    // The provider fails: red badge, its message in the tooltip.
    probe.set_upload(
        &my.join("Brand Guide.pdf"),
        Some(SyncInfo {
            state: SyncState::UploadError,
            upload_error: Some("Your Google Drive storage is full".into()),
            ..SyncInfo::default()
        }),
    );
    probe.set_upload(
        &my.join("notes.txt"),
        Some(SyncInfo {
            upload_waiting: true,
            ..SyncInfo::new(SyncState::Uploading)
        }),
    );
    app(&mut h).cloud.invalidate(&my);
    h.run_steps(3);
    let states = app(&mut h).pane(Side::Left).tab().cloud.clone().unwrap();
    let err = states.get("Brand Guide.pdf").unwrap();
    assert_eq!(err.state, SyncState::UploadError);
    assert_eq!(
        crate::cloud::tooltip(err).as_deref(),
        Some("Upload error: Your Google Drive storage is full")
    );
    assert_eq!(
        crate::cloud::tooltip(states.get("notes.txt").unwrap()).as_deref(),
        Some("Waiting to upload")
    );
    // Neither offers "Download Now" / "Remove Download" on its own.
    select(&mut h, Side::Left, &["Brand Guide.pdf"]);
    let offer = crate::pane::MenuCtx::of(
        &egui::Context::default(),
        app(&mut h).pane(Side::Left).tab(),
        false,
    )
    .cloud_offer();
    assert_eq!(offer, crate::cloud::MenuOffer::default());

    // Uploaded: the badge goes away.
    probe.set_upload(&my.join("Brand Guide.pdf"), None);
    app(&mut h).cloud.invalidate(&my);
    h.run_steps(3);
    let states = app(&mut h).pane(Side::Left).tab().cloud.clone().unwrap();
    assert_eq!(
        states.get("Brand Guide.pdf").map(|i| i.state),
        Some(SyncState::Current)
    );
}

/// A probe whose eviction is refused (like a File Provider that doesn't
/// allow it): the error reaches the user, nothing changes.
struct RefusingProbe(FakeProbe);

impl CloudProbe for RefusingProbe {
    fn folder(&self, dir: &Path) -> crate::cloud::FolderSync {
        self.0.folder(dir)
    }
    fn download(&self, path: &Path) -> Result<(), String> {
        self.0.download(path)
    }
    fn evict(&self, _path: &Path) -> Result<(), String> {
        Err("this cloud provider doesn't let Manta remove the download; use “Remove Download” in Finder".into())
    }
}

#[test]
fn refused_eviction_is_reported() {
    let fx = fixture();
    let ch = CloudHome::new(true);
    let home = ch.home.clone();
    let cloud = Cloud::new(
        Box::new(move || cloud_local::discover_in(&home)),
        Arc::new(RefusingProbe(FakeProbe::default())),
        true,
    );
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);
    open_local(&mut h, Side::Left, &ch.my_drive());
    select(&mut h, Side::Left, &["notes.txt"]);
    act(&mut h, Action::CloudEvict(Side::Left));
    h.run_steps(3);
    let toasts = app(&mut h).toasts.titles();
    assert!(
        toasts.iter().any(|t| t == "Couldn't remove the download"),
        "{toasts:?}"
    );
}

/// iCloud Drive like Finder: the top folder also shows the apps' folders
/// (by name, not `iCloud~md~obsidian/Documents`), the path bar starts at
/// the "iCloud Drive" chip, and an app's folder can't be deleted or renamed.
#[test]
fn icloud_drive_shows_app_folders_like_finder() {
    let fx = fixture();
    let ch = CloudHome::new(false);
    let mobile = ch.home.join("Library/Mobile Documents");
    let top = mobile.join("com~apple~CloudDocs");
    let obsidian = mobile.join("iCloud~md~obsidian/Documents");
    std::fs::create_dir_all(obsidian.join("Vault")).unwrap();
    // TextEdit ships with macOS: its folder gets its icon.
    let textedit = mobile.join("com~apple~TextEdit/Documents");
    std::fs::create_dir_all(&textedit).unwrap();
    std::fs::write(textedit.join("Brief.rtf"), "{\\rtf1}").unwrap();
    // Read by the listing, the path bar and the guards (this test only).
    std::env::set_var(cloud_local::HOME_OVERRIDE_ENV, &ch.home);
    let (cloud, _) = ch.cloud();
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, None);

    open_local(&mut h, Side::Left, &top);
    let tab = app(&mut h).pane(Side::Left).tab();
    let names: Vec<&str> = tab.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"Notes") && names.contains(&"obsidian"),
        "{names:?}"
    );
    assert_eq!(tab.title(), "iCloud Drive");
    assert!(
        crate::pane::crumbs(&tab.loc).is_empty(),
        "the chip is the top"
    );
    let loc = app(&mut h).pane(Side::Left).tab().loc.clone();
    assert_eq!(app(&mut h).place_label(&loc), "iCloud Drive");

    // Deleting or renaming the app's folder is refused with a note.
    select(&mut h, Side::Left, &["obsidian"]);
    act(&mut h, Action::Delete(Side::Left));
    act(&mut h, Action::Rename(Side::Left));
    let toasts = app(&mut h).toasts.titles();
    assert!(
        toasts
            .iter()
            .any(|t| t == "“obsidian” belongs to its app and stays in iCloud Drive"),
        "{toasts:?}"
    );
    assert!(
        toasts
            .iter()
            .any(|t| t == "“obsidian” belongs to its app and keeps its name"),
        "{toasts:?}"
    );
    assert!(app(&mut h).dialog.is_none());
    assert!(obsidian.is_dir());

    // Inside: the path bar reads "obsidian › Vault", still iCloud Drive.
    open_local(&mut h, Side::Left, &obsidian.join("Vault"));
    let tab = app(&mut h).pane(Side::Left).tab();
    let labels: Vec<String> = crate::pane::crumbs(&tab.loc)
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert_eq!(labels, ["obsidian", "Vault"]);
    let loc = app(&mut h).pane(Side::Left).tab().loc.clone();
    assert_eq!(app(&mut h).place_label(&loc), "iCloud Drive");

    // An app's folder shows its app's icon, like in Finder; folders inside
    // it don't.
    if cfg!(target_os = "macos") {
        let app_of = crate::thumbs::icloud_app(&textedit);
        assert!(
            app_of
                .as_deref()
                .is_some_and(|a| a.ends_with("TextEdit.app")),
            "{app_of:?}"
        );
    }
    assert_eq!(crate::thumbs::icloud_app(&obsidian.join("Vault")), None);
    assert_eq!(crate::thumbs::icloud_app(&top), None);

    if std::env::var_os("MANTA_RENDER_DIR").is_some() && wgpu_available() {
        let (cloud, _) = ch.cloud();
        let mut h = harness_de_cloud(&fx, Theme::Dark, cloud, None);
        app(&mut h).test_space = true;
        app(&mut h).test_type_icons = true;
        open_local(&mut h, Side::Left, &top);
        open_local(&mut h, Side::Right, &obsidian.join("Vault"));
        super::wait_until(&mut h, "the free space", |h| {
            h.query_by_label_contains("verfügbar").is_some()
        });
        save_to(&mut h, &render_dir("cloud"), "icloud_app_folders_de_dark");
    }
    std::env::remove_var(cloud_local::HOME_OVERRIDE_ENV);
}

#[test]
fn google_drive_connection_needs_the_users_own_client() {
    let fx = fixture();
    let dir = tempfile::tempdir().unwrap();
    let client = dir.path().join("Manta/gdrive-client.json");
    let (cloud, _) = CloudHome::new(true).cloud();
    let mut h = harness_cloud(&fx, Theme::Light, false, cloud, Some(client.clone()));
    act(&mut h, Action::ShowConnect);
    assert!(matches!(&app(&mut h).dialog, Some(Dialog::Connect(f)) if !f.gdrive));
    assert!(h.query_by_label("Google Drive").is_none(), "not offered");
    app(&mut h).dialog = None;
    h.run_steps(2);

    // Expert: the user saves an OAuth client of their own.
    manta_vfs::gdrive::OAuthClient::save_file(&client, "1234-abc.apps.googleusercontent.com", "s")
        .unwrap();
    act(&mut h, Action::ShowConnect);
    assert!(matches!(&app(&mut h).dialog, Some(Dialog::Connect(f)) if f.gdrive));
    h.get_by_label("Google Drive").click();
    h.run_steps(3);
    let Some(Dialog::Connect(f)) = &app(&mut h).dialog else {
        panic!("connect dialog")
    };
    assert_eq!(f.protocol, Protocol::GoogleDrive);
    let cfg = f.to_config().unwrap();
    assert_eq!(
        (cfg.protocol, cfg.host.as_str(), cfg.username.as_str()),
        (Protocol::GoogleDrive, "drive.google.com", "")
    );
}

// ------------------------------------------------------------------ renders

fn harness_de_cloud(fx: &Fixture, theme: Theme, cloud: Cloud, client_file: Option<PathBuf>) -> H {
    let mut h = harness_cloud(fx, theme, true, cloud, client_file);
    crate::i18n::set_locale(crate::i18n::Locale::De);
    h.run_steps(3);
    h
}

/// Right-click a row of the List view (rows are 24 pt, the first one's
/// centre ~120 pt from the top in the WIDE window).
fn right_click_row(h: &mut H, side: Side, name: &str) {
    let tab = app(h).pane(side).tab();
    let vi = tab.view_index_of(name).expect("row") as f32;
    let y = 120.0 + vi * crate::theme::size::ROW - tab.scroll_offset;
    let x = if side == Side::Left { 420.0 } else { 1000.0 };
    let pos = egui::pos2(x, y);
    h.hover_at(pos);
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn render_home() -> (CloudHome, Arc<FakeProbe>, Cloud) {
    let ch = CloudHome::new(true);
    let (cloud, probe) = ch.cloud();
    let my = ch.my_drive();
    probe.set_dataless(&my.join("Holiday.mov"), true);
    probe.set_dataless(&my.join("Report Q3.pdf"), true);
    probe.set_dataless(&my.join("Photos 2025"), false);
    (ch, probe, cloud)
}

#[test]
fn render_cloud() {
    require_gpu!();
    let fx = fixture();
    let out = render_dir("cloud");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        // Sidebar section + badges: the list (left) and icons (right) of My Drive.
        let (ch, _probe, cloud) = render_home();
        let mut h = harness_de_cloud(&fx, theme, cloud, None);
        open_local(&mut h, Side::Left, &ch.my_drive());
        open_local(&mut h, Side::Right, &ch.my_drive());
        app(&mut h).pane_mut(Side::Right).view = crate::views::ViewMode::Icons;
        select(&mut h, Side::Left, &["Holiday.mov"]);
        save_to(&mut h, &out, &format!("cloud_sidebar_badges_de_{tname}"));

        // The context menu of a cloud-only file.
        right_click_row(&mut h, Side::Left, "Holiday.mov");
        save_to(&mut h, &out, &format!("cloud_menu_de_{tname}"));

        // Upload states: uploading, waiting, failed (list and icons), with
        // the error's tooltip.
        let (ch, probe, cloud) = render_home();
        let my = ch.my_drive();
        probe.set_upload(
            &my.join("Budget 2026.xlsx"),
            Some(SyncInfo::new(SyncState::Uploading)),
        );
        probe.set_upload(
            &my.join("notes.txt"),
            Some(SyncInfo {
                upload_waiting: true,
                ..SyncInfo::new(SyncState::Uploading)
            }),
        );
        probe.set_upload(
            &my.join("Report Q3.pdf"),
            Some(SyncInfo {
                state: SyncState::UploadError,
                upload_error: Some("Dein Google Drive-Speicher ist voll.".into()),
                ..SyncInfo::default()
            }),
        );
        let mut h = harness_de_cloud(&fx, theme, cloud, None);
        open_local(&mut h, Side::Left, &my);
        open_local(&mut h, Side::Right, &my);
        app(&mut h).pane_mut(Side::Right).view = crate::views::ViewMode::Icons;
        h.run_steps(3);
        save_to(&mut h, &out, &format!("cloud_upload_badges_de_{tname}"));

        // No Google Drive yet: the hint, unfolded.
        let ch2 = CloudHome::new(false);
        let (cloud2, _) = ch2.cloud();
        let mut h = harness_de_cloud(&fx, theme, cloud2, None);
        h.ctx
            .data_mut(|d| d.insert_temp(crate::cloud::hint_open_id(), true));
        save_to(&mut h, &out, &format!("cloud_gdrive_hint_de_{tname}"));

        // Settings → General → Cloud, Expert part open.
        let dir = tempfile::tempdir().unwrap();
        let client = dir.path().join("gdrive-client.json");
        let (cloud3, _) = ch2.cloud();
        let mut h = harness_de_cloud(&fx, theme, cloud3, Some(client));
        let a = app(&mut h);
        a.settings.show(Some(crate::settings::SettingsTab::General));
        a.settings.cloud.open_expert();
        a.settings.scroll_to = Some(10_000.0);
        h.run_steps(4);
        save_to(&mut h, &out, &format!("cloud_settings_de_{tname}"));
    }
    crate::i18n::set_thread_locale(None);
}
