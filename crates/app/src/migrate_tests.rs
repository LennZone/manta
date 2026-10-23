//! Migration from RustLift (the former name) to Manta, on temp dirs.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn paths(root: &Path) -> Paths {
    Paths {
        old_support: root.join("config/rustlift"),
        new_support: root.join("config/Manta"),
        old_logs: Some(root.join("Logs/RustLift")),
        new_logs: Some(root.join("Logs/Manta")),
    }
}

fn write(p: &Path, text: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// What a RustLift user had: prefs, favorites, plugin state and plugin data.
fn old_install(p: &Paths) {
    let o = &p.old_support;
    write(
        &o.join("settings.json"),
        r#"{"language":"de","accent":"purple"}"#,
    );
    write(
        &o.join("favorites.json"),
        r#"{"items":[{"name":"Sites","path":"/Users/anna/Sites"}]}"#,
    );
    write(
        &o.join("default-viewer.json"),
        r#"{"prev_file_viewer":"com.binarynights.ForkLift"}"#,
    );
    write(
        &o.join("plugins.json"),
        r#"{"enabled":{"rustlift.git":false,"hello":true},"trusted":["hello"],
            "panels":{"rustlift.git/git/sidebar":true,"rustlift.terminal/terminal/left":true},
            "seen_panels":["rustlift.git/git","rustlift.rename/batch-rename"],
            "layout":{"pane_bottom_height":0.0,"sidebar_width":395.5},
            "retired_notified":["quick-actions"]}"#,
    );
    write(
        &o.join("plugin-data/rustlift.quickactions/actions.json"),
        r#"[{"name":"Build"}]"#,
    );
    write(
        &o.join("plugin-data/rustlift.quickactions/comments.json"),
        "// keep: not the actions file\n",
    );
    write(
        &o.join("plugin-data/rustlift.terminal/settings.json"),
        r#"{"font_size":13}"#,
    );
    write(
        &o.join("plugin-data/rustlift.terminal/shell-integration/zsh/.zshrc"),
        "# shim",
    );
    write(&o.join("plugin-data/quick-actions/recent.json"), "[]");
    write(&o.join("plugins/hello/plugin.toml"), "id = \"hello\"");
    write(
        &p.old_logs.as_ref().unwrap().join("rustlift.log"),
        "old log\n",
    );
    write(
        &p.old_logs.as_ref().unwrap().join("rustlift.log.1"),
        "older log\n",
    );
}

fn plugins_json(p: &Paths) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(p.new_support.join("plugins.json")).unwrap()).unwrap()
}

fn assert_migrated(p: &Paths) {
    let n = &p.new_support;
    assert!(!p.old_support.exists(), "old support folder is gone");
    assert_eq!(
        fs::read_to_string(n.join("settings.json")).unwrap(),
        r#"{"language":"de","accent":"purple"}"#
    );
    assert!(fs::read_to_string(n.join("favorites.json"))
        .unwrap()
        .contains("/Users/anna/Sites"));
    assert!(n.join("default-viewer.json").is_file());
    assert_eq!(
        fs::read_to_string(n.join("plugin-data/manta.quickactions/actions.json")).unwrap(),
        r#"[{"name":"Build"}]"#
    );
    assert!(n
        .join("plugin-data/manta.terminal/shell-integration/zsh/.zshrc")
        .is_file());
    assert!(!n.join("plugin-data/rustlift.terminal").exists());
    // External plugin data and plugins are not renamed.
    assert!(n.join("plugin-data/quick-actions/recent.json").is_file());
    assert!(n.join("plugins/hello/plugin.toml").is_file());

    let v = plugins_json(p);
    assert_eq!(v["enabled"]["manta.git"], false);
    assert_eq!(v["enabled"]["hello"], true);
    assert!(v["enabled"].get("rustlift.git").is_none());
    assert_eq!(v["panels"]["manta.git/git/sidebar"], true);
    assert_eq!(v["panels"]["manta.terminal/terminal/left"], true);
    assert_eq!(
        v["seen_panels"],
        serde_json::json!(["manta.git/git", "manta.rename/batch-rename"])
    );
    assert_eq!(v["layout"]["sidebar_width"], 395.5);
    assert_eq!(v["retired_notified"], serde_json::json!(["quick-actions"]));
    assert!(!fs::read_to_string(n.join("plugins.json"))
        .unwrap()
        .contains("rustlift"));

    let logs = p.new_logs.as_ref().unwrap();
    assert_eq!(
        fs::read_to_string(logs.join("manta.log")).unwrap(),
        "old log\n"
    );
    assert_eq!(
        fs::read_to_string(logs.join("manta.log.1")).unwrap(),
        "older log\n"
    );
    assert!(!p.old_logs.as_ref().unwrap().exists());
}

#[test]
fn full_migration_moves_everything_and_rewrites_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    let r = run(&p);
    assert!(r.imported, "{r:?}");
    assert!(r.warnings.is_empty(), "{r:?}");
    assert_migrated(&p);
}

#[test]
fn second_run_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    assert!(run(&p).imported);
    let before = fs::read_to_string(p.new_support.join("plugins.json")).unwrap();
    let r = run(&p);
    assert!(!r.imported, "{r:?}");
    assert!(r.info.is_empty() && r.warnings.is_empty(), "{r:?}");
    assert_eq!(
        fs::read_to_string(p.new_support.join("plugins.json")).unwrap(),
        before
    );
    assert_migrated(&p);
}

#[test]
fn nothing_to_migrate_on_a_fresh_install() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    let r = run(&p);
    assert!(
        !r.imported && r.info.is_empty() && r.warnings.is_empty(),
        "{r:?}"
    );
    assert!(!p.new_support.exists());
}

#[test]
fn cross_volume_move_copies_verifies_and_deletes() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    // Every rename of a whole folder across "volumes" fails like EXDEV.
    let exdev = |a: &Path, b: &Path| -> io::Result<()> {
        if a.ends_with("rustlift") || a.ends_with("RustLift") {
            Err(io::Error::from_raw_os_error(18))
        } else {
            fs::rename(a, b)
        }
    };
    let r = run_with(&p, &exdev);
    assert!(r.imported, "{r:?}");
    assert!(
        r.info.iter().any(|l| l.contains("copied across volumes")),
        "{r:?}"
    );
    assert_migrated(&p);
    assert!(!tmp.path().join("config/.Manta.migrating").exists());
}

#[test]
fn other_rename_errors_leave_the_old_folder_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    let denied = |_: &Path, _: &Path| -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    };
    let r = run_with(&p, &denied);
    assert!(!r.imported);
    assert!(!r.warnings.is_empty());
    assert!(
        p.old_support.join("settings.json").is_file(),
        "nothing lost"
    );
}

#[test]
fn conflict_keeps_the_new_folder_and_leaves_the_old_one() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    write(&p.new_support.join("settings.json"), r#"{"language":"en"}"#);
    let r = run(&p);
    assert!(r.warnings.iter().any(|w| w.contains("both")), "{r:?}");
    assert_eq!(
        fs::read_to_string(p.new_support.join("settings.json")).unwrap(),
        r#"{"language":"en"}"#
    );
    assert!(
        p.old_support.join("settings.json").is_file(),
        "old data kept for the user"
    );
}

#[test]
fn an_empty_new_folder_counts_as_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    fs::create_dir_all(&p.new_support).unwrap();
    fs::create_dir_all(p.new_logs.as_ref().unwrap()).unwrap();
    assert!(run(&p).imported);
    assert_migrated(&p);
}

#[test]
fn partial_migration_is_completed() {
    // An earlier run moved the folder but stopped before the ids were rewritten.
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    old_install(&p);
    fs::create_dir_all(p.new_support.parent().unwrap()).unwrap();
    fs::rename(&p.old_support, &p.new_support).unwrap();
    let r = run(&p);
    assert!(r.imported, "{r:?}");
    assert_migrated(&p);
}

#[test]
fn existing_new_plugin_data_wins_over_the_old_one() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    write(
        &p.new_support
            .join("plugin-data/manta.quickactions/actions.json"),
        "new",
    );
    write(
        &p.new_support
            .join("plugin-data/rustlift.quickactions/actions.json"),
        "old",
    );
    write(&p.new_support.join("plugin-data/rustlift.git/x"), "git");
    let r = run(&p);
    let n = &p.new_support;
    assert_eq!(
        fs::read_to_string(n.join("plugin-data/manta.quickactions/actions.json")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(n.join("plugin-data/rustlift.quickactions/actions.json")).unwrap(),
        "old"
    );
    assert_eq!(
        fs::read_to_string(n.join("plugin-data/manta.git/x")).unwrap(),
        "git"
    );
    assert!(
        r.warnings.iter().any(|w| w.contains("already exists")),
        "{r:?}"
    );
}

#[test]
fn json_rewrite_prefers_existing_new_keys() {
    let mut v = serde_json::json!({"enabled": {"rustlift.git": false, "manta.git": true}, "x": "rustlifty"});
    assert!(rewrite_json(&mut v));
    assert_eq!(
        v,
        serde_json::json!({"enabled": {"manta.git": true}, "x": "rustlifty"})
    );
}

#[test]
fn logs_merge_into_an_existing_new_log_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    let (old, new): (PathBuf, PathBuf) = (p.old_logs.clone().unwrap(), p.new_logs.clone().unwrap());
    write(&old.join("rustlift.log"), "old");
    write(&new.join("manta.log"), "new");
    run(&p);
    assert_eq!(fs::read_to_string(new.join("manta.log")).unwrap(), "new");
    assert_eq!(fs::read_to_string(new.join("rustlift.log")).unwrap(), "old");
    assert!(!old.exists());
}

#[test]
fn invalid_plugins_json_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    write(
        &p.old_support.join("plugins.json"),
        "{ not json rustlift.git",
    );
    let r = run(&p);
    assert_eq!(
        fs::read_to_string(p.new_support.join("plugins.json")).unwrap(),
        "{ not json rustlift.git"
    );
    assert!(
        r.warnings.iter().any(|w| w.contains("not valid JSON")),
        "{r:?}"
    );
}

#[test]
fn quick_action_template_comments_name_the_new_app() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    let file = "// Quick Actions for RustLift: your own commands.\n  // RustLift picks up changes.\n[{\"title\": \"Open in RustLift\"}]\n";
    write(
        &p.old_support
            .join("plugin-data/rustlift.quickactions/actions.json"),
        file,
    );
    assert!(run(&p).imported);
    let path = p
        .new_support
        .join("plugin-data/manta.quickactions/actions.json");
    let got = fs::read_to_string(&path).unwrap();
    assert_eq!(
        got,
        "// Quick Actions for Manta: your own commands.\n  // Manta picks up changes.\n[{\"title\": \"Open in RustLift\"}]\n",
        "only comments change, the user's actions stay"
    );
    assert!(!run(&p).imported, "idempotent");
}
