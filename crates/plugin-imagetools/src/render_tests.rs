//! Headless renders of the Image panel and the Image Tools sheet with
//! `egui_kittest` + wgpu, against real image files, in English and German,
//! light and dark. PNGs go to `$MANTA_IMAGETOOLS_RENDER_DIR` (default
//! `target/plugin-imagetools-renders`). Skips without a wgpu adapter.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme};
use egui_kittest::Harness;
use manta_plugin::i18n::{set_thread_lang, Lang};
use manta_plugin::{PaneId, PaneState, Plugin, PluginEvent, PluginHandle};

use crate::engine::{lock, PanelModel};
use crate::headers::{fixtures as B, insert_jpeg_exif};
use crate::{pane_state, ImageToolsPlugin, PANEL, SHEET};

fn wgpu_available() -> bool {
    // Screenshots are opt-in (slow, need a GPU): set MANTA_RENDER_DIR or
    // this crate's *_RENDER_DIR to produce them.
    if !std::env::vars_os().any(|(k, _)| k.to_string_lossy().ends_with("RENDER_DIR")) {
        return false;
    }
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
            eprintln!("render_tests: no wgpu adapter, skipping");
        }
        ok
    })
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("MANTA_IMAGETOOLS_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-imagetools-renders")
        });
    std::fs::create_dir_all(&dir).expect("render dir");
    dir
}

/// A landscape "photo": sky gradient, a sun and two ranges of hills.
pub fn landscape(w: u32, h: u32) -> image::RgbImage {
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

/// A folder of web images: a GPS-tagged photo, a thumbnail, a big PNG, a
/// logo with transparency and a favicon.
pub fn make_images(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let photo = dir.join("alps-sunrise.jpg");
    landscape(2400, 1600).save(&photo).unwrap();
    let data = std::fs::read(&photo).unwrap();
    std::fs::write(
        &photo,
        insert_jpeg_exif(&data, &B::exif_photo(Some((46.558, 7.987)), 1)),
    )
    .unwrap();
    landscape(400, 300).save(dir.join("thumbnail.jpg")).unwrap();
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
}

struct State {
    plugin: ImageToolsPlugin,
    pane: PaneState,
    panel: &'static str,
}

fn setup_ctx(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        let sf = Arc::new(sf);
        let base = fonts.families[&FontFamily::Proportional].clone();
        for (key, fam, w) in [
            ("sf", FontFamily::Proportional, 400.0),
            ("sf-medium", FontFamily::Name("rl-medium".into()), 510.0),
            ("sf-semibold", FontFamily::Name("rl-semibold".into()), 600.0),
        ] {
            let mut fd = FontData::from_blob(sf.clone(), 0);
            fd.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", w)]);
            fonts.font_data.insert(key.into(), Arc::new(fd));
            let mut chain = vec![key.to_string()];
            chain.extend(base.iter().cloned());
            fonts.families.insert(fam, chain);
        }
    }
    let icons = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/assets/fonts/Phosphor.ttf");
    if let Ok(ph) = std::fs::read(icons) {
        fonts
            .font_data
            .insert("phosphor".into(), Arc::new(FontData::from_owned(ph)));
        fonts
            .families
            .insert(FontFamily::Name("rl-icons".into()), vec!["phosphor".into()]);
    }
    ctx.set_fonts(fonts);
    for theme in [Theme::Light, Theme::Dark] {
        let dark = theme == Theme::Dark;
        ctx.style_mut_of(theme, |s| {
            use egui::TextStyle as T;
            s.text_styles
                .insert(T::Body, egui::FontId::proportional(13.0));
            s.text_styles
                .insert(T::Button, egui::FontId::proportional(13.0));
            let v = &mut s.visuals;
            let rgb = Color32::from_rgb;
            v.panel_fill = if dark {
                rgb(38, 38, 41)
            } else {
                rgb(246, 246, 248)
            };
            v.extreme_bg_color = if dark {
                rgb(28, 28, 30)
            } else {
                rgb(255, 255, 255)
            };
            v.weak_text_color = Some(if dark {
                rgb(160, 160, 167)
            } else {
                rgb(104, 104, 110)
            });
            v.widgets.noninteractive.bg_stroke = Stroke::new(
                1.0,
                if dark {
                    rgb(58, 58, 62)
                } else {
                    rgb(220, 220, 224)
                },
            );
            v.widgets.noninteractive.fg_stroke = Stroke::new(
                1.0,
                if dark {
                    rgb(242, 242, 245)
                } else {
                    rgb(29, 29, 31)
                },
            );
            v.window_fill = if dark {
                rgb(46, 46, 49)
            } else {
                rgb(252, 252, 253)
            };
        });
    }
}

fn harness(state: State, theme: Theme, size: egui::Vec2) -> Harness<'static, State> {
    Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 30.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(setup_ctx)
        .wgpu()
        .build_ui_state(
            |ui, s: &mut State| {
                let rect = ui.max_rect();
                let fill = if s.panel == SHEET {
                    ui.visuals().window_fill
                } else {
                    ui.visuals().panel_fill
                };
                ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
                let (bar, _) =
                    ui.allocate_exact_size(egui::vec2(rect.width(), 32.0), egui::Sense::hover());
                let title = if s.panel == SHEET {
                    crate::i18n::tr!("panel.tools")
                } else {
                    crate::i18n::tr!("panel.image")
                };
                ui.painter().text(
                    bar.left_center() + egui::vec2(12.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::new(13.0, FontFamily::Name("rl-semibold".into())),
                    ui.visuals().text_color(),
                );
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom(),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                let pane = s.pane.clone();
                let panel = s.panel;
                let body =
                    egui::Rect::from_min_max(egui::pos2(rect.left(), bar.bottom()), rect.max);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(body)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        s.plugin.panel_ui(panel, &pane, ui);
                    },
                );
            },
            state,
        )
}

fn start(h: &mut Harness<'static, State>, data: &Path) {
    let (tx, rx) = mpsc::channel();
    std::mem::forget(rx);
    let handle = PluginHandle::new(crate::ID.into(), tx, h.ctx.clone(), data.to_path_buf());
    let st = h.state_mut();
    st.plugin.enable(handle);
    let ev = PluginEvent::DirectoryChanged(st.pane.clone());
    st.plugin.on_event(&ev);
}

fn wait(h: &mut Harness<'static, State>, what: &str, mut done: impl FnMut(&State) -> bool) {
    let t = Instant::now();
    while !done(h.state()) {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
    // Let the preview decode land.
    for _ in 0..40 {
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn save(h: &mut Harness<'static, State>, name: &str) {
    h.run_steps(2);
    let img = h.render().expect("render");
    let path = out_dir().join(format!("{name}.png"));
    img.save(&path).expect("save");
    eprintln!("render_tests: wrote {}", path.display());
}

fn tname(t: Theme) -> &'static str {
    if t == Theme::Dark {
        "dark"
    } else {
        "light"
    }
}

fn sips_available() -> bool {
    std::process::Command::new("sips")
        .arg("--help")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn panel_state(dir: &Path, sel: &[&str]) -> PaneState {
    let sel: Vec<PathBuf> = sel.iter().map(|n| dir.join(n)).collect();
    pane_state(PaneId::Left, Some(dir), &sel)
}

fn render_all(lang: Lang, prefix: &str) {
    if !wgpu_available() {
        return;
    }
    set_thread_lang(Some(lang));
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap().join("site-images");
    make_images(&dir);
    for theme in [Theme::Light, Theme::Dark] {
        let t = tname(theme);
        let data = tmp.path().join(format!("data-{t}"));
        // One photo: preview, EXIF, GPS.
        let state = State {
            plugin: ImageToolsPlugin::new(),
            pane: panel_state(&dir, &["alps-sunrise.jpg"]),
            panel: PANEL,
        };
        let mut h = harness(state, theme, egui::vec2(340.0, 1000.0));
        start(&mut h, &data);
        wait(
            &mut h,
            "the photo",
            |s| matches!(&lock(&s.plugin.shared).panel, PanelModel::Single(d) if d.exif.gps.is_some()),
        );
        save(&mut h, &format!("{prefix}_panel_photo_{t}"));
        assert!(
            !lock(&h.state().plugin.shared).datauri,
            "2.4 MP JPEG is over 100 KB"
        );

        // Several images: the summary.
        let multi = panel_state(
            &dir,
            &[
                "alps-sunrise.jpg",
                "banner-hero.png",
                "logo.png",
                "thumbnail.jpg",
                "favicon-32.png",
            ],
        );
        h.state_mut().pane = multi.clone();
        h.state_mut()
            .plugin
            .on_event(&PluginEvent::SelectionChanged(multi));
        wait(
            &mut h,
            "the summary",
            |s| matches!(&lock(&s.plugin.shared).panel, PanelModel::Multi(v) if v.len() == 5),
        );
        save(&mut h, &format!("{prefix}_panel_summary_{t}"));

        // The logo (transparency, small).
        let logo = panel_state(&dir, &["logo.png"]);
        h.state_mut().pane = logo.clone();
        h.state_mut()
            .plugin
            .on_event(&PluginEvent::SelectionChanged(logo));
        wait(
            &mut h,
            "the logo",
            |s| matches!(&lock(&s.plugin.shared).panel, PanelModel::Single(d) if d.path.ends_with("logo.png")),
        );
        save(&mut h, &format!("{prefix}_panel_logo_{t}"));

        // Nothing selected.
        let none = panel_state(&dir, &[]);
        h.state_mut().pane = none.clone();
        h.state_mut()
            .plugin
            .on_event(&PluginEvent::SelectionChanged(none));
        wait(&mut h, "no image", |s| {
            lock(&s.plugin.shared).panel == PanelModel::NoImage
        });
        save(&mut h, &format!("{prefix}_panel_none_{t}"));
        h.state_mut().plugin.disable();

        if !sips_available() {
            eprintln!("sips not found: no sheet renders");
            continue;
        }
        // The sheet for each operation.
        for (op, sel) in [
            ("resize", vec!["alps-sunrise.jpg"]),
            (
                "convert",
                vec![
                    "alps-sunrise.jpg",
                    "banner-hero.png",
                    "thumbnail.jpg",
                    "logo.png",
                ],
            ),
            ("strip", vec!["alps-sunrise.jpg"]),
            ("optimize", vec!["banner-hero.png"]),
            ("retina", vec!["logo.png", "favicon-32.png"]),
            ("rotate-left", vec!["thumbnail.jpg"]),
        ] {
            let pane = panel_state(&dir, &sel);
            let state = State {
                plugin: ImageToolsPlugin::new(),
                pane: pane.clone(),
                panel: SHEET,
            };
            let mut h = harness(state, theme, egui::vec2(460.0, 532.0));
            start(&mut h, &data);
            h.state_mut().plugin.run_command(op, &pane);
            wait(&mut h, "the sheet", |s| {
                lock(&s.plugin.shared).modal.is_some()
            });
            save(&mut h, &format!("{prefix}_sheet_{op}_{t}"));
            if op == "resize" {
                // Custom size, then "Overwrite originals" asks first.
                {
                    let sh = h.state().plugin.shared();
                    let mut st = lock(&sh);
                    let m = st.modal.as_mut().unwrap();
                    m.opts.preset = "custom".into();
                }
                h.run_steps(3);
                save(&mut h, &format!("{prefix}_sheet_resize_custom_{t}"));
                {
                    let sh = h.state().plugin.shared();
                    let mut st = lock(&sh);
                    let m = st.modal.as_mut().unwrap();
                    m.opts.output = "overwrite".into();
                    m.confirm = true;
                }
                h.run_steps(3);
                save(&mut h, &format!("{prefix}_sheet_confirm_{t}"));
            }
            h.state_mut().plugin.disable();
        }
    }
    set_thread_lang(None);
}

#[test]
fn render_image_tools_en() {
    render_all(Lang::En, "image_tools");
}

#[test]
fn render_image_tools_de() {
    render_all(Lang::De, "image_tools_de");
}

/// A remote pane and a running job (light and dark).
#[test]
fn render_remote_and_job() {
    if !wgpu_available() {
        return;
    }
    set_thread_lang(Some(Lang::En));
    let tmp = tempfile::tempdir().unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        let state = State {
            plugin: ImageToolsPlugin::new(),
            pane: pane_state(PaneId::Right, None, &[]),
            panel: PANEL,
        };
        let mut h = harness(state, theme, egui::vec2(340.0, 360.0));
        start(&mut h, tmp.path());
        wait(&mut h, "remote", |s| {
            lock(&s.plugin.shared).panel == PanelModel::Remote
        });
        // A job in progress, as the job thread shows it.
        let job = Arc::new(crate::engine::Job {
            op: "optimize".into(),
            total: 12,
            index: std::sync::atomic::AtomicUsize::new(4),
            current: std::sync::Mutex::new(Some(PathBuf::from("/x/banner-hero.png"))),
            done: std::sync::atomic::AtomicBool::new(false),
            cancelled: std::sync::atomic::AtomicBool::new(false),
        });
        lock(&h.state().plugin.shared).job = Some(job);
        h.run_steps(3);
        save(&mut h, &format!("image_tools_job_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
    set_thread_lang(None);
}
