//! The location chip at the start of the path bar: it says where the pane
//! is („Lokal“, „Docker · …“, „SFTP · user@host“, „Google Drive“, …) and,
//! away from the Mac's own folders, has a "Zurück zu Lokal" button.
//!
//! `render_place_chips` writes German light/dark PNGs to
//! `$MANTA_RENDER_DIR/ux022`. Cloud folders are a fake home in a temp dir;
//! the SFTP pane is the fixture behind a fake connection.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::Theme;
use egui_kittest::kittest::Queryable;
use manta_core::{Auth, ConnectionConfig, Protocol, SharedVfs, VfsPath};
use manta_vfs::cloud_local;

use super::cloud_ux_tests::{harness_cloud, open_local};
use super::containers_ux::connect;
use super::{app, fixture, render_dir, save_to, settle, wgpu_available, Fixture, H};
use crate::app::Side;
use crate::cloud::{Cloud, FakeProbe};

/// A fake home with Google Drive for desktop and iCloud Drive.
fn cloud_home() -> (tempfile::TempDir, PathBuf, Cloud) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let my = home.join("Library/CloudStorage/GoogleDrive-anna@gmail.com/My Drive");
    std::fs::create_dir_all(my.join("Fotos")).unwrap();
    std::fs::write(my.join("Budget 2026.xlsx"), b"x").unwrap();
    std::fs::create_dir_all(home.join("Library/Mobile Documents/com~apple~CloudDocs")).unwrap();
    let h2 = home.clone();
    let cloud = Cloud::new(
        Box::new(move || cloud_local::discover_in(&h2)),
        Arc::new(FakeProbe::default()),
        true,
    );
    (dir, my, cloud)
}

fn harness(fx: &Fixture, theme: Theme, gpu: bool) -> (H, tempfile::TempDir, PathBuf) {
    let (dir, my, cloud) = cloud_home();
    let h = harness_cloud(fx, theme, gpu, cloud, None);
    (h, dir, my)
}

/// How many widgets carry `label` (the sidebar has a "Google Drive" row too).
fn count(h: &H, label: &str) -> usize {
    h.query_all_by_label(label).count()
}

fn has(h: &H, label: &str) -> bool {
    count(h, label) > 0
}

/// A fake SFTP connection to `deploy@web-01` that shows the fixture.
fn connect_sftp(h: &mut H, side: Side, fx: &Fixture) {
    let config = ConnectionConfig {
        name: "web-01".into(),
        protocol: Protocol::Sftp,
        host: "web-01".into(),
        port: 22,
        username: "deploy".into(),
        auth: Auth::Agent,
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
    };
    let fs: SharedVfs = Arc::new(manta_vfs::local::LocalFs);
    let home = VfsPath::new(&fx.root.to_string_lossy());
    let a = app(h);
    let action = a.actions.next_id();
    a.on_connected(
        crate::worker::ConnectRequest {
            config,
            favorite: None,
            side,
            action,
            keychain: false,
        },
        Ok((fs, home)),
    );
    settle(h);
}

/// Open a folder on the Mac in `side` (whatever the pane showed before).
fn open_on_mac(h: &mut H, side: Side, dir: &std::path::Path) {
    app(h).activate(side);
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, crate::app::Action::OpenLocal(dir.to_path_buf()));
    settle(h);
}

#[test]
fn chip_says_where_the_pane_is() {
    let fx = fixture();
    let (mut h, _dir, my) = harness(&fx, Theme::Light, false);
    assert!(has(&h, "Local"), "on the Mac");
    assert!(!has(&h, "Back to Local"), "no back button on the Mac");

    connect(&mut h, Side::Left, "shop-web");
    assert!(has(&h, "Docker · shop-web"));
    assert!(has(&h, "Back to Local"));

    connect_sftp(&mut h, Side::Right, &fx);
    assert!(has(&h, "SFTP · deploy@web-01"));

    assert_eq!(count(&h, "Back to Local"), 2, "both panes are away");
    let sidebar = count(&h, "Google Drive");
    open_on_mac(&mut h, Side::Right, &my);
    assert_eq!(
        count(&h, "Google Drive"),
        sidebar + 1,
        "the provider, without the account"
    );
    assert!(!has(&h, "Google Drive · anna@gmail.com"));
}

#[test]
fn back_arrow_returns_to_the_last_local_folder() {
    let fx = fixture();
    let (mut h, _dir, my) = harness(&fx, Theme::Light, false);
    let before = app(&mut h).pane(Side::Left).tab().loc.path.clone();

    // From a container.
    app(&mut h).activate(Side::Left);
    connect(&mut h, Side::Left, "shop-web");
    assert!(app(&mut h).pane(Side::Left).tab().loc.conn.is_some());
    h.get_by_label("Back to Local").click();
    settle(&mut h);
    let t = app(&mut h).pane(Side::Left).tab();
    assert!(t.loc.conn.is_none(), "back on the Mac");
    assert_eq!(t.loc.path, before);

    // From a cloud folder: the last folder outside it, not the cloud folder.
    let sidebar = count(&h, "Google Drive");
    open_local(&mut h, Side::Left, &my);
    open_local(&mut h, Side::Left, &my.join("Fotos"));
    assert_eq!(count(&h, "Google Drive"), sidebar + 1);
    h.get_by_label("Back to Local").click();
    settle(&mut h);
    assert_eq!(
        app(&mut h).pane(Side::Left).tab().loc.path,
        before,
        "skips the cloud folders in the history"
    );
    assert_eq!(count(&h, "Google Drive"), sidebar);
    assert!(has(&h, "Local") && !has(&h, "Back to Local"));
}

// ------------------------------------------------------------------ renders

fn render(theme: Theme, tag: &str) {
    let fx = fixture();
    let (mut h, _dir, my) = harness(&fx, theme, true);
    crate::i18n::set_locale(crate::i18n::Locale::De);
    h.run_steps(3);
    let dir = render_dir("chip");
    connect(&mut h, Side::Left, "shop-web");
    open_on_mac(&mut h, Side::Right, &my);
    save_to(&mut h, &dir, &format!("place-docker-gdrive-{tag}"));
    // The toolbar: Quick Look is the eye, hidden files a dashed file
    // (off, then on = highlighted).
    for (on, state) in [(false, "off"), (true, "on")] {
        app(&mut h).pane_mut(Side::Left).tab_mut().show_hidden = on;
        h.run_steps(3);
        let img = h.render().expect("render");
        let sub = image::imageops::crop_imm(&img, 0, 0, img.width(), 110).to_image();
        sub.save(dir.join(format!("toolbar-hidden-{state}-{tag}.png")))
            .expect("save");
    }
    app(&mut h).pane_mut(Side::Left).tab_mut().show_hidden = false;
    connect_sftp(&mut h, Side::Left, &fx);
    save_to(&mut h, &dir, &format!("place-sftp-gdrive-{tag}"));
    // Narrow panes: the chip shrinks to its icon.
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, crate::app::Action::SplitPane(Side::Right));
    settle(&mut h);
    save_to(&mut h, &dir, &format!("place-narrow-{tag}"));
    crate::i18n::set_locale(crate::i18n::Locale::En);
}

#[test]
fn render_place_chips() {
    require_gpu!();
    render(Theme::Light, "light-de");
    render(Theme::Dark, "dark-de");
}

// ------------------------------------------------------------------ room

/// A harness of `size` with the fake cloud home.
fn harness_sized(
    fx: &Fixture,
    size: [f32; 2],
    theme: Theme,
    gpu: bool,
) -> (H, tempfile::TempDir, PathBuf) {
    let (dir, my, cloud) = cloud_home();
    let h = super::harness_init(
        fx,
        size,
        theme,
        gpu,
        Box::new(|ctx: &eframe::egui::Context| {
            manta_plugin::host::PluginHost::new(ctx.clone(), Vec::new(), None)
        }),
        move |init| init.cloud = Some(cloud),
    );
    (h, dir, my)
}

fn filter_label() -> &'static str {
    crate::commands::Cmd::Filter.label()
}

fn filter_focused(h: &H, side: Side) -> bool {
    h.ctx
        .memory(|m| m.has_focus(crate::pane::filter_field_id(side)))
}

fn filter_open(h: &mut H, side: Side) -> bool {
    app(h).pane(side).tab().filter_open
}

/// Two panes in a 1280 pt window: the chip shows its text in full, the
/// filters are magnifiers.
#[test]
fn chip_keeps_its_text_at_1280() {
    let fx = fixture();
    let (mut h, _d, my) = harness(&fx, Theme::Light, false);
    connect_sftp(&mut h, Side::Left, &fx);
    open_on_mac(&mut h, Side::Right, &my);
    h.run_steps(3);
    assert!(has(&h, "SFTP · deploy@web-01"));
    assert_eq!(count(&h, filter_label()), 2, "both filters folded");
}

#[test]
fn folded_filter_opens_on_click_and_cmd_f_and_closes_on_escape() {
    use eframe::egui::{Key, Modifiers};
    let fx = fixture();
    let (mut h, _d, _my) = harness(&fx, Theme::Light, false);
    open_on_mac(&mut h, Side::Left, &fx.root);
    h.run_steps(2);
    assert!(has(&h, filter_label()));
    assert!(!filter_open(&mut h, Side::Left));

    // Click: the field opens, focused.
    h.query_all_by_label(filter_label()).next().unwrap().click();
    h.run_steps(3);
    assert!(filter_open(&mut h, Side::Left), "open after a click");
    assert!(filter_focused(&h, Side::Left));
    // Text keeps it open, even without focus.
    app(&mut h).pane_mut(Side::Left).tab_mut().filter = "rep".into();
    h.ctx
        .memory_mut(|m| m.surrender_focus(crate::pane::filter_field_id(Side::Left)));
    h.run_steps(3);
    assert!(filter_open(&mut h, Side::Left), "text keeps it open");
    app(&mut h).pane_mut(Side::Left).tab_mut().filter.clear();
    h.run_steps(3);
    assert!(
        !filter_open(&mut h, Side::Left),
        "empty and unfocused: folded"
    );

    // ⌘F opens it too; Escape with nothing typed folds it again.
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(3);
    assert!(filter_open(&mut h, Side::Left), "open after ⌘F");
    assert!(filter_focused(&h, Side::Left));
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!filter_open(&mut h, Side::Left), "Escape folds it");
    assert!(!filter_focused(&h, Side::Left));
    assert!(has(&h, filter_label()));
}

/// Wide panes keep the field.
#[test]
fn wide_panes_show_the_filter_field() {
    let fx = fixture();
    let (mut h, _d, _my) = harness_sized(&fx, [2200.0, 800.0], Theme::Light, false);
    h.run_steps(3);
    assert!(!has(&h, filter_label()));
}

/// 1280 and 1440 pt wide, two and three panes: SFTP, Google Drive, Docker.
#[test]
fn render_path_bar_room() {
    require_gpu!();
    let dir = render_dir("chip");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for w in [1280.0f32, 1440.0] {
            let fx = fixture();
            let (mut h, _d, my) = harness_sized(&fx, [w, 800.0], theme, true);
            crate::i18n::set_locale(crate::i18n::Locale::De);
            h.run_steps(3);
            connect_sftp(&mut h, Side::Left, &fx);
            open_on_mac(&mut h, Side::Right, &my);
            app(&mut h).activate(Side::Left);
            save_to(&mut h, &dir, &format!("room-{w}-2panes-{tname}"));
            let ctx = h.ctx.clone();
            app(&mut h).handle_action(&ctx, crate::app::Action::SplitPane(Side::Right));
            settle(&mut h);
            let third = app(&mut h).panes.last().unwrap().id;
            connect(&mut h, third, "shop-web");
            app(&mut h).activate(Side::Left);
            save_to(&mut h, &dir, &format!("room-{w}-3panes-{tname}"));
            // A folded filter, opened with text.
            app(&mut h).pane_mut(Side::Left).tab_mut().filter = "re".into();
            app(&mut h).pane_mut(Side::Left).tab_mut().rebuild_view();
            save_to(&mut h, &dir, &format!("room-{w}-3panes-filter-{tname}"));
            crate::i18n::set_locale(crate::i18n::Locale::En);
        }
    }
}
