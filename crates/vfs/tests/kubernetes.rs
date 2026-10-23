//! Kubernetes backend against a local cluster, read-only.
//!
//! Runs only when the current kube context is clearly local (kind,
//! minikube, Docker Desktop, ...) and reachable; skipped otherwise. Only
//! read-only calls: `get`, and `exec` of `sh -c echo`, `pwd`, the listing
//! script and `cat`. Nothing in the cluster is changed.

use std::io::Read;

use manta_core::{Vfs, VfsPath};
use manta_vfs::container::discover::{is_local_context, kube_current_context, kube_pods};
use manta_vfs::container::{kube_config, CliRunner, ContainerFs, Mode};

#[test]
fn kubernetes_read_only_browse() {
    if !containers_opted_in() {
        return;
    }
    let runner = CliRunner::shared();
    let ctx = match kube_current_context(&*runner) {
        Ok(Some(c)) if is_local_context(&c) => c,
        other => {
            eprintln!("kubernetes test skipped: no local kube context ({other:?})");
            return;
        }
    };
    let pods = match kube_pods(&*runner, &ctx, "kube-system") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("kubernetes test skipped: {ctx} unreachable: {e}");
            return;
        }
    };
    // The first running pod whose container has a shell.
    for pod in pods.iter().filter(|p| p.phase == "Running") {
        let container = pod.containers.first().cloned().unwrap_or_default();
        let cfg = kube_config(&ctx, "kube-system", &pod.name, &container);
        let Ok(fs) = ContainerFs::connect(&cfg) else {
            continue;
        };
        assert_eq!(fs.mode(), Mode::Exec);
        let root = fs.list(&VfsPath::root()).expect("list /");
        assert!(
            root.iter().any(|e| e.name == "etc" && e.is_dir()),
            "{root:?}"
        );
        let st = fs.stat(&VfsPath::new("/etc/hostname")).expect("stat");
        let mut s = String::new();
        fs.open_read(&VfsPath::new("/etc/hostname"))
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s.len() as u64, st.size);
        return;
    }
    eprintln!("kubernetes test skipped: no running kube-system pod with a shell");
}

/// Real `docker` / `kubectl` only with `MANTA_TEST_CONTAINERS=1`.
fn containers_opted_in() -> bool {
    let ok = std::env::var_os("MANTA_TEST_CONTAINERS").is_some_and(|v| v == "1");
    if !ok {
        eprintln!("skipped: set MANTA_TEST_CONTAINERS=1 to run tests against docker/kubectl");
    }
    ok
}
