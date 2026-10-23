//! Renders of the built-in Image Tools plugin in the real app, light and
//! dark: the Image panel with preview and EXIF, the "lo"/"XL" badges in the
//! file list, and the Resize sheet opened from the context menu. Skipped
//! without a wgpu adapter or `sips`.
//!
//! The PNGs go to `$MANTA_RENDER_DIR` (default `target/ux-renders`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eframe::egui::{self, Theme};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use manta_plugin::host::{HostPaths, PluginHost};

use crate::app::{AppInit, MantaApp};

const ID: &str = "manta.imagetools";
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
            eprintln!("image_tools_tests: no wgpu adapter available, skipping renders");
        }
        ok
    })
}

fn tools_available() -> bool {
    let ok = |cmd: &str, arg: &str| {
        std::process::Command::new(cmd)
            .arg(arg)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    ok("sips", "--help")
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("MANTA_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ux-renders"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A landscape "photo": sky gradient, a sun and two ranges of hills.
fn landscape(w: u32, h: u32) -> image::RgbImage {
    image::RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let sun = ((fx - 0.68).powi(2) + (fy - 0.36).powi(2) * 2.2).sqrt();
        let ridge1 = 0.62 + 0.08 * (fx * 7.0).sin() + 0.04 * (fx * 17.0 + 1.0).sin();
        let ridge2 = 0.74 + 0.05 * (fx * 4.0 + 2.0).sin() + 0.02 * (fx * 29.0).sin();
        let mix = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t.clamp(0.0, 1.0))
        };
        let mut c = mix([46.0, 72.0, 140.0], [250.0, 170.0, 120.0], fy / 0.6);
        if sun < 0.07 {
            c = [255.0, 236.0, 190.0];
        } else if sun < 0.16 {
            c = mix([255.0, 222.0, 160.0], c, (sun - 0.07) / 0.09);
        }
        if fy > ridge1 {
            c = mix([92.0, 84.0, 120.0], [60.0, 58.0, 90.0], (fy - ridge1) * 4.0);
        }
        if fy > ridge2 {
            c = mix([40.0, 70.0, 64.0], [22.0, 40.0, 38.0], (fy - ridge2) * 3.0);
        }
        image::Rgb(c.map(|v| v as u8))
    })
}

/// The folder the left pane shows.
fn make_images(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let photo = dir.join("alps-sunrise.jpg");
    landscape(2400, 1600).save(&photo).unwrap();
    landscape(400, 300).save(dir.join("thumbnail.jpg")).unwrap();
    // Noise doesn't compress: a PNG over 2 MB.
    let mut seed = 0x2545_f491_u32;
    let noise = image::RgbImage::from_fn(1400, 700, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        image::Rgb([(seed >> 8) as u8, (seed >> 16) as u8, seed as u8])
    });
    noise.save(dir.join("banner-hero.png")).unwrap();
    image::RgbaImage::from_fn(600, 200, |x, _| {
        image::Rgba([40, 110, 230, if x < 300 { 255 } else { 120 }])
    })
    .save(dir.join("logo.png"))
    .unwrap();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([230, 80, 60, 255]))
        .save(dir.join("favicon-32.png"))
        .unwrap();
    std::fs::write(dir.join("README.md"), "# Site images\n").unwrap();
    let heic = dir.join("team-photo.heic");
    let out = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "heic",
            &photo.to_string_lossy(),
            "--out",
            &heic.to_string_lossy(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // EXIF with camera, lens and a GPS position, as a phone would write.
    use manta_plugin_imagetools::headers::{fixtures, insert_jpeg_exif};
    let data = std::fs::read(&photo).unwrap();
    std::fs::write(
        &photo,
        insert_jpeg_exif(&data, &fixtures::exif_photo(Some((46.558, 7.987)), 1)),
    )
    .unwrap();
}

fn harness(dir: &Path, config: &Path, theme: Theme, language: crate::i18n::Language) -> H {
    let paths = HostPaths::under(config);
    let init = AppInit {
        start_dir: Some(dir.to_path_buf()),
        favorites: Some(crate::favorites::Favorites::default()),
        places: Some((Vec::new(), Vec::new())),
        plugins: Some(Box::new(move |ctx: &egui::Context| {
            let builtins: Vec<Box<dyn manta_plugin::Plugin>> =
                vec![Box::new(manta_plugin_imagetools::ImageToolsPlugin::new())];
            let mut host = PluginHost::new(ctx.clone(), builtins, Some(paths));
            host.set_panel_open(ID, "image", None, true);
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

/// Step until `done`, at most 30 s.
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

/// Let background work (decodes, the plugin) land, then a few frames.
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

fn save(h: &mut H, name: &str) {
    h.run_steps(3);
    let img = h.render().expect("render");
    let path = out_dir().join(format!("{name}.png"));
    img.save(&path).expect("save png");
    eprintln!("image_tools_tests: wrote {}", path.display());
}

/// y (points) of row `i` of the left file list (the first row at 120 pt,
/// rows 24 pt high).
fn row_y(i: usize) -> f32 {
    120.0 + 24.0 * i as f32
}

fn click(h: &mut H, pos: egui::Pos2, button: egui::PointerButton) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        });
    }
    h.run_steps(3);
}

/// UI texts the render steps look for, per language.
struct Texts {
    lang: crate::i18n::Language,
    prefix: &'static str,
    no_image: &'static str,
    waiting: &'static str,
    /// The plugin's submenu (its localized name).
    submenu: &'static str,
    resize_item: &'static str,
    resize_heading: &'static str,
}

const EN: Texts = Texts {
    lang: crate::i18n::Language::English,
    prefix: "image_tools",
    submenu: "Image Tools",
    no_image: "No image selected",
    waiting: "Waiting",
    resize_item: "Resize… ",
    resize_heading: "Resize “alps-sunrise.jpg”",
};

const DE: Texts = Texts {
    lang: crate::i18n::Language::German,
    prefix: "image_tools_de",
    submenu: "Bildwerkzeuge",
    no_image: "Kein Bild ausgewählt",
    waiting: "Warte",
    resize_item: "Größe ändern … ",
    resize_heading: "Größe ändern: „alps-sunrise.jpg“",
};

#[test]
fn render_image_tools() {
    render_image_tools_in(&EN);
}

/// The same renders in German (plugin texts included).
#[test]
fn render_image_tools_german() {
    render_image_tools_in(&DE);
}

fn render_image_tools_in(t: &Texts) {
    if !wgpu_available() {
        return;
    }
    if !tools_available() {
        eprintln!("sips not found, skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("site-images");
    make_images(&dir);
    let prefix = t.prefix;
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let config = tmp.path().join(format!("config-{tname}"));
        let mut h = harness(&dir, &config, theme, t.lang);
        wait_for(&mut h, "the file list", |h| {
            has_label(h, t.no_image) || has_label(h, t.waiting)
        });

        // Select the photo (sorted by name: alps-sunrise.jpg is the first row).
        click(
            &mut h,
            egui::pos2(330.0, row_y(0)),
            egui::PointerButton::Primary,
        );
        wait_for(&mut h, "the photo's EXIF", |h| has_label(h, "Canon EOS R6"));
        settle(&mut h, 900);
        save(&mut h, &format!("{prefix}_panel_{tname}"));

        // The file list with the badges, cropped (2 px per point).
        let img = h.render().expect("render");
        let crop = image::imageops::crop_imm(&img, 420, 160, 780, 420).to_image();
        let path = out_dir().join(format!("{prefix}_badges_{tname}.png"));
        crop.save(&path).unwrap();
        eprintln!("image_tools_tests: wrote {}", path.display());

        // Context menu → Image Tools → Resize…
        click(
            &mut h,
            egui::pos2(330.0, row_y(0)),
            egui::PointerButton::Secondary,
        );
        h.get_by_label_contains(t.submenu).hover();
        h.run_steps(4);
        save(&mut h, &format!("{prefix}_menu_{tname}"));
        // Menu buttons carry their (empty) shortcut text after a space.
        // Keyboard, not a simulated click: the click's pointer position
        // would land in the other pane once the menu closes.
        h.get_by_label_contains(t.resize_item).focus();
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        wait_for(&mut h, "the resize modal", |h| {
            has_label(h, t.resize_heading)
        });
        settle(&mut h, 400);
        save(&mut h, &format!("{prefix}_resize_{tname}"));
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
    }
    crate::i18n::set_thread_locale(None);
}

/// The manual's screenshot in every language (see `manual_shots`).
#[test]
fn render_manual_images_image_tools() {
    if !crate::manual_shots::enabled() || !wgpu_available() || !tools_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("site-images");
    make_images(&dir);
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
        // The file list (no language-neutral label to wait for).
        settle(&mut h, 2_000);
        click(
            &mut h,
            egui::pos2(330.0, row_y(0)),
            egui::PointerButton::Primary,
        );
        wait_for(&mut h, "the photo's EXIF", |h| has_label(h, "Canon EOS R6"));
        settle(&mut h, 900);
        let img = h.render().expect("render");
        crate::manual_shots::save(&img, 2.0, None, code, "image-tools");
    }
    crate::i18n::set_thread_locale(None);
}
