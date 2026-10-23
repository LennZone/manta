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
