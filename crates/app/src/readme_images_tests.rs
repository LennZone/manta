//! `render_readme_images`: the two screenshots at the top of README.md
//! (`docs/images/readme-overview.png`, `docs/images/readme-full.png`),
//! English, 2560 × 1600 pixels, from synthetic fixtures only: temp
//! folders under `Users/anna`, a fake Google Drive folder, a Docker
//! container answered by a scripted runner (no docker), a terminal that
//! replays a scripted session in a local `sh` (no docker), a throwaway Git
//! repository with made-up authors, a fake CPU / GPU / RAM sampler and a
//! fake transfer. Opt-in because it writes into the source tree:
//!
//! `MANTA_README_IMAGES=1 cargo test -p manta render_readme_images`
//!
//! Child of `ux_tests`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Theme};
use manta_core::{SharedVfs, VfsPath};
use manta_ops::{JobKind, JobState, JobStatus};
use manta_plugin::PaneState;
use manta_plugin_sysstats::{FakeSampler, MemInfo, Pressure, SysStatsPlugin};
use manta_plugin_terminal::remote::Target;
use manta_plugin_terminal::session::RemoteSpawn;
use manta_plugin_terminal::TerminalPlugin;
use manta_vfs::cloud_local::{SyncInfo, SyncState};
use manta_vfs::container::listing::LIST_SCRIPT;
use manta_vfs::container::{docker_config, CmdOutput, ContainerFs, Proc, Runner};

use super::containers_ux::{listing, FakeProc, Item};
use super::*;
use crate::app::Side;
use crate::i18n::{set_locale, Locale};
use crate::worker::ConnectRequest;

/// 1600 × 1000 points at 1.6 pixels per point: 2560 × 1600 pixels (wide
/// enough for three panes with their location chips and the Git panel).
const SIZE: [f32; 2] = [1600.0, 1000.0];
const PPP: f32 = 1.6;

fn enabled() -> bool {
    std::env::var_os("MANTA_README_IMAGES").is_some() && wgpu_available()
}

fn out(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/images");
    std::fs::create_dir_all(&d).expect("create docs/images");
    d.join(format!("{name}.png"))
}

fn shot(h: &mut H, name: &str) {
    crate::manual_shots::use_fonts(&h.ctx);
    h.run_steps(4);
    let img = h.render().expect("render");
    let path = out(name);
    crate::manual_shots::write_indexed_png(&img, &path);
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "readme image: wrote {} ({}×{}, {} KB)",
        path.display(),
        img.width(),
        img.height(),
        len / 1024
    );
}

fn step_for(h: &mut H, d: Duration) {
    let start = Instant::now();
    while start.elapsed() < d {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ------------------------------------------------------------ fixtures

/// `Users/anna` with the standard Projects tree, a Git repository
/// `Developer/storefront` and a Google Drive folder.
pub(super) struct Home {
    pub(super) fx: Fixture,
    pub(super) repo: PathBuf,
    pub(super) drive: PathBuf,
    pub(super) home: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    git_as(dir, args, "Anna Berg", 0);
}

/// `git` as a made-up author, commits dated `ago` seconds back.
fn git_as(dir: &Path, args: &[&str], author: &str, ago: u64) {
    let email = format!(
        "{}@example.com",
        author.split(' ').next().unwrap_or("dev").to_lowercase()
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let date = format!("@{} +0000", now - ago);
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        // Hermetic: never the developer's own name, e-mail or hooks.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", &email)
        .env("GIT_COMMITTER_NAME", author)
        .env("GIT_COMMITTER_EMAIL", &email)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

pub(super) fn home() -> Home {
    let fx = fixture_at("Users/anna/Projects");
    let home = fx.root.parent().unwrap().to_path_buf();

    // A small Laravel shop with work in progress.
    let repo = home.join("Developer/storefront");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    git(&repo, &["config", "core.hooksPath", "/dev/null"]);
    for (rel, text) in [
        (".gitignore", "/vendor\n/node_modules\n.env\n"),
        (".env.example", "APP_NAME=Storefront\nAPP_ENV=local\n"),
        ("README.md", "# Storefront\n\nThe demo shop.\n"),
        ("artisan", "#!/usr/bin/env php\n<?php\n"),
        ("composer.json", "{\n  \"name\": \"acme/storefront\"\n}\n"),
        ("package.json", "{\n  \"private\": true\n}\n"),
        ("vite.config.js", "export default {};\n"),
        ("app/Models/Product.php", "<?php\n"),
        ("app/Http/Controllers/ShopController.php", "<?php\n"),
        ("config/app.php", "<?php\nreturn [];\n"),
        ("database/migrations/0001_create_products.php", "<?php\n"),
        ("public/index.php", "<?php\n"),
        ("resources/views/shop.blade.php", "<h1>Shop</h1>\n"),
        (
            "routes/web.php",
            "<?php\n\nRoute::get('/', ShopController::class);\n",
        ),
        ("tests/Feature/ShopTest.php", "<?php\n"),
    ] {
        put(&repo, rel, text);
    }
    git(&repo, &["add", "-A"]);
    git_as(
        &repo,
        &["commit", "-q", "-m", "Initial shop"],
        "Anna Berg",
        12 * 86_400,
    );
    for (rel, text, msg, who, ago) in [
        (
            "app/Models/Product.php",
            "<?php\n\nclass Product extends Model {}\n",
            "Add the product model",
            "Ben Ortiz",
            6 * 86_400,
        ),
        (
            "resources/views/shop.blade.php",
            "<h1>Shop</h1>\n<x-product-grid />\n",
            "Product grid on the start page",
            "Chris Lee",
            2 * 86_400,
        ),
        (
            "config/app.php",
            "<?php\nreturn ['name' => 'Storefront'];\n",
            "Name the app",
            "Anna Berg",
            5 * 3_600,
        ),
    ] {
        put(&repo, rel, text);
        git(&repo, &["add", "-A"]);
        git_as(&repo, &["commit", "-q", "-m", msg], who, ago);
    }
    // Staged, modified and untracked work.
    put(
        &repo,
        "app/Http/Controllers/CartController.php",
        "<?php\n\nclass CartController {}\n",
    );
    put(
        &repo,
        "routes/web.php",
        "<?php\n\nRoute::get('/', ShopController::class);\nRoute::post('/cart', CartController::class);\n",
    );
    git(
        &repo,
        &[
            "add",
            "app/Http/Controllers/CartController.php",
            "routes/web.php",
        ],
    );
    put(
        &repo,
        "resources/views/shop.blade.php",
        "<h1>Shop</h1>\n<x-product-grid />\n<x-cart-button />\n",
    );
    put(
        &repo,
        "README.md",
        "# Storefront\n\nThe demo shop, now with a cart.\n",
    );
    put(&repo, "CHANGELOG.md", "## Unreleased\n- Cart\n");

    // Google Drive for desktop.
    let drive = home.join("Library/CloudStorage/GoogleDrive-anna@example.com/My Drive");
    std::fs::create_dir_all(home.join("Library/Mobile Documents/com~apple~CloudDocs")).unwrap();
    std::fs::create_dir_all(home.join("Library/CloudStorage/OneDrive-Personal")).unwrap();
    for d in ["Photos 2025", "Invoices", "Shared with Ben"] {
        std::fs::create_dir_all(drive.join(d)).unwrap();
    }
    for (name, len) in [
        ("Budget 2026.xlsx", 48_000usize),
        ("Holiday.mov", 9_000),
        ("Launch plan.pdf", 2_400),
        ("Logo final.png", 3_100),
        ("Meeting notes.txt", 1_200),
        ("Product shots.zip", 6_000),
        ("Report Q3.pdf", 2_400),
    ] {
        std::fs::write(drive.join(name), vec![b'x'; len]).unwrap();
    }
    Home {
        fx,
        repo,
        drive,
        home,
    }
}

impl Home {
    fn cloud(&self) -> crate::cloud::Cloud {
        let probe = Arc::new(crate::cloud::FakeProbe::default());
        for f in ["Holiday.mov", "Report Q3.pdf", "Product shots.zip"] {
            probe.set_dataless(&self.drive.join(f), true);
        }
        for f in ["Logo final.png", "Launch plan.pdf"] {
            probe.set_upload(
                &self.drive.join(f),
                Some(SyncInfo::new(SyncState::Uploading)),
            );
        }
        let home = self.home.clone();
        crate::cloud::Cloud::new(
            Box::new(move || manta_vfs::cloud_local::discover_in(&home)),
            probe,
            true,
        )
    }
}

/// Answers like docker would for `shop-web` (running, WORKDIR
/// /var/www/html, a Laravel app).
pub(super) struct FakeShop;

pub(super) const WEB: &str = "/var/www/html";

impl FakeShop {
    fn answer(args: &[String]) -> CmdOutput {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["inspect", .., "shop-web"] => CmdOutput::ok(format!("true\t{WEB}\t/shop-web\n")),
            ["exec", _, "sh", "-c", "echo manta-ok"] => CmdOutput::ok("manta-ok\n"),
            ["exec", "shop-web", "sh", "-c", script, "sh", "L", dir]
                if *script == LIST_SCRIPT && *dir == WEB =>
            {
                let items: &[Item] = &[
                    ("app", 'd', None, 0, 3, 0o755),
                    ("bootstrap", 'd', None, 0, 200, 0o755),
                    ("config", 'd', None, 0, 50, 0o755),
                    ("database", 'd', None, 0, 26, 0o755),
                    ("public", 'd', None, 0, 5, 0o755),
                    ("resources", 'd', None, 0, 2, 0o755),
                    ("routes", 'd', None, 0, 2, 0o755),
                    ("storage", 'd', None, 0, 1, 0o775),
                    ("vendor", 'd', None, 0, 30, 0o755),
                    (".env", 'f', None, 1_204, 30, 0o640),
                    ("artisan", 'f', None, 425, 400, 0o755),
                    ("composer.json", 'f', None, 2_310, 30, 0o644),
                    ("composer.lock", 'f', None, 298_114, 30, 0o644),
                    ("package.json", 'f', None, 612, 60, 0o644),
                    ("vite.config.js", 'f', None, 318, 60, 0o644),
                ];
                CmdOutput::ok(listing(items))
            }
            _ => CmdOutput::fail("Error: unexpected"),
        }
    }
}

impl Runner for FakeShop {
    fn run(
        &self,
        _: &str,
        args: &[String],
        _: Option<&[u8]>,
        _: Duration,
    ) -> io::Result<CmdOutput> {
        Ok(Self::answer(args))
    }
    fn spawn(&self, _: &str, args: &[String]) -> io::Result<Box<dyn Proc>> {
        Ok(Box::new(FakeProc(Some(Self::answer(args)))))
    }
}

/// The scripted container shell: a prompt, `ls`, `php artisan --version`.
fn shell_transcript() -> String {
    let prompt = format!("\x1b[1;32mshop-web\x1b[0m:\x1b[1;34m{WEB}\x1b[0m# ");
    let dir = |n: &str| format!("\x1b[1;34m{n}\x1b[0m");
    let mut s = String::new();
    s += &format!("{prompt}ls\r\n");
    s += &format!(
        "{}       {}   {}  {}\r\n",
        dir("app"),
        dir("config"),
        dir("resources"),
        dir("vendor")
    );
    s += &format!(
        "\x1b[1;32martisan\x1b[0m   {} {}     composer.json\r\n",
        dir("database"),
        dir("routes")
    );
    s += &format!(
        "{} {}   {}    package.json\r\n",
        dir("bootstrap"),
        dir("public"),
        dir("storage")
    );
    s += &format!("{prompt}php artisan --version\r\n");
    s += "Laravel Framework \x1b[32m12.8.1\x1b[0m\r\n";
    s += &format!("{prompt}php artisan migrate\r\n");
    s += "\r\n   \x1b[30;44m INFO \x1b[0m Running migrations.\r\n\r\n";
    s += "  2026_10_21_create_carts \x1b[90m....\x1b[0m 18ms \x1b[1;32mDONE\x1b[0m\r\n\r\n";
    s += &prompt;
    s
}

/// A local `sh` that prints [`shell_transcript`] and waits (never docker).
fn scripted_shell(target: &Target, pane: &PaneState) -> RemoteSpawn {
    RemoteSpawn {
        program: PathBuf::from("/bin/sh"),
        args: vec![
            "-c".into(),
            "stty -echo 2>/dev/null; printf '%s' \"$1\"; exec sleep 600".into(),
            "sh".into(),
            shell_transcript(),
        ],
        osc_host: target.osc_host(),
        start_dir: pane.path.as_str().to_string(),
    }
}

fn plugins() -> Box<dyn FnOnce(&egui::Context) -> PluginHost> {
    Box::new(|ctx: &egui::Context| {
        const G: u64 = 1 << 30;
        let mem = MemInfo {
            total: 32 * G,
            app: 14 * G,
            wired: 3 * G,
            compressed: G,
            swap_used: Some(0),
            pressure: Some(Pressure::Normal),
        };
        PluginHost::new(
            ctx.clone(),
            vec![
                Box::new(manta_plugin_git::GitPlugin::new()),
                Box::new(TerminalPlugin::new().with_remote_override(Arc::new(scripted_shell))),
                Box::new(SysStatsPlugin::with_sampler(
                    Box::new(move || Box::new(FakeSampler::new(0.23, Some(0.08), Some(mem)))),
                    Duration::from_millis(40),
                )),
            ],
            None,
        )
    })
}

fn copy_job() -> JobStatus {
    JobStatus {
        id: 1,
        kind: JobKind::Copy,
        title: "Copying 3 items to My Drive".into(),
        state: JobState::Running,
        current_file: Some("Product shots.zip".into()),
        bytes_done: 42,
        bytes_total: 100,
        files_done: 1,
        files_total: 3,
        speed: 8_400_000.0,
        errors: vec![],
        resume: manta_ops::ResumeInfo {
            can_pause: true,
            ..Default::default()
        },
    }
}

/// Connect `side` to `shop-web` through the scripted runner.
pub(super) fn open_container(h: &mut H, side: Side) {
    let cfg = docker_config("shop-web");
    let fs: SharedVfs = Arc::new(ContainerFs::connect_with(&cfg, Arc::new(FakeShop)).unwrap());
    let home = fs.home().unwrap();
    let a = app(h);
    let action = a.actions.next_id();
    a.on_connected(
        ConnectRequest {
            config: cfg,
            favorite: None,
            side,
            action,
            keychain: false,
        },
        Ok((fs, home)),
    );
    settle(h);
}

/// A connected SFTP server in the sidebar (the "Production web" favorite).
fn connected_server(h: &mut H) {
    let a = app(h);
    let Some(crate::favorites::Favorite::Connection { config, .. }) = a
        .favorites
        .items
        .iter()
        .find(|f| matches!(f, crate::favorites::Favorite::Connection { .. }))
        .cloned()
    else {
        return;
    };
    let fs: SharedVfs = Arc::new(manta_vfs::LocalFs::new());
    a.connections.push(crate::app::Connection {
        id: 4242,
        label: "deploy@web-01".into(),
        config,
        fs,
        notice_shown: true,
    });
}

// ------------------------------------------------------------ renders

#[test]
fn render_readme_images() {
    if !enabled() {
        return;
    }
    set_locale(Locale::En);
    let h0 = home();
    let fx = &h0.fx;

    // 1: the overview, light.
    // 1280 × 800 points at 2x: 2560 × 1600 pixels, larger text.
    let mut h = harness_init_ppp(
        fx,
        WIDE,
        2.0,
        Theme::Light,
        true,
        Box::new(|ctx: &egui::Context| PluginHost::new(ctx.clone(), Vec::new(), None)),
        |_| {},
    );
    select(&mut h, Side::Left, &["README.md", "report-q3.pdf"]);
    shot(&mut h, "readme-overview");

    // 2: everything at once, dark.
    let cloud = h0.cloud();
    let mut h = harness_init_ppp(fx, SIZE, PPP, Theme::Dark, true, plugins(), move |init| {
        init.cloud = Some(cloud);
    });
    act_split(&mut h);
    for (p, w) in app(&mut h).panes.iter_mut().zip([0.33, 0.4, 0.27]) {
        p.weight = w;
    }
    let order = app(&mut h).pane_order();
    let (left, middle, right) = (order[0], order[1], order[2]);
    // Left: Google Drive.
    app(&mut h).navigate_path(left, VfsPath::new(&h0.drive.to_string_lossy()));
    settle(&mut h);
    // Middle: inside the container, the terminal below it.
    open_container(&mut h, middle);
    // Right: the Git repository, with the Git panel.
    app(&mut h).navigate_path(right, VfsPath::new(&h0.repo.to_string_lossy()));
    settle(&mut h);
    connected_server(&mut h);
    {
        let a = app(&mut h);
        for p in a.plugins.pane_ids() {
            a.plugins
                .set_panel_open("manta.terminal", "terminal", Some(p), p == middle);
        }
        a.plugins.set_panel_open("manta.git", "git", None, true);
        a.activate(right);
        a.fake_jobs = Some(vec![copy_job()]);
        a.layout.sidebar_width = 270.0;
        a.layout.pane_bottom_height = 330.0;
    }
    select(&mut h, right, &["routes"]);
    // Git status, the system load sampler and the scripted shell.
    step_for(&mut h, Duration::from_secs(3));
    app(&mut h).toasts.clear();
    shot(&mut h, "readme-full");
}

fn act_split(h: &mut H) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, crate::app::Action::SplitPane(Side::Left));
    settle(h);
}
