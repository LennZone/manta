//! The built-in Project Info plugin driven through the real plugin host,
//! pointed at the Laravel fixture. A fake built-in Terminal records what
//! would be typed. Chips, panel, picker, the confirm sheet, recent runs,
//! ⌥⌘R, manifest polling, the language switch and the migration of the
//! Python plugin's recent.json.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use manta_core::{Protocol, VfsPath};
use manta_plugin::host::{HostEvent, HostPaths, PanelContent, PluginHost, TERMINAL_PLUGIN};
use manta_plugin::{
    PaneId, PaneState, PanelPlacement, PanelSpec, Plugin, PluginEvent, PluginHandle, PluginInfo,
    ToastLevel,
};
use manta_plugin_projectinfo::{ProjectInfoPlugin, PLUGIN_ID as ID};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).unwrap();
        }
    }
}

fn local(pane: PaneId, dir: &Path, active: bool, selection: &[&str]) -> PaneState {
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

type Typed = Arc<Mutex<Vec<(PaneId, String, Option<String>)>>>;

/// Stands in for the built-in Terminal: records `RunInTerminal`.
struct FakeTerminal(Typed);

impl Plugin for FakeTerminal {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: TERMINAL_PLUGIN.into(),
            name: "Terminal".into(),
            version: "1.0.0".into(),
            description: String::new(),
            author: String::new(),
            builtin: true,
        }
    }
    fn enable(&mut self, _h: PluginHandle) {}
    fn disable(&mut self) {}
    fn panels(&self) -> Vec<PanelSpec> {
        vec![PanelSpec {
            id: "terminal".into(),
            title: "Terminal".into(),
            placement: PanelPlacement::PaneBottom,
            default_open: false,
        }]
    }
    fn on_event(&mut self, ev: &PluginEvent) {
        if let PluginEvent::RunInTerminal { pane, command, cwd } = ev {
            self.0.lock().unwrap().push((
                *pane,
                command.clone(),
                cwd.as_ref().map(|c| c.as_str().to_string()),
            ));
        }
    }
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

fn panel(h: &PluginHost, panel: &str) -> String {
    match h.panel_content(ID, panel, None) {
        PanelContent::Blocks(b) => serde_json::to_string(&*b).unwrap(),
        _ => String::new(),
    }
}

struct Setup {
    _tmp: tempfile::TempDir,
    paths: HostPaths,
    shop: PathBuf,
    ui: PathBuf,
    typed: Typed,
    host: PluginHost,
}

fn setup(prepare: impl FnOnce(&HostPaths, &Path)) -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let sites = tmp.path().join("Sites");
    let shop = sites.join("acme-shop");
    let ui = sites.join("storefront-ui");
    copy_dir(&fixtures.join("acme-shop"), &shop);
    copy_dir(&fixtures.join("storefront-ui"), &ui);
    std::fs::create_dir_all(shop.join(".git")).unwrap();
    std::fs::write(shop.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let paths = HostPaths::under(&tmp.path().join("config"));
    prepare(&paths, &shop);
    let typed: Typed = Arc::default();
    let builtins: Vec<Box<dyn Plugin>> = vec![
        Box::new(FakeTerminal(typed.clone())),
        Box::new(ProjectInfoPlugin::new().with_home(&tmp.path().to_string_lossy())),
    ];
    let mut host = PluginHost::new(egui::Context::default(), builtins, Some(paths.clone()));
    host.set_pane(local(PaneId::Left, &shop, true, &[]), 1);
    host.set_pane(local(PaneId::Right, &ui, false, &[]), 1);
    Setup {
        _tmp: tmp,
        paths,
        shop,
        ui,
        typed,
        host,
    }
}

fn chip(h: &PluginHost, pane: PaneId) -> Option<manta_plugin::Chip> {
    h.chips(pane)
        .into_iter()
        .find(|(id, _)| id == ID)
        .map(|(_, c)| c)
}

#[test]
fn chips_panel_runs_and_recent() {
    let mut s = setup(|_, _| {});
    let (shop, ui) = (s.shop.clone(), s.ui.clone());
    let host = &mut s.host;
    until(host, "chips", |h, _| {
        chip(h, PaneId::Left).is_some() && chip(h, PaneId::Right).is_some()
    });
    let c = chip(host, PaneId::Left).unwrap();
    assert_eq!(c.text, "Laravel · 28 scripts");
    assert_eq!(c.icon.as_deref(), Some("rocket"));
    assert_eq!(c.command.as_deref(), Some("toggle"));
    assert!(
        c.tooltip.as_deref().unwrap().contains("~/Sites/acme-shop"),
        "{c:?}"
    );
    assert_eq!(chip(host, PaneId::Right).unwrap().text, "pnpm · 11 scripts");

    // The chip toggles the panel.
    let active = local(PaneId::Left, &shop, true, &[]);
    host.run_command(&format!("{ID}:toggle"), &active);
    host.poll();
    assert!(host.is_panel_open(ID, "project", None));
    let p = panel(host, "project");
    for want in [
        "\"acme-shop\"",
        "on main",
        "Laravel 11",
        "Open in VS Code",
        ".env",
        "php artisan serve",
        "Docker Compose",
    ] {
        assert!(p.contains(want), "{want} in {p}");
    }
    assert!(!p.contains("Go to Root"), "the pane is at the root");

    // A safe script runs right away in the terminal, cd'd to the root.
    let action = format!("run\t{}\tlaravel/migrate", shop.display());
    host.panel_action(ID, "project", &action, &active);
    host.poll();
    let typed = s.typed.lock().unwrap().clone();
    assert_eq!(
        typed,
        vec![(
            PaneId::Left,
            "php artisan migrate".to_string(),
            Some(shop.to_string_lossy().into_owned())
        )]
    );
    let host = &mut s.host;
    assert!(
        host.is_panel_open(TERMINAL_PLUGIN, "terminal", Some(PaneId::Left)),
        "the terminal opened"
    );
    // It shows up under Recent and is saved.
    until(host, "recent", |h, _| {
        panel(h, "project").contains("\"Recent\"")
    });
    let recent = s.paths.data_root.join(ID).join("recent.json");
    let host = &mut s.host;
    until(host, "recent.json", |_, _| {
        std::fs::read_to_string(&recent).is_ok_and(|t| t.contains("php artisan migrate"))
    });

    // A destructive one asks first; Cancel runs nothing.
    let fresh = format!("run\t{}\tlaravel/migrate:fresh --seed", shop.display());
    host.panel_action(ID, "project", &fresh, &active);
    host.poll();
    assert!(host.is_panel_open(ID, "confirm", None));
    let c = panel(host, "confirm");
    assert!(
        c.contains("Run “migrate:fresh --seed”?") && c.contains("drops database tables"),
        "{c}"
    );
    host.panel_action(ID, "confirm", "cancel", &active);
    host.poll();
    assert!(!host.is_panel_open(ID, "confirm", None));
    assert_eq!(s.typed.lock().unwrap().len(), 1);
    // … and Run runs it.
    let host = &mut s.host;
    host.panel_action(ID, "project", &fresh, &active);
    host.panel_action(ID, "confirm", "confirm", &active);
    host.poll();
    assert_eq!(
        s.typed.lock().unwrap().last().unwrap().1,
        "php artisan migrate:fresh --seed"
    );

    // ⌥⌘R re-runs the newest (destructive: asks again).
    let host = &mut s.host;
    let rerun = host
        .commands()
        .iter()
        .find(|c| c.full_id == format!("{ID}:rerun"))
        .unwrap()
        .clone();
    assert_eq!(rerun.spec.shortcut.as_deref(), Some("⌥⌘R"));
    host.run_command(&rerun.full_id, &active);
    host.poll();
    assert!(host.is_panel_open(ID, "confirm", None));
    host.panel_action(ID, "confirm", "confirm", &active);
    host.poll();
    assert_eq!(s.typed.lock().unwrap().len(), 3);

    // The picker lists every script; a click closes it and runs.
    let host = &mut s.host;
    host.run_command(&format!("{ID}:run"), &active);
    host.poll();
    assert!(host.is_panel_open(ID, "picker", None));
    let pk = panel(host, "picker");
    assert!(
        pk.contains("Run a script in acme-shop") && pk.contains("composer run-script lint"),
        "{pk}"
    );
    host.panel_action(
        ID,
        "picker",
        &format!("recent\t{}\t1", shop.display()),
        &active,
    );
    host.poll();
    assert!(!host.is_panel_open(ID, "picker", None));
    assert_eq!(
        s.typed.lock().unwrap().last().unwrap().1,
        "php artisan migrate"
    );

    // The other pane (Node project) when it becomes active; a gone script.
    let host = &mut s.host;
    host.set_pane(local(PaneId::Left, &shop, false, &[]), 1);
    host.set_pane(local(PaneId::Right, &ui, true, &[]), 1);
    until(host, "node panel", |h, _| {
        panel(h, "project").contains("\"storefront-ui\"")
    });
    host.panel_action(
        ID,
        "project",
        &format!("run\t{}\tnode/nope", ui.display()),
        &local(PaneId::Right, &ui, true, &[]),
    );
    until(host, "gone toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Toast { level: ToastLevel::Warning, text } if text.contains("script is gone")))
    });
    // Rerun with nothing run there yet.
    host.run_command(
        &format!("{ID}:rerun"),
        &local(PaneId::Right, &ui, true, &[]),
    );
    until(host, "nothing run toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Toast { text, .. } if text.contains("Nothing run in storefront-ui yet")))
    });
    host.shutdown(Duration::from_secs(1));
}

#[test]
fn subfolders_manifest_polling_and_show_scripts() {
    let mut s = setup(|_, _| {});
    let shop = s.shop.clone();
    let host = &mut s.host;
    let sub = shop.join("app/Http");
    host.set_pane(local(PaneId::Left, &sub, true, &[]), 2);
    until(host, "subfolder chip", |h, _| {
        chip(h, PaneId::Left).is_some_and(|c| c.text.starts_with("Laravel"))
    });
    // The panel re-renders right after the chip; wait for it under load too.
    until(host, "go to root button", |h, _| {
        panel(h, "project").contains("Go to Root")
    });
    host.panel_action(
        ID,
        "project",
        &format!("goto\t{}", shop.display()),
        &local(PaneId::Left, &sub, true, &[]),
    );
    // The click is handled off the UI thread: wait for the navigation.
    until(host, "navigate to root", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Navigate { pane: PaneId::Left, path } if path.as_str() == shop.to_string_lossy()))
    });

    // A new composer script appears within a second or two.
    host.set_pane(local(PaneId::Left, &shop, true, &["composer.json"]), 3);
    until(host, "show scripts", |h, _| {
        h.context_menu_commands()
            .any(|c| c.full_id == format!("{ID}:show-scripts"))
    });
    let text = std::fs::read_to_string(shop.join("composer.json")).unwrap();
    std::fs::write(
        shop.join("composer.json"),
        text.replacen(
            "\"scripts\": {",
            "\"scripts\": {\n        \"stan\": \"phpstan analyse\",",
            1,
        ),
    )
    .unwrap();
    until(host, "new script", |h, _| {
        panel(h, "project").contains("phpstan analyse")
    });
    assert_eq!(
        chip(host, PaneId::Left).unwrap().text,
        "Laravel · 29 scripts"
    );
    host.set_pane(local(PaneId::Left, &shop, true, &["README.md"]), 3);
    until(host, "no show scripts", |h, _| {
        !h.context_menu_commands()
            .any(|c| c.full_id.ends_with(":show-scripts"))
    });
    host.shutdown(Duration::from_secs(1));
}

#[test]
fn remote_and_empty_folders() {
    let mut s = setup(|_, _| {});
    let host = &mut s.host;
    let mut remote = local(PaneId::Left, Path::new("/srv/www"), true, &[]);
    remote.local_path = None;
    remote.protocol = Protocol::Sftp;
    remote.backend_label = "anna@example.org (SFTP)".into();
    host.set_pane(remote, 2);
    until(host, "remote", |h, _| {
        panel(h, "project").contains("Remote folder")
    });
    assert!(chip(host, PaneId::Left).is_none());
    let empty = s._tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let host = &mut s.host;
    host.set_pane(local(PaneId::Left, &empty, true, &[]), 3);
    until(host, "no project", |h, _| {
        panel(h, "project").contains("No project here")
    });
    host.run_command(
        &format!("{ID}:rerun"),
        &local(PaneId::Left, &empty, true, &[]),
    );
    until(host, "no project toast", |_, ev| {
        ev.iter().any(
            |e| matches!(e, HostEvent::Toast { text, .. } if text.contains("There is no project")),
        )
    });
    host.shutdown(Duration::from_secs(1));
}

/// recent.json of the Python plugin is taken over; German texts follow
/// the language switch.
#[test]
fn migrated_recent_and_german() {
    let mut s = setup(|paths, shop| {
        let old = paths.data_root.join("project-info");
        std::fs::create_dir_all(&old).unwrap();
        let shop = shop.to_string_lossy().into_owned();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let recent = serde_json::json!({ shop.clone(): [
            {"title": "dev", "section": "pnpm", "command": "pnpm run dev", "cwd": shop, "danger": null, "ts": now - 240.0},
        ]});
        std::fs::write(old.join("recent.json"), recent.to_string()).unwrap();
        std::fs::write(old.join("plugin.log"), "old log").unwrap();
    });
    let shop = s.shop.clone();
    let host = &mut s.host;
    until(host, "migrated recent", |h, _| {
        panel(h, "project").contains("pnpm run dev") && panel(h, "project").contains("4 min ago")
    });
    assert!(!s.paths.data_root.join(ID).join("plugin.log").exists());
    let host = &mut s.host;
    host.run_command(
        &format!("{ID}:rerun"),
        &local(PaneId::Left, &shop, true, &[]),
    );
    host.poll();
    assert_eq!(s.typed.lock().unwrap().last().unwrap().1, "pnpm run dev");

    let host = &mut s.host;
    manta_plugin::i18n::set_thread_lang(Some(manta_plugin::i18n::Lang::De));
    host.deliver(ID, PluginEvent::LocaleChanged("de".into()));
    host.deliver(TERMINAL_PLUGIN, PluginEvent::LocaleChanged("de".into()));
    until(host, "German chip", |h, _| {
        chip(h, PaneId::Left).is_some_and(|c| c.text == "Laravel · 28 Skripte")
    });
    let p = panel(host, "project");
    assert!(
        p.contains("Zuletzt ausgeführt")
            && p.contains("Tests ausführen")
            && p.contains("gerade eben"),
        "{p}"
    );
    manta_plugin::i18n::set_thread_lang(None);
    host.shutdown(Duration::from_secs(1));
}
