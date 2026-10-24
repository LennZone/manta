//! More of the user manual's screenshots (see `manual_images_tests.rs`):
//! Go to Path, the path bar, View Options, the four views, dragging to
//! Favorites, the Git panel, the terminal, Disk Usage and Compare & Sync's
//! A/B picker. Synthetic data only: temp folders under `Users/anna`, a
//! fake home for `~`, an in-memory SFTP server at `example.com`, a
//! throwaway Git repository with made-up authors (no global Git config),
//! a scripted container shell that only prints text (never a login shell
//! or the user's profile). Opt-in like the others:
//!
//! `MANTA_MANUAL_IMAGES=1 cargo test -p manta render_manual_images -- --test-threads=1`
//!
//! Child of `ux_tests`.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Rect, Theme};
use egui_kittest::kittest::Queryable;
use image::RgbaImage;
use manta_core::{ConnectionConfig, Entry, Protocol, SharedVfs, Vfs, VfsPath, VfsResult};
use manta_ops::memfs::MemFs;
use manta_ops::CompareStatus;
use manta_plugin::{FileAction, FileActionKind, FileActionPhase, PaneId, PaneState};
use manta_plugin_terminal::remote::Target;
use manta_plugin_terminal::session::RemoteSpawn;
use manta_plugin_terminal::TerminalPlugin;

use super::*;
use crate::app::Action;
use crate::i18n::{set_locale, Locale};
use crate::manual_shots::{self, Crop};
use crate::pane::DragPayload;
use crate::views::ViewMode;
use crate::worker::ConnectRequest;

const PPP: f32 = 2.0;

macro_rules! manual_only {
    () => {
        if !manual_shots::enabled() || !wgpu_available() {
            return;
        }
    };
}

fn shot(h: &mut H, lang: &str, name: &str, crop: Crop) {
    h.event(egui::Event::PointerGone);
    let img = render(h);
    let rect = manual_shots::crop_rect(&h.ctx, crop);
    manual_shots::save(&img, PPP, rect, lang, name);
}

/// Render the current frame in the test's language.
fn render(h: &mut H) -> RgbaImage {
    manual_shots::use_fonts(&h.ctx);
    app(h).plugins.relocalize();
    h.run_steps(4);
    h.render().expect("render")
}

fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    settle(h);
}

/// Step for `d` (background threads: Git, the terminal, folder sizes).
fn step_for(h: &mut H, d: Duration) {
    let start = Instant::now();
    while start.elapsed() < d {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Step until `f` holds (at most 20 s; the shot is taken either way).
fn step_until(h: &mut H, what: &str, mut f: impl FnMut(&mut H) -> bool) {
    let start = Instant::now();
    while !f(h) {
        if start.elapsed() > Duration::from_secs(20) {
            eprintln!("manual image: gave up waiting for {what}");
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn local(p: &std::path::Path) -> VfsPath {
    VfsPath::new(&p.to_string_lossy())
}

fn pane_rect(h: &mut H, side: Side) -> Rect {
    app(h)
        .pane_rects
        .iter()
        .find(|(s, _)| *s == side)
        .map(|(_, r)| *r)
        .expect("pane drawn")
}

/// `r` (points) out of `img` (pixels).
fn cut(img: &RgbaImage, r: Rect) -> RgbaImage {
    let x = (r.left() * PPP).max(0.0) as u32;
    let y = (r.top() * PPP).max(0.0) as u32;
    let w = ((r.width() * PPP) as u32).min(img.width() - x);
    let h = ((r.height() * PPP) as u32).min(img.height() - y);
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

/// Lay `parts` out in a grid `cols` wide, `gap` pixels apart, on `bg`.
fn grid(parts: &[RgbaImage], cols: usize, gap: u32, bg: image::Rgba<u8>) -> RgbaImage {
    let cols = cols.max(1);
    let rows = parts.len().div_ceil(cols);
    let cw = parts.iter().map(|p| p.width()).max().unwrap_or(0);
    let ch = parts.iter().map(|p| p.height()).max().unwrap_or(0);
    let w = cols as u32 * cw + (cols as u32 + 1) * gap;
    let h = rows as u32 * ch + (rows as u32 + 1) * gap;
    let mut out = RgbaImage::from_pixel(w, h, bg);
    for (i, p) in parts.iter().enumerate() {
        let x = gap + (i % cols) as u32 * (cw + gap);
        let y = gap + (i / cols) as u32 * (ch + gap);
        image::imageops::overlay(&mut out, p, x as i64, y as i64);
    }
    out
}

/// The window background (top-left corner of a render).
fn window_bg(img: &RgbaImage) -> image::Rgba<u8> {
    *img.get_pixel(2, img.height() / 2)
}

// ------------------------------------------------------------ fake backends

/// The local disk, but `~` is the fixture's `Users/anna`.
struct HomeFs {
    inner: manta_vfs::LocalFs,
    home: VfsPath,
}

impl Vfs for HomeFs {
    fn label(&self) -> String {
        self.inner.label()
    }
    fn protocol(&self) -> Protocol {
        self.inner.protocol()
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(self.home.clone())
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.inner.list(dir)
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        self.inner.stat(path)
    }
    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.inner.create_dir(path)
    }
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.inner.remove_file(path)
    }
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.inner.remove_dir(path)
    }
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.inner.rename(from, to)
    }
    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        self.inner.open_read(path)
    }
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.inner.open_write(path)
    }
    fn local_path(&self, path: &VfsPath) -> Option<PathBuf> {
        self.inner.local_path(path)
    }
}

/// An SFTP server in memory (`anna@example.com`).
struct MemSftp(MemFs);

impl Vfs for MemSftp {
    fn label(&self) -> String {
        "anna@example.com (SFTP)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Sftp
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::new("/home/anna"))
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.0.list(dir)
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        self.0.stat(path)
    }
    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.0.create_dir(path)
    }
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.0.remove_file(path)
    }
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.0.remove_dir(path)
    }
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.0.rename(from, to)
    }
    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        self.0.open_read(path)
    }
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.0.open_write(path)
    }
}

/// Show `/srv/www/shop` on the in-memory server in `side`.
fn open_sftp(h: &mut H, side: Side) {
    let mem = MemFs::new();
    let now = chrono::Utc::now();
    mem.mkdir_p("/srv/www/shop/public");
    mem.mkdir_p("/srv/www/shop/storage");
    mem.put("/srv/www/shop/index.php", b"<?php", now);
    mem.put("/srv/www/shop/.htaccess", b"x", now);
    let fs: SharedVfs = Arc::new(MemSftp(mem));
    let config = ConnectionConfig {
        name: String::new(),
        protocol: Protocol::Sftp,
        host: "example.com".into(),
        port: 22,
        username: "anna".into(),
        auth: manta_core::Auth::Agent,
        initial_path: Some("/srv/www/shop".into()),
        s3: None,
        container: None,
        ftp: None,
        webdav: None,
        smb: None,
    };
    let a = app(h);
    let action = a.actions.next_id();
    a.on_connected(
        ConnectRequest {
            config,
            favorite: None,
            side,
            action,
            keychain: false,
        },
        Ok((fs, VfsPath::new("/srv/www/shop"))),
    );
    settle(h);
}

// ------------------------------------------------------------ navigation

/// 04: Go to Path (fake home), the path bar (SFTP and a Git project);
/// 03: View Options and the four views; 07: dragging onto Favorites.
#[test]
fn render_manual_images_navigation() {
    manual_only!();
    for (locale, lang) in manual_shots::langs() {
        go_to_path(locale, lang);
        path_bar(locale, lang);
        view_options(locale, lang);
        views_overview(locale, lang);
        favorites_drag(locale, lang);
    }
    set_locale(Locale::En);
}

fn light_at(fx: &Fixture, locale: Locale, size: [f32; 2]) -> H {
    set_locale(locale);
    let mut h = harness(fx, size, Theme::Light);
    set_locale(locale);
    h.run_steps(3);
    h
}

fn go_to_path(locale: Locale, lang: &str) {
    let fx = fixture_at("Users/anna/Projects");
    let home = fx.root.parent().unwrap().to_path_buf();
    for d in [
        "Desktop",
        "Developer",
        "Documents",
        "Documents/Invoices",
        "Downloads",
        "Music",
        "Pictures",
    ] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    let mut h = light_at(&fx, locale, WIDE);
    app(&mut h).goto_recent = vec![
        (None, local(&home.join("Documents/Invoices"))),
        (None, local(&home.join("Downloads"))),
        (None, local(&home.join("Downloads/Fonts"))),
        (None, local(&fx.root.join("photos"))),
    ];
    act(&mut h, Action::GoToPath(Side::Left));
    {
        let g = app(&mut h).goto.as_mut().expect("popup");
        g.fs = Arc::new(HomeFs {
            inner: manta_vfs::LocalFs::new(),
            home: local(&home),
        });
        g.home = Some(local(&home));
    }
    h.event(egui::Event::Text("~/Do".into()));
    h.run_steps(2);
    // Tab lists the matching folders of the (fake) home: Documents and
    // Downloads instead of the three recent folders.
    app(&mut h).goto.as_mut().expect("popup").tab_pressed = true;
    step_until(&mut h, "matching folders", |h| {
        app(h).goto.as_ref().is_some_and(|g| g.rows_len() == 2)
    });
    h.run_steps(3);
    shot(&mut h, lang, "go-to-path", Crop::Modal);
}

/// Two path bars, one above the other: a pane on an SFTP server (location
/// chip, Back to Local) and a local Git project (Git and Project chips).
fn path_bar(locale: Locale, lang: &str) {
    let h0 = super::readme_images_tests::home();
    let config = tempfile::tempdir().unwrap();
    let cfg = config.path().to_path_buf();
    let home = h0.home.to_string_lossy().into_owned();
    set_locale(locale);
    let mut h = harness_full(
        &h0.fx,
        [1440.0, 600.0],
        Theme::Light,
        true,
        Box::new(move |ctx: &egui::Context| {
            PluginHost::new(
                ctx.clone(),
                vec![
                    Box::new(manta_plugin_git::GitPlugin::new()),
                    Box::new(manta_plugin_projectinfo::ProjectInfoPlugin::new().with_home(&home)),
                ],
                Some(manta_plugin::host::HostPaths::under(&cfg)),
            )
        }),
    );
    set_locale(locale);
    open_sftp(&mut h, Side::Left);
    app(&mut h).navigate_path(Side::Right, local(&h0.repo));
    settle(&mut h);
    step_until(&mut h, "path bar chips", |h| {
        let ids: Vec<String> = h
            .state()
            .plugins
            .chips(PaneId::Right)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        ids.iter().any(|i| i == "manta.git") && ids.iter().any(|i| i == PI)
    });
    app(&mut h).activate(Side::Left);
    h.event(egui::Event::PointerGone);
    let img = render(&mut h);
    let bar = |h: &mut H, side: Side| {
        let r = pane_rect(h, side);
        Rect::from_min_size(r.min, egui::vec2(r.width(), crate::theme::size::PATH_BAR))
            .expand2(egui::vec2(0.0, 4.0))
    };
    let parts = [
        cut(&img, bar(&mut h, Side::Left)),
        cut(&img, bar(&mut h, Side::Right)),
    ];
    let out = grid(&parts, 1, 12, window_bg(&img));
    manual_shots::save(&out, PPP, None, lang, "path-bar");
}

fn photos_harness(locale: Locale, size: [f32; 2]) -> (Fixture, H, PathBuf) {
    let fx = fixture_at("Users/anna/Projects");
    let dir = super::views_ux_tests::photos(&fx);
    let mut h = light_at(&fx, locale, size);
    act(&mut h, Action::ClosePane(Side::RIGHT));
    app(&mut h).activate(Side::LEFT);
    super::views_ux_tests::goto(&mut h, Side::LEFT, &dir);
    (fx, h, dir)
}

/// The View Options popover over an Icons view.
fn view_options(locale: Locale, lang: &str) {
    let (_fx, mut h, _dir) = photos_harness(locale, [960.0, 640.0]);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
    super::views_ux_tests::settle_thumbs(&mut h);
    let id = egui::Id::unique(("view-options", Side::LEFT));
    egui::Popup::open_id(&h.ctx, id.with("popup"));
    h.run_steps(3);
    let img = render(&mut h);
    let pane = pane_rect(&mut h, Side::LEFT);
    let popup = h.ctx.memory(|m| {
        m.layer_ids()
            .filter(|l| l.order == egui::Order::Foreground)
            .filter_map(|l| m.area_rect(l.id))
            .filter(|r| r.width() > 60.0 && r.width() < pane.width())
            .max_by(|a, b| a.area().total_cmp(&b.area()))
    });
    let bottom = popup.map_or(pane.top() + 300.0, |r| r.bottom()) + 16.0;
    let left = pane.left() - 4.0;
    let r = Rect::from_min_max(
        egui::pos2(left, pane.top() - 4.0),
        egui::pos2(pane.right(), bottom),
    );
    manual_shots::save(&img, PPP, Some(r), lang, "view-options");
}

/// 2 × 2: Icons, List, Columns, Gallery of the same photo folder.
fn views_overview(locale: Locale, lang: &str) {
    let mut parts = Vec::new();
    let mut bg = image::Rgba([255, 255, 255, 255]);
    for mode in [
        ViewMode::Icons,
        ViewMode::List,
        ViewMode::Columns,
        ViewMode::Gallery,
    ] {
        let (_fx, mut h, _dir) = photos_harness(locale, [900.0, 560.0]);
        act(&mut h, Action::SetView(Side::LEFT, mode));
        if let Some(i) = app(&mut h)
            .pane(Side::LEFT)
            .tab()
            .view_index_of("Hafen.jpg")
        {
            app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
        }
        if mode == ViewMode::Columns {
            app(&mut h)
                .pane_mut(Side::LEFT)
                .tab_mut()
                .view_state
                .cols
                .wait_loaded(Duration::from_secs(5));
        }
        super::views_ux_tests::settle_thumbs(&mut h);
        h.event(egui::Event::PointerGone);
        let img = render(&mut h);
        bg = window_bg(&img);
        let r = pane_rect(&mut h, Side::LEFT);
        parts.push(cut(&img, r));
    }
    let out = grid(&parts, 2, 24, bg);
    manual_shots::save(&out, PPP, None, lang, "views-overview");
}

/// Two folders dragged onto Favorites: insertion line and badge.
fn favorites_drag(locale: Locale, lang: &str) {
    let fx = fixture_at("Users/anna/Projects");
    let mut h = light_at(&fx, locale, WIDE);
    app(&mut h).activate(Side::LEFT);
    select(&mut h, Side::LEFT, &["src", "photos"]);
    let fs = app(&mut h).pane(Side::LEFT).tab().loc.fs.clone();
    let payload = DragPayload {
        from: Side::LEFT,
        fs,
        paths: vec![local(&fx.root.join("src")), local(&fx.root.join("photos"))],
        dirs: vec![true, true],
        conn: None,
        archive: false,
    };
    let r = app(&mut h).fav_rects[0];
    let pos = egui::pos2(r.center().x + 10.0, r.bottom() - 3.0);
    egui::DragAndDrop::set_payload(&h.ctx, payload);
    h.hover_at(pos);
    h.run_steps(3);
    let img = render(&mut h);
    let r = Rect::from_min_max(egui::pos2(0.0, 40.0), egui::pos2(560.0, 360.0));
    manual_shots::save(&img, PPP, Some(r), lang, "favorites-drag");
    egui::DragAndDrop::clear_payload(&h.ctx);
}

// ------------------------------------------------------------ tools

/// 12: the Git panel; 11: the terminal; 14: Disk Usage; 13: Compare &
/// Sync with three panes and the A/B picker open.
#[test]
fn render_manual_images_tools() {
    manual_only!();
    for (locale, lang) in manual_shots::langs() {
        git_panel(locale, lang);
        terminal(locale, lang);
        disk_usage(locale, lang);
        compare_sync_ab(locale, lang);
    }
    set_locale(Locale::En);
}

fn git_panel(locale: Locale, lang: &str) {
    let h0 = super::readme_images_tests::home();
    set_locale(locale);
    let mut h = harness_full(
        &h0.fx,
        WIDE,
        Theme::Light,
        true,
        Box::new(|ctx: &egui::Context| {
            PluginHost::new(
                ctx.clone(),
                vec![Box::new(manta_plugin_git::GitPlugin::new())],
                None,
            )
        }),
    );
    set_locale(locale);
    act(&mut h, Action::ClosePane(Side::Right));
    app(&mut h).activate(Side::Left);
    app(&mut h).navigate_path(Side::Left, local(&h0.repo));
    settle(&mut h);
    app(&mut h)
        .plugins
        .set_panel_open("manta.git", "git", None, true);
    step_until(&mut h, "git chip", |h| {
        h.state()
            .plugins
            .chips(PaneId::Left)
            .iter()
            .any(|(id, _)| id == "manta.git")
    });
    step_for(&mut h, Duration::from_millis(800));
    select(&mut h, Side::Left, &["routes"]);
    if let Some(field) = h
        .query_all_by_role(egui::accesskit::Role::MultilineTextInput)
        .next()
    {
        field.focus();
    }
    h.run_steps(2);
    h.event(egui::Event::Text("Add the cart".into()));
    h.run_steps(2);
    shot(&mut h, lang, "git-panel", Crop::Full);
}

/// What the scripted container shell prints: a `cd`, then a copy made in
/// the pane above, echoed with its result (localized like the plugin).
fn terminal_transcript() -> String {
    use manta_plugin_terminal::actions::{result_text, styled_command, styled_result};
    let web = super::readme_images_tests::WEB;
    let prompt = |dir: &str| format!("\x1b[1;32mshop-web\x1b[0m:\x1b[1;34m{dir}\x1b[0m# ");
    let storage = format!("{web}/storage/app");
    let mut s = String::new();
    s += &format!("{}cd storage/app\r\n", prompt(web));
    s += &format!("{}ls\r\n", prompt(&storage));
    s += "\x1b[1;34mbackup\x1b[0m  \x1b[1;34mpublic\x1b[0m  \x1b[1;34muploads\x1b[0m\r\n";
    let mut a = FileAction::new(
        1,
        PaneId::Left,
        FileActionKind::Copy,
        vec![VfsPath::new(&format!("{storage}/uploads"))],
    );
    a.destination = Some(VfsPath::new(&format!("{storage}/backup")));
    a.phase = FileActionPhase::Finished {
        ok: true,
        files: 128,
        bytes: 46_800_000,
        errors: vec![],
        cancelled: false,
        elapsed_ms: 1_400,
    };
    s += &styled_command("cp -R 'uploads' 'backup/'");
    s += "\r\n";
    if let Some((outcome, text)) = result_text(&a, None) {
        s += &styled_result(outcome, &text);
        s += "\r\n";
    }
    s += &prompt(&storage);
    s
}

fn terminal(locale: Locale, lang: &str) {
    let fx = fixture_at("Users/anna/Projects");
    set_locale(locale);
    let transcript = terminal_transcript();
    let spawn = move |target: &Target, pane: &PaneState| RemoteSpawn {
        // Prints the transcript and waits: no login shell, no profile.
        program: PathBuf::from("/bin/sh"),
        args: vec![
            "-c".into(),
            "stty -echo 2>/dev/null; printf '%s' \"$1\"; exec sleep 600".into(),
            "sh".into(),
            transcript.clone(),
        ],
        osc_host: target.osc_host(),
        start_dir: pane.path.as_str().to_string(),
    };
    let mut h = harness_full(
        &fx,
        [1120.0, 720.0],
        Theme::Dark,
        true,
        Box::new(move |ctx: &egui::Context| {
            PluginHost::new(
                ctx.clone(),
                vec![Box::new(
                    TerminalPlugin::new().with_remote_override(Arc::new(spawn)),
                )],
                None,
            )
        }),
    );
    set_locale(locale);
    act(&mut h, Action::ClosePane(Side::Right));
    app(&mut h).activate(Side::Left);
    super::readme_images_tests::open_container(&mut h, Side::Left);
    {
        let a = app(&mut h);
        a.plugins
            .set_panel_open("manta.terminal", "terminal", Some(PaneId::Left), true);
        a.layout.pane_bottom_height = 300.0;
    }
    step_for(&mut h, Duration::from_secs(3));
    app(&mut h).toasts.clear();
    shot(&mut h, lang, "terminal", Crop::Full);
}

fn disk_usage(locale: Locale, lang: &str) {
    let fx = fixture_at("Users/anna/Projects");
    set_locale(locale);
    let mut h = harness_full(
        &fx,
        WIDE,
        Theme::Light,
        true,
        Box::new(|ctx: &egui::Context| {
            PluginHost::new(
                ctx.clone(),
                vec![Box::new(manta_plugin_foldersize::FolderSizePlugin::new())],
                None,
            )
        }),
    );
    set_locale(locale);
    app(&mut h).activate(Side::Left);
    app(&mut h).plugins.set_panel_open(
        "manta.foldersize",
        manta_plugin_foldersize::PANEL_ID,
        None,
        true,
    );
    step_for(&mut h, Duration::from_millis(1500));
    shot(&mut h, lang, "disk-usage", Crop::Full);
}

fn compare_sync_ab(locale: Locale, lang: &str) {
    let fx = fixture_at("Users/anna/manta-demo");
    let home = fx.root.parent().unwrap().to_path_buf();
    let downloads = home.join("Downloads");
    std::fs::create_dir_all(downloads.join("src")).unwrap();
    std::fs::write(downloads.join("README.md"), "# manta-demo\n").unwrap();
    std::fs::write(downloads.join("notes.txt"), "Buy milk\n").unwrap();
    let mut h = light_at(&fx, locale, WIDE);
    act(&mut h, Action::SplitPane(Side::Left));
    let order = app(&mut h).pane_order();
    app(&mut h).navigate_path(order[1], local(&downloads));
    app(&mut h).navigate_path(order[2], local(&fx.root.join("Design")));
    settle(&mut h);
    app(&mut h).activate(order[0]);
    app(&mut h).activate(order[1]);
    app(&mut h).activate(order[0]);
    act(&mut h, Action::ShowSync);
    step_until(&mut h, "comparison", |h| !h.state().sync.loading);
    // The cards show the (temp) folder's path: say where it would be.
    let local_label = t!("pane.local");
    app(&mut h).sync.left_label = format!("{local_label} /Users/anna/manta-demo");
    app(&mut h).sync.right_label = format!("{local_label} /Users/anna/Downloads");
    app(&mut h).sync.items = Some(vec![
        item("src", true, CompareStatus::Identical, Some(0), Some(0)),
        item(
            "src/main.rs",
            false,
            CompareStatus::LeftNewer,
            Some(1_204),
            Some(1_180),
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
            "notes.txt",
            false,
            CompareStatus::Identical,
            Some(9),
            Some(9),
        ),
    ]);
    h.run_steps(3);
    if let Some(combo) = h.query_all_by_role(egui::accesskit::Role::ComboBox).next() {
        combo.click();
    }
    h.run_steps(4);
    shot(&mut h, lang, "compare-sync-ab", Crop::Full);
}
