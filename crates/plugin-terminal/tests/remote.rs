//! Terminal sessions inside real Docker containers.
//!
//! SAFETY: only throwaway containers started here (label `manta-test`,
//! unique names, `--rm`) are used, and a Drop guard removes them. The
//! user's own containers are never touched. Skips without Docker.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_plugin_terminal::palette::Theme;
use manta_plugin_terminal::remote::{find_program, probe, Probe, Target};
use manta_plugin_terminal::session::{GridSize, RemoteSpawn, Session, SpawnOptions};
use manta_plugin_terminal::shim::Integration;

/// A throwaway container, removed on drop.
struct Ctr {
    name: String,
    docker: PathBuf,
}

impl Ctr {
    fn start(image: &str, entrypoint_sleep: bool) -> Option<Ctr> {
        Ctr::start_with(image, entrypoint_sleep, true)
    }

    fn start_with(image: &str, entrypoint_sleep: bool, rm: bool) -> Option<Ctr> {
        if !containers_opted_in() {
            return None;
        }
        let docker = find_program("docker")?;
        // Docker must answer quickly, else skip.
        let ok = Command::new(&docker)
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
            .ok()
            .is_some_and(|o| o.status.success());
        if !ok {
            return None;
        }
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let name = format!(
            "manta-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut args: Vec<&str> = vec!["run", "-d", "--label", "manta-test", "--name", &name];
        if rm {
            args.push("--rm");
        }
        if entrypoint_sleep {
            args.extend(["--entrypoint", "sleep", image, "300"]);
        } else {
            args.extend([image, "sleep", "300"]);
        }
        let out = Command::new(&docker).args(&args).output().ok()?;
        if !out.status.success() {
            eprintln!(
                "skipping: cannot start {image}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
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

fn spawn(ctr: &Ctr, dir: &str) -> Session {
    let target = Target::Docker {
        container: ctr.name.clone(),
    };
    let data = std::env::temp_dir();
    Session::spawn(SpawnOptions {
        shell: PathBuf::from("/bin/sh"),
        cwd: PathBuf::from("/"),
        size: GridSize {
            cols: 100,
            rows: 30,
        },
        cell_px: (8, 16),
        scrollback: 1000,
        shim_root: data.join("manta-remote-test-shims"),
        theme: Theme::dark(),
        waker: Arc::new(|| {}),
        spool: None,
        zdotdir: None,
        remote: Some(RemoteSpawn {
            program: ctr.docker.clone(),
            args: target.args(dir),
            osc_host: target.osc_host(),
            start_dir: dir.into(),
        }),
    })
    .expect("spawn")
}

fn wait(
    s: &mut Session,
    limit: Duration,
    mut cond: impl FnMut(&mut Session, Option<PathBuf>) -> bool,
) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        let nav = s.tick(true);
        if cond(s, nav) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn cwd(s: &Session) -> Option<String> {
    s.follow
        .shell_cwd
        .as_ref()
        .map(|d| d.raw.to_string_lossy().into_owned())
}

fn scenario(ctr: &Ctr, expect_push: bool) {
    let mut s = spawn(ctr, "/");
    assert!(
        wait(&mut s, Duration::from_secs(20), |s, _| s.is_idle()),
        "no prompt in {}:\n{}",
        ctr.name,
        s.lock().screen_text()
    );
    assert!(s.is_remote());
    let integ = s.integration();
    assert_eq!(
        integ == Some(Integration::Push),
        expect_push,
        "integration {integ:?}"
    );
    assert_eq!(cwd(&s).as_deref(), Some("/"));

    // Half-typed input (survives with bash's push/restore keys).
    s.write(b"echo kee");
    std::thread::sleep(Duration::from_millis(300));

    // Pane → terminal: the pane shows /etc inside the container. /etc must
    // not be canonicalized on this Mac (/private/etc).
    s.pane_changed(Some(Path::new("/etc")));
    assert!(
        wait(&mut s, Duration::from_secs(5), |s, _| cwd(s).as_deref()
            == Some("/etc")),
        "no OSC 7 for /etc:\n{}",
        s.lock().screen_text()
    );
    assert!(wait(&mut s, Duration::from_secs(5), |s, _| s.is_idle()));
    std::thread::sleep(Duration::from_millis(300));
    if expect_push {
        s.write(b"p_me\r");
        assert!(
            wait(&mut s, Duration::from_secs(5), |s, _| s
                .lock()
                .screen_text()
                .lines()
                .any(|l| l.trim() == "keep_me")),
            "half-typed input lost:\n{}",
            s.lock().screen_text()
        );
    } else {
        s.write(b"\x15");
    }
    assert!(wait(&mut s, Duration::from_secs(5), |s, _| s.is_idle()));

    // Terminal → pane: a cd typed in the container navigates the pane.
    s.write(b"cd /tmp\r");
    s.user_submitted();
    let mut nav = None;
    assert!(wait(&mut s, Duration::from_secs(5), |_, n| {
        if n.is_some() {
            nav = n;
        }
        nav.is_some()
    }));
    assert_eq!(nav, Some(PathBuf::from("/tmp")));

    let detached = s.shutdown(Duration::from_millis(800));
    assert_eq!(detached, 0, "threads left");
    assert!(s.has_exited());
}

#[test]
fn ash_in_alpine() {
    let Some(ctr) = Ctr::start("alpine:3", false) else {
        eprintln!("skipping: no docker / alpine");
        return;
    };
    scenario(&ctr, false);
}

#[test]
fn bash_in_container() {
    // postgres:*-alpine ships bash; debian:stable-slim if it can be pulled.
    let ctr =
        Ctr::start("postgres:18-alpine", true).or_else(|| Ctr::start("debian:stable-slim", false));
    let Some(ctr) = ctr else {
        eprintln!("skipping: no bash image");
        return;
    };
    scenario(&ctr, true);
}

#[test]
fn stopped_container_is_detected() {
    // Without --rm, so it stays after stopping (the guard removes it).
    let Some(ctr) = Ctr::start_with("alpine:3", false, false) else {
        return;
    };
    let t = Target::Docker {
        container: ctr.name.clone(),
    };
    assert_eq!(probe(&t, Duration::from_secs(10)), Probe::Ready);
    // Our own container only.
    let _ = Command::new(&ctr.docker)
        .args(["stop", "-t", "1", &ctr.name])
        .output();
    assert_eq!(probe(&t, Duration::from_secs(10)), Probe::Stopped);
    let missing = Target::Docker {
        container: format!("{}-missing", ctr.name),
    };
    assert!(matches!(
        probe(&missing, Duration::from_secs(10)),
        Probe::Failed(_)
    ));
}

/// The command string `ssh` hands to the remote login shell, run through a
/// local `sh -c` (no sshd needed): quoting, login files and hooks work.
#[test]
fn ssh_command_string_through_a_login_shell() {
    let target = Target::Ssh {
        user: "anna".into(),
        host: "example.org".into(),
        port: 22,
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap().join("it's here");
    std::fs::create_dir(&dir).unwrap();
    let dir_s = dir.to_string_lossy().into_owned();
    let args = target.args(&dir_s);
    let remote_cmd = args.last().unwrap().clone();
    let mut s = Session::spawn(SpawnOptions {
        shell: PathBuf::from("/bin/sh"),
        cwd: PathBuf::from("/"),
        size: GridSize {
            cols: 100,
            rows: 30,
        },
        cell_px: (8, 16),
        scrollback: 1000,
        shim_root: std::env::temp_dir().join("manta-remote-test-shims"),
        theme: Theme::dark(),
        waker: Arc::new(|| {}),
        spool: None,
        zdotdir: None,
        remote: Some(RemoteSpawn {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), remote_cmd],
            osc_host: target.osc_host(),
            start_dir: dir_s.clone(),
        }),
    })
    .unwrap();
    assert!(
        wait(&mut s, Duration::from_secs(20), |s, _| s.is_idle()
            && cwd(s).as_deref() == Some(dir_s.as_str())),
        "{}",
        s.lock().screen_text()
    );
    s.write(b"cd /\r");
    s.user_submitted();
    let mut nav = None;
    assert!(wait(&mut s, Duration::from_secs(5), |_, n| {
        nav = nav.take().or(n);
        nav.is_some()
    }));
    assert_eq!(nav, Some(PathBuf::from("/")));
    s.shutdown(Duration::from_millis(500));
}

/// Real `docker` / `kubectl` only with `MANTA_TEST_CONTAINERS=1`.
fn containers_opted_in() -> bool {
    let ok = std::env::var_os("MANTA_TEST_CONTAINERS").is_some_and(|v| v == "1");
    if !ok {
        eprintln!("skipped: set MANTA_TEST_CONTAINERS=1 to run tests against docker/kubectl");
    }
    ok
}
