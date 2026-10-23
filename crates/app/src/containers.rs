//! The sidebar's "Containers" section: Docker containers and Kubernetes
//! pods, discovered in the background (read-only `docker ps` / `kubectl
//! get`) every few seconds while the sidebar is visible.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use manta_core::{ConnectionConfig, Protocol};
use manta_vfs::container::discover::{self, DockerContainer, KubePod};
use manta_vfs::container::{docker_config, find_program, kube_config, Runner};

use crate::app::Action;
use crate::icons;
use crate::sidebar::{row, Row};
use crate::theme::{self, pal, space, text};
use crate::widgets;

/// How often the list is refreshed while visible.
pub const REFRESH: Duration = Duration::from_secs(5);

/// What discovery found.
#[derive(Debug, Clone, Default)]
pub struct Discovered {
    /// The docker CLI is installed.
    pub docker_cli: bool,
    /// Containers, or why they couldn't be listed (daemon not running).
    pub docker: Option<Result<Vec<DockerContainer>, String>>,
    /// The kubeconfig's current context.
    pub kube_context: Option<String>,
    pub namespaces: Vec<String>,
    /// Namespace the pods are from.
    pub namespace: String,
    pub pods: Option<Result<Vec<KubePod>, String>>,
}

impl Discovered {
    /// Show the section at all?
    pub fn any(&self) -> bool {
        self.docker_cli || self.kube_context.is_some()
    }
}

/// Discovery state and the section's UI state.
pub struct Containers {
    data: Arc<Mutex<Discovered>>,
    busy: Arc<AtomicBool>,
    last: Option<Instant>,
    /// Run discovery at all (off in tests and renders).
    pub live: bool,
    pub show_stopped: bool,
    /// Compose groups the user opened or closed (default: open while
    /// something in it runs).
    pub group_open: HashMap<String, bool>,
    /// Pods whose containers are listed.
    pub expanded: HashSet<String>,
    /// The namespace the user picked (else the context's default).
    namespace: Option<String>,
}

impl Default for Containers {
    fn default() -> Self {
        Containers {
            data: Arc::default(),
            busy: Arc::default(),
            last: None,
            live: true,
            show_stopped: false,
            group_open: HashMap::new(),
            expanded: HashSet::new(),
            namespace: None,
        }
    }
}

/// A `kubeconfig` exists (don't even start kubectl otherwise).
fn kubeconfig_exists() -> bool {
    match std::env::var_os("KUBECONFIG") {
        Some(v) if !v.is_empty() => std::env::split_paths(&v).any(|p| p.is_file()),
        _ => dirs::home_dir().is_some_and(|h| h.join(".kube/config").is_file()),
    }
}

impl Containers {
    /// Fixed data, no discovery (renders and tests).
    pub fn with_data(d: Discovered) -> Containers {
        Containers {
            data: Arc::new(Mutex::new(d)),
            live: false,
            ..Default::default()
        }
    }

    pub fn snapshot(&self) -> Discovered {
        self.data.lock().map(|d| d.clone()).unwrap_or_default()
    }

    /// Start a background refresh when one is due. Call every frame the
    /// sidebar is shown; never blocks.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if !self.live {
            return;
        }
        let due = self.last.is_none_or(|t| t.elapsed() >= REFRESH);
        if due && !self.busy.swap(true, Ordering::SeqCst) {
            self.last = Some(Instant::now());
            let (data, busy, ctx2) = (self.data.clone(), self.busy.clone(), ctx.clone());
            let ns = self.namespace.clone();
            let spawned = std::thread::Builder::new()
                .name("manta-containers".into())
                .spawn(move || {
                    let prev = data.lock().map(|d| d.clone()).unwrap_or_default();
                    let d = discover_all(&prev, ns);
                    if let Ok(mut slot) = data.lock() {
                        *slot = d;
                    }
                    busy.store(false, Ordering::SeqCst);
                    ctx2.request_repaint();
                });
            if spawned.is_err() {
                self.busy.store(false, Ordering::SeqCst);
            }
        }
        ctx.request_repaint_after(REFRESH);
    }

    /// Refresh now (header button, namespace change).
    pub fn refresh_now(&mut self) {
        self.last = None;
    }

    pub fn set_namespace(&mut self, ns: String) {
        if let Ok(mut d) = self.data.lock() {
            d.namespace = ns.clone();
            d.pods = None;
        }
        self.namespace = Some(ns);
        self.refresh_now();
    }
}

/// The runner container discovery (sidebar and connect dialog) uses, and
/// whether `docker` / `kubectl` (with a kubeconfig) are there.
///
/// Tests and renders get [`synthetic::Synthetic`] — made-up containers and
/// pods — so nothing can ever list or show the developer's own; only
/// `MANTA_TEST_CONTAINERS=1` lets them reach the real CLIs.
pub(crate) struct DiscoveryEnv {
    pub runner: Arc<dyn Runner>,
    pub docker: bool,
    pub kube: bool,
}

pub(crate) fn discovery_env() -> DiscoveryEnv {
    #[cfg(test)]
    if !manta_vfs::container::real_cli_opted_in() {
        return DiscoveryEnv {
            runner: Arc::new(synthetic::Synthetic),
            docker: true,
            kube: true,
        };
    }
    DiscoveryEnv {
        runner: manta_vfs::container::CliRunner::shared(),
        docker: find_program("docker").is_some(),
        kube: find_program("kubectl").is_some() && kubeconfig_exists(),
    }
}

/// One discovery round (blocking; runs on a background thread).
fn discover_all(prev: &Discovered, ns: Option<String>) -> Discovered {
    let env = discovery_env();
    let runner = env.runner;
    let mut d = Discovered {
        docker_cli: env.docker,
        ..Default::default()
    };
    if d.docker_cli {
        d.docker = Some(discover::docker_containers(&*runner).map_err(|e| e.to_string()));
    }
    if env.kube {
        if let Ok(Some(ctx)) = discover::kube_current_context(&*runner) {
            let same = prev.kube_context.as_deref() == Some(ctx.as_str());
            d.namespace = match ns {
                Some(n) => n,
                None if same && !prev.namespace.is_empty() => prev.namespace.clone(),
                None => discover::kube_default_namespace(&*runner, &ctx)
                    .unwrap_or_else(|_| "default".into()),
            };
            d.namespaces = if same && !prev.namespaces.is_empty() {
                prev.namespaces.clone()
            } else {
                discover::kube_namespaces(&*runner, &ctx).unwrap_or_default()
            };
            d.pods =
                Some(discover::kube_pods(&*runner, &ctx, &d.namespace).map_err(|e| e.to_string()));
            d.kube_context = Some(ctx);
        }
    }
    d
}

type Key = (String, u16, String, Protocol);

fn key_of(c: &ConnectionConfig) -> Key {
    (c.host.clone(), c.port, c.username.clone(), c.protocol)
}

/// A caption line inside the section (errors, empty states).
fn note(ui: &mut egui::Ui, msg: &str) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.add_space(space::SM + 2.0);
        ui.add(
            egui::Label::new(
                RichText::new(msg)
                    .font(theme::regular(text::CAPTION))
                    .color(p.text_tertiary),
            )
            .wrap(),
        );
    });
    ui.add_space(space::XXS);
}

/// A small sub-heading row ("Kubernetes · kind-dev").
fn subheading(ui: &mut egui::Ui, glyph: &str, title: &str, detail: &str) {
    let p = pal(ui);
    ui.add_space(space::SM);
    ui.horizontal(|ui| {
        ui.add_space(space::SM);
        ui.label(
            RichText::new(glyph)
                .font(theme::icon_font(12.0))
                .color(p.text_tertiary),
        );
        ui.label(
            RichText::new(title)
                .font(theme::medium(text::CAPTION))
                .color(p.text_secondary),
        );
        if !detail.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(detail)
                        .font(theme::regular(text::CAPTION))
                        .color(p.text_tertiary),
                )
                .truncate(),
            );
        }
    });
    ui.add_space(space::XXS);
}

/// A Docker Compose project and its containers, like Docker Desktop's
/// container groups.
#[derive(Debug)]
pub struct Group<'a> {
    pub name: &'a str,
    pub containers: Vec<&'a DockerContainer>,
}

impl Group<'_> {
    pub fn running(&self) -> usize {
        self.containers.iter().filter(|c| c.running()).count()
    }
}

/// Split containers into Compose groups and the ones without a project.
/// Groups with something running come first, then by name; inside a group
/// running containers come first, then by name.
pub fn group_containers(list: &[DockerContainer]) -> (Vec<Group<'_>>, Vec<&DockerContainer>) {
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut loose = Vec::new();
    for c in list {
        match c.project.as_deref() {
            Some(name) => match groups.iter_mut().find(|g| g.name == name) {
                Some(g) => g.containers.push(c),
                None => groups.push(Group {
                    name,
                    containers: vec![c],
                }),
            },
            None => loose.push(c),
        }
    }
    let order = |a: &&DockerContainer, b: &&DockerContainer| {
        b.running()
            .cmp(&a.running())
            .then_with(|| a.name.cmp(&b.name))
    };
    for g in &mut groups {
        g.containers.sort_by(order);
    }
    groups.sort_by(|a, b| {
        (b.running() > 0)
            .cmp(&(a.running() > 0))
            .then_with(|| a.name.cmp(b.name))
    });
    loose.sort_by(order);
    (groups, loose)
}

/// One container row (running: green dot; stopped: dimmed, opens read-only).
fn container_row(
    ui: &mut egui::Ui,
    c: &DockerContainer,
    indent: f32,
    is_active: &dyn Fn(&ConnectionConfig) -> bool,
    actions: &mut Vec<Action>,
) {
    let p = pal(ui);
    let cfg = docker_config(&c.name);
    let running = c.running();
    let (resp, _) = row(
        ui,
        Row {
            icon: icons::SHIPPING_CONTAINER,
            label: &c.name,
            detail: Some(short_image(&c.image)),
            selected: is_active(&cfg),
            status_dot: true,
            dot: (!running).then_some(p.text_tertiary),
            dimmed: !running,
            indent,
            ..Row::default()
        },
    );
    let mut tip = format!("{}\n{} · {}", c.name, c.image, c.status);
    if !running {
        tip.push_str(&format!("\n{}", t!("containers.opens_read_only")));
    }
    if resp.on_hover_text(tip).clicked() {
        actions.push(Action::OpenContainer(Box::new(cfg)));
    }
}

fn pod_dot(p: &theme::Palette, phase: &str) -> egui::Color32 {
    match phase {
        "Running" => p.success,
        "Pending" => p.warning,
        "Failed" | "Unknown" => p.danger,
        _ => p.text_tertiary,
    }
}

/// The section body (the caller draws the header). `active` is the
/// connection key of the active pane, to highlight its row.
pub fn section_ui(
    ui: &mut egui::Ui,
    st: &mut Containers,
    active: Option<&Key>,
    actions: &mut Vec<Action>,
) {
    let d = st.snapshot();
    let p = pal(ui);
    let is_active = |cfg: &ConnectionConfig| active == Some(&key_of(cfg));

    if d.docker_cli {
        match &d.docker {
            None => note(ui, t!("containers.looking")),
            Some(Err(e)) => {
                let msg = if e.contains("daemon not running") {
                    t!("containers.docker_not_running")
                } else {
                    e.as_str()
                };
                note(ui, msg);
            }
            Some(Ok(list)) => {
                if list.is_empty() {
                    note(ui, t!("containers.none"));
                }
                let (groups, loose) = group_containers(list);
                // Compose projects, collapsible like in Docker Desktop.
                for g in &groups {
                    let total = g.containers.len();
                    let up = g.running();
                    let open = st.group_open.get(g.name).copied().unwrap_or(up > 0);
                    let detail = match up {
                        0 => t!("containers.group.stopped").to_string(),
                        n if n == total => t!("containers.group.running", n = n),
                        n => t!("containers.group.running_of", n = n, total = total),
                    };
                    let (resp, _) = row(
                        ui,
                        Row {
                            icon: if open {
                                icons::CARET_DOWN
                            } else {
                                icons::CARET_RIGHT
                            },
                            label: g.name,
                            detail: Some(&detail),
                            dimmed: up == 0,
                            ..Row::default()
                        },
                    );
                    let tip = t!("containers.compose_tip", count = total, name = g.name);
                    if resp.on_hover_text(tip).clicked() {
                        st.group_open.insert(g.name.to_string(), !open);
                    }
                    if open {
                        for c in &g.containers {
                            container_row(ui, c, 10.0, &is_active, actions);
                        }
                    }
                }
                // Containers outside any Compose project.
                let (running, stopped): (Vec<_>, Vec<_>) =
                    loose.into_iter().partition(|c| c.running());
                for c in &running {
                    container_row(ui, c, 0.0, &is_active, actions);
                }
                if !stopped.is_empty() {
                    let caret = if st.show_stopped {
                        icons::CARET_DOWN
                    } else {
                        icons::CARET_RIGHT
                    };
                    let label = t!("containers.stopped_n", n = stopped.len());
                    let (resp, _) = row(
                        ui,
                        Row {
                            icon: caret,
                            label: &label,
                            dimmed: true,
                            ..Row::default()
                        },
                    );
                    if resp.on_hover_text(t!("containers.stopped.tip")).clicked() {
                        st.show_stopped = !st.show_stopped;
                    }
                    if st.show_stopped {
                        for c in &stopped {
                            container_row(ui, c, 10.0, &is_active, actions);
                        }
                    }
                }
            }
        }
    }

    let Some(context) = &d.kube_context else {
        return;
    };
    subheading(ui, icons::STEERING_WHEEL, "Kubernetes", context);
    // Namespace picker.
    ui.horizontal(|ui| {
        ui.add_space(space::SM);
        let mut chosen = d.namespace.clone();
        let mut names = d.namespaces.clone();
        if !names.contains(&chosen) {
            names.insert(0, chosen.clone());
        }
        egui::ComboBox::from_id_salt("sidebar-k8s-namespace")
            .width(ui.available_width() - space::XS)
            .selected_text(
                RichText::new(format!("{}  {}", icons::STACK, chosen))
                    .font(theme::regular(text::SMALL)),
            )
            .show_ui(ui, |ui| {
                for n in &names {
                    ui.selectable_value(&mut chosen, n.clone(), n);
                }
            })
            .response
            .on_hover_text(t!("containers.namespace"));
        if chosen != d.namespace {
            actions.push(Action::SetKubeNamespace(chosen));
        }
    });
    ui.add_space(space::XXS);
    match &d.pods {
        None => note(ui, t!("containers.loading_pods")),
        Some(Err(e)) => note(ui, e),
        Some(Ok(pods)) if pods.is_empty() => note(ui, t!("containers.no_pods")),
        Some(Ok(pods)) => {
            for pod in pods {
                let single = pod.containers.len() <= 1;
                let first = pod.containers.first().cloned().unwrap_or_default();
                let cfg = kube_config(context, &d.namespace, &pod.name, &first);
                let expanded = st.expanded.contains(&pod.name);
                let detail = if single {
                    String::new()
                } else {
                    t!("containers.n_containers", count = pod.containers.len())
                };
                let running = pod.phase == "Running";
                let (resp, _) = row(
                    ui,
                    Row {
                        icon: icons::CUBE,
                        label: &pod.name,
                        detail: Some(&detail),
                        selected: single && is_active(&cfg),
                        status_dot: true,
                        dot: Some(pod_dot(p, &pod.phase)),
                        dimmed: !running,
                        ..Row::default()
                    },
                );
                let resp = resp.on_hover_text(format!("{} · {}", pod.name, pod.phase));
                if resp.clicked() {
                    if single {
                        actions.push(Action::OpenContainer(Box::new(cfg)));
                    } else if !st.expanded.remove(&pod.name) {
                        st.expanded.insert(pod.name.clone());
                    }
                }
                if !single && expanded {
                    for c in &pod.containers {
                        let cfg = kube_config(context, &d.namespace, &pod.name, c);
                        let (resp, _) = row(
                            ui,
                            Row {
                                icon: icons::PACKAGE,
                                label: c,
                                selected: is_active(&cfg),
                                dimmed: !running,
                                indent: 14.0,
                                ..Row::default()
                            },
                        );
                        if widgets::tooltip(resp, &format!("{}/{c}", pod.name), "").clicked() {
                            actions.push(Action::OpenContainer(Box::new(cfg)));
                        }
                    }
                }
            }
        }
    }
}

/// `nginx:alpine`, `postgres:18` — registries and digests dropped.
pub fn short_image(image: &str) -> &str {
    let no_digest = image.split('@').next().unwrap_or(image);
    no_digest.rsplit('/').next().unwrap_or(no_digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_names_are_shortened() {
        assert_eq!(short_image("nginx:alpine"), "nginx:alpine");
        assert_eq!(
            short_image("registry.example.com/team/blog:dev"),
            "blog:dev"
        );
        assert_eq!(
            short_image("ghcr.io/navikt/mock-oauth2-server:6.0.4"),
            "mock-oauth2-server:6.0.4"
        );
        assert_eq!(short_image("alpine@sha256:abc"), "alpine");
    }
}

/// Made-up `docker` / `kubectl` answers for tests and renders.
#[cfg(test)]
pub(crate) mod synthetic {
    use std::io;
    use std::time::Duration;

    use manta_vfs::container::{CmdOutput, Proc, Runner};

    /// `docker ps` rows: id, name, image, state, status, compose project,
    /// compose service.
    pub const CONTAINERS: [[&str; 7]; 4] = [
        [
            "c1",
            "shop-web",
            "nginx:alpine",
            "running",
            "Up 3 hours",
            "shop",
            "web",
        ],
        [
            "c2",
            "shop-db",
            "mysql:8.4",
            "running",
            "Up 3 hours (healthy)",
            "shop",
            "db",
        ],
        [
            "c3",
            "api-server-1",
            "node:22-alpine",
            "running",
            "Up 40 minutes",
            "api",
            "server",
        ],
        [
            "c4",
            "api-db-1",
            "postgres:17-alpine",
            "exited",
            "Exited (0) 2 days ago",
            "api",
            "db",
        ],
    ];

    pub struct Synthetic;

    impl Synthetic {
        fn answer(program: &str, args: &[String]) -> CmdOutput {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            match (program, a.as_slice()) {
                ("docker", ["ps", ..]) => CmdOutput::ok(
                    CONTAINERS
                        .iter()
                        .map(|r| r.join("\t") + "\n")
                        .collect::<String>(),
                ),
                ("kubectl", ["config", "current-context"]) => CmdOutput::ok("kind-dev\n"),
                ("kubectl", ["config", "get-contexts", ..]) => CmdOutput::ok("kind-dev\n"),
                ("kubectl", ["config", "view", ..]) => CmdOutput::ok("shop"),
                ("kubectl", a) if a.contains(&"namespaces") => CmdOutput::ok("default\nshop\n"),
                ("kubectl", a) if a.contains(&"pods") => CmdOutput::ok(
                    "web-7d9f8c6b5-x2k4p\tRunning\tapp,nginx,\nredis-0\tRunning\tredis,\n",
                ),
                _ => CmdOutput::fail(format!("{program}: not available in tests")),
            }
        }
    }

    impl Runner for Synthetic {
        fn run(
            &self,
            program: &str,
            args: &[String],
            _: Option<&[u8]>,
            _: Duration,
        ) -> io::Result<CmdOutput> {
            Ok(Self::answer(program, args))
        }

        fn spawn(&self, program: &str, _: &[String]) -> io::Result<Box<dyn Proc>> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{program} is disabled in tests"),
            ))
        }
    }

    #[test]
    fn synthetic_discovery_lists_made_up_containers_only() {
        let env = super::discovery_env();
        if manta_vfs::container::real_cli_opted_in() {
            return;
        }
        let list = manta_vfs::container::discover::docker_containers(&*env.runner).unwrap();
        let names: Vec<_> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["api-server-1", "shop-db", "shop-web", "api-db-1"]);
        let pods =
            manta_vfs::container::discover::kube_pods(&*env.runner, "kind-dev", "shop").unwrap();
        assert_eq!(pods.len(), 2);
    }
}
