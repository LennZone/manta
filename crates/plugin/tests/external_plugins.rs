//! Runs real plugin processes: the example Python plugin (skipped when
//! `python3` isn't installed) and a crashing shell plugin.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use manta_core::{Protocol, VfsPath};
use manta_plugin::host::{HostEvent, HostPaths, PanelContent, PluginHost};
use manta_plugin::protocol::Block;
use manta_plugin::{PaneId, PaneState, PluginStatus, ToastLevel, Tone};

fn python_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

fn state(pane: PaneId, dir: &Path, selection: &[&str], active: bool) -> PaneState {
    PaneState {
        remote: None,
        pane,
        path: VfsPath::new(&dir.to_string_lossy()),
        local_path: Some(dir.to_path_buf()),
        protocol: Protocol::Local,
        backend_label: "Local".into(),
        selection: selection
            .iter()
            .map(|n| VfsPath::new(&dir.join(n).to_string_lossy()))
            .collect(),
        active,
    }
}

/// Poll the host until `done` returns true (or panic after 15 s).
fn until(
    host: &mut PluginHost,
    what: &str,
    mut done: impl FnMut(&mut PluginHost, &[HostEvent]) -> bool,
) {
    let start = Instant::now();
    let mut events = Vec::new();
    loop {
        events.extend(host.poll());
        if done(host, &events) {
            return;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let log = host
                .rows()
                .into_iter()
                .map(|r| format!("{}: {:?}\n  {}", r.info.id, r.status, r.log.join("\n  ")))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("timed out waiting for {what}; events {events:?}\n{log}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn hello_python_example() {
    if !python_available() {
        eprintln!("python3 not found, skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let paths = HostPaths::under(&tmp.path().join("config"));
    let example =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/hello-python");
    copy_dir(&example, &paths.plugins_dir.join("hello-python"));

    let folder = tmp.path().join("files");
    std::fs::create_dir_all(&folder).unwrap();
    let big = std::fs::File::create(folder.join("big.bin")).unwrap();
    big.set_len(11 * 1024 * 1024).unwrap();
    std::fs::write(folder.join("small.txt"), "hi").unwrap();

    let mut host = PluginHost::new(egui::Context::default(), Vec::new(), Some(paths.clone()));
    assert_eq!(host.rows().len(), 1);
    assert!(
        !host.rows()[0].enabled,
        "external plugins start switched off"
    );
    host.set_pane(state(PaneId::Left, &folder, &[], true), 1);
    host.set_pane(state(PaneId::Right, tmp.path(), &[], false), 1);
    host.set_enabled("hello-python", true);
    assert!(host.is_trusted("hello-python"));

    // Badge on the large file, chip with the item count.
    until(&mut host, "decorations", |h, _| {
        h.decorations(PaneId::Left).contains_key("big.bin")
            && h.chips(PaneId::Left)
                .iter()
                .any(|(_, c)| c.text == "2 items")
    });
    let d = &host.decorations(PaneId::Left)["big.bin"];
    assert_eq!(d.badges[0].text, "L");
    assert_eq!(d.badges[0].tone, Tone::Warning);
    assert!(!host.decorations(PaneId::Left).contains_key("small.txt"));
    assert_eq!(host.rows()[0].status, PluginStatus::Running);

    // Commands from plugin.toml, namespaced.
    let ids: Vec<String> = host.commands().iter().map(|c| c.full_id.clone()).collect();
    assert!(
        ids.contains(&"hello-python:say-hello".to_string()),
        "{ids:?}"
    );
    assert_eq!(host.context_menu_commands().count(), 1);

    let active = state(PaneId::Left, &folder, &["big.bin"], true);
    host.run_command("hello-python:say-hello", &active);
    until(&mut host, "toast", |_, ev| {
        ev.iter().any(|e| {
            matches!(e, HostEvent::Toast { level: ToastLevel::Success, text } if text.contains("Hello"))
        })
    });

    host.run_command("hello-python:file-info", &active);
    until(&mut host, "file info panel", |h, _| {
        h.is_panel_open("hello-python", "file-info", None)
            && matches!(
                h.panel_content("hello-python", "file-info", None),
                PanelContent::Blocks(b) if b.iter().any(|x| matches!(x, Block::KeyValue { .. }))
            )
    });
    host.panel_action("hello-python", "file-info", "close", &active);
    until(&mut host, "panel closed", |h, _| {
        !h.is_panel_open("hello-python", "file-info", None)
    });

    // Switching off ends the process quickly.
    let t = Instant::now();
    host.set_enabled("hello-python", false);
    assert!(
        t.elapsed() < Duration::from_millis(100),
        "disable must not block"
    );
    host.shutdown(Duration::from_secs(2));
    assert_eq!(host.rows()[0].status, PluginStatus::Stopped);
    assert!(host.decorations(PaneId::Left).is_empty());
    assert!(host.commands().is_empty());
    let log = std::fs::read_to_string(paths.data_root.join("hello-python/plugin.log")).unwrap();
    assert!(log.contains("hello-python started"), "{log}");
}

/// The Node and shell examples work in the sandbox with the permissions
/// they declare (each skipped when its runtime isn't installed).
#[test]
fn examples_run_in_the_sandbox() {
    for (example, needs) in [("hello-node", "node"), ("hello-shell", "jq")] {
        let found = std::process::Command::new(needs)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !found {
            eprintln!("{needs} not found, skipping {example}");
            continue;
        }
        let tmp = tempfile::tempdir().unwrap();
        let paths = HostPaths::under(&tmp.path().join("config"));
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/plugins")
            .join(example);
        copy_dir(&src, &paths.plugins_dir.join(example));
        let folder = tmp.path().join("files");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.txt"), "a").unwrap();
        std::fs::write(folder.join("b.txt"), "b").unwrap();
        let mut host = PluginHost::new(egui::Context::default(), Vec::new(), Some(paths));
        host.set_pane(state(PaneId::Left, &folder, &[], true), 1);
        host.set_enabled(example, true);
        until(&mut host, &format!("{example} chip"), |h, _| {
            h.chips(PaneId::Left)
                .iter()
                .any(|(_, c)| c.text == "2 items")
        });
        let row = &host.rows()[0];
        if cfg!(target_os = "macos") {
            assert_eq!(
                row.sandbox,
                Some(manta_plugin::sandbox::SandboxState::Enforced)
            );
            assert!(
                row.log.iter().any(|l| l.contains("sandboxed")),
                "{:?}",
                row.log
            );
        }
        host.shutdown(Duration::from_secs(2));
    }
}

#[test]
fn crashing_plugin_is_restarted_then_given_up() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = HostPaths::under(tmp.path());
    let dir = paths.plugins_dir.join("crasher");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        r#"id = "crasher"
name = "Crasher"
version = "1"
api_version = 1
command = ["/bin/sh", "-c", "echo 'something broke' >&2; exit 3"]
"#,
    )
    .unwrap();
    let mut host = PluginHost::new(egui::Context::default(), Vec::new(), Some(paths));
    host.set_enabled("crasher", true);
    until(
        &mut host,
        "error",
        |h, _| matches!(&h.rows()[0].status, PluginStatus::Error(m) if m.contains("not restarted")),
    );
    let row = &host.rows()[0];
    let PluginStatus::Error(msg) = &row.status else {
        unreachable!()
    };
    assert!(msg.contains("status 3"), "{msg}");
    assert!(row.log.iter().any(|l| l.contains("something broke")));
    let starts = row
        .log
        .iter()
        .filter(|l| l.contains("started (pid"))
        .count();
    assert_eq!(starts, 4, "first start plus 3 restarts: {:?}", row.log);
}

#[test]
fn missing_program_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = HostPaths::under(tmp.path());
    let dir = paths.plugins_dir.join("nope");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        "id = \"nope\"\nname = \"Nope\"\nversion = \"1\"\napi_version = 1\n\
         command = [\"definitely-not-a-program-manta\"]\n",
    )
    .unwrap();
    let mut host = PluginHost::new(egui::Context::default(), Vec::new(), Some(paths));
    host.set_enabled("nope", true);
    until(
        &mut host,
        "error",
        |h, _| matches!(&h.rows()[0].status, PluginStatus::Error(m) if m.contains("not found")),
    );
}
