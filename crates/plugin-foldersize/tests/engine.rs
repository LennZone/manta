//! The plugin end to end: events in, `SetSizes` out.

use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{
    FileAction, FileActionKind, FileActionPhase, HostRequest, PaneId, Plugin, PluginEvent,
    PluginHandle, SizeInfo,
};
use manta_plugin_foldersize::{pane_state, FolderSizePlugin, CALCULATE_CMD};

fn file(p: &Path, len: usize) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, vec![b'x'; len]).unwrap();
}

fn start(data: &Path) -> (FolderSizePlugin, Receiver<(String, HostRequest)>) {
    let (tx, rx) = mpsc::channel();
    let h = PluginHandle::new(
        "manta.foldersize".into(),
        tx,
        egui::Context::default(),
        data.to_path_buf(),
    );
    let mut p = FolderSizePlugin::new();
    p.enable(h);
    (p, rx)
}

/// Collect `SetSizes` until `done` holds for the merged map (or time out).
fn collect(
    rx: &Receiver<(String, HostRequest)>,
    sizes: &mut std::collections::HashMap<String, SizeInfo>,
    mut done: impl FnMut(&std::collections::HashMap<String, SizeInfo>) -> bool,
) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if let Ok((_, HostRequest::SetSizes { sizes: s, .. })) =
            rx.recv_timeout(Duration::from_millis(50))
        {
            sizes.extend(s);
        }
        if done(sizes) {
            return true;
        }
    }
    false
}

#[test]
fn sizes_for_subfolders_cache_and_invalidate() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    file(&root.join("small/a.txt"), 1_000);
    file(&root.join("big/x/y.bin"), 300_000);
    file(&root.join("plain.txt"), 5);
    let (mut p, rx) = start(data.path());
    let s = pane_state(PaneId::Left, Some(&root));
    p.on_event(&PluginEvent::DirectoryChanged(s.clone()));
    let mut sizes = Default::default();
    assert!(collect(&rx, &mut sizes, |m| m
        .get("big")
        .is_some_and(|s| s.complete)
        && m.get("small").is_some_and(|s| s.complete)));
    assert_eq!(sizes["big"].bytes, 300_000);
    assert_eq!(sizes["big"].files, 1);
    assert!(sizes["big"].on_disk.unwrap() > 0);
    assert_eq!(sizes["small"].bytes, 1_000);
    assert!(
        !sizes.contains_key("plain.txt"),
        "files keep their own size"
    );

    // Navigating back answers from the cache at once.
    p.on_event(&PluginEvent::DirectoryChanged(pane_state(
        PaneId::Left,
        Some(tmp.path()),
    )));
    p.on_event(&PluginEvent::DirectoryChanged(s.clone()));
    let t = Instant::now();
    let mut again = Default::default();
    assert!(collect(&rx, &mut again, |m| m
        .get("big")
        .is_some_and(|s| s.complete && s.bytes == 300_000)));
    assert!(t.elapsed() < Duration::from_secs(2));

    // A copy into big/x invalidates big.
    file(&root.join("big/x/more.bin"), 50_000);
    let mut a = FileAction::new(
        1,
        PaneId::Right,
        FileActionKind::Copy,
        vec![VfsPath::new("/elsewhere/more.bin")],
    );
    a.destination = Some(VfsPath::new(&root.join("big/x").to_string_lossy()));
    a.phase = FileActionPhase::Finished {
        ok: true,
        files: 1,
        bytes: 50_000,
        errors: vec![],
        cancelled: false,
        elapsed_ms: 1,
    };
    p.on_event(&PluginEvent::FileAction(a));
    let mut after = Default::default();
    assert!(collect(&rx, &mut after, |m| m
        .get("big")
        .is_some_and(|s| s.complete && s.bytes == 350_000)));
    p.disable();
}

#[test]
fn on_demand_mode_measures_only_when_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    fs::write(data.path().join("settings.json"), r#"{"auto":false}"#).unwrap();
    let root = tmp.path().join("root");
    file(&root.join("one/a.bin"), 10);
    file(&root.join("two/b.bin"), 20);
    let (mut p, rx) = start(data.path());
    assert!(!p.settings().auto);
    assert!(p.commands().iter().any(|c| c.id == CALCULATE_CMD));
    let mut s = pane_state(PaneId::Left, Some(&root));
    p.on_event(&PluginEvent::DirectoryChanged(s.clone()));
    let mut sizes = Default::default();
    assert!(
        !collect_for(&rx, &mut sizes, Duration::from_millis(600)),
        "nothing measured automatically"
    );
    s.selection = vec![VfsPath::new(&root.join("two").to_string_lossy())];
    p.run_command(CALCULATE_CMD, &s);
    assert!(collect(&rx, &mut sizes, |m| m
        .get("two")
        .is_some_and(|s| s.complete)));
    assert_eq!(sizes["two"].bytes, 20);
    assert!(!sizes.contains_key("one"));
    p.disable();
}

fn collect_for(
    rx: &Receiver<(String, HostRequest)>,
    sizes: &mut std::collections::HashMap<String, SizeInfo>,
    d: Duration,
) -> bool {
    let t = Instant::now();
    let mut any = false;
    while t.elapsed() < d {
        if let Ok((_, HostRequest::SetSizes { sizes: s, .. })) =
            rx.recv_timeout(Duration::from_millis(20))
        {
            any |= !s.is_empty();
            sizes.extend(s);
        }
    }
    any
}

#[test]
fn moving_away_cancels_stale_work() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    // Enough files that a walk takes a moment.
    for i in 0..30 {
        for j in 0..200 {
            file(&root.join(format!("heavy/d{i}/f{j}")), 1);
        }
    }
    let other = tmp.path().join("other");
    file(&other.join("tiny/a"), 1);
    let (mut p, rx) = start(data.path());
    p.on_event(&PluginEvent::DirectoryChanged(pane_state(
        PaneId::Left,
        Some(&root),
    )));
    p.on_event(&PluginEvent::DirectoryChanged(pane_state(
        PaneId::Left,
        Some(&other),
    )));
    // Results for the new folder come; whatever arrives for the old folder
    // is addressed to it (the host drops it as stale).
    let other_v = VfsPath::new(&other.to_string_lossy());
    let t = Instant::now();
    let mut got_tiny = false;
    while t.elapsed() < Duration::from_secs(10) && !got_tiny {
        if let Ok((_, HostRequest::SetSizes { dir, sizes, .. })) =
            rx.recv_timeout(Duration::from_millis(50))
        {
            if dir == other_v {
                got_tiny = sizes.iter().any(|(n, s)| n == "tiny" && s.complete);
            } else {
                assert!(sizes.iter().all(|(n, _)| n == "heavy"));
            }
        }
    }
    assert!(got_tiny);
    p.disable();
}

#[test]
fn remote_panes_get_nothing() {
    let data = tempfile::tempdir().unwrap();
    let (mut p, rx) = start(data.path());
    p.on_event(&PluginEvent::DirectoryChanged(pane_state(
        PaneId::Right,
        None,
    )));
    let mut sizes = Default::default();
    assert!(!collect_for(&rx, &mut sizes, Duration::from_millis(300)));
    p.disable();
}

/// Sizes from the last run show at once and are then re-measured quietly
/// (something deeper inside may have changed); measured sizes are saved.
#[test]
fn sizes_survive_a_restart_and_are_checked_again() {
    use manta_plugin_foldersize::{store, walker};
    let tmp = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    // Enough files that the re-measure takes a moment.
    for i in 0..2_000 {
        file(&root.join(format!("big/x/{i}.bin")), 100);
    }
    let big = root.join("big");
    let stamp = walker::stamp(&fs::metadata(&big).unwrap());
    // What the last run measured (wrong by now: the folder grew inside).
    let mut old = std::collections::HashMap::new();
    let mut totals = walker::Totals {
        logical: 123,
        files: 1,
        done: true,
        ..Default::default()
    };
    totals.by_cat[0] = 123;
    old.insert(
        big.clone(),
        store::Stored {
            stamp,
            totals,
            measured: 1,
        },
    );
    let cache = data.path().join(store::FILE);
    store::save(&cache, old.iter()).unwrap();

    let (mut p, rx) = start(data.path());
    p.on_event(&PluginEvent::DirectoryChanged(pane_state(
        PaneId::Left,
        Some(&root),
    )));
    let mut seen = Vec::new();
    let mut sizes = Default::default();
    assert!(collect(&rx, &mut sizes, |m| {
        if let Some(s) = m.get("big") {
            if seen.last() != Some(&s.bytes) {
                seen.push(s.bytes);
            }
        }
        m.get("big").is_some_and(|s| s.bytes == 200_000)
    }));
    assert_eq!(seen.first(), Some(&123), "the saved size first: {seen:?}");
    assert!(sizes["big"].complete);

    // The new size is saved (a few seconds later, or when the plugin stops).
    p.disable();
    let t = Instant::now();
    while store::load(&cache)
        .get(&big)
        .is_none_or(|s| s.totals.logical != 200_000)
    {
        assert!(t.elapsed() < Duration::from_secs(10), "never saved");
        std::thread::sleep(Duration::from_millis(20));
    }
}
