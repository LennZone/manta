//! Renders of the built-in Duplicates plugin in the real app, light and
//! dark: the Duplicates panel with results and the "Move to Trash"
//! confirmation. Also checks that "Select in Pane" (the `Select` request)
//! selects the files in the pane. Skipped without a wgpu adapter.
//!
//! The PNGs go to `$MANTA_RENDER_DIR` (default `target/ux-renders`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eframe::egui::{self, Theme};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use manta_plugin::host::{HostPaths, PluginHost};

use crate::app::{AppInit, MantaApp, Side};

const ID: &str = "manta.duplicates";
const SIZE: [f32; 2] = [1280.0, 800.0];

type H = Harness<'static, MantaApp>;

fn wgpu_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let ok = std::panic::catch_unwind(|| {
            egui_kittest::wgpu::create_render_state(
                egui_kittest::wgpu::default_wgpu_setup(),
                egui_wgpu::RendererOptions::PREDICTABLE,
            )
        })
        .is_ok();
        if !ok {
            eprintln!("duplicates_tests: no wgpu adapter available, skipping renders");
        }
        ok
    })
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("MANTA_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ux-renders"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `len` bytes that differ per `seed`.
fn bytes(seed: u32, len: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(2_654_435_761) | 1;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as u8
        })
        .collect()
}

fn write(root: &Path, rel: &str, data: &[u8], days_ago: u64) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, data).unwrap();
    let t = std::time::SystemTime::now()
        - Duration::from_secs(days_ago * 86_400 + 3_600 * (days_ago % 7));
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

/// A photo-and-documents folder with a few groups of copies.
fn make_tree(root: &Path) {
    let photo = bytes(1, 2_400_000);
    write(root, "IMG_2041.jpg", &photo, 420);
    write(root, "Exports/IMG_2041.jpg", &photo, 200);
    write(root, "Backup 2023/IMG_2041 copy.jpg", &photo, 90);
    let audio = bytes(2, 5_100_000);
    write(root, "Voice Memo 14.m4a", &audio, 30);
    write(root, "Backup 2023/Voice Memo 14.m4a", &audio, 12);
    let hero = bytes(3, 850_000);
    write(root, "site/assets/hero.png", &hero, 60);
    write(root, "site-old/assets/hero.png", &hero, 400);
    let invoice = bytes(4, 180_000);
    write(root, "Invoice-2024-03.pdf", &invoice, 210);
    write(root, "Downloads/Invoice-2024-03 (1).pdf", &invoice, 209);
    write(root, "Downloads/Invoice-2024-03 (2).pdf", &invoice, 150);
    write(root, "notes.txt", &bytes(5, 2_000), 5);
    write(root, "Backup 2023/notes.txt", &bytes(5, 2_000), 40);
    write(root, "node_modules/left-pad/hero.png", &hero, 3);
    write(root, "README.md", b"# Archive\n", 2);
}

fn harness(dir: &Path, config: &Path, theme: Theme, language: crate::i18n::Language) -> H {
    let paths = HostPaths::under(config);
    // Never touch the real Trash from a test.
    let data = paths.data_root.join(ID);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("settings.json"),
        r#"{"trash_command": ["/usr/bin/false"]}"#,
    )
    .unwrap();
    let init = AppInit {
        start_dir: Some(dir.to_path_buf()),
        favorites: Some(crate::favorites::Favorites::default()),
        places: Some((Vec::new(), Vec::new())),
        plugins: Some(Box::new(move |ctx: &egui::Context| {
            let builtins: Vec<Box<dyn manta_plugin::Plugin>> =
                vec![Box::new(manta_plugin_duplicates::DuplicatesPlugin::new())];
            let mut host = PluginHost::new(ctx.clone(), builtins, Some(paths));
            host.set_panel_open(ID, "duplicates", None, true);
            host
        })),
        // The app sets this thread's language from the prefs before the
        // plugin starts, so the plugin's texts follow it.
        prefs: Some(crate::prefs::Prefs {
            language,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(SIZE[0], SIZE[1]))
        .with_pixels_per_point(2.0)
        .with_theme(theme)
        .with_max_steps(10_000)
        .allow_missing_glyphs()
        .with_context_setup(|ctx| {
            ctx.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
            crate::theme::install(ctx);
        })
        .wgpu()
        .build_eframe(move |cc| MantaApp::with_init(&cc.egui_ctx, init));
    h.run_steps(4);
    h
}

fn wait_for(h: &mut H, what: &str, mut done: impl FnMut(&mut H) -> bool) {
    let start = Instant::now();
    loop {
        h.step();
        if done(h) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn settle(h: &mut H, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    h.run_steps(4);
}

fn has_label(h: &H, text: &str) -> bool {
    h.query_by_label_contains(text).is_some()
}

/// Run one of the plugin's commands for the left pane.
fn command(h: &mut H, id: &str) {
    let app = h.state_mut();
    let pane = app
        .plugins
        .pane(manta_plugin::PaneId::Left)
        .cloned()
        .expect("left pane state");
    app.plugins.run_command(&format!("{ID}:{id}"), &pane);
    h.run_steps(2);
}

fn save(h: &mut H, name: &str) {
    h.run_steps(3);
    let img = h.render().expect("render");
    let path = out_dir().join(format!("{name}.png"));
    img.save(&path).expect("save png");
    eprintln!("duplicates_tests: wrote {}", path.display());
}

#[test]
fn render_duplicates() {
    render_duplicates_in(
        crate::i18n::Language::English,
        "duplicates",
        "Finds identical files",
        "11.1 MB",
        "Select in Pane",
        "Move Extras to Trash",
    );
}

/// The same renders in German (plugin texts included).
#[test]
fn render_duplicates_german() {
    render_duplicates_in(
        crate::i18n::Language::German,
        "duplicates_de",
        "Sucht identische Dateien",
        "11,1 MB",
        "Im Bereich auswählen",
        "Kopien in den Papierkorb",
    );
}

fn render_duplicates_in(
    lang: crate::i18n::Language,
    prefix: &str,
    intro: &str,
    reclaimable: &str,
    select: &str,
    trash_all: &str,
) {
    if !wgpu_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap().join("Archive");
    make_tree(&dir);
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let config = tmp.path().join(format!("config-{tname}"));
        let mut h = harness(&dir, &config, theme, lang);
        wait_for(&mut h, "the panel", |h| has_label(h, intro));
        command(&mut h, "scan");
        wait_for(&mut h, "the results", |h| has_label(h, reclaimable));
        settle(&mut h, 600);
        save(&mut h, &format!("{prefix}_panel_{tname}"));

        // "Select in Pane" of the first group selects its extras there.
        if tname == "light" && lang == crate::i18n::Language::English {
            h.get_all_by_label(select)
                .nth(1)
                .expect("the first group's button")
                .click();
            wait_for(&mut h, "the selection", |h| {
                !h.state().pane(Side::Left).tab().selected.is_empty()
            });
            settle(&mut h, 300);
            let tab = h.state().pane(Side::Left).tab();
            assert_eq!(
                tab.loc.path.to_string(),
                dir.join("Backup 2023").to_string_lossy()
            );
            assert_eq!(
                tab.selected.iter().cloned().collect::<Vec<_>>(),
                vec!["Voice Memo 14.m4a".to_string()]
            );
            save(&mut h, "duplicates_selected_light");
        }

        h.get_by_label(trash_all).click();
        wait_for(&mut h, "the confirmation", |h| {
            h.state().plugins.is_panel_open(ID, "confirm", None)
        });
        settle(&mut h, 300);
        save(&mut h, &format!("{prefix}_confirm_{tname}"));
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
    }
    crate::i18n::set_thread_locale(None);
}

/// The manual's screenshot in every language (see `manual_shots`).
#[test]
fn render_manual_images_duplicates() {
    if !crate::manual_shots::enabled() || !wgpu_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap().join("Archive");
    make_tree(&dir);
    for (locale, code) in crate::manual_shots::langs() {
        crate::i18n::set_thread_locale(Some(locale));
        let config = tmp.path().join(format!("config-{code}"));
        let mut h = harness(
            &dir,
            &config,
            Theme::Light,
            crate::manual_shots::language(locale),
        );
        crate::manual_shots::use_fonts(&h.ctx);
        wait_for(&mut h, "the panel", |h| {
            h.state().plugins.is_panel_open(ID, "duplicates", None)
        });
        settle(&mut h, 600);
        command(&mut h, "scan");
        // A copy only the results list shows (the pane shows the top level).
        wait_for(&mut h, "the results", |h| {
            has_label(h, "Invoice-2024-03 (2).pdf")
        });
        settle(&mut h, 600);
        let img = h.render().expect("render");
        crate::manual_shots::save(&img, 2.0, None, code, "duplicates");
    }
    crate::i18n::set_thread_locale(None);
}
