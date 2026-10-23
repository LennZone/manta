//! Headless renders of the Duplicates panel and the "Move to Trash" sheet
//! with `egui_kittest` + wgpu, against a real temp tree, in English and
//! German, light and dark. PNGs go to `$MANTA_DUPLICATES_RENDER_DIR`
//! (default `target/plugin-duplicates-renders`). Skips without a wgpu
//! adapter.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme};
use egui_kittest::Harness;
use manta_plugin::i18n::{set_thread_lang, Lang};
use manta_plugin::{PaneId, PaneState, Plugin, PluginEvent, PluginHandle};

use crate::{lock, pane_state, Action, DuplicatesPlugin, Target, CONFIRM, PANEL};

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
    let dir = std::env::var_os("MANTA_DUPLICATES_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-duplicates-renders")
        });
    std::fs::create_dir_all(&dir).expect("render dir");
    dir
}

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
pub fn make_tree(root: &Path) {
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

struct State {
    plugin: DuplicatesPlugin,
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
                let fill = if s.panel == CONFIRM {
                    ui.visuals().window_fill
                } else {
                    ui.visuals().panel_fill
                };
                ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
                let (bar, _) =
                    ui.allocate_exact_size(egui::vec2(rect.width(), 32.0), egui::Sense::hover());
                let title = if s.panel == CONFIRM {
                    crate::i18n::tr!("panel.confirm")
                } else {
                    crate::i18n::tr!("panel.duplicates")
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

fn wait(h: &mut Harness<'static, State>, mut done: impl FnMut(&State) -> bool) {
    let t = Instant::now();
    while !done(h.state()) && t.elapsed() < Duration::from_secs(20) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(4);
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

fn render_all(lang: Lang, prefix: &str) {
    if !wgpu_available() {
        return;
    }
    set_thread_lang(Some(lang));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("Archive");
    make_tree(&root);
    for theme in [Theme::Light, Theme::Dark] {
        let data = tmp.path().join(format!("data-{}", tname(theme)));
        // The intro, then the results.
        let state = State {
            plugin: DuplicatesPlugin::new(),
            pane: pane_state(PaneId::Left, Some(&root)),
            panel: PANEL,
        };
        let mut h = harness(state, theme, egui::vec2(340.0, 900.0));
        start(&mut h, &data);
        h.run_steps(4);
        save(&mut h, &format!("{prefix}_intro_{}", tname(theme)));
        let pane = h.state().pane.clone();
        h.state_mut().plugin.apply(Action::Scan, &pane);
        wait(&mut h, |s| lock(&s.plugin.shared).result.is_some());
        save(&mut h, &format!("{prefix}_results_{}", tname(theme)));
        {
            let st = lock(&h.state().plugin.shared);
            let (groups, files, size) = st.result.as_ref().unwrap().totals();
            assert_eq!((groups, files), (5, 7), "node_modules is skipped");
            assert_eq!(
                size,
                2_400_000 * 2 + 5_100_000 + 850_000 + 180_000 * 2 + 2_000
            );
        }
        // Keep another copy in the first group, then ask to trash all.
        h.state_mut().plugin.apply(Action::Keep(0, 0), &pane);
        h.state_mut()
            .plugin
            .apply(Action::AskTrash(Target::All), &pane);
        h.run_steps(2);
        save(&mut h, &format!("{prefix}_results_chosen_{}", tname(theme)));
        let plugin = std::mem::take(&mut h.state_mut().plugin);
        drop(h);

        // The confirmation sheet (the host's modal is 440 × 340 + title).
        let state = State {
            plugin,
            pane: pane_state(PaneId::Left, Some(&root)),
            panel: CONFIRM,
        };
        let mut h = harness(state, theme, egui::vec2(440.0, 372.0));
        h.run_steps(4);
        save(&mut h, &format!("{prefix}_confirm_{}", tname(theme)));
        h.state_mut().plugin.disable();

        // A remote pane.
        let state = State {
            plugin: DuplicatesPlugin::new(),
            pane: pane_state(PaneId::Right, None),
            panel: PANEL,
        };
        let mut h = harness(state, theme, egui::vec2(340.0, 520.0));
        start(&mut h, &data);
        h.run_steps(4);
        save(&mut h, &format!("{prefix}_remote_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
    set_thread_lang(None);
}

#[test]
fn render_duplicates_en() {
    render_all(Lang::En, "duplicates");
}

#[test]
fn render_duplicates_de() {
    render_all(Lang::De, "duplicates_de");
}

/// A scan in progress (hashing a big tree, cancelled after the render).
#[test]
fn render_scanning() {
    if !wgpu_available() {
        return;
    }
    set_thread_lang(Some(Lang::En));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("Big");
    for i in 0..400 {
        write(
            &root,
            &format!("d{}/f{i}.bin", i % 20),
            &bytes(i % 50, 300_000),
            1,
        );
    }
    for theme in [Theme::Light, Theme::Dark] {
        let state = State {
            plugin: DuplicatesPlugin::new(),
            pane: pane_state(PaneId::Left, Some(&root)),
            panel: PANEL,
        };
        let mut h = harness(state, theme, egui::vec2(340.0, 480.0));
        start(&mut h, &tmp.path().join("data"));
        let pane = h.state().pane.clone();
        h.state_mut().plugin.apply(Action::Scan, &pane);
        wait(&mut h, |s| {
            let st = lock(&s.plugin.shared);
            st.scan
                .as_ref()
                .is_some_and(|sc| sc.progress.done.load(std::sync::atomic::Ordering::Relaxed) > 0)
                || st.result.is_some()
        });
        save(&mut h, &format!("duplicates_scanning_{}", tname(theme)));
        h.state_mut().plugin.apply(Action::Cancel, &pane);
        h.state_mut().plugin.disable();
    }
    set_thread_lang(None);
}
