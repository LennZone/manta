//! Profile generation, and real sandboxed runs of a test plugin (macOS):
//! it tries to read a file in a temporary home folder, write there, open
//! a network connection and start another program. Everything happens in
//! temporary folders; the user's real files are never touched.

use super::*;
use std::io::Read as _;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn perms(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

fn sample_paths() -> Paths {
    Paths {
        plugin_dir: PathBuf::from("/plugins/demo"),
        data_dir: PathBuf::from("/data/demo"),
        exec: vec![
            PathBuf::from("/plugins/demo/run.sh"),
            PathBuf::from("/bin/sh"),
        ],
        tool_dirs: vec![PathBuf::from("/Users/anna/.nvm/versions/node/v22")],
        home: Some(PathBuf::from("/Users/anna")),
    }
}

#[test]
fn profile_denies_by_default_and_grants_only_what_is_declared() {
    let none = profile(Policy::default(), &sample_paths());
    assert!(none.contains("(deny default)"), "{none}");
    assert!(
        none.contains("(allow file-read* (subpath \"/plugins/demo\") (subpath \"/data/demo\"))")
    );
    assert!(none.contains("(allow file-write* (subpath \"/data/demo\"))"));
    assert!(none.contains(
        "(allow process-exec* (literal \"/plugins/demo/run.sh\") (literal \"/bin/sh\"))"
    ));
    assert!(none.contains("(subpath \"/Users/anna/.nvm/versions/node/v22\")"));
    assert!(none.contains("(literal \"/Users/anna/.CFUserTextEncoding\")"));
    // Nothing broad without permissions.
    for line in none.lines() {
        let l = line.trim();
        assert_ne!(l, "(allow file-read*)", "{none}");
        assert_ne!(l, "(allow file-write*)", "{none}");
        assert_ne!(l, "(allow network*)", "{none}");
        assert_ne!(l, "(allow process-exec*)", "{none}");
    }
    assert!(!none.contains("(subpath \"/Users/anna\")"), "{none}");

    let all = profile(
        Policy::from_permissions(&perms(&[
            "read_files",
            "write_files",
            "network",
            "run_programs",
            "terminal",
            "camera",
        ])),
        &sample_paths(),
    );
    for rule in [
        "(allow file-read*)\n",
        "(allow file-write*)\n",
        "(allow network*)\n",
        "(allow process-exec*)\n",
    ] {
        assert!(all.contains(rule), "missing {rule}: {all}");
    }
    assert!(all.contains("com.apple.dnssd.service"));
    assert!(!none.contains("com.apple.dnssd.service"));
}

#[test]
fn paths_are_quoted() {
    let mut p = sample_paths();
    p.plugin_dir = PathBuf::from("/plugins/we\"ird\\name");
    let text = profile(Policy::default(), &p);
    assert!(
        text.contains("(subpath \"/plugins/we\\\"ird\\\\name\")"),
        "{text}"
    );
}

#[test]
fn policy_from_permissions() {
    let p = Policy::from_permissions(&perms(&["network", "terminal", "nonsense"]));
    assert_eq!(
        p,
        Policy {
            network: true,
            ..Policy::default()
        }
    );
}

#[test]
fn finds_programs_and_their_interpreters() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let script = dir.join("run.sh");
    std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
    let env_script = dir.join("run.py");
    std::fs::write(&env_script, "#!/usr/bin/env sh\nprint\n").unwrap();
    let path_var = OsString::from("/usr/bin:/bin");
    assert_eq!(
        resolve_program("./run.sh", dir, &path_var),
        Some(dir.join("./run.sh"))
    );
    assert_eq!(resolve_program("missing.sh", dir, &path_var), None);
    assert_eq!(
        resolve_program("sh", dir, &path_var),
        Some(PathBuf::from("/bin/sh"))
    );

    let t = exec_targets(&script, &path_var);
    assert!(
        t.contains(&script) && t.contains(&PathBuf::from("/bin/sh")),
        "{t:?}"
    );
    assert!(
        t.contains(&PathBuf::from("/bin/bash")),
        "sh hands over to bash: {t:?}"
    );
    let t = exec_targets(&env_script, &path_var);
    assert!(t.contains(&PathBuf::from("/usr/bin/env")), "{t:?}");
    assert!(t.contains(&PathBuf::from("/bin/sh")), "{t:?}");
}

#[test]
fn interpreter_install_folders() {
    let home = Path::new("/Users/anna");
    let plugin = Path::new("/Users/anna/Library/Application Support/Manta/plugins/demo");
    let exec = vec![
        PathBuf::from("/Users/anna/.nvm/versions/node/v22/bin/node"),
        PathBuf::from("/Users/anna/bin/tool"), // prefix = home: too broad
        PathBuf::from("/Users/anna/.local/bin/x"), // prefix = ~/.local: too broad
        plugin.join("run.sh"),
        PathBuf::from("/bin/sh"),
        PathBuf::from("/usr/local/bin/tool"), // prefix = /usr/local: a system area
        PathBuf::from("/opt/homebrew/bin/tool"),
        PathBuf::from("/opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/bin/python3.14"),
    ];
    let dirs = tool_dirs(&exec, plugin, Some(home));
    assert!(
        dirs.contains(&PathBuf::from("/Users/anna/.nvm/versions/node/v22")),
        "{dirs:?}"
    );
    assert!(
        dirs.contains(&PathBuf::from(
            "/opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14"
        )),
        "{dirs:?}"
    );
    for broad in ["/", "/usr", "/usr/local", "/opt/homebrew"] {
        assert!(!dirs.contains(&PathBuf::from(broad)), "{broad}: {dirs:?}");
    }
    assert!(
        !dirs
            .iter()
            .any(|d| d == home || d == Path::new("/Users/anna/.local")),
        "{dirs:?}"
    );
    assert!(!dirs.iter().any(|d| plugin.starts_with(d)), "{dirs:?}");
}

#[test]
fn unavailable_sandbox_falls_back_with_a_reason() {
    let missing = Path::new("/nonexistent/sandbox-exec");
    let err = probe(missing).unwrap_err();
    assert!(!err.is_empty());
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("run.sh"), "#!/bin/sh\necho unsandboxed\n").unwrap();
    make_exec(&tmp.path().join("run.sh"));
    let launch = command(
        &["./run.sh".to_string()],
        tmp.path(),
        &tmp.path().join("data"),
        &[],
        OsStr::new("/usr/bin:/bin"),
        missing,
        Err(err.clone()),
    )
    .unwrap();
    assert_eq!(launch.state, SandboxState::Unavailable(err));
    assert!(launch.profile.is_none());
    let mut c = launch.command;
    let out = c.current_dir(tmp.path()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "unsandboxed");

    let e = command(
        &["./nope.sh".to_string()],
        tmp.path(),
        &tmp.path().join("data"),
        &[],
        OsStr::new("/usr/bin:/bin"),
        missing,
        Ok(()),
    )
    .err()
    .unwrap();
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
}

fn make_exec(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The test plugin: a bash script that reports what it was allowed to do.
const PROBE_SCRIPT: &str = r#"#!/bin/bash
if read -r line < "$HOME/secret.txt" 2>/dev/null; then echo "read_home=ok:$line"; else echo "read_home=denied"; fi
if echo x 2>/dev/null > "$HOME/written.txt"; then echo "write_home=ok"; else echo "write_home=denied"; fi
if echo x 2>/dev/null > "$MANTA_PLUGIN_DATA_DIR/own.txt"; then echo "write_data=ok"; else echo "write_data=denied"; fi
if read -r line < "./plugin.toml" 2>/dev/null; then echo "read_own=ok"; else echo "read_own=denied"; fi
if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then echo "network=ok"; else echo "network=denied"; fi
if /usr/bin/true 2>/dev/null; then echo "run_program=ok"; else echo "run_program=denied"; fi
if read -r v <<<"here" && [ "$v" = here ]; then echo "temp_file=ok"; else echo "temp_file=denied:$TMPDIR"; fi
"#;

struct Probe {
    out: String,
    connected: bool,
    home: tempfile::TempDir,
}

impl Probe {
    fn has(&self, what: &str) -> bool {
        self.out
            .lines()
            .any(|l| l == what || l.starts_with(&format!("{what}:")))
    }
}

/// Run the test plugin in the sandbox with `permissions`.
fn run_probe(permissions: &[&str]) -> Probe {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("secret.txt"), "top secret\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("plugins/probe");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        "id = \"probe\"\nname = \"Probe\"\nversion = \"1\"\napi_version = 1\ncommand = [\"./probe.sh\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("probe.sh"), PROBE_SCRIPT).unwrap();
    make_exec(&dir.join("probe.sh"));
    let data = root.path().join("plugin-data/probe");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let connected = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let accept = {
        let (connected, stop) = (connected.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if listener.accept().is_ok() {
                    connected.store(true, Ordering::SeqCst);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    };

    let launch = command(
        &["./probe.sh".to_string()],
        &dir,
        &data,
        &perms(permissions),
        OsStr::new("/usr/bin:/bin"),
        Path::new(SANDBOX_EXEC),
        Ok(()),
    )
    .unwrap();
    assert_eq!(launch.state, SandboxState::Enforced);
    let mut c = launch.command;
    c.current_dir(&dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("MANTA_PLUGIN_DATA_DIR", &data)
        .env("PORT", port.to_string())
        .envs(env(&data))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < Duration::from_secs(20), "probe hangs");
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    // Give the listener a moment to see a connection.
    std::thread::sleep(Duration::from_millis(50));
    stop.store(true, Ordering::SeqCst);
    accept.join().unwrap();
    assert!(
        out.lines().count() >= 6,
        "probe didn't finish: {out}\nstderr: {err}\nprofile:\n{}",
        launch.profile.unwrap_or_default()
    );
    Probe {
        out,
        connected: connected.load(Ordering::SeqCst),
        home,
    }
}

fn sandbox_ready() -> bool {
    match available() {
        Ok(()) => true,
        Err(e) => {
            if cfg!(target_os = "macos") {
                panic!("sandbox-exec should work on macOS: {e}");
            }
            eprintln!("no sandbox here ({e}), skipping");
            false
        }
    }
}

#[test]
fn undeclared_permissions_are_denied() {
    if !sandbox_ready() {
        return;
    }
    let p = run_probe(&[]);
    assert!(p.has("read_home=denied"), "{}", p.out);
    assert!(p.has("write_home=denied"), "{}", p.out);
    assert!(!p.home.path().join("written.txt").exists());
    assert!(p.has("network=denied"), "{}", p.out);
    assert!(!p.connected, "no connection may reach the listener");
    assert!(p.has("run_program=denied"), "{}", p.out);
    // Its own folder and data folder always work.
    assert!(p.has("read_own=ok"), "{}", p.out);
    assert!(p.has("write_data=ok"), "{}", p.out);
    assert!(p.has("temp_file=ok"), "here documents work: {}", p.out);
}

#[test]
fn declared_permissions_are_granted() {
    if !sandbox_ready() {
        return;
    }
    let p = run_probe(&["read_files", "write_files", "network", "run_programs"]);
    assert!(p.has("read_home=ok:top secret"), "{}", p.out);
    assert!(p.has("write_home=ok"), "{}", p.out);
    assert!(p.home.path().join("written.txt").exists());
    assert!(p.has("network=ok"), "{}", p.out);
    assert!(p.connected, "the listener saw the connection");
    assert!(p.has("run_program=ok"), "{}", p.out);
}

/// `python3` from /usr/bin is a shim that runs the Command Line Tools' or
/// Xcode's Python through xcrun: it works without any permission.
#[test]
fn apple_python_shim_runs_in_the_sandbox() {
    if !sandbox_ready() {
        return;
    }
    // Without developer tools the shim would offer to install them.
    let Some(dev) = developer_dir() else {
        eprintln!("no developer tools, skipping");
        return;
    };
    if !dev.exists() || !Path::new("/usr/bin/python3").exists() {
        return;
    }
    // The shim must work outside the sandbox first (some CI images have
    // Xcode without a python3).
    let plain = std::process::Command::new("/usr/bin/python3")
        .args(["-c", "print('python ok')"])
        .output();
    if !plain.is_ok_and(|o| o.status.success()) {
        eprintln!("/usr/bin/python3 doesn't run here even without the sandbox, skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let launch = command(
        &[
            "/usr/bin/python3".into(),
            "-c".into(),
            "print('python ok')".into(),
        ],
        tmp.path(),
        &data,
        &[],
        OsStr::new("/usr/bin:/bin"),
        Path::new(SANDBOX_EXEC),
        Ok(()),
    )
    .unwrap();
    let mut c = launch.command;
    let out = c
        .current_dir(tmp.path())
        .envs(env(&data))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "python ok",
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn each_permission_grants_only_itself() {
    if !sandbox_ready() {
        return;
    }
    let p = run_probe(&["read_files"]);
    assert!(p.has("read_home=ok:top secret"), "{}", p.out);
    assert!(
        p.has("write_home=denied") && p.has("network=denied"),
        "{}",
        p.out
    );
    let p = run_probe(&["network"]);
    assert!(p.has("network=ok") && p.connected, "{}", p.out);
    assert!(
        p.has("read_home=denied") && p.has("run_program=denied"),
        "{}",
        p.out
    );
}
