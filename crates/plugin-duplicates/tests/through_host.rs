//! The Duplicates plugin through the real plugin host: context commands
//! only for one selected folder, a remote pane gets a toast, a scan finds
//! the groups, Select in Pane sends a `Select`, and trashing (with a fake
//! trash command from settings.json) moves exactly the extras, after the
//! confirmation, then refreshes the panes and reports. Nothing is ever
//! deleted permanently.

use std::path::Path;
use std::time::{Duration, Instant};

use manta_core::{Protocol, VfsPath};
use manta_plugin::host::{HostEvent, HostPaths, PluginHost};
use manta_plugin::{PaneId, PaneState, Plugin, ToastLevel};
use manta_plugin_duplicates::{lock, Action, DuplicatesPlugin, SharedRef, Target, ID};

fn state(pane: PaneId, dir: &Path, active: bool) -> PaneState {
    PaneState {
        remote: None,
        pane,
        path: VfsPath::new(&dir.to_string_lossy()),
        local_path: Some(dir.to_path_buf()),
        protocol: Protocol::Local,
        backend_label: "Local".into(),
        selection: Vec::new(),
        active,
    }
}

fn write(path: &Path, data: &[u8], mtime: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
    let t = std::time::UNIX_EPOCH + Duration::from_secs(mtime);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn until(
    host: &mut PluginHost,
    what: &str,
    mut done: impl FnMut(&mut PluginHost, &[HostEvent]) -> bool,
) -> Vec<HostEvent> {
    let start = Instant::now();
    let mut events = Vec::new();
    loop {
        events.extend(host.poll());
        if done(host, &events) {
            return events;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}; events {events:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A click in the panel: queued, then the panel is drawn once through the
/// host (as the app does every frame).
fn click(
    host: &mut PluginHost,
    clicks: &std::sync::Arc<std::sync::Mutex<Vec<Action>>>,
    pane: &PaneState,
    a: Action,
) {
    clicks.lock().unwrap().push(a);
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        host.panel_ui(ID, "duplicates", pane, ui)
    });
    out.textures_delta.clear();
}

fn menu(h: &PluginHost) -> Vec<String> {
    h.context_menu_commands()
        .map(|c| c.spec.title.clone())
        .collect()
}

#[test]
fn scans_selects_and_trashes_through_the_host() {
    manta_plugin::i18n::set_thread_lang(Some(manta_plugin::i18n::Lang::En));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let paths = HostPaths::under(&root.join("config"));
    let files = root.join("files");
    let trash = root.join("fake trash");
    let photo = vec![7u8; 300_000]; // over 128 KB: quick hash, then SHA-256
    let doc = vec![3u8; 5_000];
    write(&files.join("photo.jpg"), &photo, 1_000);
    write(&files.join("backup/photo.jpg"), &photo, 2_000);
    write(&files.join("backup/old/photo copy.jpg"), &photo, 3_000);
    write(&files.join("doc.pdf"), &doc, 5_000);
    write(&files.join("doc copy.pdf"), &doc, 4_000);
    let mut other = photo.clone();
    other[150_000] = 1; // same size and ends, other middle
    write(&files.join("other.jpg"), &other, 1_000);
    write(&files.join("node_modules/photo.jpg"), &photo, 1_000);
    write(&files.join("tiny-a.txt"), b"tiny", 1_000);
    write(&files.join("tiny-b.txt"), b"tiny", 1_000);

    // The trash command is injectable through settings.json: here it moves
    // the files into a folder of ours.
    let data_dir = paths.data_root.join(ID);
    std::fs::create_dir_all(&data_dir).unwrap();
    let settings = serde_json::json!({
        "trash_command": ["/bin/sh", "-c", "mkdir -p \"$0\" && mv \"$@\" \"$0\"/", trash.to_string_lossy()],
    });
    std::fs::write(data_dir.join("settings.json"), settings.to_string()).unwrap();

    let plugin = DuplicatesPlugin::new();
    let shared: SharedRef = plugin.shared();
    let clicks = plugin.click_queue();
    let builtins: Vec<Box<dyn Plugin>> = vec![Box::new(plugin)];
    let mut host = PluginHost::new(egui::Context::default(), builtins, Some(paths));
    let active = state(PaneId::Left, &files, true);
    host.set_pane(active.clone(), 1);
    host.set_pane(state(PaneId::Right, &root, false), 1);
    until(&mut host, "commands", |h, _| {
        h.commands()
            .iter()
            .any(|c| c.full_id == format!("{ID}:scan"))
    });
    assert!(
        menu(&host).is_empty(),
        "no folder selected: {:?}",
        menu(&host)
    );
    until(&mut host, "settings", |_, _| {
        !lock(&shared).settings.trash_command.is_empty()
    });

    // "Find Duplicates Here" is offered while one folder is selected.
    let mut on_folder = active.clone();
    on_folder.selection = vec![VfsPath::new(&files.join("backup").to_string_lossy())];
    host.set_pane(on_folder.clone(), 1);
    until(&mut host, "folder commands", |h, _| {
        menu(h).contains(&"Find Duplicates Here".to_string())
    });
    on_folder.selection = vec![VfsPath::new(&files.join("doc.pdf").to_string_lossy())];
    host.set_pane(on_folder, 1);
    until(&mut host, "not for files", |h, _| menu(h).is_empty());
    host.set_pane(active.clone(), 1);

    // Remote panes get a calm toast and the panel instead of a scan.
    let mut remote = state(PaneId::Left, Path::new("/var/www"), true);
    remote.local_path = None;
    remote.protocol = Protocol::Sftp;
    host.run_command(&format!("{ID}:scan"), &remote);
    until(&mut host, "remote toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Toast { level: ToastLevel::Info, text } if text.contains("local folders only")))
    });
    assert!(
        host.is_panel_open(ID, "duplicates", None),
        "the panel opens"
    );

    host.run_command(&format!("{ID}:scan"), &active);
    until(&mut host, "results", |_, _| lock(&shared).result.is_some());
    {
        let st = lock(&shared);
        let r = st.result.as_ref().unwrap();
        assert_eq!(
            r.totals(),
            (2, 3, 605_000),
            "photo ×3 (node_modules skipped, other.jpg differs) and doc ×2; tiny files below 1 KB"
        );
        // The oldest copy is kept: photo.jpg (1000) and doc copy.pdf (4000).
        let kept: Vec<&str> = r
            .groups
            .iter()
            .map(|g| g.files[st.kept(g)].rel.as_str())
            .collect();
        assert_eq!(kept, vec!["photo.jpg", "doc copy.pdf"]);
    }

    // Select in Pane: the extras in the scanned folder itself.
    let plugin_apply = |host: &mut PluginHost, a: Action| click(host, &clicks, &active, a);
    plugin_apply(&mut host, Action::Select(Target::All));
    let ev = until(&mut host, "select", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Select { .. }))
    });
    assert!(
        ev.contains(&HostEvent::Select {
            pane: PaneId::Left,
            dir: VfsPath::new(&files.to_string_lossy()),
            names: vec!["doc.pdf".into()]
        }),
        "{ev:?}"
    );

    // Trash everything: the confirmation first.
    plugin_apply(&mut host, Action::AskTrash(Target::All));
    until(&mut host, "confirm sheet", |h, _| {
        h.is_panel_open(ID, "confirm", None)
    });
    assert_eq!(
        lock(&shared)
            .pending
            .as_ref()
            .map(|w| w.iter().map(|(_, ex)| ex.len()).sum::<usize>()),
        Some(3)
    );
    plugin_apply(&mut host, Action::ConfirmTrash);
    let ev = until(&mut host, "result toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Toast { level: ToastLevel::Success, text } if text.contains("Moved 3 files")))
    });
    assert!(
        ev.iter()
            .any(|e| matches!(e, HostEvent::Refresh { pane: None })),
        "{ev:?}"
    );
    assert!(!host.is_panel_open(ID, "confirm", None));
    for kept in [
        "photo.jpg",
        "doc copy.pdf",
        "other.jpg",
        "node_modules/photo.jpg",
        "tiny-a.txt",
        "tiny-b.txt",
    ] {
        assert!(files.join(kept).exists(), "{kept} stays");
    }
    for gone in ["backup/photo.jpg", "backup/old/photo copy.jpg", "doc.pdf"] {
        assert!(!files.join(gone).exists(), "{gone} is trashed");
    }
    let mut trashed: Vec<String> = std::fs::read_dir(&trash)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    trashed.sort();
    assert_eq!(trashed, vec!["doc.pdf", "photo copy.jpg", "photo.jpg"]);
    assert!(
        lock(&shared).result.as_ref().unwrap().groups.is_empty(),
        "no duplicates left"
    );
    host.shutdown(Duration::from_secs(2));
}

#[test]
fn changed_files_are_left_alone() {
    manta_plugin::i18n::set_thread_lang(Some(manta_plugin::i18n::Lang::En));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let paths = HostPaths::under(&root.join("config"));
    let files = root.join("files");
    write(&files.join("a.bin"), &[1u8; 4_000], 1_000);
    write(&files.join("b.bin"), &[1u8; 4_000], 2_000);
    let data_dir = paths.data_root.join(ID);
    std::fs::create_dir_all(&data_dir).unwrap();
    // A "trash" that would fail loudly if it were ever called.
    std::fs::write(
        data_dir.join("settings.json"),
        r#"{"trash_command": ["/usr/bin/false"]}"#,
    )
    .unwrap();
    let plugin = DuplicatesPlugin::new();
    let shared = plugin.shared();
    let clicks = plugin.click_queue();
    let mut host = PluginHost::new(
        egui::Context::default(),
        vec![Box::new(plugin)],
        Some(paths),
    );
    let active = state(PaneId::Left, &files, true);
    host.set_pane(active.clone(), 1);
    until(&mut host, "settings", |_, _| {
        !lock(&shared).settings.trash_command.is_empty()
    });
    host.run_command(&format!("{ID}:scan"), &active);
    until(&mut host, "results", |_, _| lock(&shared).result.is_some());
    // b.bin changes after the scan: it must stay.
    write(&files.join("b.bin"), &[2u8; 4_000], 9_000);
    click(&mut host, &clicks, &active, Action::AskTrash(Target::All));
    click(&mut host, &clicks, &active, Action::ConfirmTrash);
    until(&mut host, "toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Toast { level: ToastLevel::Error, text } if text.contains("changed since the scan")))
    });
    assert!(files.join("a.bin").exists() && files.join("b.bin").exists());
    host.shutdown(Duration::from_secs(2));
}
