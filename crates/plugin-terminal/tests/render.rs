//! Visual check: renders the terminal panel (real zsh in a PTY) with
//! egui_kittest + wgpu and writes PNGs for a human/agent to look at.
//!
//! Output dir: `$MANTA_RENDER_DIR/plugins/terminal` (default: the system
//! temp dir). Skips when zsh or a wgpu adapter is unavailable.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use manta_core::{Protocol, VfsPath};
use manta_plugin::{
    FileAction, FileActionKind, FileActionPhase, PaneId, PaneState, Plugin, PluginEvent,
    PluginHandle,
};
use manta_plugin_terminal::TerminalPlugin;

const DEMO: &str = r#"printf '\033[1mbold\033[0m \033[3mitalic\033[0m \033[4munderline\033[0m \033[7minverse\033[0m \033[2mdim\033[0m \033[9mstrike\033[0m \033[1;3;4mall\033[0m\n'
for i in 0 1 2 3 4 5 6 7; do printf "\033[3${i}mcolor$i \033[0m"; done; printf '\n'
for i in 0 1 2 3 4 5 6 7; do printf "\033[9${i}mbright$i \033[0m"; done; printf '\n'
for i in 0 1 2 3 4 5 6 7; do printf "\033[4${i}m    \033[0m"; done; for i in 0 1 2 3 4 5 6 7; do printf "\033[10${i}m    \033[0m"; done; printf '\n'
i=16; while [ $i -lt 232 ]; do printf "\033[48;5;${i}m "; i=$((i+1)); if [ $(( (i-16) % 72 )) -eq 0 ]; then printf '\033[0m\n'; fi; done; printf '\033[0m'
i=232; while [ $i -lt 256 ]; do printf "\033[48;5;${i}m   "; i=$((i+1)); done; printf '\033[0m\n'
i=0; while [ $i -lt 72 ]; do r=$((255 - i*3)); g=$((i*3)); printf "\033[48;2;${r};${g};160m "; i=$((i+1)); done; printf '\033[0m\n'
printf '\033[38;2;255;140;0mtruecolor orange\033[0m \033[38;5;45m256-colour 45\033[0m  ┌──┬──┐ │ab│cd│ └──┴──┘ ═║╔╗ █▀▄░▒▓  Übergröße café ✓ 日本語\n'
"#;

fn out_dir() -> PathBuf {
    let base = std::env::var_os("MANTA_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("plugins").join("terminal");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pane_state(dir: Option<&Path>) -> PaneState {
    let path = dir
        .map(|d| VfsPath::new(&d.to_string_lossy()))
        .unwrap_or_else(|| VfsPath::new("/home/anna"));
    PaneState {
        remote: None,
        pane: PaneId::Left,
        path,
        local_path: dir.map(Path::to_path_buf),
        protocol: if dir.is_some() {
            Protocol::Local
        } else {
            Protocol::Sftp
        },
        backend_label: if dir.is_some() {
            "Local".into()
        } else {
            "anna@files.example.org (SFTP)".into()
        },
        selection: vec![],
        active: true,
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    other: PathBuf,
    data: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("Projects");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("README.md"), "# demo\n").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(root.join("demo.sh"), DEMO).unwrap();
    std::fs::write(root.join("build.sh"), "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join("build.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let other = root.join("docs");
    let data = tmp.path().join("plugin-data");
    std::fs::create_dir_all(&data).unwrap();
    Fixture {
        _tmp: tmp,
        root,
        other,
        data,
    }
}

type Shared = Rc<RefCell<(TerminalPlugin, PaneState, bool)>>;

fn harness(theme: egui::Theme, state: Shared, data: PathBuf) -> Harness<'static> {
    harness_shell(theme, state, data, "/bin/zsh")
}

fn harness_shell(
    theme: egui::Theme,
    state: Shared,
    data: PathBuf,
    shell: &'static str,
) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 380.0))
        .with_pixels_per_point(2.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(install_app_like_fonts)
        .wgpu()
        .build_ui(move |ui| {
            let mut st = state.borrow_mut();
            let (plugin, pane, enabled) = &mut *st;
            if !*enabled {
                let (tx, _rx) = std::sync::mpsc::channel();
                std::mem::forget(_rx);
                plugin.enable(PluginHandle::new(
                    "manta.terminal".into(),
                    tx,
                    ui.ctx().clone(),
                    data.clone(),
                ));
                plugin.settings_mut().shell = shell.into();
                *enabled = true;
            }
            let pane = pane.clone();
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    plugin.panel_ui("terminal", &pane, ui);
                });
        })
}

/// Step the UI in real time until `cond` holds.
fn pump(
    h: &mut Harness<'_>,
    state: &Shared,
    limit: Duration,
    cond: impl Fn(&TerminalPlugin) -> bool,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        h.step();
        if cond(&state.borrow().0) {
            // A couple more frames so the last output is painted.
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(30));
                h.step();
            }
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn save(h: &mut Harness<'_>, name: &str) {
    let img = h.render().expect("render");
    let path = out_dir().join(format!("{name}.png"));
    img.save(&path).unwrap();
    eprintln!("wrote {}", path.display());
}

fn screen(state: &Shared) -> String {
    state
        .borrow()
        .0
        .debug_screen(PaneId::Left)
        .unwrap_or_default()
}

fn run_theme(theme: egui::Theme, tag: &str) {
    let fx = fixture();
    let state: Shared = Rc::new(RefCell::new((
        TerminalPlugin::new(),
        pane_state(Some(&fx.root)),
        false,
    )));
    let mut h = harness(theme, state.clone(), fx.data.clone());

    assert!(
        pump(&mut h, &state, Duration::from_secs(20), |p| p
            .debug_is_idle(PaneId::Left)),
        "no prompt:\n{}",
        screen(&state)
    );
    state
        .borrow()
        .0
        .debug_write(PaneId::Left, b"clear; sh demo.sh; CLICOLOR=1 ls -laG\r");
    assert!(pump(&mut h, &state, Duration::from_secs(10), |p| {
        p.debug_screen(PaneId::Left)
            .is_some_and(|s| s.contains("README.md"))
            && p.debug_is_idle(PaneId::Left)
    }));
    // Focus the terminal (block cursor).
    click_center(&mut h);
    save(&mut h, &format!("colors-{tag}"));

    // "will cd when idle": a program runs while the pane navigates.
    state.borrow().0.debug_write(PaneId::Left, b"sleep 3\r");
    pump(&mut h, &state, Duration::from_secs(2), |p| {
        !p.debug_is_idle(PaneId::Left)
    });
    let other = pane_state(Some(&fx.other));
    state.borrow_mut().1 = other.clone();
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::DirectoryChanged(other));
    assert!(pump(&mut h, &state, Duration::from_secs(2), |p| p
        .debug_has_pending(PaneId::Left)));
    save(&mut h, &format!("pending-{tag}"));
    // ...and it is applied once sleep ends.
    assert!(pump(&mut h, &state, Duration::from_secs(8), |p| !p
        .debug_has_pending(PaneId::Left)
        && p.debug_is_idle(PaneId::Left)));
    state.borrow().0.debug_write(PaneId::Left, b"pwd\r");
    assert!(pump(&mut h, &state, Duration::from_secs(5), |p| p
        .debug_screen(PaneId::Left)
        .is_some_and(|s| s
            .lines()
            .any(|l| l.ends_with("Projects/docs")))));
    save(&mut h, &format!("followed-{tag}"));

    // Remote pane: dimmed session + note.
    let remote = pane_state(None);
    state.borrow_mut().1 = remote.clone();
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::DirectoryChanged(remote));
    pump(&mut h, &state, Duration::from_millis(200), |_| true);
    save(&mut h, &format!("remote-{tag}"));

    state.borrow_mut().0.disable();
}

fn action(
    id: u64,
    kind: FileActionKind,
    sources: Vec<PathBuf>,
    dest: Option<PathBuf>,
) -> FileAction {
    let v = |p: &Path| VfsPath::new(&p.to_string_lossy());
    let mut a = FileAction::new(
        id,
        PaneId::Left,
        kind,
        sources.iter().map(|p| v(p)).collect(),
    );
    a.destination = dest.as_deref().map(v);
    a
}

fn finished(
    mut a: FileAction,
    ok: bool,
    files: u64,
    bytes: u64,
    errors: &[&str],
    ms: u64,
) -> FileAction {
    a.phase = FileActionPhase::Finished {
        ok,
        files,
        bytes,
        errors: errors.iter().map(|s| s.to_string()).collect(),
        cancelled: false,
        elapsed_ms: ms,
    };
    a
}

/// GUI file actions echoed in the terminal: copy ✓, delete ✓, move ✗,
/// with half-typed input kept below.
fn run_actions(theme: egui::Theme, tag: &str) {
    run_actions_in(theme, tag, false);
}

/// `german`: the plugin UI and echo lines in German (this thread only).
fn run_actions_in(theme: egui::Theme, tag: &str, german: bool) {
    let failed = if german {
        "✗ fehlgeschlagen"
    } else {
        "✗ failed"
    };
    let fx = fixture();
    let state: Shared = Rc::new(RefCell::new((
        TerminalPlugin::new(),
        pane_state(Some(&fx.root)),
        false,
    )));
    let mut h = harness(theme, state.clone(), fx.data.clone());
    assert!(
        pump(&mut h, &state, Duration::from_secs(20), |p| p
            .debug_is_idle(PaneId::Left)),
        "no prompt:\n{}",
        screen(&state)
    );
    state.borrow().0.debug_write(PaneId::Left, b"clear; ls\r");
    assert!(pump(&mut h, &state, Duration::from_secs(10), |p| {
        p.debug_screen(PaneId::Left)
            .is_some_and(|s| s.contains("README.md"))
            && p.debug_is_idle(PaneId::Left)
    }));
    let r = &fx.root;
    let events = [
        action(
            1,
            FileActionKind::Copy,
            vec![
                r.join("README.md"),
                r.join("Cargo.toml"),
                r.join("build.sh"),
            ],
            Some(r.join("docs")),
        ),
        finished(
            action(
                1,
                FileActionKind::Copy,
                vec![
                    r.join("README.md"),
                    r.join("Cargo.toml"),
                    r.join("build.sh"),
                ],
                Some(r.join("docs")),
            ),
            true,
            3,
            12_400_000,
            &[],
            400,
        ),
        action(2, FileActionKind::Delete, vec![r.join("old build")], None),
        finished(
            action(2, FileActionKind::Delete, vec![r.join("old build")], None),
            true,
            14,
            0,
            &[],
            30,
        ),
        action(
            3,
            FileActionKind::Move,
            vec![r.join("demo.sh")],
            Some(PathBuf::from("/Volumes/Backup")),
        ),
        finished(
            action(
                3,
                FileActionKind::Move,
                vec![r.join("demo.sh")],
                Some(PathBuf::from("/Volumes/Backup")),
            ),
            false,
            0,
            0,
            &["/Volumes/Backup: Read-only file system", "x"],
            120,
        ),
    ];
    state.borrow().0.debug_write(PaneId::Left, b"git sta");
    pump(&mut h, &state, Duration::from_millis(300), |_| false);
    for ev in events {
        state.borrow_mut().0.on_event(&PluginEvent::FileAction(ev));
    }
    assert!(
        pump(&mut h, &state, Duration::from_secs(5), |p| p
            .debug_screen(PaneId::Left)
            .is_some_and(|s| s.contains(failed) && s.contains("git sta"))),
        "no echo:\n{}",
        screen(&state)
    );
    click_center(&mut h);
    save(&mut h, &format!("actions-{tag}"));

    // While a program runs, the lines wait (pill), then print.
    state.borrow().0.debug_write(PaneId::Left, b"\x15sleep 2\r");
    pump(&mut h, &state, Duration::from_secs(2), |p| {
        !p.debug_is_idle(PaneId::Left)
    });
    let a = action(
        4,
        FileActionKind::NewFolder,
        vec![r.join("New Folder")],
        None,
    );
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::FileAction(a.clone()));
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::FileAction(finished(a, true, 0, 0, &[], 4)));
    pump(&mut h, &state, Duration::from_millis(300), |_| false);
    save(&mut h, &format!("actions-busy-{tag}"));
    assert!(pump(&mut h, &state, Duration::from_secs(5), |p| p
        .debug_screen(PaneId::Left)
        .is_some_and(|s| s.contains("mkdir 'New Folder'"))));
    if german {
        // The remote banner and the note after the shell exits.
        pump(&mut h, &state, Duration::from_millis(300), |p| {
            p.debug_is_idle(PaneId::Left)
        });
        let remote = pane_state(None);
        state.borrow_mut().1 = remote.clone();
        state
            .borrow_mut()
            .0
            .on_event(&PluginEvent::DirectoryChanged(remote));
        pump(&mut h, &state, Duration::from_millis(200), |_| false);
        save(&mut h, &format!("remote-{tag}"));
        let local = pane_state(Some(&fx.root));
        state.borrow_mut().1 = local.clone();
        state
            .borrow_mut()
            .0
            .on_event(&PluginEvent::DirectoryChanged(local));
        pump(&mut h, &state, Duration::from_millis(300), |_| false);
        state.borrow().0.debug_write(PaneId::Left, b"exit\r");
        assert!(
            pump(&mut h, &state, Duration::from_secs(5), |p| p
                .debug_screen(PaneId::Left)
                .is_some_and(|s| s.contains("Prozess beendet"))),
            "no exit note:\n{}",
            screen(&state)
        );
        save(&mut h, &format!("exited-{tag}"));
    }
    state.borrow_mut().0.disable();
}

/// The settings (Settings → Plugins → Terminal) in the current language.
fn render_settings(theme: egui::Theme, tag: &str) {
    let fx = fixture();
    let data = fx.data.clone();
    let plugin = Rc::new(RefCell::new((TerminalPlugin::new(), false)));
    let mut h = Harness::builder()
        .with_size(egui::vec2(560.0, 230.0))
        .with_pixels_per_point(2.0)
        .with_theme(theme)
        .with_context_setup(install_app_like_fonts)
        .wgpu()
        .build_ui(move |ui| {
            let mut st = plugin.borrow_mut();
            let (p, enabled) = &mut *st;
            if !*enabled {
                let (tx, rx) = std::sync::mpsc::channel();
                std::mem::forget(rx);
                p.enable(PluginHandle::new(
                    "manta.terminal".into(),
                    tx,
                    ui.ctx().clone(),
                    data.clone(),
                ));
                *enabled = true;
            }
            egui::Frame::new()
                .inner_margin(16.0)
                .fill(ui.visuals().panel_fill)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    p.settings_ui(ui);
                });
        });
    h.run_steps(3);
    save(&mut h, &format!("settings-{tag}"));
}

/// German renders (`terminal` panel with echoed actions, waiting pill,
/// remote banner, exit note, settings), light and dark.
#[test]
fn render_german_light_and_dark() {
    if !Path::new("/bin/zsh").exists()
        || !Path::new(SF_PRO).exists()
        || !Path::new(SF_MONO).exists()
        || !gpu_ok()
    {
        eprintln!("skipping: needs zsh, SF fonts and a wgpu adapter");
        return;
    }
    use manta_plugin::i18n::{set_thread_lang, Lang};
    set_thread_lang(Some(Lang::De));
    for (theme, tag) in [
        (egui::Theme::Light, "de-light"),
        (egui::Theme::Dark, "de-dark"),
    ] {
        run_actions_in(theme, tag, true);
        render_settings(theme, tag);
    }
    set_thread_lang(None);
}

/// A shell without integration (sh): the action shows as an overlay.
fn run_fallback(theme: egui::Theme, tag: &str) {
    let fx = fixture();
    let state: Shared = Rc::new(RefCell::new((
        TerminalPlugin::new(),
        pane_state(Some(&fx.root)),
        false,
    )));
    let mut h = harness_shell(theme, state.clone(), fx.data.clone(), "/bin/sh");
    // sh announces nothing; wait out the integration grace period.
    pump(&mut h, &state, Duration::from_millis(4500), |_| false);
    state.borrow().0.debug_write(PaneId::Left, b"clear; ls\r");
    pump(&mut h, &state, Duration::from_millis(500), |_| false);
    let r = &fx.root;
    let a = action(
        1,
        FileActionKind::Copy,
        vec![r.join("README.md"), r.join("Cargo.toml")],
        Some(r.join("docs")),
    );
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::FileAction(a.clone()));
    state
        .borrow_mut()
        .0
        .on_event(&PluginEvent::FileAction(finished(
            a,
            true,
            2,
            4_096,
            &[],
            40,
        )));
    pump(&mut h, &state, Duration::from_millis(400), |_| false);
    assert!(
        !screen(&state).contains("cp -R"),
        "never typed into a shell without integration"
    );
    save(&mut h, &format!("actions-fallback-{tag}"));
    state.borrow_mut().0.disable();
}

#[test]
fn render_file_actions_light_and_dark() {
    if !Path::new("/bin/zsh").exists()
        || !Path::new(SF_PRO).exists()
        || !Path::new(SF_MONO).exists()
        || !gpu_ok()
    {
        eprintln!("skipping: needs zsh, SF fonts and a wgpu adapter");
        return;
    }
    run_actions(egui::Theme::Dark, "dark");
    run_actions(egui::Theme::Light, "light");
    if Path::new("/bin/sh").exists() {
        run_fallback(egui::Theme::Dark, "dark");
        run_fallback(egui::Theme::Light, "light");
    }
}

/// Click into the middle of the window to focus the terminal.
fn click_center(h: &mut Harness<'_>) {
    let center = egui::pos2(380.0, 200.0);
    h.event(egui::Event::PointerMoved(center));
    let button = |pressed| egui::Event::PointerButton {
        pos: center,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.event(button(true));
    h.step();
    h.event(button(false));
    h.step();
    h.step();
}

const SF_PRO: &str = "/System/Library/Fonts/SFNS.ttf";
const SF_MONO: &str = "/System/Library/Fonts/SFNSMono.ttf";

/// Same faces as the app's theme on macOS: SF Pro text, SF Mono code.
fn install_app_like_fonts(ctx: &egui::Context) {
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
    // Monospace falls back to SF Pro for symbols, proportional to SF Mono.
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

fn gpu_ok() -> bool {
    std::panic::catch_unwind(|| {
        let mut h = Harness::builder()
            .with_size(egui::vec2(10.0, 10.0))
            .with_context_setup(install_app_like_fonts)
            .wgpu()
            .build_ui(|ui| {
                ui.label("x");
            });
        h.render().is_ok()
    })
    .unwrap_or(false)
}

#[test]
fn render_terminal_light_and_dark() {
    if !Path::new("/bin/zsh").exists() {
        eprintln!("skipping: no zsh");
        return;
    }
    if !Path::new(SF_PRO).exists() || !Path::new(SF_MONO).exists() {
        eprintln!("skipping: SF fonts not installed");
        return;
    }
    if !gpu_ok() {
        eprintln!("skipping: no wgpu adapter");
        return;
    }
    run_theme(egui::Theme::Dark, "dark");
    run_theme(egui::Theme::Light, "light");
}

/// 200×60 full of styled text must paint fast (measured without GPU).
#[test]
fn paint_200x60_is_fast() {
    use manta_plugin_terminal::session::{detached_shared, GridSize};
    use manta_plugin_terminal::view::{self, PaintOptions, ViewState};
    if !Path::new(SF_MONO).exists() {
        return;
    }
    let mut shared = detached_shared(
        GridSize {
            cols: 200,
            rows: 60,
        },
        5000,
    );
    let mut text = String::new();
    for row in 0..60 {
        for col in 0..20 {
            text.push_str(&format!(
                "\x1b[38;5;{}m{}word{:02} ",
                (row * 20 + col) % 256,
                if col % 3 == 0 { "\x1b[1m" } else { "\x1b[22m" },
                col
            ));
        }
        text.push_str("\x1b[0m\r\n");
    }
    shared.feed(text.as_bytes());
    let shared = std::cell::RefCell::new(shared);
    let mut vs = ViewState::default();
    let times = std::cell::RefCell::new(Vec::new());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1700.0, 1000.0))
        .with_context_setup(install_app_like_fonts)
        .build_ui(|ui| {
            let font = egui::FontId::monospace(12.0);
            let m = vs.metrics(ui.ctx(), &font);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            let t = Instant::now();
            let sh = shared.borrow();
            view::paint(
                ui,
                &painter,
                rect,
                &sh.term,
                &sh.floats,
                &mut vs,
                &m,
                &PaintOptions {
                    theme: manta_plugin_terminal::palette::Theme::dark(),
                    font,
                    bold_font: None,
                    focused: true,
                    blink_on: true,
                    dimmed: false,
                },
            );
            times.borrow_mut().push(t.elapsed());
        });
    for _ in 0..20 {
        h.step();
    }
    let t = times.borrow();
    // The median of the steady frames: on a loaded machine single frames
    // are preempted (100 ms and more), which says nothing about the paint.
    let mut steady: Vec<_> = t[t.len() / 2..].to_vec();
    steady.sort();
    let median = steady[steady.len() / 2];
    eprintln!(
        "paint 200x60: first {:?}, steady median {:?}, all {:?}",
        t[0], median, *t
    );
    // Generous bound for unoptimised debug builds.
    assert!(median.as_millis() < 60, "too slow: {median:?}");
}

/// Letter spacing really puts glyph k of a run at x = k · cell_w (what
/// `Metrics::fit` relies on to keep runs on the stretched grid).
#[test]
fn letter_spacing_keeps_glyphs_on_the_grid() {
    if !Path::new(SF_MONO).exists() {
        return;
    }
    let ppp = 2.0;
    let mut h = Harness::builder()
        .with_size(egui::vec2(200.0, 100.0))
        .with_pixels_per_point(ppp)
        .with_context_setup(install_app_like_fonts)
        .build_ui(|_| {});
    h.step();
    let font = egui::FontId::monospace(12.0);
    let advance = h.ctx.fonts_mut(|f| f.glyph_width(&font, 'M'));
    let spacing = advance / 97.0 * 0.83;
    let cell_w = advance + spacing;
    let text = "ok | 17:18:50 /Users/anna/Projects ".repeat(3);
    let mut job = egui::text::LayoutJob::single_section(
        text.clone(),
        egui::TextFormat {
            font_id: font,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    let galley = h.ctx.fonts_mut(|f| f.layout_job(job));
    let glyphs = &galley.rows[0].row.glyphs;
    assert_eq!(glyphs.len(), text.chars().count());
    let devs: Vec<f32> = glyphs
        .iter()
        .enumerate()
        .map(|(k, g)| g.pos.x - k as f32 * cell_w)
        .collect();
    // egui snaps each glyph to the pixel grid: at most one physical pixel
    // off, and the error never accumulates along the run.
    for (k, d) in devs.iter().enumerate() {
        assert!(d.abs() <= 1.0 / ppp + 0.01, "glyph {k}: off by {d}");
    }
}
