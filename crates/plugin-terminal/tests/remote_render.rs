//! Renders the terminal inside a Docker container (German UI, light and
//! dark): following `/usr/share/nginx/html` in an nginx container, a stopped
//! container, and a place without a shell (S3).
//!
//! Writes `terminal-remote-*.png` to `$MANTA_RENDER_DIR/phases/terminal-remote`
//! (default: the system temp dir). Only throwaway containers with the label
//! `manta-test` are used and removed afterwards. Skips without Docker,
//! the nginx/alpine images, SF fonts or a GPU.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use manta_core::{Protocol, VfsPath};
use manta_plugin::i18n::{set_thread_lang, Lang};
use manta_plugin::{PaneId, PaneState, Plugin, PluginEvent, PluginHandle, RemoteEndpoint};
use manta_plugin_terminal::remote::find_program;
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

struct Ctr {
    name: String,
    docker: PathBuf,
}

impl Ctr {
    fn start(tag: &str, args: &[&str]) -> Option<Ctr> {
        if !containers_opted_in() {
            return None;
        }
        let docker = find_program("docker")?;
        let name = format!("manta-test-{tag}-{}", std::process::id());
        let mut a = vec![
            "run",
            "-d",
            "--label",
            "manta-test",
            "--name",
            name.as_str(),
        ];
        a.extend_from_slice(args);
        let out = Command::new(&docker).args(&a).output().ok()?;
        if !out.status.success() {
            eprintln!("skipping: {}", String::from_utf8_lossy(&out.stderr));
            return None;
        }
        Some(Ctr { name, docker })
    }
}

impl Drop for Ctr {
    fn drop(&mut self) {
        let _ = Command::new(&self.docker)
            .args(["rm", "-f", &self.name])
            .output();
    }
}

fn ep(protocol: Protocol, host: &str) -> RemoteEndpoint {
    RemoteEndpoint {
        protocol,
        user: String::new(),
        host: host.into(),
        port: 0,
        context: String::new(),
    }
}

fn pane(remote: RemoteEndpoint, path: &str, label: String) -> PaneState {
    PaneState {
        pane: PaneId::Left,
        path: VfsPath::new(path),
        local_path: None,
        protocol: remote.protocol,
        backend_label: label,
        selection: vec![],
        active: true,
        remote: Some(remote),
    }
}

type Shared = Rc<RefCell<(TerminalPlugin, PaneState)>>;

fn pump(
    h: &mut Harness<'_>,
    st: &Shared,
    limit: Duration,
    cond: impl Fn(&TerminalPlugin) -> bool,
) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        h.step();
        if cond(&st.borrow().0) {
            for _ in 0..4 {
                std::thread::sleep(Duration::from_millis(30));
                h.step();
            }
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn set_pane(st: &Shared, p: PaneState) {
    st.borrow_mut().1 = p.clone();
    st.borrow_mut()
        .0
        .on_event(&PluginEvent::DirectoryChanged(p));
}

fn out_dir() -> PathBuf {
    let base = std::env::var_os("MANTA_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("phases").join("terminal-remote");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(theme: egui::Theme, tag: &str, web: &Ctr, stopped: &Ctr) {
    set_thread_lang(Some(Lang::De));
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().to_path_buf();
    let html = "/usr/share/nginx/html";
    let st: Shared = Rc::new(RefCell::new((
        TerminalPlugin::new(),
        pane(
            ep(Protocol::Docker, &web.name),
            html,
            format!("{} (Docker)", web.name),
        ),
    )));
    let s2 = st.clone();
    let mut enabled = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 320.0))
        .with_pixels_per_point(2.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(install_fonts)
        .wgpu()
        .build_ui(move |ui| {
            let mut g = s2.borrow_mut();
            let (plugin, pane) = &mut *g;
            if !enabled {
                let (tx, rx) = std::sync::mpsc::channel();
                std::mem::forget(rx);
                plugin.enable(PluginHandle::new(
                    "manta.terminal".into(),
                    tx,
                    ui.ctx().clone(),
                    data.clone(),
                ));
                enabled = true;
            }
            let pane = pane.clone();
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    plugin.panel_ui("terminal", &pane, ui);
                });
        });
    let left = PaneId::Left;
    let save = |h: &mut Harness<'_>, name: &str| {
        let img = h.render().expect("render");
        let p = out_dir().join(format!("terminal-remote-{name}-{tag}.png"));
        img.save(&p).unwrap();
        eprintln!("wrote {}", p.display());
    };

    // Starts at / then follows the pane into the html folder.
    assert!(pump(&mut h, &st, Duration::from_secs(20), |p| p
        .debug_shell_cwd(left)
        .as_deref()
        == Some(html)
        && p.debug_is_idle(left)));
    st.borrow().0.debug_write(left, b"ls -la\r");
    pump(&mut h, &st, Duration::from_secs(5), |p| {
        p.debug_screen(left)
            .is_some_and(|s| s.contains("index.html"))
            && p.debug_is_idle(left)
    });
    set_pane(
        &st,
        pane(
            ep(Protocol::Docker, &web.name),
            "/etc/nginx",
            format!("{} (Docker)", web.name),
        ),
    );
    pump(&mut h, &st, Duration::from_secs(5), |p| {
        p.debug_shell_cwd(left).as_deref() == Some("/etc/nginx") && p.debug_is_idle(left)
    });
    set_pane(
        &st,
        pane(
            ep(Protocol::Docker, &web.name),
            html,
            format!("{} (Docker)", web.name),
        ),
    );
    pump(&mut h, &st, Duration::from_secs(5), |p| {
        p.debug_shell_cwd(left).as_deref() == Some(html) && p.debug_is_idle(left)
    });
    save(&mut h, "container");

    // A place without a shell: the last session stays, dimmed, with a note.
    set_pane(
        &st,
        pane(
            ep(Protocol::S3, "s3.eu-central-1.amazonaws.com"),
            "/bucket",
            "S3".into(),
        ),
    );
    pump(&mut h, &st, Duration::from_millis(300), |_| false);
    save(&mut h, "noshell");

    // A stopped container: a calm message instead of a shell.
    set_pane(
        &st,
        pane(
            ep(Protocol::Docker, &stopped.name),
            "/",
            format!("{} (Docker)", stopped.name),
        ),
    );
    assert!(pump(&mut h, &st, Duration::from_secs(10), |p| p
        .debug_probe(left)
        .as_deref()
        == Some("Stopped")));
    save(&mut h, "stopped");
    st.borrow_mut().0.disable();
    set_thread_lang(None);
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
fn render_terminal_in_containers() {
    if !Path::new(SF_PRO).exists() || !Path::new(SF_MONO).exists() || !gpu_ok() {
        eprintln!("skipping: needs SF fonts and a GPU");
        return;
    }
    let Some(web) = Ctr::start("web", &["--rm", "nginx:alpine"]) else {
        return;
    };
    let Some(stopped) = Ctr::start("stopped", &["alpine:3", "sleep", "300"]) else {
        return;
    };
    let _ = Command::new(&stopped.docker)
        .args(["stop", "-t", "1", &stopped.name])
        .output();
    run(egui::Theme::Dark, "dark", &web, &stopped);
    run(egui::Theme::Light, "light", &web, &stopped);
}

/// Real `docker` / `kubectl` only with `MANTA_TEST_CONTAINERS=1`.
fn containers_opted_in() -> bool {
    let ok = std::env::var_os("MANTA_TEST_CONTAINERS").is_some_and(|v| v == "1");
    if !ok {
        eprintln!("skipped: set MANTA_TEST_CONTAINERS=1 to run tests against docker/kubectl");
    }
    ok
}
