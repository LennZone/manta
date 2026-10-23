//! Screenshots for the user manual (`docs/manual/images/<lang>/NAME.png`,
//! listed in `docs/manual/images/NEEDED.md`), written by the
//! `render_manual_images*` tests from the synthetic test fixtures.
//!
//! Opt-in, because they write into the source tree:
//! `MANTA_MANUAL_IMAGES=1 cargo test -p manta render_manual_images -- --test-threads=1`
//!
//! Each shot is cropped (a dialog, a window, a panel), scaled down to at
//! most [`MAX_W`] pixels and stored as a 256-colour PNG to keep the
//! embedded manual small.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Order, Rect};
use image::RgbaImage;

use crate::i18n::{Language, Locale};

/// Widest image in the manual (pixels).
pub const MAX_W: u32 = 1600;

/// Every manual language: (UI locale, folder under `docs/manual/images/`).
pub const LANGS: [(Locale, &str); 7] = [
    (Locale::De, "de"),
    (Locale::En, "en"),
    (Locale::Zh, "zh-Hans"),
    (Locale::Es, "es"),
    (Locale::Fr, "fr"),
    (Locale::It, "it"),
    (Locale::Ru, "ru"),
];

/// The languages to render: all, or the folders listed in
/// `MANTA_MANUAL_LANGS` (comma-separated, e.g. `zh-Hans,ru`).
pub fn langs() -> Vec<(Locale, &'static str)> {
    let only = std::env::var("MANTA_MANUAL_LANGS").unwrap_or_default();
    let only: Vec<&str> = only
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    LANGS
        .into_iter()
        .filter(|(_, code)| only.is_empty() || only.contains(code))
        .collect()
}

/// The preference value that selects `locale`.
pub fn language(locale: Locale) -> Language {
    Language::ALL
        .into_iter()
        .find(|l| l.locale() == Some(locale))
        .unwrap_or(Language::English)
}

/// Fonts for the current UI language, like `MantaApp::apply_language`
/// (Chinese adds PingFang SC to the chain; without it Han characters come
/// from whatever the system fallback picks first).
pub fn use_fonts(ctx: &egui::Context) {
    ctx.set_fonts(crate::theme::font_definitions());
}

/// Are manual screenshots wanted in this test run?
pub fn enabled() -> bool {
    std::env::var_os("MANTA_MANUAL_IMAGES").is_some()
}

pub fn images_dir(lang: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/manual/images")
        .join(lang);
    std::fs::create_dir_all(&d).expect("create docs/manual/images/<lang>");
    d
}

/// What part of the render to keep.
#[derive(Debug, Clone, Copy)]
pub enum Crop {
    Full,
    /// A rectangle in points.
    Rect(Rect),
    /// The topmost modal sheet (with a margin for its shadow).
    Modal,
    /// An auxiliary window drawn embedded (Settings, Compare & Sync, …).
    Window(crate::aux_window::AuxKind),
}

/// The crop rectangle in points (`None`: everything).
pub fn crop_rect(ctx: &egui::Context, crop: Crop) -> Option<Rect> {
    let screen = ctx.content_rect();
    let pad = |r: Rect, m: f32| r.expand(m).intersect(screen);
    match crop {
        Crop::Full => None,
        Crop::Rect(r) => Some(r.intersect(screen)),
        Crop::Window(kind) => ctx
            .data(|d| d.get_temp::<Rect>(kind.window_id().with("rect")))
            .map(|r| pad(r, 18.0)),
        Crop::Modal => {
            let rects: Vec<Rect> = ctx.memory(|m| {
                m.layer_ids()
                    .filter(|l| l.order == Order::Foreground)
                    .filter_map(|l| m.area_rect(l.id))
                    .filter(|r| r.width() < screen.width() * 0.95 && r.width() > 120.0)
                    .collect()
            });
            rects
                .into_iter()
                .max_by(|a, b| a.area().total_cmp(&b.area()))
                .map(|r| pad(r, 22.0))
        }
    }
}

/// Crop `img` (rendered at `ppp` pixels per point), scale it down to
/// [`MAX_W`] and save it as `docs/manual/images/<lang>/<name>.png`.
pub fn save(img: &RgbaImage, ppp: f32, crop: Option<Rect>, lang: &str, name: &str) -> PathBuf {
    let img = match crop {
        Some(r) => {
            let x = (r.left() * ppp).max(0.0) as u32;
            let y = (r.top() * ppp).max(0.0) as u32;
            let w = ((r.width() * ppp) as u32).min(img.width() - x);
            let h = ((r.height() * ppp) as u32).min(img.height() - y);
            image::imageops::crop_imm(img, x, y, w, h).to_image()
        }
        None => img.clone(),
    };
    let img = if img.width() > MAX_W {
        let h = (img.height() as f64 * MAX_W as f64 / img.width() as f64).round() as u32;
        image::imageops::resize(&img, MAX_W, h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let path = images_dir(lang).join(format!("{name}.png"));
    write_indexed_png(&img, &path);
    eprintln!(
        "manual image: wrote {} ({}×{})",
        path.display(),
        img.width(),
        img.height()
    );
    path
}

/// 256-colour PNG (NeuQuant), about a third of a true-colour PNG.
pub(crate) fn write_indexed_png(img: &RgbaImage, path: &Path) {
    let (w, h) = img.dimensions();
    let nq = color_quant::NeuQuant::new(10, 256, img.as_raw());
    let map = nq.color_map_rgba();
    let idx: Vec<u8> = img
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| nq.index_of(p) as u8)
        .collect();
    let rgb: Vec<u8> = map
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    let alpha: Vec<u8> = map.as_chunks::<4>().0.iter().map(|c| c[3]).collect();
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(rgb);
    if alpha.iter().any(|a| *a != 255) {
        enc.set_trns(alpha);
    }
    enc.set_compression(png::Compression::High);
    enc.write_header()
        .expect("png header")
        .write_image_data(&idx)
        .expect("png data");
}
