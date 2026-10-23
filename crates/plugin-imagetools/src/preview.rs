//! The panel's preview picture, through egui's image loader: our own
//! loader for `rlimagetools://<path>?m=<mtime>` URIs decodes the file on a
//! background thread with the `image` crate (EXIF orientation applied,
//! longest edge at most 1024 px). HEIC/TIFF/AVIF arrive here as PNGs made
//! by sips (see `engine::preview_path`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::task::Poll;

use egui::{load, vec2, ColorImage, CornerRadius, Rect, Sense, Stroke, Ui};

use crate::i18n::tr;
use crate::kit::{self, pal, space, text};

/// Longest edge (pixels) an image is decoded at.
pub const MAX_EDGE: u32 = 1024;
/// Larger files are not read.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const SCHEME: &str = "rlimagetools://";

pub fn uri(path: &Path, mtime: i128) -> String {
    format!("{SCHEME}{}?m={mtime}", path.display())
}

fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix(SCHEME)?;
    let (path, _) = rest.rsplit_once("?m=")?;
    Some(PathBuf::from(path))
}

/// Read and decode `path`, fitting it into `edge` × `edge` pixels.
pub fn decode(path: &Path, edge: u32) -> Result<ColorImage, String> {
    use image::ImageDecoder as _;
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE_BYTES {
        return Err(tr!("preview.failed").into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let source = vec2(img.width() as f32, img.height() as f32);
    if img.width() > edge || img.height() > edge {
        img = img.thumbnail(edge, edge);
    }
    let rgba = img.to_rgba8();
    let mut out = ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    out.source_size = source;
    Ok(out)
}

type Entry = Poll<Result<Arc<ColorImage>, String>>;

#[derive(Default)]
pub struct PreviewLoader {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
}

impl PreviewLoader {
    pub const ID: &'static str = concat!(module_path!(), "::PreviewLoader");
}

/// Install the loader once per context.
pub fn install(ctx: &egui::Context) {
    if !ctx.is_loader_installed(PreviewLoader::ID) {
        ctx.add_image_loader(Arc::new(PreviewLoader::default()));
    }
}

impl load::ImageLoader for PreviewLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str, _hint: load::SizeHint) -> load::ImageLoadResult {
        let Some(path) = path_of(uri) else {
            return Err(load::LoadError::NotSupported);
        };
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(uri) {
            Some(Poll::Ready(Ok(img))) => return Ok(load::ImagePoll::Ready { image: img.clone() }),
            Some(Poll::Ready(Err(e))) => return Err(load::LoadError::Loading(e.clone())),
            Some(Poll::Pending) => return Ok(load::ImagePoll::Pending { size: None }),
            None => {}
        }
        cache.insert(uri.to_string(), Poll::Pending);
        drop(cache);
        let (cache, ctx, uri) = (self.cache.clone(), ctx.clone(), uri.to_string());
        std::thread::Builder::new()
            .name("it-preview".into())
            .spawn(move || {
                let result = decode(&path, MAX_EDGE).map(Arc::new);
                let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
                if map.contains_key(&uri) {
                    map.insert(uri, Poll::Ready(result));
                }
                drop(map);
                ctx.request_repaint();
            })
            .map_err(|e| load::LoadError::Loading(e.to_string()))?;
        Ok(load::ImagePoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|e| match e {
                Poll::Ready(Ok(img)) => img.pixels.len() * 4,
                _ => 0,
            })
            .sum()
    }

    fn has_pending(&self) -> bool {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|e| e.is_pending())
    }
}

fn checkerboard(ui: &Ui, rect: Rect, r: CornerRadius) {
    let p = pal(ui);
    ui.painter().rect_filled(rect, r, p.content);
    let cell = 8.0;
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
    let (cols, rows) = (
        (rect.width() / cell).ceil() as i32,
        (rect.height() / cell).ceil() as i32,
    );
    if cols * rows > 40_000 {
        return;
    }
    for y in 0..rows {
        for x in 0..cols {
            if (x + y) % 2 == 0 {
                let min = rect.min + vec2(x as f32 * cell, y as f32 * cell);
                painter.rect_filled(
                    Rect::from_min_size(min, vec2(cell, cell)),
                    0.0,
                    p.neutral_soft,
                );
            }
        }
    }
}

/// Draw the picture aspect-fit into the panel's width and `max_h`, rounded,
/// on a checkerboard (transparency); a caption below.
pub fn show(ui: &mut Ui, path: &Path, mtime: i128, max_h: f32, caption: Option<&str>) {
    install(ui.ctx());
    let p = pal(ui);
    let width = ui.available_width();
    let r = CornerRadius::same(kit::PANEL_R - 2);
    let hint = load::SizeHint::Size {
        width: MAX_EDGE,
        height: MAX_EDGE,
        maintain_aspect_ratio: true,
    };
    match ui
        .ctx()
        .try_load_texture(&uri(path, mtime), egui::TextureOptions::LINEAR, hint)
    {
        Ok(load::TexturePoll::Ready { texture }) => {
            let size = texture.size;
            let scale = (width / size.x).min(max_h / size.y).min(1.0);
            let scale = if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            let shown = (size * scale).max(vec2(8.0, 8.0));
            let (rect, _) = ui.allocate_exact_size(vec2(width, shown.y), Sense::hover());
            let img_rect = Rect::from_center_size(rect.center(), shown);
            checkerboard(ui, img_rect, r);
            egui::Image::from_texture(texture)
                .corner_radius(r)
                .paint_at(ui, img_rect);
            ui.painter().rect_stroke(
                img_rect,
                r,
                Stroke::new(1.0, p.separator),
                egui::StrokeKind::Outside,
            );
        }
        Ok(load::TexturePoll::Pending { .. }) => {
            let (rect, _) = ui.allocate_exact_size(vec2(width, max_h.min(120.0)), Sense::hover());
            ui.painter().rect_filled(rect, r, p.stripe);
            ui.painter().rect_stroke(
                rect,
                r,
                Stroke::new(1.0, p.separator),
                egui::StrokeKind::Inside,
            );
            egui::Spinner::new()
                .size(16.0)
                .color(p.text3)
                .paint_at(ui, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)));
        }
        Err(_) => kit::callout(
            ui,
            kit::icon::FILE_IMAGE,
            tr!("preview.failed"),
            manta_plugin::Tone::Muted,
        ),
    }
    if let Some(c) = caption.filter(|c| !c.is_empty()) {
        ui.add_space(-space::XS);
        ui.add(
            egui::Label::new(
                egui::RichText::new(c)
                    .font(kit::regular(text::SMALL))
                    .color(p.text2),
            )
            .wrap(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_round_trip_and_decode_applies_the_size_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a b?.png");
        image::RgbImage::new(2048, 1024)
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let u = uri(&path, 42);
        assert_eq!(path_of(&u).as_deref(), Some(path.as_path()));
        assert!(path_of("file:///x.png").is_none());
        let img = decode(&path, MAX_EDGE).unwrap();
        assert_eq!(img.size, [1024, 512]);
        assert_eq!(img.source_size, vec2(2048.0, 1024.0));
    }
}
