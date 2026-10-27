//! The built-in Quick Actions plugin driven through the real plugin host:
//! menu filtering, runs and their toasts, the panel, the confirm sheet,
//! live reload, at most two parallel jobs, Cancel killing the whole process
//! group, the language switch and the migration from the Python plugin.

use std::path::Path;
use std::time::{Duration, Instant};

use manta_core::{Protocol, VfsPath};
use manta_plugin::host::{HostEvent, HostPaths, PanelContent, PluginHost};
use manta_plugin::protocol::Block;
use manta_plugin::{PaneId, PaneState, Plugin, ToastLevel};
use manta_plugin_quickactions::{QuickActionsPlugin, PLUGIN_ID as ID};

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

/// Poll until `done` (events so far are passed in), at most 20 s.
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

fn toast_with<'a>(ev: &'a [HostEvent], level: ToastLevel, needle: &str) -> Option<&'a str> {
    ev.iter().find_map(|e| match e {
        HostEvent::Toast { level: l, text } if *l == level && text.contains(needle) => {
            Some(text.as_str())
        }
        _ => None,
    })
}

fn has_cmd(h: &PluginHost, id: &str) -> bool {
    h.commands()
        .iter()
        .any(|c| c.full_id == format!("{ID}:{id}"))
}

fn panel_json(h: &PluginHost, panel: &str) -> String {
    match h.panel_content(ID, panel, None) {
        PanelContent::Blocks(b) => serde_json::to_string(&*b).unwrap(),
        _ => String::new(),
    }
}

struct Setup {
    _tmp: tempfile::TempDir,
    paths: HostPaths,
    files: std::path::PathBuf,
    host: PluginHost,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let paths = HostPaths::under(&tmp.path().join("config"));
    let files = tmp.path().join("files");
    let other = tmp.path().join("other side");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(files.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(files.join("it's b.txt"), "x\n").unwrap();
    std::fs::create_dir(files.join("folder")).unwrap();
    let builtins: Vec<Box<dyn Plugin>> = vec![Box::new(QuickActionsPlugin::new())];
    let mut host = PluginHost::new(egui::Context::default(), builtins, Some(paths.clone()));
    host.set_pane(
        state(PaneId::Left, &files, &["a.txt", "it's b.txt"], true),
        1,
    );
    host.set_pane(state(PaneId::Right, &other, &[], false), 1);
    Setup {
        _tmp: tmp,
        paths,
        files,
        host,
    }
}

fn actions_file(s: &Setup) -> std::path::PathBuf {
    s.paths.data_root.join(ID).join("actions.json")
}

#[test]
fn count_lines_and_friends_through_the_host() {
    let mut s = setup();
    let files = s.files.clone();
    let host = &mut s.host;
    assert!(host.wants_enabled(ID), "built-ins start switched on");

    // The actions for the selection show up in the context menu, filtered.
    until(host, "commands", |h, _| has_cmd(h, "run.count-lines"));
    let menu: Vec<String> = host
        .context_menu_commands()
        .map(|c| c.spec.title.clone())
        .collect();
    assert!(menu.contains(&"Count Lines".to_string()), "{menu:?}");
    assert!(
        !menu.contains(&"Resize Images to 50%".to_string()),
        "not for text files: {menu:?}"
    );
    assert!(
        !menu.iter().any(|t| t.starts_with("Edit Quick Actions")),
        "palette only: {menu:?}"
    );
    assert!(actions_file(&s).exists(), "defaults written on first run");
    let host = &mut s.host;

    // Selecting a folder: Count Lines (files only) goes away.
    host.set_pane(state(PaneId::Left, &files, &["folder"], true), 1);
    until(host, "filtered commands", |h, _| {
        !has_cmd(h, "run.count-lines")
    });
    let sel = ["a.txt", "it's b.txt"];
    host.set_pane(state(PaneId::Left, &files, &sel, true), 1);
    until(host, "commands back", |h, _| has_cmd(h, "run.count-lines"));

    // Run it: the result is a toast.
    let active = state(PaneId::Left, &files, &sel, true);
    host.run_command(&format!("{ID}:run.count-lines"), &active);
    let ev = until(host, "count toast", |_, ev| {
        toast_with(ev, ToastLevel::Success, "total").is_some()
    });
    let text = toast_with(&ev, ToastLevel::Success, "total").unwrap();
    assert!(
        text.contains("3 a.txt") && text.contains("1 it's b.txt") && text.contains("4 total"),
        "{text}"
    );

    // The panel lists the run.
    until(host, "panel with the run", |h, _| {
        match h.panel_content(ID, "actions", None) {
        PanelContent::Blocks(b) => b.iter().any(|x| {
            matches!(x, Block::List { rows } if rows.iter().any(|r| r.text == "Count Lines" && r.action.as_deref().is_some_and(|a| a.starts_with("show:"))))
        }),
        _ => false,
    }
    });
    assert!(
        panel_json(host, "actions").contains("$ wc -l a.txt 'it'\\\"'\\\"'s b.txt'"),
        "{}",
        panel_json(host, "actions")
    );

    // Not applicable from the palette: a clear toast, nothing runs.
    host.run_command(&format!("{ID}:run.resize-50"), &active);
    until(host, "not applicable", |_, ev| {
        toast_with(ev, ToastLevel::Warning, "isn't available here").is_some()
    });

    // A terminal action asks first (modal), then goes to the terminal,
    // which is off here: the host says so.
    host.run_command(&format!("{ID}:run.rsync-other"), &active);
    until(host, "confirm sheet", |h, _| {
        h.is_panel_open(ID, "confirm", None)
    });
    assert!(
        panel_json(host, "confirm").contains("Copy 2 items to the other pane"),
        "{}",
        panel_json(host, "confirm")
    );
    host.panel_action(ID, "confirm", "confirm:run", &active);
    let ev = until(host, "terminal off toast", |_, ev| {
        toast_with(ev, ToastLevel::Warning, "Terminal plugin is off").is_some()
    });
    assert!(
        toast_with(&ev, ToastLevel::Warning, "rsync -a --progress").is_some(),
        "{ev:?}"
    );
    assert!(!host.is_panel_open(ID, "confirm", None));

    // Cancel in the sheet: nothing runs.
    host.run_command(&format!("{ID}:run.rsync-other"), &active);
    until(host, "confirm sheet again", |h, _| {
        h.is_panel_open(ID, "confirm", None)
    });
    host.panel_action(ID, "confirm", "confirm:cancel", &active);
    let ev = until(host, "closed", |h, _| !h.is_panel_open(ID, "confirm", None));
    std::thread::sleep(Duration::from_millis(200));
    let mut ev = ev;
    ev.extend(host.poll());
    assert!(
        toast_with(&ev, ToastLevel::Warning, "rsync").is_none(),
        "{ev:?}"
    );

    // A failing command: error toast with its stderr. actions.json is
    // reloaded by itself (mtime watch) with a toast.
    std::fs::write(
        actions_file(&s),
        r#"{"actions": [{"id": "boom", "title": "Boom", "command": "echo 'it broke' >&2; exit 4"}, // comment
        ]}"#,
    )
    .unwrap();
    let host = &mut s.host;
    let ev = until(host, "reload", |h, _| has_cmd(h, "run.boom"));
    let ev = if toast_with(&ev, ToastLevel::Success, "Loaded 1 quick action").is_some() {
        ev
    } else {
        until(host, "reload toast", |_, e| {
            toast_with(e, ToastLevel::Success, "Loaded 1 quick action").is_some()
        })
    };
    assert!(
        toast_with(&ev, ToastLevel::Success, "Loaded 1 quick action").is_some(),
        "{ev:?}"
    );
    host.run_command(&format!("{ID}:run.boom"), &active);
    let ev = until(host, "error toast", |_, ev| {
        toast_with(ev, ToastLevel::Error, "Boom").is_some()
    });
    assert!(
        toast_with(&ev, ToastLevel::Error, "it broke").is_some(),
        "{ev:?}"
    );

    // A broken file keeps the old actions and says why.
    std::thread::sleep(Duration::from_millis(1100)); // a new mtime
    std::fs::write(actions_file(&s), "{ not json").unwrap();
    let host = &mut s.host;
    until(host, "broken file", |_, ev| {
        toast_with(ev, ToastLevel::Error, "isn't valid JSON").is_some()
    });
    assert!(has_cmd(host, "run.boom"));
    assert!(panel_json(host, "actions").contains("actions.json: actions.json isn't valid JSON"));
    host.shutdown(Duration::from_secs(1));
}

/// The processes listed in `file` (one PID per line).
fn pids_in(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `pid` still exists (`kill -0`).
fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn two_jobs_at_a_time_and_cancel_kills_the_group() {
    let mut s = setup();
    let marker = s.files.join("survivor");
    let pids = s.files.join("pids");
    // A background child in the job's process group: it notes its PID and
    // would touch the marker much later. Checking that the PIDs are gone
    // (not a short sleep racing the cancel) keeps this independent of load.
    let quote = manta_plugin_quickactions::core::shell_quote;
    let child = format!(
        "echo $$ >> {p}; sleep 60; touch {m}",
        p = quote(&pids.to_string_lossy()),
        m = quote(&marker.to_string_lossy())
    );
    let cmd = format!("sh -c {c} & sleep 60", c = quote(&child));
    let json = serde_json::json!({"actions": [
        {"id": "slow", "title": "Slow", "command": cmd},
    ]});
    // After the first load: it writes the defaults when there is no file,
    // which could overwrite ours (written between its check and its write).
    until(&mut s.host, "default actions", |h, _| {
        has_cmd(h, "run.count-lines")
    });
    std::fs::write(actions_file(&s), json.to_string()).unwrap();
    let files = s.files.clone();
    let host = &mut s.host;
    until(host, "slow action", |h, _| has_cmd(h, "run.slow"));
    let active = state(PaneId::Left, &files, &["a.txt"], true);
    for _ in 0..3 {
        host.run_command(&format!("{ID}:run.slow"), &active);
    }
    until(host, "two running, one waiting", |h, _| {
        let p = panel_json(h, "actions");
        p.matches("running ·").count() == 2 && p.contains("\"waiting\"")
    });
    // Both running jobs started their background child.
    until(host, "background children", |_, _| {
        pids_in(&pids).len() >= 2
    });
    // The progress toast offers Cancel.
    let ev = until(host, "progress toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::ActionToast { text, button, .. } if text.contains("Running “Slow” on 1 item") && button == "Cancel"))
    });
    let cancel = ev
        .iter()
        .find_map(|e| match e {
            HostEvent::ActionToast { command, .. } => Some(command.clone()),
            _ => None,
        })
        .unwrap();
    // Cancel all three (the toast's command for one, the panel for the rest).
    host.run_command(&cancel, &active);
    for id in 1..=3 {
        host.panel_action(ID, "actions", &format!("cancel:{id}"), &active);
    }
    until(host, "cancelled, with a toast", |h, ev| {
        let p = panel_json(h, "actions");
        p.matches("\"cancelled\"").count() == 3
            && toast_with(ev, ToastLevel::Info, "Cancelled “Slow”").is_some()
    });
    // The background children were in the jobs' process groups: they are
    // gone too and never touch the marker.
    let start = Instant::now();
    loop {
        let left: Vec<String> = pids_in(&pids).into_iter().filter(|p| alive(p)).collect();
        if left.is_empty() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the whole process group was stopped; still running: {left:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!marker.exists(), "the whole process group was stopped");
    host.shutdown(Duration::from_secs(1));
}

#[test]
fn output_panel_refresh_and_titles_follow_the_language() {
    let mut s = setup();
    let host = &mut s.host;
    let files = s.files.clone();
    until(host, "commands", |h, _| has_cmd(h, "run.sha256"));
    let active = state(PaneId::Left, &files, &["a.txt"], true);
    host.run_command(&format!("{ID}:run.sha256"), &active);
    until(host, "panel opened with the output", |h, _| {
        h.is_panel_open(ID, "actions", None) && panel_json(h, "actions").contains("  a.txt")
    });

    // German titles, live.
    manta_plugin::i18n::set_thread_lang(Some(manta_plugin::i18n::Lang::De));
    host.deliver(ID, manta_plugin::PluginEvent::LocaleChanged("de".into()));
    host.refresh_infos();
    let title = |h: &PluginHost, id: &str| {
        h.commands()
            .iter()
            .find(|c| c.full_id == format!("{ID}:{id}"))
            .map(|c| c.spec.title.clone())
    };
    until(host, "German titles", |h, _| {
        title(h, "run.count-lines").as_deref() == Some("Zeilen zählen")
            && title(h, "edit").as_deref() == Some("Schnellaktionen bearbeiten …")
    });
    assert!(panel_json(host, "actions").contains("Zuletzt ausgeführt"));
    assert!(host
        .rows()
        .iter()
        .any(|r| r.info.id == ID && r.info.name == "Schnellaktionen"));
    manta_plugin::i18n::set_thread_lang(None);
    host.deliver(ID, manta_plugin::PluginEvent::LocaleChanged("en".into()));
    until(host, "English titles", |h, _| {
        title(h, "run.count-lines").as_deref() == Some("Count Lines")
    });
    host.shutdown(Duration::from_secs(1));
}

/// The Python plugin's actions.json (with the user's edits) is taken over.
#[test]
fn migrates_the_python_plugins_actions() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = HostPaths::under(&tmp.path().join("config"));
    let old = paths.data_root.join("quick-actions");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(
        old.join("actions.json"),
        r#"// mine
        {"actions": [{"id": "mine", "title": "Mine", "title_de": "Meins", "command": "true",},]}"#,
    )
    .unwrap();
    let files = tmp.path().join("f");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("x"), "").unwrap();
    let builtins: Vec<Box<dyn Plugin>> = vec![Box::new(QuickActionsPlugin::new())];
    let mut host = PluginHost::new(egui::Context::default(), builtins, Some(paths.clone()));
    host.set_pane(state(PaneId::Left, &files, &["x"], true), 1);
    until(&mut host, "the user's action", |h, _| {
        has_cmd(h, "run.mine")
    });
    assert!(
        !host
            .commands()
            .iter()
            .any(|c| c.full_id.ends_with("run.count-lines")),
        "not the defaults"
    );
    let text = std::fs::read_to_string(paths.data_root.join(ID).join("actions.json")).unwrap();
    assert!(text.starts_with("// mine"));
    host.shutdown(Duration::from_secs(1));
}
