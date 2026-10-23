//! Headless renders of the Disk Usage panel with `egui_kittest` + wgpu,
//! against a real temp tree. PNGs go to `$MANTA_FOLDERSIZE_RENDER_DIR`
//! (default `target/plugin-foldersize-renders`). Skips without a wgpu
//! adapter.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme};
use egui_kittest::Harness;
use manta_plugin::{PaneId, PaneState, Plugin, PluginEvent, PluginHandle};

use crate::engine::lock;
use crate::{pane_state, FolderSizePlugin};

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
    let dir = std::env::var_os("MANTA_FOLDERSIZE_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-foldersize-renders")
        });
    std::fs::create_dir_all(&dir).expect("render dir");
    dir
}

/// A sparse file of `len` bytes (instant to create).
fn sized(root: &Path, rel: &str, len: u64) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::File::create(&p).unwrap().set_len(len).unwrap();
}

const MB: u64 = 1_000_000;

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path().join("Anna");
    sized(&r, "Movies/Holiday 2025.mov", 3_400 * MB);
    sized(&r, "Movies/Screen Recording.mov", 820 * MB);
    sized(&r, "Movies/Interview raw.mp4", 1_250 * MB);
    for i in 0..40 {
        sized(
            &r,
            &format!("Pictures/Iceland/IMG_{:04}.heic", 3000 + i),
            3 * MB + i * 70_000,
        );
    }
    for i in 0..12 {
        sized(&r, &format!("Pictures/Exports/export-{i}.png"), 9 * MB);
    }
    for i in 0..30 {
        sized(
            &r,
            &format!("Projects/webapp/node_modules/pkg{i}/index.js"),
            4 * MB + i * 40_000,
        );
    }
    sized(&r, "Projects/webapp/src/app.ts", 120_000);
    sized(&r, "Projects/manta/target/debug/manta", 410 * MB);
    sized(
        &r,
        "Projects/manta/target/debug/deps/libegui.rlib",
        280 * MB,
    );
    for i in 0..20 {
        sized(&r, &format!("Music/Album {i}/track.m4a"), 22 * MB);
    }
    sized(&r, "Documents/Taxes 2025.pdf", 14 * MB);
    sized(&r, "Documents/Thesis.pages", 96 * MB);
    sized(&r, "Documents/Budget.numbers", 3 * MB);
    sized(&r, "Applications/Figma.app/Contents/MacOS/Figma", 520 * MB);
    sized(&r, "Downloads/Xcode_17.xip", 2_900 * MB);
    sized(&r, "Downloads/archive-2024.zip", 640 * MB);
    sized(&r, "Downloads/setup.dmg", 210 * MB);
    sized(&r, "backup.sparsebundle.zip", 1_100 * MB);
    sized(&r, "notes.md", 4_000);
    sized(&r, ".zsh_history", 90_000);
    tmp
}

struct State {
    plugin: FolderSizePlugin,
    pane: PaneState,
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

fn harness(state: State, theme: Theme) -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(340.0, 820.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 30.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(setup_ctx)
        .wgpu()
        .build_ui_state(
            |ui, s: &mut State| {
                let rect = ui.max_rect();
                ui.painter()
                    .rect_filled(rect, CornerRadius::ZERO, ui.visuals().panel_fill);
                let (bar, _) =
                    ui.allocate_exact_size(egui::vec2(rect.width(), 32.0), egui::Sense::hover());
                ui.painter().text(
                    bar.left_center() + egui::vec2(12.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    crate::i18n::tr!("panel.title"),
                    egui::FontId::new(13.0, FontFamily::Name("rl-semibold".into())),
                    ui.visuals().text_color(),
                );
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom(),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                let pane = s.pane.clone();
                s.plugin.panel_ui(crate::PANEL_ID, &pane, ui);
            },
            state,
        )
}

fn start(h: &mut Harness<'static, State>) {
    let (tx, rx) = mpsc::channel();
    std::mem::forget(rx);
    let data = std::env::temp_dir().join("manta-foldersize-render-data");
    let handle = PluginHandle::new("manta.foldersize".into(), tx, h.ctx.clone(), data);
    let st = h.state_mut();
    st.plugin.enable(handle);
    let ev = PluginEvent::DirectoryChanged(st.pane.clone());
    st.plugin.on_event(&ev);
}

/// Step until every item of `dir` is measured.
fn wait_measured(h: &mut Harness<'static, State>, dir: &Path) {
    let t = Instant::now();
    loop {
        h.step();
        let done = {
            let d = lock(&h.state().plugin.panel_data()).clone_state();
            d.0.as_deref() == Some(dir) && d.1 && d.2
        };
        if done || t.elapsed() > Duration::from_secs(10) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(4);
}

impl crate::engine::PanelData {
    /// (dir, listed, all complete) for tests.
    fn clone_state(&self) -> (Option<PathBuf>, bool, bool) {
        (
            self.dir.clone(),
            self.listed,
            self.items.iter().all(|i| i.complete),
        )
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

#[test]
fn render_disk_usage_panel() {
    if !wgpu_available() {
        return;
    }
    let tmp = fixture();
    let root = tmp.path().join("Anna");
    for theme in [Theme::Light, Theme::Dark] {
        let state = State {
            plugin: FolderSizePlugin::new(),
            pane: pane_state(PaneId::Left, Some(&root)),
        };
        let mut h = harness(state, theme);
        start(&mut h);
        wait_measured(&mut h, &root);
        save(&mut h, &format!("panel_treemap_{}", tname(theme)));

        // Every tile lies inside the treemap and none overlap.
        let tiles = h.state().plugin.panel.tiles.clone();
        assert!(tiles.len() >= 8, "{} tiles", tiles.len());
        for (i, a) in tiles.iter().enumerate() {
            for b in &tiles[i + 1..] {
                assert!(
                    !a.0.intersects(b.0.shrink(0.5)),
                    "{:?} overlaps {:?}",
                    a.1,
                    b.1
                );
            }
        }

        // Hover the largest tile: tooltip.
        let (r, _, _) = tiles[0].clone();
        h.event(egui::Event::PointerMoved(r.center()));
        h.run_steps(30);
        save(&mut h, &format!("panel_treemap_hover_{}", tname(theme)));

        // Zoom two levels (Projects › website) via the UI state (as a click does).
        let pictures = root.join("Projects").join("website");
        h.state_mut()
            .plugin
            .panel_ui_state()
            .zoom
            .push(root.join("Projects"));
        h.state_mut()
            .plugin
            .panel_ui_state()
            .zoom
            .push(pictures.clone());
        h.event(egui::Event::PointerGone);
        h.run_steps(2);
        wait_measured(&mut h, &pictures);
        save(&mut h, &format!("panel_treemap_zoomed_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
}

#[test]
fn clicking_a_folder_zooms_and_double_click_navigates() {
    if !wgpu_available() {
        return;
    }
    let tmp = fixture();
    let root = tmp.path().join("Anna");
    let state = State {
        plugin: FolderSizePlugin::new(),
        pane: pane_state(PaneId::Left, Some(&root)),
    };
    let mut h = harness(state, Theme::Light);
    start(&mut h);
    wait_measured(&mut h, &root);
    let tiles = h.state().plugin.panel.tiles.clone();
    let (r, path, _) = tiles.iter().find(|t| t.2).cloned().expect("a folder tile");
    h.event(egui::Event::PointerMoved(r.center()));
    h.run_steps(1);
    let click = |h: &mut Harness<'static, State>| {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: r.center(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
    };
    click(&mut h);
    assert_eq!(
        h.state().plugin.panel.zoom,
        vec![path.clone()],
        "a click zooms in"
    );
    click(&mut h);
    assert!(
        h.state().plugin.panel.zoom.is_empty(),
        "the double-click undoes the zoom (and navigates)"
    );
    h.state_mut().plugin.disable();
}

#[test]
fn render_disk_usage_remote() {
    if !wgpu_available() {
        return;
    }
    for theme in [Theme::Light, Theme::Dark] {
        let state = State {
            plugin: FolderSizePlugin::new(),
            pane: pane_state(PaneId::Right, None),
        };
        let mut h = harness(state, theme);
        start(&mut h);
        h.run_steps(4);
        save(&mut h, &format!("panel_remote_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
}

/// German: treemap, hover tooltip, a zoomed folder and the remote empty
/// state (light and dark).
#[test]
fn render_disk_usage_de() {
    if !wgpu_available() {
        return;
    }
    use manta_plugin::i18n::{set_thread_lang, Lang};
    set_thread_lang(Some(Lang::De));
    let tmp = fixture();
    let root = tmp.path().join("Anna");
    for theme in [Theme::Light, Theme::Dark] {
        let state = State {
            plugin: FolderSizePlugin::new(),
            pane: pane_state(PaneId::Left, Some(&root)),
        };
        let mut h = harness(state, theme);
        start(&mut h);
        wait_measured(&mut h, &root);
        save(&mut h, &format!("disk_usage_de_{}", tname(theme)));
        let tiles = h.state().plugin.panel.tiles.clone();
        let (r, _, _) = tiles.iter().find(|t| t.2).cloned().expect("a folder tile");
        h.event(egui::Event::PointerMoved(r.center()));
        h.run_steps(30);
        save(&mut h, &format!("disk_usage_de_hover_{}", tname(theme)));
        h.state_mut().plugin.disable();

        let state = State {
            plugin: FolderSizePlugin::new(),
            pane: pane_state(PaneId::Right, None),
        };
        let mut h = harness(state, theme);
        start(&mut h);
        h.run_steps(4);
        save(&mut h, &format!("disk_usage_de_remote_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
    set_thread_lang(None);
}
