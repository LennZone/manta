//! Finding containers and pods for the sidebar and the connect dialog
//! (read-only CLI calls: `docker ps`, `kubectl get`), and starting,
//! restarting and stopping Docker containers from the sidebar. Blocking,
//! so run them off the UI thread.

use std::time::Duration;

use manta_core::{VfsError, VfsResult};

use super::runner::{map_error, spawn_error, Runner, DISCOVERY_TIMEOUT};

/// A Docker container as `docker ps -a` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused`, `created`, `restarting`, `dead`.
    pub state: String,
    /// Human text such as "Up 3 hours (healthy)".
    pub status: String,
    /// Docker Compose project (`com.docker.compose.project` label); Docker
    /// Desktop groups containers by it.
    pub project: Option<String>,
    /// Compose service name (`com.docker.compose.service` label).
    pub service: Option<String>,
}

impl DockerContainer {
    pub fn running(&self) -> bool {
        self.state == "running"
    }

    /// What the sidebar offers for this container: a stopped one can be
    /// started; a running, paused or restarting one restarted or stopped.
    pub fn ops(&self) -> &'static [DockerOp] {
        match self.state.as_str() {
            "running" | "paused" | "restarting" => &[DockerOp::Restart, DockerOp::Stop],
            _ => &[DockerOp::Start],
        }
    }
}

/// A lifecycle command for a Docker container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockerOp {
    Start,
    Restart,
    Stop,
}

impl DockerOp {
    /// The `docker` subcommand.
    pub fn verb(self) -> &'static str {
        match self {
            DockerOp::Start => "start",
            DockerOp::Restart => "restart",
            DockerOp::Stop => "stop",
        }
    }
}

/// `docker stop` waits up to 10 s for the process to exit before it kills
/// it; leave room for that and for slow health checks on start.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(60);

/// `docker start|restart|stop <id>`; errors name the container by `name`.
pub fn docker_control(runner: &dyn Runner, op: DockerOp, id: &str, name: &str) -> VfsResult<()> {
    let args = [op.verb().to_string(), id.to_string()];
    let out = runner
        .run("docker", &args, None, CONTROL_TIMEOUT)
        .map_err(|e| spawn_error("docker", &e))?;
    if !out.success {
        return Err(map_error("docker", &out.stderr, name));
    }
    Ok(())
}

/// A pod and its containers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubePod {
    pub name: String,
    /// `Running`, `Pending`, `Succeeded`, `Failed`, `Unknown`.
    pub phase: String,
    pub containers: Vec<String>,
}

fn run(runner: &dyn Runner, program: &str, args: &[&str]) -> VfsResult<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let out = runner
        .run(program, &args, None, DISCOVERY_TIMEOUT)
        .map_err(|e| spawn_error(program, &e))?;
    if !out.success {
        return Err(map_error(program, &out.stderr, program));
    }
    Ok(out.stdout_str())
}

const PS_FORMAT: &str = "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.State}}\t{{.Status}}\t{{.Label \"com.docker.compose.project\"}}\t{{.Label \"com.docker.compose.service\"}}";

/// All containers (running first, then by name).
pub fn docker_containers(runner: &dyn Runner) -> VfsResult<Vec<DockerContainer>> {
    let text = run(
        runner,
        "docker",
        &["ps", "-a", "--no-trunc", "--format", PS_FORMAT],
    )?;
    let mut v = parse_ps(&text);
    v.sort_by(|a, b| {
        b.running()
            .cmp(&a.running())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(v)
}

pub fn parse_ps(text: &str) -> Vec<DockerContainer> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let id = f.next()?.trim().to_string();
            let name = f.next()?.trim().split(',').next().unwrap_or("").to_string();
            if id.is_empty() || name.is_empty() {
                return None;
            }
            Some(DockerContainer {
                id,
                name,
                image: f.next().unwrap_or("").trim().to_string(),
                state: f.next().unwrap_or("").trim().to_lowercase(),
                status: f.next().unwrap_or("").trim().to_string(),
                project: f
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                service: f
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            })
        })
        .collect()
}

/// The kubeconfig's current context, if one is set.
pub fn kube_current_context(runner: &dyn Runner) -> VfsResult<Option<String>> {
    match run(runner, "kubectl", &["config", "current-context"]) {
        Ok(s) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
        Err(VfsError::Connection(m)) if m.contains("current-context is not set") => Ok(None),
        Err(e) => Err(e),
    }
}

/// All context names in the kubeconfig.
pub fn kube_contexts(runner: &dyn Runner) -> VfsResult<Vec<String>> {
    Ok(lines(&run(
        runner,
        "kubectl",
        &["config", "get-contexts", "-o", "name"],
    )?))
}

/// The namespace a context defaults to (`default` if none is set).
pub fn kube_default_namespace(runner: &dyn Runner, context: &str) -> VfsResult<String> {
    let filter = format!("jsonpath={{.contexts[?(@.name==\"{context}\")].context.namespace}}");
    let ns = run(runner, "kubectl", &["config", "view", "-o", &filter])?;
    Ok(Some(ns.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".into()))
}

/// Namespaces of the cluster behind `context`.
pub fn kube_namespaces(runner: &dyn Runner, context: &str) -> VfsResult<Vec<String>> {
    let mut args = ctx_flag(context);
    args.extend([
        "get",
        "namespaces",
        "-o",
        "jsonpath={range .items[*]}{.metadata.name}{\"\\n\"}{end}",
    ]);
    Ok(lines(&run(runner, "kubectl", &args)?))
}

/// Pods (with their containers) in `namespace`.
pub fn kube_pods(runner: &dyn Runner, context: &str, namespace: &str) -> VfsResult<Vec<KubePod>> {
    let mut args = ctx_flag(context);
    args.extend([
        "-n",
        namespace,
        "get",
        "pods",
        "-o",
        "jsonpath={range .items[*]}{.metadata.name}{\"\\t\"}{.status.phase}{\"\\t\"}{range .spec.containers[*]}{.name}{\",\"}{end}{\"\\n\"}{end}",
    ]);
    Ok(parse_pods(&run(runner, "kubectl", &args)?))
}

pub fn parse_pods(text: &str) -> Vec<KubePod> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let name = f.next()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let phase = f.next().unwrap_or("").trim().to_string();
            let containers = f
                .next()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(String::from)
                .collect();
            Some(KubePod {
                name,
                phase,
                containers,
            })
        })
        .collect()
}

fn ctx_flag(context: &str) -> Vec<&str> {
    if context.is_empty() {
        Vec::new()
    } else {
        vec!["--context", context]
    }
}

fn lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// A context that runs on this machine (kind, minikube, Docker Desktop,
/// Rancher, OrbStack, k3d, colima): the only kind the tests talk to.
pub fn is_local_context(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "kind-",
        "minikube",
        "docker-desktop",
        "docker-for-desktop",
        "rancher-desktop",
        "orbstack",
        "k3d-",
        "colima",
    ]
    .iter()
    .any(|p| n.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::super::runner::mock::MockRunner;
    use super::super::runner::CmdOutput;
    use super::*;

    #[test]
    fn container_ops_follow_the_state() {
        let c = |state: &str| DockerContainer {
            id: "1".into(),
            name: "web".into(),
            image: "nginx".into(),
            state: state.into(),
            status: String::new(),
            project: None,
            service: None,
        };
        use DockerOp::*;
        assert_eq!(c("running").ops(), [Restart, Stop]);
        assert_eq!(c("paused").ops(), [Restart, Stop]);
        assert_eq!(c("restarting").ops(), [Restart, Stop]);
        assert_eq!(c("exited").ops(), [Start]);
        assert_eq!(c("created").ops(), [Start]);
    }

    #[test]
    fn docker_control_runs_the_verb_on_the_id() {
        let r = MockRunner::new(|_, args, _| match args[0].as_str() {
            "stop" => CmdOutput::fail("Error response from daemon: No such container: 3f2a"),
            _ => CmdOutput::ok("3f2a\n"),
        });
        docker_control(&*r, DockerOp::Start, "3f2a", "web").unwrap();
        docker_control(&*r, DockerOp::Restart, "3f2a", "web").unwrap();
        let e = docker_control(&*r, DockerOp::Stop, "3f2a", "web").unwrap_err();
        assert!(e.to_string().contains("web not found"), "{e}");
        assert_eq!(
            r.calls(),
            [
                ["docker", "start", "3f2a"],
                ["docker", "restart", "3f2a"],
                ["docker", "stop", "3f2a"]
            ]
        );
    }

    #[test]
    fn parses_docker_ps() {
        let text = "3f2a\tweb\tnginx:alpine\trunning\tUp 3 hours (healthy)\n\
                    9b1c\tdb,legacy-link\tpostgres:18\texited\tExited (0) 2 days ago\n\
                    \n";
        let v = parse_ps(text);
        assert_eq!(v.len(), 2);
        assert_eq!(
            (v[0].name.as_str(), v[0].image.as_str(), v[0].running()),
            ("web", "nginx:alpine", true)
        );
        assert_eq!(
            (v[1].name.as_str(), v[1].state.as_str(), v[1].running()),
            ("db", "exited", false)
        );
    }

    #[test]
    fn parses_compose_labels() {
        let text = "1\tapi-db-1\tpostgres:18\trunning\tUp\tapi\tdb\n\
                    2\tmailpit\tmailpit\texited\tExited\t\t\n\
                    3\told\ti\texited\tExited\n";
        let v = parse_ps(text);
        assert_eq!(
            (v[0].project.as_deref(), v[0].service.as_deref()),
            (Some("api"), Some("db"))
        );
        assert_eq!(
            (v[1].project.as_deref(), v[1].service.as_deref()),
            (None, None)
        );
        assert_eq!(v[2].project, None, "older format without label columns");
    }

    #[test]
    fn running_containers_come_first() {
        let r = MockRunner::new(|_, _, _| {
            CmdOutput::ok(
                "1\tzeta\ti\texited\tExited\n2\talpha\ti\texited\tExited\n3\tmid\ti\trunning\tUp\n",
            )
        });
        let v = docker_containers(&*r).unwrap();
        let names: Vec<_> = v.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["mid", "alpha", "zeta"]);
        assert_eq!(r.calls()[0][..3], ["docker", "ps", "-a"]);
    }

    #[test]
    fn kube_commands() {
        let r = MockRunner::new(|_, args, _| {
            if args.contains(&"pods".to_string()) {
                CmdOutput::ok("web-0\tRunning\tapp,sidecar,\njob-1\tSucceeded\tmain,\n")
            } else if args.contains(&"current-context".to_string()) {
                CmdOutput::fail("error: current-context is not set")
            } else {
                CmdOutput::ok("default\nkube-system\n")
            }
        });
        let pods = kube_pods(&*r, "kind-dev", "shop").unwrap();
        assert_eq!(pods[0].containers, ["app", "sidecar"]);
        assert_eq!(pods[1].phase, "Succeeded");
        let call = &r.calls()[0];
        assert_eq!(
            call[..6],
            ["kubectl", "--context", "kind-dev", "-n", "shop", "get"]
        );
        assert_eq!(
            kube_namespaces(&*r, "").unwrap(),
            ["default", "kube-system"]
        );
        assert_eq!(r.calls()[1][1..3], ["get", "namespaces"]);
        assert_eq!(kube_current_context(&*r).unwrap(), None);
    }

    #[test]
    fn local_contexts() {
        assert!(is_local_context("kind-dev"));
        assert!(is_local_context("docker-desktop"));
        assert!(is_local_context("minikube"));
        assert!(!is_local_context(
            "arn:aws:eks:eu-central-1:1234:cluster/prod"
        ));
        assert!(!is_local_context("gke_shop_europe-west3_prod"));
    }
}
