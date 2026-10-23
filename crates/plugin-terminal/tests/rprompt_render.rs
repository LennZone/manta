//! Renders the terminal panel with the user's own zsh config while the panel
//! width changes (wide → narrow → wider), to look at right prompts.
//!
//! Writes `rprompt-<tag>-<theme>-<step>.png` to
//! `$MANTA_RENDER_DIR/plugins/terminal` (tag: `$RPROMPT_TAG`, default
//! "after"). Only uses the public plugin API, so it also runs against older
//! revisions for before/after comparisons. Skips without zsh, SF fonts or GPU.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use manta_core::{Protocol, VfsPath};
use manta_plugin::{PaneId, PaneState, Plugin, PluginHandle};
use manta_plugin_terminal::TerminalPlugin;

const SF_PRO: &str = "/System/Library/Fonts/SFNS.ttf";
const SF_MONO: &str = "/System/Library/Fonts/SFNSMono.ttf";

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    for (name, path, family) in [
        ("sf", SF_PRO, egui::FontFamily::Proportional),
        ("sf-mono", SF_MONO, egui::FontFamily::Monospace),
    ] {
        let bytes = std::fs::read(path).unwrap();
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts.families.entry(family).or_default().push(name.into());
    }
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("sf".into());
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("sf-mono".into());
    ctx.set_fonts(fonts);
}

type Shared = Rc<RefCell<(TerminalPlugin, PaneState, bool)>>;

fn pane(dir: &Path) -> PaneState {
    PaneState {
        remote: None,
        pane: PaneId::Left,
        path: VfsPath::new(&dir.to_string_lossy()),
        local_path: Some(dir.to_path_buf()),
        protocol: Protocol::Local,
        backend_label: "Local".into(),
        selection: vec![],
        active: true,
    }
}

fn pump(h: &mut Harness<'_>, state: &Shared, ms: u64, cond: impl Fn(&TerminalPlugin) -> bool) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        h.step();
        if cond(&state.borrow().0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn settle(h: &mut Harness<'_>, state: &Shared, ms: u64) {
    pump(h, state, ms, |_| false);
}

fn out_dir() -> PathBuf {
    let base = std::env::var_os("MANTA_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("plugins").join("terminal");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(theme: egui::Theme, name: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("blog");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let state: Shared = Rc::new(RefCell::new((TerminalPlugin::new(), pane(&root), false)));
    let st = state.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 330.0))
        .with_pixels_per_point(2.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(install_fonts)
        .wgpu()
        .build_ui(move |ui| {
            let mut s = st.borrow_mut();
            let (plugin, pane, enabled) = &mut *s;
            if !*enabled {
                let (tx, rx) = std::sync::mpsc::channel();
                std::mem::forget(rx);
                plugin.enable(PluginHandle::new(
                    "manta.terminal".into(),
                    tx,
                    ui.ctx().clone(),
                    data.clone(),
                ));
                plugin.settings_mut().shell = "/bin/zsh".into();
                *enabled = true;
            }
            let pane = pane.clone();
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    plugin.panel_ui("terminal", &pane, ui);
                });
        });
    pump(&mut h, &state, 20_000, |p| p.debug_is_idle(PaneId::Left));
    settle(&mut h, &state, 300);
    let tag = std::env::var("RPROMPT_TAG").unwrap_or_else(|_| "after".into());
    let save = |h: &mut Harness<'_>, step: &str| {
        let img = h.render().expect("render");
        let path = out_dir().join(format!("rprompt-{tag}-{name}-{step}.png"));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    };
    for cmd in ["cd src", "cd ..", "true"] {
        state
            .borrow()
            .0
            .debug_write(PaneId::Left, format!("{cmd}\r").as_bytes());
        settle(&mut h, &state, 150);
        pump(&mut h, &state, 3000, |p| p.debug_is_idle(PaneId::Left));
        settle(&mut h, &state, 200);
    }
    save(&mut h, "1-wide");
    // Narrower (e.g. a sidebar opened), in a few drag steps.
    for w in [900.0, 780.0, 640.0] {
        h.set_size(egui::vec2(w, 330.0));
        settle(&mut h, &state, 60);
    }
    settle(&mut h, &state, 700);
    save(&mut h, "2-narrow");
    state.borrow().0.debug_write(PaneId::Left, b"true\r");
    settle(&mut h, &state, 700);
    // Wider again.
    for w in [760.0, 900.0, 1100.0] {
        h.set_size(egui::vec2(w, 330.0));
        settle(&mut h, &state, 60);
    }
    settle(&mut h, &state, 700);
    save(&mut h, "3-wider");
    state.borrow_mut().0.disable();
}

fn gpu_ok() -> bool {
    std::panic::catch_unwind(|| {
        let mut h = Harness::builder()
            .with_size(egui::vec2(10.0, 10.0))
            .with_context_setup(install_fonts)
            .wgpu()
            .build_ui(|ui| {
                ui.label("x");
            });
        h.render().is_ok()
    })
    .unwrap_or(false)
}

#[test]
fn render_rprompt_across_widths() {
    if !Path::new("/bin/zsh").exists()
        || !Path::new(SF_PRO).exists()
        || !Path::new(SF_MONO).exists()
    {
        eprintln!("skipping: needs zsh and SF fonts");
        return;
    }
    if !gpu_ok() {
        eprintln!("skipping: no wgpu adapter");
        return;
    }
    run(egui::Theme::Dark, "dark");
    run(egui::Theme::Light, "light");
}
