//! Containers in the app: the sidebar section (mock discovery data), panes
//! inside a running and a stopped Docker container (real `ContainerFs`
//! over a scripted runner — no docker involved), the shell command, and
//! the connect dialog's Container tab, light and dark.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::Theme;
use egui_kittest::kittest::Queryable;
use manta_core::{Protocol, SharedVfs, VfsPath};
use manta_vfs::container::discover::{DockerContainer, KubePod};
use manta_vfs::container::listing::LIST_SCRIPT;
use manta_vfs::container::{docker_config, kube_config, CmdOutput, ContainerFs, Proc, Runner};

use super::{app, harness, harness_with, save, select, settle, H};
use crate::app::{Action, Side};
use crate::commands::Cmd;
use crate::containers::{Containers, Discovered};
use crate::dialogs::{ConnectForm, Dialog, Found};
use crate::worker::ConnectRequest;

const TALL: [f32; 2] = [1280.0, 880.0];
const T: i64 = 1_792_400_000;

/// `(name, kind d/f/o, link, size, age in hours, mode)` → output of the
/// listing script.
pub(super) type Item = (&'static str, char, Option<&'static str>, u64, i64, u32);

pub(super) fn listing(items: &[Item]) -> Vec<u8> {
    let mut v = Vec::new();
    for (name, kind, link, ..) in items {
        v.extend_from_slice(format!("./{name}\0{kind}").as_bytes());
        if let Some(l) = link {
            v.extend_from_slice(format!("L{l}").as_bytes());
        }
        v.push(0);
    }
    v.push(0);
    for (_, kind, _, size, hours, mode) in items {
        let ty = if *kind == 'd' { 0o040000 } else { 0o100000 };
        v.extend_from_slice(format!("{size} {} {:x}\n", T - hours * 3600, ty | mode).as_bytes());
    }
    v
}

fn tar_dir(names: &[(&str, bool, u64)]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Directory);
    h.set_size(0);
    h.set_mode(0o755);
    h.set_mtime(T as u64);
    b.append_data(&mut h, "./", io::empty()).unwrap();
    for (name, dir, size) in names {
        let mut h = tar::Header::new_gnu();
        h.set_mtime((T - 86_400 * 9) as u64);
        if *dir {
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o755);
            b.append_data(&mut h, format!("{name}/"), io::empty())
                .unwrap();
        } else {
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(*size);
            h.set_mode(0o644);
            b.append_data(&mut h, name, io::repeat(b'x').take_bytes(*size))
                .unwrap();
        }
    }
    b.into_inner().unwrap()
}

trait TakeBytes: io::Read + Sized {
    fn take_bytes(self, n: u64) -> io::Take<Self> {
        self.take(n)
    }
}
impl<R: io::Read> TakeBytes for R {}

/// Answers like docker would for `shop-web` (running, WORKDIR
/// /usr/share/nginx/html) and `blog_database` (stopped).
struct FakeDocker;

impl FakeDocker {
    fn answer(args: &[String]) -> CmdOutput {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["inspect", .., "shop-web"] => {
                CmdOutput::ok("true\t/usr/share/nginx/html\t/shop-web\n")
            }
            ["inspect", .., "blog_database"] => CmdOutput::ok("false\t\t/blog_database\n"),
            ["exec", _, "sh", "-c", "echo manta-ok"] => CmdOutput::ok("manta-ok\n"),
            ["exec", "shop-web", "sh", "-c", script, "sh", "L", "/usr/share/nginx/html"]
                if *script == LIST_SCRIPT =>
            {
                CmdOutput::ok(listing(&[
                    ("assets", 'd', None, 0, 30, 0o755),
                    ("releases", 'd', None, 0, 52, 0o755),
                    ("50x.html", 'f', None, 497, 2400, 0o644),
                    ("index.html", 'f', None, 4_812, 5, 0o644),
                    ("config.js", 'f', None, 18_204, 5, 0o644),
                    ("app.min.js", 'f', None, 412_877, 26, 0o644),
                    ("app.css", 'f', None, 38_116, 26, 0o644),
                    ("current", 'd', Some("releases/v2.4.1"), 0, 26, 0o755),
                    ("robots.txt", 'f', None, 67, 2400, 0o644),
                    (".well-known", 'd', None, 0, 2400, 0o755),
                ]))
            }
            ["cp", "blog_database:/", "-"] => CmdOutput::ok(tar_dir(&[
                ("bin", true, 0),
                ("docker-entrypoint-initdb.d", true, 0),
                ("etc", true, 0),
                ("var", true, 0),
                ("usr", true, 0),
                ("tmp", true, 0),
                ("entrypoint.sh", false, 7_142),
            ])),
            _ => CmdOutput::fail("Error: unexpected"),
        }
    }
}

impl Runner for FakeDocker {
    fn run(
        &self,
        _: &str,
        args: &[String],
        _: Option<&[u8]>,
        _: Duration,
    ) -> io::Result<CmdOutput> {
        Ok(Self::answer(args))
    }
    fn spawn(&self, _: &str, args: &[String]) -> io::Result<Box<dyn Proc>> {
        Ok(Box::new(FakeProc(Some(Self::answer(args)))))
    }
}

pub(super) struct FakeProc(pub(super) Option<CmdOutput>);

impl Proc for FakeProc {
    fn take_stdin(&mut self) -> Option<Box<dyn io::Write + Send>> {
        Some(Box::new(io::sink()))
    }
    fn take_stdout(&mut self) -> Option<Box<dyn io::Read + Send>> {
        Some(Box::new(io::Cursor::new(
            self.0
                .as_ref()
                .map(|o| o.stdout.clone())
                .unwrap_or_default(),
        )))
    }
    fn wait(&mut self) -> io::Result<(bool, String)> {
        let o = self.0.take().unwrap_or_default();
        Ok((o.success, o.stderr))
    }
    fn kill(&mut self) {}
}

/// Connect `side` to a container through the fake runner, as a finished
/// connect would.
pub(super) fn connect(h: &mut H, side: Side, name: &str) {
    let cfg = docker_config(name);
    let fs: SharedVfs =
        Arc::new(ContainerFs::connect_with(&cfg, Arc::new(FakeDocker)).expect("connect"));
    let home = fs.home().unwrap();
    let a = app(h);
    let action = a.actions.next_id();
    a.on_connected(
        ConnectRequest {
            config: cfg,
            favorite: None,
            side,
            action,
            keychain: false,
        },
        Ok((fs, home)),
    );
    settle(h);
}

fn container(id: &str, name: &str, image: &str, running: bool, status: &str) -> DockerContainer {
    DockerContainer {
        id: id.into(),
        name: name.into(),
        image: image.into(),
        state: if running { "running" } else { "exited" }.into(),
        status: status.into(),
        project: None,
        service: None,
    }
}

/// `container` in Compose project `project` (service = name without the
/// project prefix).
fn in_project(project: &str, mut c: DockerContainer) -> DockerContainer {
    let svc = c
        .name
        .trim_start_matches(project)
        .trim_matches(|ch| ch == '-' || ch == '_');
    c.service = Some(svc.trim_end_matches("-1").to_string());
    c.project = Some(project.to_string());
    c
}

pub(super) fn discovered() -> Discovered {
    Discovered {
        docker_cli: true,
        docker: Some(Ok(vec![
            in_project(
                "shop",
                container(
                    "a1",
                    "shop-web",
                    "nginx:alpine",
                    true,
                    "Up 37 hours (healthy)",
                ),
            ),
            in_project(
                "shop",
                container("a2", "shop-db", "mysql:8.4", true, "Up 37 hours (healthy)"),
            ),
            in_project(
                "api",
                container("a3", "api-server-1", "node:22-alpine", true, "Up 8 hours"),
            ),
            in_project(
                "api",
                container(
                    "a4",
                    "api-db-1",
                    "postgres:17-alpine",
                    true,
                    "Up 8 hours (healthy)",
                ),
            ),
            in_project(
                "api",
                container(
                    "a7",
                    "api-proxy-1",
                    "caddy:2-alpine",
                    false,
                    "Exited (0) 1 hour ago",
                ),
            ),
            in_project(
                "blog",
                container(
                    "a5",
                    "blog_database",
                    "mysql:8.0",
                    false,
                    "Exited (0) 2 days ago",
                ),
            ),
            container(
                "a6",
                "mailpit",
                "axllent/mailpit:latest",
                false,
                "Exited (137) 5 weeks ago",
            ),
            container("a8", "redis-scratch", "redis:7", true, "Up 2 hours"),
        ])),
        kube_context: Some("kind-dev".into()),
        namespaces: vec!["default".into(), "kube-system".into(), "shop".into()],
        namespace: "shop".into(),
        pods: Some(Ok(vec![
            KubePod {
                name: "web-7d9f8c6b5-x2k4p".into(),
                phase: "Running".into(),
                containers: vec!["app".into(), "nginx".into()],
            },
            KubePod {
                name: "redis-0".into(),
                phase: "Running".into(),
                containers: vec!["redis".into()],
            },
            KubePod {
                name: "migrate-29351".into(),
                phase: "Pending".into(),
                containers: vec!["migrate".into()],
            },
        ])),
    }
}

pub(super) fn with_containers(h: &mut H) {
    let mut c = Containers::with_data(discovered());
    c.show_stopped = true;
    c.expanded.insert("web-7d9f8c6b5-x2k4p".into());
    app(h).containers = c;
    h.run_steps(2);
}

#[test]
fn container_panes_shell_and_reuse() {
    let fx = super::fixture();
    let mut h = harness_with(&fx, TALL, Theme::Light, false);
    with_containers(&mut h);
    connect(&mut h, Side::Left, "shop-web");
    connect(&mut h, Side::Right, "blog_database");
    let a = app(&mut h);

    // Running container: remote-like, WORKDIR, a shell inside it.
    let tab = a.pane(Side::Left).tab();
    assert_eq!(tab.loc.fs.protocol(), Protocol::Docker);
    assert_eq!(tab.loc.path, VfsPath::new("/usr/share/nginx/html"));
    assert_eq!(
        tab.loc.fs.local_path(&tab.loc.path),
        None,
        "plugins see no local path"
    );
    assert!(tab
        .entries
        .iter()
        .any(|e| e.name == "current" && e.is_dir() && e.link_target.is_some()));
    let ep = a
        .endpoint(&tab.loc.fs)
        .expect("endpoint for the terminal echo");
    assert_eq!(
        (ep.protocol, ep.host.as_str()),
        (Protocol::Docker, "shop-web")
    );
    assert_eq!(
        a.container_shell_command(Side::Left).as_deref(),
        Some("docker exec -it shop-web sh -c 'cd /usr/share/nginx/html && exec ${SHELL:-sh}'")
    );
    a.active = Side::Left;
    assert!(a.cmd_enabled(Cmd::OpenShellInContainer));

    // Stopped container: read-only, listed from the tar, no shell.
    let tab = a.pane(Side::Right).tab();
    assert!(tab.loc.read_only());
    assert!(tab
        .entries
        .iter()
        .any(|e| e.name == "entrypoint.sh" && e.size == 7_142));
    assert_eq!(a.container_shell_command(Side::Right), None);
    a.active = Side::Right;
    assert!(!a.cmd_enabled(Cmd::OpenShellInContainer));

    // Clicking the running container again reuses the connection.
    let before = a.connections.len();
    a.active = Side::Left;
    let ctx = egui_ctx(&h);
    let a = app(&mut h);
    a.handle_action(
        &ctx,
        Action::OpenContainer(Box::new(docker_config("shop-web"))),
    );
    assert_eq!(app(&mut h).connections.len(), before);
}

fn egui_ctx(h: &H) -> eframe::egui::Context {
    h.ctx.clone()
}

#[test]
fn container_tab_builds_configs() {
    let mut f = ConnectForm::new(Side::Left);
    f.ctr.set_found(Found::default());
    f.set_protocol_for_tests(Protocol::Docker);
    assert!(f.to_config().unwrap_err().contains("Choose a container"));
    f.ctr.container = "shop-db".into();
    f.initial_path = "/var/lib/mysql".into();
    let cfg = f.to_config().unwrap();
    assert_eq!(cfg, {
        let mut c = docker_config("shop-db");
        c.initial_path = Some("/var/lib/mysql".into());
        c
    });
    // A favorite opens the same tab again.
    let back = ConnectForm::from_config(&cfg, Side::Left, Some(0));
    assert_eq!(
        (back.protocol, back.ctr.container.as_str()),
        (Protocol::Docker, "shop-db")
    );

    f.set_protocol_for_tests(Protocol::Kubernetes);
    f.protocol = Protocol::Kubernetes;
    f.initial_path.clear();
    assert!(f.to_config().unwrap_err().contains("Choose a pod"));
    f.ctr.context = "kind-dev".into();
    f.ctr.namespace = "shop".into();
    f.ctr.pod = "redis-0".into();
    f.ctr.pod_container = "redis".into();
    assert_eq!(
        f.to_config().unwrap(),
        kube_config("kind-dev", "shop", "redis-0", "redis")
    );
    // Leaving the tab and coming back keeps Kubernetes.
    let k = ConnectForm::from_config(
        &kube_config("kind-dev", "shop", "redis-0", "redis"),
        Side::Left,
        None,
    );
    assert_eq!(
        (k.protocol, k.ctr.pod.as_str(), k.ctr.namespace.as_str()),
        (Protocol::Kubernetes, "redis-0", "shop")
    );
}

#[test]
fn containers_group_by_compose_project() {
    let d = discovered();
    let list = d.docker.unwrap().unwrap();
    let (groups, loose) = crate::containers::group_containers(&list);
    let names: Vec<_> = groups.iter().map(|g| g.name).collect();
    // Groups with something running first, then by name.
    assert_eq!(names, ["api", "shop", "blog"]);
    let api: Vec<_> = groups[0]
        .containers
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        api,
        ["api-db-1", "api-server-1", "api-proxy-1"],
        "running first"
    );
    assert_eq!((groups[0].running(), groups[2].running()), (2, 0));
    let loose: Vec<_> = loose.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(loose, ["redis-scratch", "mailpit"]);
}

#[test]
fn render_containers() {
    if !super::wgpu_available() {
        return;
    }
    let fx = super::fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, TALL, theme);
        with_containers(&mut h);
        connect(&mut h, Side::Right, "blog_database");
        connect(&mut h, Side::Left, "shop-web");
        select(&mut h, Side::Left, &["index.html"]);
        save(&mut h, &format!("containers_sidebar_panes_{tname}"));

        // Connect dialog, Container tab: Docker picker.
        let mut form = ConnectForm::new(Side::Left);
        form.set_protocol_for_tests(Protocol::Docker);
        let d = discovered();
        form.ctr.set_found(Found {
            docker: d.docker.clone(),
            ..Default::default()
        });
        form.ctr.container = "shop-db".into();
        form.save_favorite = true;
        form.name = "Shop DB".into();
        app(&mut h).dialog = Some(Dialog::Connect(form));
        save(&mut h, &format!("containers_connect_docker_{tname}"));

        // Kubernetes.
        let mut form = ConnectForm::new(Side::Left);
        form.set_protocol_for_tests(Protocol::Kubernetes);
        form.ctr.set_found(Found {
            contexts: vec!["kind-dev".into(), "docker-desktop".into()],
            current_context: Some("kind-dev".into()),
            namespaces: d.namespaces.clone(),
            pods: d.pods.clone(),
            pods_for: ("kind-dev".into(), "shop".into()),
            ..Default::default()
        });
        form.ctr.namespace = "shop".into();
        form.ctr.pod = "web-7d9f8c6b5-x2k4p".into();
        form.ctr.pod_container = "app".into();
        form.test_result = Some(Ok("/srv/app".into()));
        app(&mut h).dialog = Some(Dialog::Connect(form));
        save(&mut h, &format!("containers_connect_k8s_{tname}"));
        app(&mut h).dialog = None;
    }
}

/// "Back to Local" in a container pane goes back to the local folder the
/// tab came from (and to the home folder without one).
#[test]
fn local_chip_returns_from_a_container() {
    let fx = super::fixture();
    let mut h = harness_with(&fx, TALL, Theme::Light, false);
    let before = app(&mut h).pane(Side::Left).tab().loc.path.clone();
    connect(&mut h, Side::Left, "shop-web");
    assert!(
        app(&mut h).pane(Side::Left).tab().loc.conn.is_some(),
        "inside the container"
    );
    app(&mut h).handle_action(
        &eframe::egui::Context::default(),
        crate::app::Action::GoLocal(Side::Left),
    );
    settle(&mut h);
    let t = app(&mut h).pane(Side::Left).tab();
    assert!(
        t.loc.conn.is_none() && t.loc.archive.is_none(),
        "back on the Mac"
    );
    assert_eq!(t.loc.path, before, "the folder the tab was in before");
}

/// The context menu's Start / Restart / Stop: `docker start` etc. run in
/// the background (synthetic runner), the row says "Starting…" meanwhile,
/// and a second click on the same container is ignored until it is done.
#[test]
fn container_start_restart_stop() {
    use manta_vfs::container::discover::DockerOp;
    let fx = super::fixture();
    let mut h = harness(&fx, TALL, Theme::Light);
    with_containers(&mut h);
    let ctx = egui_ctx(&h);
    let control = |id: &str, name: &str, op| Action::DockerControl {
        id: id.into(),
        name: name.into(),
        op,
    };
    for (id, name, op, done) in [
        ("a6", "mailpit", DockerOp::Start, "Started “mailpit”"),
        (
            "a8",
            "redis-scratch",
            DockerOp::Restart,
            "Restarted “redis-scratch”",
        ),
        (
            "a8",
            "redis-scratch",
            DockerOp::Stop,
            "Stopped “redis-scratch”",
        ),
    ] {
        app(&mut h).handle_action(&ctx, control(id, name, op));
        assert_eq!(app(&mut h).containers.pending(id), Some(op));
        // Busy: a second command on the same container is not started.
        app(&mut h).handle_action(&ctx, control(id, name, DockerOp::Stop));
        assert_eq!(app(&mut h).containers.pending(id), Some(op));
        let start = std::time::Instant::now();
        while app(&mut h).containers.pending(id).is_some() {
            assert!(
                start.elapsed() < crate::test_support::wait(),
                "{op:?} timed out"
            );
            h.step();
            std::thread::sleep(Duration::from_millis(5));
        }
        let titles = h.state().toasts.titles();
        assert!(titles.iter().any(|t| t == done), "{titles:?}");
    }
}

/// Right-click a sidebar row by its label.
fn right_click_label(h: &mut H, label: &str) {
    let pos = h.get_by_label_contains(label).rect().center();
    h.hover_at(pos);
    for pressed in [true, false] {
        h.input_mut()
            .events
            .push(eframe::egui::Event::PointerButton {
                pos,
                button: eframe::egui::PointerButton::Secondary,
                pressed,
                modifiers: eframe::egui::Modifiers::NONE,
            });
        h.run_steps(1);
    }
    h.run_steps(3);
}

/// A menu item (its accessible name, without the icon) is shown.
fn menu_has(h: &H, item: &str) -> bool {
    h.query_by_label(item).is_some()
}

/// A running container offers Restart and Stop, a stopped one Start; the
/// menu item runs the command.
#[test]
fn container_context_menu_follows_the_state() {
    let fx = super::fixture();
    let mut h = harness(&fx, TALL, Theme::Light);
    with_containers(&mut h);

    right_click_label(&mut h, "redis-scratch");
    assert!(menu_has(&h, "Restart") && menu_has(&h, "Stop"));
    assert!(!menu_has(&h, "Start"));
    if super::wgpu_available() {
        save(&mut h, "containers_menu_running");
    }
    // A click elsewhere closes the menu (it covers the rows below).
    let away = eframe::egui::pos2(1000.0, 700.0);
    h.hover_at(away);
    for pressed in [true, false] {
        h.input_mut()
            .events
            .push(eframe::egui::Event::PointerButton {
                pos: away,
                button: eframe::egui::PointerButton::Primary,
                pressed,
                modifiers: eframe::egui::Modifiers::NONE,
            });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(!menu_has(&h, "Restart"), "menu closed");

    right_click_label(&mut h, "mailpit");
    assert!(menu_has(&h, "Start"));
    assert!(!menu_has(&h, "Restart") && !menu_has(&h, "Stop"));
    h.get_by_label("Start").click();
    h.run_steps(1);
    assert!(app(&mut h).containers.pending("a6").is_some());
    if super::wgpu_available() {
        save(&mut h, "containers_started");
    }
}

/// A container that stopped while open: its pane offers Start Container
/// (not Try Again); once `docker start` is done the folder is listed again.
#[test]
fn stopped_container_pane_offers_start() {
    let fx = super::fixture();
    let mut h = harness_with(&fx, TALL, Theme::Light, false);
    with_containers(&mut h);
    connect(&mut h, Side::Left, "shop-web");
    {
        let t = app(&mut h).pane_mut(Side::Left).tab_mut();
        t.error = Some(
            "connection error: container is not running: Error response from daemon: \
             container 3f2a is not running"
                .into(),
        );
    }
    h.run_steps(3);
    assert!(h.query_by_label("Try Again").is_none());
    h.get_by_label("Start Container").click();
    h.run_steps(1);
    let start = std::time::Instant::now();
    while app(&mut h).pane(Side::Left).tab().error.is_some()
        || app(&mut h).pane(Side::Left).tab().loading
    {
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "never listed again"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    let t = app(&mut h).pane(Side::Left).tab();
    assert!(!t.container_starting);
    assert!(t.entries.iter().any(|e| e.name == "current"));
    let titles = h.state().toasts.titles();
    assert!(
        titles.iter().any(|t| t == "Started “shop-web”"),
        "{titles:?}"
    );
}

/// A pod (or any other error) keeps Try Again.
#[test]
fn other_listing_errors_keep_try_again() {
    let fx = super::fixture();
    let mut h = harness_with(&fx, TALL, Theme::Light, false);
    app(&mut h).pane_mut(Side::Left).tab_mut().error =
        Some("connection error: container is not running".into());
    h.run_steps(3);
    assert!(
        h.query_by_label("Try Again").is_some(),
        "a local pane is not a container"
    );
    assert!(h.query_by_label("Start Container").is_none());
}
