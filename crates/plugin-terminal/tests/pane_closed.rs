//! Several panes: closing a pane (`PluginEvent::PaneClosed`) hangs up the
//! shell below it and leaves the other panes' shells alone. Runs a real
//! zsh in a PTY (skips without one); needs no GPU.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use manta_core::{Protocol, VfsPath};
use manta_plugin::{PaneId, PaneState, Plugin, PluginEvent, PluginHandle};
use manta_plugin_terminal::TerminalPlugin;

fn pane_state(pane: PaneId, dir: &Path) -> PaneState {
    PaneState {
        remote: None,
        pane,
        path: VfsPath::new(&dir.to_string_lossy()),
        local_path: Some(dir.to_path_buf()),
        protocol: Protocol::Local,
        backend_label: "Local".into(),
        selection: vec![],
        active: pane == PaneId(2),
    }
}

fn alive(pid: i32) -> bool {
    // Signal 0: does the process (still) exist? Reaped zombies count as gone.
    unsafe { libc::kill(pid, 0) == 0 }
}

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
    ctx.set_fonts(fonts);
}

type Shared = Rc<RefCell<(TerminalPlugin, Vec<PaneState>, bool)>>;

#[test]
fn closing_a_pane_hangs_up_its_terminal() {
    if !Path::new("/bin/zsh").exists()
        || !Path::new(SF_PRO).exists()
        || !Path::new(SF_MONO).exists()
    {
        eprintln!("skipping: no zsh or system fonts");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    let data: PathBuf = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    // Terminals below pane 1 ("right") and pane 3 (id 2) of a three-pane window.
    let panes = vec![pane_state(PaneId::RIGHT, &dir), pane_state(PaneId(2), &dir)];
    let state: Shared = Rc::new(RefCell::new((TerminalPlugin::new(), panes, false)));
    let st = state.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 300.0))
        .with_context_setup(install_fonts)
        .build_ui(move |ui| {
            let mut s = st.borrow_mut();
            let (plugin, panes, enabled) = &mut *s;
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
            let panes = panes.clone();
            ui.columns(panes.len().max(1), |cols| {
                for (ui, p) in cols.iter_mut().zip(&panes) {
                    ui.set_min_size(egui::vec2(400.0, 280.0));
                    plugin.panel_ui("terminal", p, ui);
                }
            });
        });
    let start = Instant::now();
    let pids = |s: &Shared, p: PaneId| s.borrow().0.debug_pids(p);
    while (pids(&state, PaneId(2)).is_empty() || pids(&state, PaneId::RIGHT).is_empty())
        && start.elapsed() < Duration::from_secs(20)
    {
        h.step();
        std::thread::sleep(Duration::from_millis(25));
    }
    let closed = pids(&state, PaneId(2));
    let kept = pids(&state, PaneId::RIGHT);
    assert!(
        !closed.is_empty() && !kept.is_empty(),
        "both shells started"
    );
    assert!(closed.iter().chain(&kept).all(|p| alive(*p)));

    // The host says pane 3 is gone (and stops drawing it).
    state.borrow_mut().1.retain(|p| p.pane != PaneId(2));
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::PaneClosed(PaneId(2)));
    assert_eq!(
        state.borrow().0.debug_panes(),
        vec![PaneId::RIGHT],
        "its state is gone"
    );
    let start = Instant::now();
    while closed.iter().any(|p| alive(*p)) && start.elapsed() < Duration::from_secs(5) {
        h.step();
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        closed.iter().all(|p| !alive(*p)),
        "the closed pane's shell was hung up"
    );
    assert!(
        kept.iter().all(|p| alive(*p)),
        "the other pane's shell keeps running"
    );
    state.borrow_mut().0.disable();
}
