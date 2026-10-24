//! scripts/plugin-test.py (the documented way to test a plugin without
//! Manta) knows the same message types as the host, and the example
//! templates in examples/plugins pass it. Skipped without `python3`;
//! hello-node needs `node`, hello-shell needs `jq`. hello-rust is only
//! checked for its manifest (building it is left to its README).

use std::path::{Path, PathBuf};
use std::process::Command;

use manta_plugin::protocol::{PLUGIN_MESSAGE_TYPES, PROTOCOL_VERSION};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn has(program: &str, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn harness(args: &[&str]) -> (bool, String) {
    let out = Command::new("python3")
        .arg("-B")
        .arg(repo().join("scripts/plugin-test.py"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

#[test]
fn harness_knows_the_protocol() {
    if !has("python3", "--version") {
        eprintln!("python3 not found, skipping");
        return;
    }
    let (ok, text) = harness(&["--list-types"]);
    assert!(ok, "{text}");
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(v["api_version"], PROTOCOL_VERSION);
    let plugin: Vec<&str> = v["plugin"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(plugin, PLUGIN_MESSAGE_TYPES);
    let host: Vec<&str> = v["host"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(host, manta_plugin_protocol::HOST_MESSAGE_TYPES);
}

fn check(name: &str, needs: Option<(&str, &str)>, manifest_only: bool) {
    if !has("python3", "--version") {
        eprintln!("python3 not found, skipping");
        return;
    }
    let dir = repo().join("examples/plugins").join(name);
    assert!(
        Path::new(&dir).join("plugin.toml").is_file(),
        "{name} is missing"
    );
    let run_session = !manifest_only && needs.is_none_or(|(p, a)| has(p, a));
    let dir_s = dir.to_string_lossy().to_string();
    let mut args = vec![dir_s.as_str(), "--strict"];
    if !run_session {
        args.push("--manifest-only");
    }
    for locale in ["en", "de"] {
        let mut a = args.clone();
        a.extend(["--locale", locale]);
        let (ok, text) = harness(&a);
        assert!(
            ok,
            "{name} ({locale}) fails scripts/plugin-test.py:\n{text}"
        );
        if run_session {
            assert!(text.contains("exited"), "{text}");
        }
    }
}

#[test]
fn hello_python_passes_the_harness() {
    check("hello-python", None, false);
}

#[test]
fn hello_node_passes_the_harness() {
    check("hello-node", Some(("node", "--version")), false);
}

#[test]
fn hello_shell_passes_the_harness() {
    check("hello-shell", Some(("jq", "--version")), false);
}

#[test]
fn hello_rust_manifest_is_valid() {
    check("hello-rust", None, true);
}

/// plugins/registry.json (the community directory) is valid.
#[test]
fn community_registry_is_valid() {
    if !has("python3", "--version") {
        eprintln!("python3 not found, skipping");
        return;
    }
    let out = Command::new("python3")
        .arg("-B")
        .arg(repo().join("scripts/plugin-registry.py"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// scripts/plugin-pack.py packs a plugin folder into a .mantaplugin that
/// Manta's installer accepts, always with the same SHA-256, and refuses a
/// broken plugin.toml.
#[test]
fn plugin_pack_makes_an_installable_archive() {
    if !has("python3", "--version") {
        eprintln!("python3 not found, skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let pack = |src: &Path, out: &Path| {
        let o = Command::new("python3")
            .arg("-B")
            .arg(repo().join("scripts/plugin-pack.py"))
            .arg(src)
            .arg("-o")
            .arg(out)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).to_string(),
        )
    };
    let src = repo().join("examples/plugins/hello-shell");
    let (ok, text) = pack(&src, &tmp.path().join("a"));
    assert!(ok, "{text}");
    let file = tmp.path().join("a/hello-shell-0.1.0.mantaplugin");
    assert!(file.is_file(), "{text}");
    let sha = manta_plugin::registry::sha256_file(&file).unwrap();
    assert!(text.contains(&format!("sha256       {sha}")), "{text}");
    assert!(text.contains("\"latest\""), "{text}");
    // Reproducible.
    let (ok, _) = pack(&src, &tmp.path().join("b"));
    assert!(ok);
    let again =
        manta_plugin::registry::sha256_file(&tmp.path().join("b/hello-shell-0.1.0.mantaplugin"))
            .unwrap();
    assert_eq!(sha, again);
    // Manta installs it (review step), executable bit kept.
    let staging = tmp.path().join(".review-x");
    let r = manta_plugin::install::prepare(&file, &staging).unwrap();
    assert_eq!(
        (r.id.as_str(), r.version.as_str()),
        ("hello-shell", "0.1.0")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(r.root.join("main.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o100, 0);
    }
    // A broken manifest: nothing written.
    let bad = tmp.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("plugin.toml"), "id = \"Bad Id\"\n").unwrap();
    let (ok, text) = pack(&bad, &tmp.path().join("c"));
    assert!(!ok, "{text}");
    assert!(
        !tmp.path().join("c").exists()
            || std::fs::read_dir(tmp.path().join("c"))
                .unwrap()
                .next()
                .is_none()
    );
}
