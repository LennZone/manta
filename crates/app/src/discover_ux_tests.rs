//! Settings › Plugins › Discover (the community directory), "Browse
//! Plugins…", double-clicked and dropped `.mantaplugin` files, and the
//! sandbox in the plugin cards and sheets. Child of `ux_tests`.
//!
//! Nothing here contacts the internet: the directory is injected
//! (`set_discover_for_test`) or "downloaded" from a closed local port
//! (`TEST_REGISTRY_URL`); plugin files live in temporary folders.

use std::io::Write as _;

use eframe::App as _;

use egui_kittest::kittest::Queryable;
use manta_plugin::host::{Discover, HostPaths};
use manta_plugin::registry::Entry;

use super::*;
use crate::app::Action;
use crate::commands::Cmd;
use crate::settings::{Confirm, PluginsPage, SettingsTab};

fn entry(id: &str, name: (&str, &str), version: &str, min: &str, perms: &[&str]) -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": { "en": name.0, "de": name.1 },
        "description": {
            "en": format!("{} for your folders.", name.0),
            "de": format!("{} für deine Ordner.", name.1),
        },
        "author": "Anna Berger",
        "repository": format!("https://github.com/anna-berger/manta-{id}"),
        "license": "MIT",
        "latest": {
            "version": version,
            "url": format!("https://github.com/anna-berger/manta-{id}/releases/download/v{version}/{id}-{version}.mantaplugin"),
            "sha256": "a".repeat(64),
        },
        "min_manta_version": min,
        "platforms": ["any"],
        "permissions": perms,
    }))
    .expect("entry")
}

/// A directory with an update (hello-python), an installed plugin
/// (cloud-sync), a new one and one that needs a newer Manta.
fn sample_directory() -> Discover {
    Discover {
        loading: false,
        requested: true,
        plugins: vec![
            entry(
                "cloud-sync",
                ("Cloud Sync", "Cloud-Sync"),
                "2.3.1",
                "0.3",
                &["read_files", "network"],
            ),
            entry(
                "git-blame",
                ("Git Blame", "Git Blame"),
                "1.0.0",
                "0.3",
                &["read_files", "run_programs"],
            ),
            entry(
                "gpu-stats",
                ("GPU Stats", "GPU-Statistik"),
                "0.9.0",
                "9.0",
                &[],
            ),
            entry(
                "hello-python",
                ("Hello Python", "Hallo Python"),
                "0.2.0",
                "0.3",
                &["read_files"],
            ),
        ],
        from_cache: false,
        fetched_at: Some(SystemTime::now() - Duration::from_secs(7200)),
        error: None,
    }
}

fn discover_harness(fx: &Fixture, theme: Theme, gpu: bool, directory: Option<Discover>) -> H {
    let mut h = harness_full(
        fx,
        WIDE,
        theme,
        gpu,
        Box::new(move |ctx: &egui::Context| {
            let mut host = plugin_host(ctx, decorators());
            if let Some(d) = directory {
                host.set_discover_for_test(d);
            }
            host
        }),
    );
    settle(&mut h);
    h
}

/// A `.mantaplugin` in `dir` (one folder `ext/` with plugin.toml + run.sh).
fn plugin_file(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    z.start_file("ext/plugin.toml", opts).unwrap();
    z.write_all(
        b"id = \"ext\"\nname = \"Ext\"\nversion = \"1.0.0\"\nauthor = \"Anna\"\napi_version = 1\n\
          command = [\"./run.sh\"]\npermissions = [\"read_files\"]\n",
    )
    .unwrap();
    z.start_file("ext/run.sh", opts).unwrap();
    z.write_all(b"#!/bin/sh\ncat\n").unwrap();
    z.finish().unwrap();
    path
}

/// A harness whose plugin host installs into `config` (a temp folder).
fn installing_harness(fx: &Fixture, config: &Path) -> H {
    let paths = HostPaths::under(config);
    let mut h = harness_full(
        fx,
        WIDE,
        Theme::Light,
        false,
        Box::new(move |ctx: &egui::Context| PluginHost::new(ctx.clone(), Vec::new(), Some(paths))),
    );
    settle(&mut h);
    h
}

/// Step until the install sheet is up (or panic).
fn wait_for_review(h: &mut H) -> manta_plugin::install::PluginReview {
    for _ in 0..300 {
        h.run_steps(1);
        if let Some(Confirm::Install(r)) = &app(h).settings.confirm {
            return (**r).clone();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no install review");
}

#[test]
fn browse_plugins_opens_discover() {
    let fx = fixture();
    let mut h = discover_harness(&fx, Theme::Light, false, Some(sample_directory()));
    crate::i18n::set_locale(crate::i18n::Locale::En);
    let actions = app(&mut h).cmd_actions(Cmd::BrowsePlugins);
    assert!(matches!(actions[..], [Action::BrowsePlugins]));
    for a in actions {
        super::views_ux_tests::act(&mut h, a);
    }
    h.run_steps(3);
    let a = app(&mut h);
    assert!(a.settings.open);
    assert_eq!(a.settings.tab, SettingsTab::Plugins);
    assert_eq!(a.settings.plugins_page, PluginsPage::Discover);
    assert!(a.opened_links.is_empty(), "no browser");
    // Name in the UI language, Install / Update / Installed.
    assert!(h.query_by_label("Git Blame").is_some());
    assert!(h.query_by_label("Needs Manta 9.0 or newer").is_some());
    let rows = app(&mut h).plugins.registry_rows();
    let hello = rows.iter().find(|r| r.entry.id == "hello-python").unwrap();
    assert!(hello.update_available);
    assert!(rows
        .iter()
        .find(|r| r.entry.id == "cloud-sync")
        .unwrap()
        .installed_version
        .is_some());
    // The installed card offers the directory's update too.
    app(&mut h).settings.plugins_page = PluginsPage::Installed;
    h.run_steps(3);
    assert!(h
        .query_by_label_contains("Version 0.2.0 is available in the plugin directory")
        .is_some());
}

/// Privacy: the directory is loaded only once Discover is shown; offline,
/// the tab says so.
#[test]
fn discover_loads_the_directory_only_when_opened() {
    let fx = fixture();
    let mut h = discover_harness(&fx, Theme::Light, false, None);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    app(&mut h).settings.show(Some(SettingsTab::Plugins));
    h.run_steps(4);
    assert!(
        !app(&mut h).plugins.discover().requested,
        "the Installed page doesn't load anything"
    );
    app(&mut h).settings.plugins_page = PluginsPage::Discover;
    h.run_steps(2);
    assert!(app(&mut h).plugins.discover().requested);
    for _ in 0..300 {
        h.run_steps(1);
        if !app(&mut h).plugins.discover().loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(2);
    assert!(app(&mut h).plugins.discover().error.is_some());
    assert!(h
        .query_by_label("Couldn't load the plugin directory")
        .is_some());
}

/// Opening a .mantaplugin file (double-click in Finder, `open x.mantaplugin`)
/// shows the install sheet; other paths of the same request still open.
#[test]
fn opening_a_plugin_file_shows_the_install_sheet() {
    let fx = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let mut h = installing_harness(&fx, &tmp.path().join("config"));
    let file = plugin_file(tmp.path(), "ext-1.0.0.mantaplugin");
    app(&mut h).apply_open_requests(vec![crate::default_viewer::OpenRequest {
        kind: crate::default_viewer::RequestKind::Open,
        paths: vec![file.clone(), fx.root.join("src")],
    }]);
    let r = wait_for_review(&mut h);
    assert_eq!((r.id.as_str(), r.version.as_str()), ("ext", "1.0.0"));
    assert_eq!(r.source, file);
    let a = app(&mut h);
    assert!(a.settings.open && a.settings.tab == SettingsTab::Plugins);
    let src = VfsPath::new(&fx.root.join("src").to_string_lossy());
    assert!(
        a.pane(Side::Left).tabs.iter().any(|t| t.loc.path == src),
        "the folder opened as usual"
    );
    assert!(crate::default_viewer::is_plugin_file(&file));
    assert!(!crate::default_viewer::is_plugin_file(&fx.root));
}

/// Dragging a .mantaplugin over the Plugins tab shows a hint; dropping it
/// shows the install sheet.
#[test]
fn dropping_a_plugin_file_on_the_plugins_tab() {
    let fx = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let mut h = installing_harness(&fx, &tmp.path().join("config"));
    crate::i18n::set_locale(crate::i18n::Locale::En);
    let file = plugin_file(tmp.path(), "ext.mantaplugin");
    app(&mut h).settings.show(Some(SettingsTab::Plugins));
    h.run_steps(3);
    let ctx = h.ctx.clone();

    let mut raw = egui::RawInput {
        hovered_files: vec![crate::native_drag::test_hooks::hovered(&file)],
        ..Default::default()
    };
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    assert!(
        raw.hovered_files.is_empty(),
        "the panes behind don't see it"
    );
    h.run_steps(1);
    assert!(h.query_by_label(t!("plugins.drop")).is_some(), "drop hint");

    let mut raw = egui::RawInput {
        dropped_files: vec![crate::native_drag::test_hooks::dropped(&file)],
        ..Default::default()
    };
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    assert!(raw.dropped_files.is_empty());
    let r = wait_for_review(&mut h);
    assert_eq!(r.id, "ext");
    // Something else dropped there is ignored.
    app(&mut h).settings.confirm = None;
    let txt = tmp.path().join("notes.txt");
    std::fs::write(&txt, "x").unwrap();
    let mut raw = egui::RawInput {
        dropped_files: vec![crate::native_drag::test_hooks::dropped(&txt)],
        ..Default::default()
    };
    app(&mut h).raw_input_hook(&ctx, &mut raw);
    h.run_steps(5);
    assert!(app(&mut h).settings.confirm.is_none());
}

/// Discover (light, dark, German), offline and empty states, the
/// directory's update on an installed card, and the install sheet of a
/// verified download: `MANTA_RENDER_DIR=… cargo test -p manta render_plugin_discover`.
#[test]
fn render_plugin_discover() {
    require_gpu!();
    let fx = fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        crate::i18n::set_locale(crate::i18n::Locale::En);
        let mut h = discover_harness(&fx, theme, true, Some(sample_directory()));
        crate::i18n::set_locale(crate::i18n::Locale::En);
        app(&mut h).settings.show_discover();
        h.run_steps(4);
        save(&mut h, &format!("plugins_discover_{tname}"));
        if theme == Theme::Dark {
            continue;
        }
        app(&mut h).settings.plugins_page = PluginsPage::Installed;
        h.run_steps(4);
        save(&mut h, "plugins_installed_with_directory_update_light");
        // Offline with the saved list.
        let mut d = sample_directory();
        d.from_cache = true;
        d.error = Some("can't reach raw.githubusercontent.com (offline?)".into());
        app(&mut h).plugins.set_discover_for_test(d);
        app(&mut h).settings.plugins_page = PluginsPage::Discover;
        h.run_steps(4);
        save(&mut h, "plugins_discover_offline_light");
        // Nothing listed yet.
        app(&mut h).plugins.set_discover_for_test(Discover {
            requested: true,
            ..Default::default()
        });
        h.run_steps(4);
        save(&mut h, "plugins_discover_empty_light");
        // The install sheet of a download from the directory.
        let mut r = sample_review("cloud-sync", "Cloud Sync", "node index.js", true);
        r.verified_download = Some("https://github.com/anna-berger/manta-cloud-sync/releases/download/v1.2.0/cloud-sync-1.2.0.mantaplugin".into());
        r.source = "cloud-sync-1.2.0.mantaplugin".into();
        app(&mut h).settings.confirm = Some(Confirm::Install(Box::new(r)));
        h.run_steps(4);
        save(&mut h, "plugins_install_sheet_directory_light");
    }
    crate::i18n::set_locale(crate::i18n::Locale::De);
    let mut h = discover_harness(&fx, Theme::Light, true, Some(sample_directory()));
    crate::i18n::set_locale(crate::i18n::Locale::De);
    app(&mut h).settings.show_discover();
    h.run_steps(4);
    save(&mut h, "plugins_discover_de_light");
    crate::i18n::set_locale(crate::i18n::Locale::En);
}
