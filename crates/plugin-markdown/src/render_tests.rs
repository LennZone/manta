//! Headless tests of the Markdown panel with `egui_kittest`: behaviour
//! tests (no GPU needed) and PNG renders (wgpu; skipped without an
//! adapter). PNGs go to `$MANTA_MARKDOWN_RENDER_DIR` (default
//! `target/plugin-markdown-renders`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme};
use egui_kittest::Harness;
use manta_core::VfsPath;
use manta_plugin::{
    FileContent, HostRequest, PaneId, PaneState, Plugin, PluginEvent, PluginHandle,
};

use crate::doc::Block;
use crate::loader::Content;
use crate::render::ImageState;
use crate::{pane_state, MarkdownPlugin, Want, PANEL_ID, PLUGIN_ID};

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
    let dir = std::env::var_os("MANTA_MARKDOWN_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-markdown-renders")
        });
    std::fs::create_dir_all(&dir).expect("render dir");
    dir
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// A made-up app screenshot (dark window with a sidebar and cards).
fn screenshot_png(path: &Path) {
    let (w, h) = (960u32, 540u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let in_rect = |x0: u32, y0: u32, x1: u32, y1: u32| x >= x0 && x < x1 && y >= y0 && y < y1;
        if y < 40 {
            image::Rgb([42, 44, 52])
        } else if x < 200 {
            if in_rect(
                20,
                70 + (y.saturating_sub(70) / 36) * 36,
                180,
                70 + (y.saturating_sub(70) / 36) * 36 + 22,
            ) && y < 330
            {
                image::Rgb([58, 62, 74])
            } else {
                image::Rgb([33, 35, 42])
            }
        } else if in_rect(232, 80, 568, 300)
            || in_rect(600, 80, 928, 300)
            || in_rect(232, 330, 928, 500)
        {
            let t = (x - 232) as f32 / 700.0;
            image::Rgb([
                (70.0 + 60.0 * t) as u8,
                (90.0 + 40.0 * (1.0 - t)) as u8,
                160,
            ])
        } else {
            image::Rgb([24, 26, 31])
        }
    });
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    img.save(path).unwrap();
}

/// The rich fixture in a temp folder (with a generated screenshot).
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("aurora");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rich"),
        &root,
    );
    screenshot_png(&root.join("images/screenshot.png"));
    (tmp, root)
}

pub struct State {
    pub plugin: MarkdownPlugin,
    pub pane: PaneState,
}

fn setup_ctx(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let base = fonts.families[&FontFamily::Proportional].clone();
    let phosphor = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/assets/fonts/Phosphor.ttf"),
    )
    .ok();
    if let Some(p) = phosphor {
        fonts
            .font_data
            .insert("phosphor".into(), Arc::new(FontData::from_owned(p)));
        let mut icons = vec!["phosphor".to_string()];
        icons.extend(base.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name("rl-icons".into()), icons);
    }
    if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        let sf = Arc::new(sf);
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
    if let Ok(m) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
        fonts
            .font_data
            .insert("sf-mono".into(), Arc::new(FontData::from_owned(m)));
        let mut chain = vec!["sf-mono".to_string()];
        chain.extend(fonts.families[&FontFamily::Monospace].iter().cloned());
        fonts.families.insert(FontFamily::Monospace, chain);
    }
    ctx.set_fonts(fonts);
    egui_extras::install_image_loaders(ctx);
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
                rgb(44, 44, 46)
            } else {
                rgb(236, 236, 238)
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

fn harness(state: State, theme: Theme, size: [f32; 2], gpu: bool) -> Harness<'static, State> {
    let b = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 30.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(setup_ctx);
    let b = if gpu { b.wgpu() } else { b };
    b.build_ui_state(
        |ui, s: &mut State| {
            let rect = ui.max_rect();
            let dark = ui.visuals().dark_mode;
            // The app's sidebar: content surface with a title bar.
            let content = if dark {
                Color32::from_rgb(30, 30, 32)
            } else {
                Color32::WHITE
            };
            ui.painter().rect_filled(rect, CornerRadius::ZERO, content);
            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(rect.width(), 28.0), egui::Sense::hover());
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
            s.plugin.panel_ui(PANEL_ID, &pane, ui);
        },
        state,
    )
}

/// Enable the plugin; returns the host requests it sends.
fn start(h: &mut Harness<'static, State>, data: &Path) -> Receiver<(String, HostRequest)> {
    let (tx, rx) = mpsc::channel();
    let handle = PluginHandle::new(PLUGIN_ID.into(), tx, h.ctx.clone(), data.to_path_buf());
    let st = h.state_mut();
    st.plugin.enable(handle);
    let ev = PluginEvent::DirectoryChanged(st.pane.clone());
    st.plugin.on_event(&ev);
    rx
}

fn wait(h: &mut Harness<'static, State>, what: &str, mut f: impl FnMut(&State) -> bool) {
    let t = Instant::now();
    while !f(h.state()) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn doc_of(s: &State) -> Option<Arc<crate::doc::Doc>> {
    match &s.plugin.view.shown()?.content {
        Content::Doc { doc, .. } => Some(doc.clone()),
        _ => None,
    }
}

fn images_ready(s: &State) -> bool {
    !s.plugin.view.images.is_empty()
        && s.plugin
            .view
            .images
            .values()
            .all(|i| !matches!(i, ImageState::Loading))
}

fn save(h: &mut Harness<'static, State>, name: &str) {
    // Images decode in the background.
    for _ in 0..30 {
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
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

// ------------------------------------------------------------------ behaviour

#[test]
fn fixture_renders_every_element() {
    let (_tmp, root) = fixture();
    let data = tempfile::tempdir().unwrap();
    let state = State {
        plugin: MarkdownPlugin::new(),
        pane: pane_state(PaneId::Left, &root, &[]),
    };
    let mut h = harness(state, Theme::Light, [460.0, 900.0], false);
    let _rx = start(&mut h, data.path());
    wait(&mut h, "readme", |s| doc_of(s).is_some());
    let doc = doc_of(h.state()).unwrap();
    assert!(
        h.state().plugin.view.shown().unwrap().readme,
        "nothing selected: the README"
    );

    // Every element kind is in the tree.
    let count = |f: &dyn Fn(&Block) -> bool| doc.count(f);
    assert_eq!(count(&|b| matches!(b, Block::Heading { .. })), 5);
    assert_eq!(
        count(&|b| matches!(
            b,
            Block::Code {
                highlight: Some(_),
                ..
            }
        )),
        2,
        "bash and rust are highlighted"
    );
    assert_eq!(count(&|b| matches!(b, Block::Table { .. })), 1);
    assert_eq!(
        count(&|b| matches!(b, Block::Quote { alert: Some(_), .. })),
        1
    );
    assert_eq!(count(&|b| matches!(b, Block::Quote { alert: None, .. })), 1);
    assert_eq!(count(&|b| matches!(b, Block::Rule)), 1);
    assert_eq!(
        count(&|b| matches!(b, Block::Footnote { number: 1, .. })),
        1
    );
    assert_eq!(
        count(&|b| matches!(b, Block::List { start: Some(1), .. })),
        1
    );
    let tasks = doc
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::List { items, start: None } => {
                Some(items.iter().map(|i| i.task).collect::<Vec<_>>())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        tasks,
        vec![Some(true), Some(true), Some(false), Some(false)]
    );
    let slugs: Vec<&str> = doc.headings.iter().map(|h| h.slug.as_str()).collect();
    assert_eq!(
        slugs,
        [
            "aurora",
            "features",
            "usage",
            "options",
            "why-another-generator"
        ]
    );

    // Drawn: anchors recorded, the local image loaded, web badges blocked.
    wait(&mut h, "images", images_ready);
    h.run_steps(3);
    let v = &h.state().plugin.view;
    for s in &slugs {
        assert!(v.anchor_y.contains_key(*s), "{s} drawn");
    }
    let shot = crate::links::ImageSource::File(VfsPath::new(
        &root.join("images/screenshot.png").to_string_lossy(),
    ));
    assert!(
        matches!(v.images.get(&shot), Some(ImageState::Ready { .. })),
        "{:?}",
        v.images.keys()
    );
    assert!(
        v.images
            .keys()
            .all(|k| !matches!(k, crate::links::ImageSource::Web(_))),
        "web images stay off"
    );
    h.state_mut().plugin.disable();
}

#[test]
fn links_navigate_and_go_back() {
    let (_tmp, root) = fixture();
    let data = tempfile::tempdir().unwrap();
    let state = State {
        plugin: MarkdownPlugin::new(),
        pane: pane_state(PaneId::Left, &root, &["README.md"]),
    };
    let mut h = harness(state, Theme::Light, [460.0, 900.0], false);
    let _rx = start(&mut h, data.path());
    wait(&mut h, "readme", |s| doc_of(s).is_some());
    let guide = VfsPath::new(&root.join("docs/guide.md").to_string_lossy());
    h.state_mut().plugin.open_markdown(guide.clone(), None);
    wait(&mut h, "guide", |s| {
        doc_of(s).is_some_and(|d| d.headings[0].text == "Guide")
    });
    assert_eq!(h.state().plugin.view.shown().unwrap().path, guide);
    // The guide links back to the README's Features section.
    let doc = doc_of(h.state()).unwrap();
    let dir = guide.parent().unwrap();
    let back = crate::links::classify(&doc.links[0], &dir);
    let crate::links::Target::Markdown { path, anchor } = back else {
        panic!("{back:?}")
    };
    assert_eq!(
        path,
        VfsPath::new(&root.join("README.md").to_string_lossy())
    );
    assert_eq!(anchor.as_deref(), Some("features"));
    h.state_mut().plugin.go_back();
    wait(&mut h, "readme again", |s| {
        doc_of(s).is_some_and(|d| d.headings[0].text == "Aurora")
    });
    assert!(h.state().plugin.view.history.is_empty());
    h.state_mut().plugin.disable();
}

#[test]
fn live_reload_keeps_scroll() {
    let (_tmp, root) = fixture();
    let data = tempfile::tempdir().unwrap();
    let state = State {
        plugin: MarkdownPlugin::new(),
        pane: pane_state(PaneId::Left, &root, &["README.md"]),
    };
    let mut h = harness(state, Theme::Light, [460.0, 600.0], false);
    let _rx = start(&mut h, data.path());
    wait(&mut h, "readme", |s| doc_of(s).is_some());
    h.run_steps(3);
    // Jump to "Usage" (as the TOC does): the view scrolls.
    h.state_mut().plugin.view.anchor = Some("usage".into());
    h.run_steps(10);
    let scrolled = h.state().plugin.view.scroll_y;
    assert!(scrolled > 100.0, "scrolled to Usage: {scrolled}");
    let v1 = h.state().plugin.view.shown().unwrap().version;

    // Edit the file on disk: the preview reloads by itself.
    let readme = root.join("README.md");
    let text = std::fs::read_to_string(&readme)
        .unwrap()
        .replace("## Why another generator?", "## Why Aurora?");
    std::fs::write(&readme, text).unwrap();
    wait(&mut h, "reload", |s| {
        s.plugin.view.shown().is_some_and(|l| l.version > v1)
    });
    let doc = doc_of(h.state()).unwrap();
    assert!(
        doc.headings.iter().any(|x| x.slug == "why-aurora"),
        "{:?}",
        doc.headings
    );
    h.run_steps(5);
    let after = h.state().plugin.view.scroll_y;
    assert!(
        (after - scrolled).abs() < 1.0,
        "scroll kept: {scrolled} → {after}"
    );
    h.state_mut().plugin.disable();
}

#[test]
fn remote_pane_reads_through_the_host() {
    let (_tmp, root) = fixture();
    let data = tempfile::tempdir().unwrap();
    let mut pane = pane_state(PaneId::Right, &root, &["README.md"]);
    pane.local_path = None;
    pane.protocol = manta_core::Protocol::Sftp;
    let state = State {
        plugin: MarkdownPlugin::new(),
        pane,
    };
    let mut h = harness(state, Theme::Light, [460.0, 900.0], false);
    let rx = start(&mut h, data.path());
    h.run_steps(2);
    // The plugin asks the host for the bytes …
    let reqs: Vec<HostRequest> = rx.try_iter().map(|(_, r)| r).collect();
    let path = VfsPath::new(&root.join("README.md").to_string_lossy());
    assert!(
        reqs.iter().any(|r| matches!(r, HostRequest::ReadFile { pane: PaneId::Right, path: p, .. } if *p == path)),
        "{reqs:?}"
    );
    // … and renders the answer.
    let bytes = std::fs::read(root.join("README.md")).unwrap();
    let c = FileContent {
        pane: PaneId::Right,
        path: path.clone(),
        size: Some(bytes.len() as u64),
        bytes,
        truncated: false,
        modified: None,
        error: None,
    };
    h.state_mut().plugin.on_event(&PluginEvent::FileContent(c));
    wait(&mut h, "remote doc", |s| doc_of(s).is_some());
    // Its local image is read through the host too.
    wait(&mut h, "image request", |_| true);
    h.run_steps(3);
    let reqs: Vec<HostRequest> = rx.try_iter().map(|(_, r)| r).collect();
    assert!(
        reqs.iter().any(|r| matches!(r, HostRequest::ReadFile { path: p, .. } if p.as_str().ends_with("images/screenshot.png"))),
        "{reqs:?}"
    );
    assert!(matches!(
        h.state().plugin.view.current(),
        Some(Want::File { local: false, .. })
    ));
    h.state_mut().plugin.disable();
}

#[test]
fn empty_states() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("big.md"), "x\n".repeat(1_100_000)).unwrap();
    std::fs::write(dir.join("blob.md"), b"\x00\x01binary").unwrap();
    let data = tempfile::tempdir().unwrap();
    let render = wgpu_available();
    for theme in [Theme::Light, Theme::Dark] {
        for (sel, name) in [
            (None, "no_readme"),
            (Some("main.rs"), "not_markdown"),
            (Some("big.md"), "too_large"),
            (Some("blob.md"), "unreadable"),
        ] {
            let sel: Vec<&str> = sel.into_iter().collect();
            let state = State {
                plugin: MarkdownPlugin::new(),
                pane: pane_state(PaneId::Left, &dir, &sel),
            };
            let mut h = harness(state, theme, [400.0, 520.0], render);
            let _rx = start(&mut h, data.path());
            wait(&mut h, name, |s| match name {
                "not_markdown" => matches!(s.plugin.view.current(), Some(Want::NotMarkdown { .. })),
                "no_readme" => matches!(
                    s.plugin.view.shown().map(|l| &l.content),
                    Some(Content::NoReadme)
                ),
                "too_large" => matches!(
                    s.plugin.view.shown().map(|l| &l.content),
                    Some(Content::TooLarge { .. })
                ),
                _ => matches!(
                    s.plugin.view.shown().map(|l| &l.content),
                    Some(Content::Unreadable(_))
                ),
            });
            if render {
                save(&mut h, &format!("md_empty_{name}_{}", tname(theme)));
            }
            h.state_mut().plugin.disable();
        }
    }
}

// ------------------------------------------------------------------ renders

#[test]
fn render_rich_readme() {
    if !wgpu_available() {
        return;
    }
    let (_tmp, root) = fixture();
    let data = tempfile::tempdir().unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        for (size, name) in [([460.0, 2900.0], "full"), ([460.0, 900.0], "top")] {
            let state = State {
                plugin: MarkdownPlugin::new(),
                pane: pane_state(PaneId::Left, &root, &[]),
            };
            let mut h = harness(state, theme, size, true);
            let _rx = start(&mut h, data.path());
            wait(&mut h, "readme", |s| doc_of(s).is_some());
            h.run_steps(2);
            wait(&mut h, "images", images_ready);
            save(&mut h, &format!("md_readme_{name}_{}", tname(theme)));
            if name == "top" {
                h.state_mut().plugin.view.toc_open = true;
                h.state_mut().plugin.view.anchor = Some("usage".into());
                h.run_steps(6);
                h.state_mut().plugin.view.toc_open = true;
                save(&mut h, &format!("md_readme_toc_{}", tname(theme)));
            }
            h.state_mut().plugin.disable();
        }
    }
}

/// German: the empty and error states and the README with its contents
/// menu (light and dark).
#[test]
fn render_markdown_de() {
    if !wgpu_available() {
        return;
    }
    use manta_plugin::i18n::{set_thread_lang, Lang};
    set_thread_lang(Some(Lang::De));
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("leer");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("gross.md"), "x\n".repeat(1_100_000)).unwrap();
    std::fs::write(dir.join("blob.md"), b"\x00\x01binary").unwrap();
    let data = tempfile::tempdir().unwrap();
    let (_rich, root) = fixture();
    for theme in [Theme::Light, Theme::Dark] {
        for (sel, name) in [
            (None, "empty"),
            (Some("main.rs"), "not_markdown"),
            (Some("gross.md"), "too_large"),
            (Some("blob.md"), "unreadable"),
        ] {
            let sel: Vec<&str> = sel.into_iter().collect();
            let state = State {
                plugin: MarkdownPlugin::new(),
                pane: pane_state(PaneId::Left, &dir, &sel),
            };
            let mut h = harness(state, theme, [340.0, 520.0], true);
            let _rx = start(&mut h, data.path());
            wait(&mut h, name, |s| match name {
                "not_markdown" => matches!(s.plugin.view.current(), Some(Want::NotMarkdown { .. })),
                "empty" => matches!(
                    s.plugin.view.shown().map(|l| &l.content),
                    Some(Content::NoReadme)
                ),
                "too_large" => matches!(
                    s.plugin.view.shown().map(|l| &l.content),
                    Some(Content::TooLarge { .. })
                ),
                _ => matches!(
                    s.plugin.view.shown().map(|l| &l.content),
                    Some(Content::Unreadable(_))
                ),
            });
            save(&mut h, &format!("markdown_de_{name}_{}", tname(theme)));
            h.state_mut().plugin.disable();
        }
        let state = State {
            plugin: MarkdownPlugin::new(),
            pane: pane_state(PaneId::Left, &root, &[]),
        };
        let mut h = harness(state, theme, [460.0, 2900.0], true);
        let _rx = start(&mut h, data.path());
        wait(&mut h, "readme", |s| doc_of(s).is_some());
        h.run_steps(2);
        wait(&mut h, "images", images_ready);
        save(&mut h, &format!("markdown_de_readme_{}", tname(theme)));
        h.state_mut().plugin.view.toc_open = true;
        h.run_steps(4);
        h.state_mut().plugin.view.toc_open = true;
        save(&mut h, &format!("markdown_de_readme_toc_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
    set_thread_lang(None);
}
