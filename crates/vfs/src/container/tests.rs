//! `ContainerFs` against a scripted runner: command building, modes,
//! streaming read and write, error mapping.

use std::io::{Read, Write};
use std::sync::Arc;

use manta_core::{EntryKind, Protocol, Vfs, VfsError, VfsPath};

use super::runner::mock::MockRunner;
use super::runner::CmdOutput;
use super::*;

fn tar_file(name: &str, data: &[u8]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_entry_type(tar::EntryType::Regular);
    b.append_data(&mut h, name, data).unwrap();
    b.into_inner().unwrap()
}

/// A running alpine-like container named `web` with WORKDIR /app.
fn docker_runner(running: bool, shell: bool) -> Arc<MockRunner> {
    MockRunner::new(move |prog, args, _| {
        assert_eq!(prog, "docker");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["inspect", .., "web"] => CmdOutput::ok(format!("{running}\t/app\t/web\n")),
            ["inspect", ..] => CmdOutput::fail("Error: No such container: nope"),
            ["exec", "web", "sh", "-c", "echo manta-ok"] if shell => CmdOutput::ok("manta-ok\n"),
            ["exec", "web", "sh", "-c", "echo manta-ok"] => CmdOutput::fail(
                "OCI runtime exec failed: exec failed: unable to start container process: exec: \"sh\": executable file not found in $PATH: unknown",
            ),
            ["exec", "web", "sh", "-c", _, "sh", "L", "/app"] => {
                CmdOutput::ok(b"./index.html\0f\0./static\0d\0\0" .iter().chain(b"120 1700000000 81a4\n4096 1700000000 41ed\n").copied().collect::<Vec<u8>>())
            }
            ["exec", "web", "sh", "-c", _, "sh", "S", "/app/gone"] => CmdOutput::fail("/app/gone: No such file or directory"),
            ["exec", "web", "mkdir", "/app/locked"] => CmdOutput::fail("mkdir: can't create directory '/app/locked': Permission denied"),
            ["exec", "web", ..] => CmdOutput::ok(""),
            ["exec", "-i", "web", "sh", "-c", ..] => CmdOutput::ok(""),
            ["cp", "-L", "web:/app/index.html", "-"] => CmdOutput::ok(tar_file("index.html", b"<h1>hi</h1>")),
            ["cp", "-L", ..] => CmdOutput::fail("Error response from daemon: Could not find the file /x in container web"),
            ["cp", "web:/app", "-"] => {
                let mut b = tar::Builder::new(Vec::new());
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Directory);
                h.set_size(0);
                b.append_data(&mut h, "app/", std::io::empty()).unwrap();
                let mut h = tar::Header::new_gnu();
                h.set_size(2);
                b.append_data(&mut h, "app/a b.txt", &b"hi"[..]).unwrap();
                CmdOutput::ok(b.into_inner().unwrap())
            }
            other => panic!("unexpected command {other:?}"),
        }
    })
}

#[test]
fn docker_running_container_uses_exec() {
    let r = docker_runner(true, true);
    let fs = ContainerFs::connect_with(&docker_config("web"), r.clone()).unwrap();
    assert_eq!(
        (fs.mode(), fs.protocol(), fs.label().as_str()),
        (Mode::Exec, Protocol::Docker, "web (Docker)")
    );
    assert_eq!(fs.home().unwrap(), VfsPath::new("/app"));
    assert!(!fs.is_read_only());
    assert_eq!(fs.local_path(&VfsPath::new("/app")), None);

    let e = fs.list(&VfsPath::new("/app")).unwrap();
    assert_eq!(
        e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["index.html", "static"]
    );
    assert_eq!(e[0].path.as_str(), "/app/index.html");
    assert_eq!(e[1].kind, EntryKind::Directory);
    // The path is an argument of the script, never part of it.
    let list_call = r
        .calls()
        .into_iter()
        .find(|c| c.contains(&"L".to_string()))
        .unwrap();
    assert_eq!(list_call[..5], ["docker", "exec", "web", "sh", "-c"]);
    assert_eq!(list_call[5], listing::LIST_SCRIPT);
    assert_eq!(list_call[6..], ["sh", "L", "/app"]);

    assert!(matches!(
        fs.stat(&VfsPath::new("/app/gone")),
        Err(VfsError::NotFound(_))
    ));
    assert!(matches!(
        fs.create_dir(&VfsPath::new("/app/locked")),
        Err(VfsError::PermissionDenied(_))
    ));

    let quote = VfsPath::new("/app/it's a \"file\"");
    fs.create_dir(&VfsPath::new("/app/new dir")).unwrap();
    fs.rename(&quote, &VfsPath::new("/app/b")).unwrap();
    fs.set_permissions(&quote, 0o100755).unwrap();
    fs.remove_tree(&VfsPath::new("/app/new dir")).unwrap();
    assert!(fs.remove_tree(&VfsPath::root()).is_err(), "never rm -rf /");
    let calls = r.calls();
    let has = |want: &[&str]| {
        calls
            .iter()
            .any(|c| c.iter().map(String::as_str).collect::<Vec<_>>() == want)
    };
    assert!(has(&["docker", "exec", "web", "mkdir", "/app/new dir"]));
    assert!(has(&[
        "docker",
        "exec",
        "web",
        "chmod",
        "755",
        "/app/it's a \"file\""
    ]));
    assert!(has(&["docker", "exec", "web", "rm", "-rf", "/app/new dir"]));
    let mv = calls
        .iter()
        .find(|c| c.iter().any(|a| a.contains("mv -f")))
        .unwrap();
    assert_eq!(mv[mv.len() - 2..], ["/app/it's a \"file\"", "/app/b"]);
}

#[test]
fn docker_read_streams_from_tar() {
    let r = docker_runner(true, true);
    let fs = ContainerFs::connect_with(&docker_config("web"), r).unwrap();
    let mut s = String::new();
    fs.open_read(&VfsPath::new("/app/index.html"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert_eq!(s, "<h1>hi</h1>");
    match fs.open_read(&VfsPath::new("/x")) {
        Err(VfsError::NotFound(p)) => assert_eq!(p, "/x"),
        Err(e) => panic!("unexpected {e:?}"),
        Ok(_) => panic!("read a missing file"),
    }
}

#[test]
fn write_streams_into_cat_and_appends_after_flush() {
    let r = docker_runner(true, true);
    let fs = ContainerFs::connect_with(&docker_config("web"), r.clone()).unwrap();
    let mut w = fs.open_write(&VfsPath::new("/app/up load.bin")).unwrap();
    w.write_all(b"hello ").unwrap();
    assert_eq!(&*r.written.lock().unwrap(), b"hello ");
    w.flush().unwrap();
    w.write_all(b"world").unwrap();
    w.flush().unwrap();
    let calls = r.calls();
    let writes: Vec<_> = calls
        .iter()
        .filter(|c| c.get(2).map(String::as_str) == Some("-i"))
        .collect();
    assert_eq!(writes.len(), 2);
    assert_eq!(
        writes[0][3..],
        ["web", "sh", "-c", "cat > \"$1\"", "sh", "/app/up load.bin"]
    );
    assert_eq!(writes[1][6], "cat >> \"$1\"");
}

#[test]
fn stopped_container_is_read_only_and_lists_via_tar() {
    let r = docker_runner(false, true);
    let fs = ContainerFs::connect_with(&docker_config("web"), r.clone()).unwrap();
    assert_eq!(fs.mode(), Mode::Stopped);
    assert!(fs.is_read_only());
    let e = fs.list(&VfsPath::new("/app")).unwrap();
    assert_eq!((e[0].name.as_str(), e[0].size), ("a b.txt", 2));
    let err = fs.create_dir(&VfsPath::new("/app/x")).unwrap_err();
    assert!(err.to_string().contains("stopped"), "{err}");
    assert!(fs.open_write(&VfsPath::new("/app/x")).is_err());
    // No exec was attempted on the stopped container.
    assert!(r.calls().iter().all(|c| c[1] != "exec"));
}

#[test]
fn container_without_shell_is_read_only() {
    let fs = ContainerFs::connect_with(&docker_config("web"), docker_runner(true, false)).unwrap();
    assert_eq!(fs.mode(), Mode::NoShell);
    assert!(fs.is_read_only());
    assert!(fs
        .remove_file(&VfsPath::new("/app/x"))
        .unwrap_err()
        .to_string()
        .contains("no shell"));
}

#[test]
fn friendly_connect_errors() {
    let r = docker_runner(true, true);
    match ContainerFs::connect_with(&docker_config("nope"), r) {
        Err(VfsError::NotFound(m)) => assert!(m.contains("container nope"), "{m}"),
        Err(e) => panic!("unexpected {e:?}"),
        Ok(_) => panic!("connected to a missing container"),
    }
    let daemon = MockRunner::new(|_, _, _| {
        CmdOutput::fail("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?")
    });
    let err = ContainerFs::connect_with(&docker_config("web"), daemon)
        .err()
        .unwrap();
    assert!(err.to_string().contains("daemon not running"), "{err}");
}

fn kube_runner(phase: &'static str) -> Arc<MockRunner> {
    MockRunner::new(move |prog, args, _| {
        assert_eq!(prog, "kubectl");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        match a.as_slice() {
            [.., "get", "pod", "web-0", "-o", _] => CmdOutput::ok(phase),
            [.., "--", "sh", "-c", "echo manta-ok"] => CmdOutput::ok("manta-ok\n"),
            [.., "--", "pwd"] => CmdOutput::ok("/srv\n"),
            [.., "--", "sh", "-c", _, "sh", "S", "/srv/a.txt"] => CmdOutput::ok(
                b"/srv/a.txt\0f\0\0"
                    .iter()
                    .chain(b"5 1700000000 81a4\n")
                    .copied()
                    .collect::<Vec<u8>>(),
            ),
            [.., "--", "cat", "/srv/a.txt"] => CmdOutput::ok("hello"),
            other => panic!("unexpected command {other:?}"),
        }
    })
}

#[test]
fn kubernetes_command_building() {
    let r = kube_runner("Running");
    let cfg = kube_config("kind-dev", "shop", "web-0", "app");
    assert_eq!(
        (cfg.host.as_str(), cfg.username.as_str()),
        ("shop/web-0", "app")
    );
    let fs = ContainerFs::connect_with(&cfg, r.clone()).unwrap();
    assert_eq!(
        (fs.protocol(), fs.label().as_str()),
        (Protocol::Kubernetes, "web-0/app (Kubernetes)")
    );
    assert_eq!(fs.home().unwrap(), VfsPath::new("/srv"));
    let calls = r.calls();
    assert_eq!(
        calls[0],
        [
            "kubectl",
            "--context",
            "kind-dev",
            "-n",
            "shop",
            "get",
            "pod",
            "web-0",
            "-o",
            "jsonpath={.status.phase}"
        ]
    );
    assert_eq!(
        calls[1][..9],
        [
            "kubectl",
            "--context",
            "kind-dev",
            "-n",
            "shop",
            "exec",
            "web-0",
            "-c",
            "app"
        ]
    );
    assert_eq!(calls[1][9], "--");

    let mut s = String::new();
    fs.open_read(&VfsPath::new("/srv/a.txt"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert_eq!(s, "hello");

    let t = fs
        .target()
        .exec_args(true, &["sh", "-c", "cat > \"$1\"", "sh", "/srv/x"]);
    assert_eq!(
        t[..7],
        ["--context", "kind-dev", "-n", "shop", "exec", "-i", "web-0"]
    );
}

#[test]
fn kubernetes_pending_pod_is_refused() {
    let err = ContainerFs::connect_with(&kube_config("", "", "web-0", ""), kube_runner("Pending"))
        .err()
        .unwrap();
    assert!(err.to_string().contains("not running (Pending)"), "{err}");
}

#[test]
fn shell_commands_are_quoted() {
    let d = Target::Docker {
        container: "web".into(),
    };
    assert_eq!(
        d.shell_command(&VfsPath::new("/var/www")),
        r#"docker exec -it web sh -c 'cd /var/www && exec ${SHELL:-sh}'"#
    );
    // A folder with a quote and a space: quoted inside, then the whole script.
    let cmd = d.shell_command(&VfsPath::new("/srv/it's here"));
    assert_eq!(
        cmd,
        r#"docker exec -it web sh -c 'cd '\''/srv/it'\''\'\'''\''s here'\'' && exec ${SHELL:-sh}'"#
    );
    let k = Target::Kube {
        context: "kind-dev".into(),
        namespace: "shop".into(),
        pod: "web-0".into(),
        container: "app".into(),
    };
    assert_eq!(
        k.shell_command(&VfsPath::new("/srv")),
        r#"kubectl --context kind-dev -n shop exec -it web-0 -c app -- sh -c 'cd /srv && exec ${SHELL:-sh}'"#
    );
}

#[test]
fn config_round_trip() {
    let cfg = docker_config("shop-db");
    assert_eq!(
        Target::from_config(&cfg).unwrap(),
        Target::Docker {
            container: "shop-db".into()
        }
    );
    // Older favorites without options fall back to the host field.
    let mut bare = cfg.clone();
    bare.container = None;
    assert_eq!(
        Target::from_config(&bare).unwrap(),
        Target::Docker {
            container: "shop-db".into()
        }
    );
    let mut none = kube_config("", "", "", "");
    none.host.clear();
    assert!(Target::from_config(&none).is_err());
}

#[test]
fn resume_reads_with_tail_and_appends_with_cat() {
    let r = MockRunner::new(|prog, args, _| {
        assert_eq!(prog, "docker");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["inspect", .., "web"] => CmdOutput::ok("true\t/app\t/web\n"),
            ["exec", "web", "sh", "-c", "echo manta-ok"] => CmdOutput::ok("manta-ok\n"),
            ["exec", "web", "sh", "-c", _, "sh", "S", "/app/big.bin"] => CmdOutput::ok(
                b"./big.bin\0f\0\0"
                    .iter()
                    .chain(b"10 1700000000 81a4\n")
                    .copied()
                    .collect::<Vec<u8>>(),
            ),
            ["exec", "web", "sh", "-c", script, "sh", "/app/big.bin", "5"]
                if script.contains("tail -c +\"$2\"") =>
            {
                CmdOutput::ok("56789")
            }
            ["exec", "-i", "web", "sh", "-c", ..] => CmdOutput::ok(""),
            other => panic!("unexpected command {other:?}"),
        }
    });
    let fs = ContainerFs::connect_with(&docker_config("web"), r.clone()).unwrap();
    let caps = fs.supports_resume();
    assert!(caps.read_at && caps.append);
    let mut s = String::new();
    fs.open_read_at(&VfsPath::new("/app/big.bin"), 4)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert_eq!(s, "56789");
    assert!(
        fs.open_read_at(&VfsPath::new("/app/big.bin"), 11).is_err(),
        "offset beyond the end"
    );

    let mut w = fs.open_append(&VfsPath::new("/app/big.bin")).unwrap();
    w.write_all(b"abc").unwrap();
    w.flush().unwrap();
    let calls = r.calls();
    let append = calls
        .iter()
        .find(|c| c.get(2).map(String::as_str) == Some("-i"))
        .unwrap();
    assert_eq!(
        append[3..],
        ["web", "sh", "-c", "cat >> \"$1\"", "sh", "/app/big.bin"]
    );
    fs.reconnect().unwrap();
}

#[test]
fn stopped_container_can_only_read_at_by_skipping() {
    let r = docker_runner(false, true);
    let fs = ContainerFs::connect_with(&docker_config("web"), r).unwrap();
    let caps = fs.supports_resume();
    assert!(!caps.read_at && !caps.append);
    let mut s = String::new();
    fs.open_read_at(&VfsPath::new("/app/index.html"), 4)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert_eq!(s, "hi</h1>");
    assert!(fs.open_append(&VfsPath::new("/app/x")).is_err());
}
