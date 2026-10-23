//! Placeholders, quoting, config and applicability, ported from the Python
//! plugin's `tests/test_core.py` and `tests/test_i18n.py`. The expanded
//! commands really run through `/bin/sh` with hostile file names.

use std::collections::HashMap;
use std::path::Path;

use manta_plugin::i18n::{with_lang, Lang};
use manta_plugin_quickactions::core::{self, Val};
use serde_json::json;

const NASTY: &[&str] = &[
    "plain.txt",
    "with space.txt",
    "it's.txt",
    "double\"quote.txt",
    "$(touch PWNED).txt",
    "`touch PWNED2`.txt",
    "semi;colon&amp.txt",
    "-rf",
    "*",
    "ünïcødé ✓.txt",
    "new\nline.txt",
    "back\\slash.txt",
    "{file}.txt",
];

/// Run like the plugin does; returns the NUL-separated output words.
fn sh(cmd: &str, cwd: Option<&Path>) -> Vec<String> {
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c").arg(cmd);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let mut words: Vec<String> = text.split('\0').map(str::to_string).collect();
    words.pop();
    words
}

fn action(command: &str, extra: serde_json::Value) -> core::Action {
    let mut raw = json!({"id": "t", "title": "Test", "command": command});
    if let serde_json::Value::Object(m) = extra {
        raw.as_object_mut().unwrap().extend(m);
    }
    core::normalize_action(&raw).unwrap()
}

fn vals<'a>(pairs: &[(&'a str, Val)]) -> HashMap<&'a str, Val> {
    pairs.iter().cloned().collect()
}

struct Tmp {
    dir: tempfile::TempDir,
    paths: Vec<String>,
}

fn tmp() -> Tmp {
    let dir = tempfile::tempdir().unwrap();
    let paths = NASTY
        .iter()
        .map(|n| format!("{}/{n}", dir.path().display()))
        .collect();
    Tmp { dir, paths }
}

fn no_pwned(t: &Tmp) {
    assert!(!t.dir.path().join("PWNED").exists());
    assert!(!t.dir.path().join("PWNED2").exists());
}

#[test]
fn files_reach_the_command_unchanged() {
    let t = tmp();
    let cmd = core::expand(
        "printf '%s\\0' {files}",
        &vals(&[("files", Val::Many(t.paths.clone()))]),
    )
    .unwrap();
    assert_eq!(sh(&cmd, Some(t.dir.path())), t.paths);
    no_pwned(&t);
}

#[test]
fn per_item_placeholders() {
    let t = tmp();
    let a = action("printf '%s\\0' {file} {name} {stem} {ext} {dir}", json!({}));
    let items: Vec<(String, bool)> = t.paths.iter().map(|p| (p.clone(), false)).collect();
    let folder = t.dir.path().to_string_lossy().into_owned();
    let cmds = core::plan(&a, &items, &folder, None).unwrap();
    assert_eq!(cmds.len(), NASTY.len());
    for ((cmd, cwd), name) in cmds.iter().zip(NASTY) {
        let (stem, ext) = match name.rfind('.') {
            Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
            _ => (*name, ""),
        };
        assert_eq!(cwd, &folder);
        assert_eq!(
            sh(cmd, Some(Path::new(cwd))),
            vec![
                format!("{folder}/{name}"),
                name.to_string(),
                stem.into(),
                ext.into(),
                folder.clone()
            ]
        );
    }
    no_pwned(&t);
}

#[test]
fn double_quotes_give_plain_text() {
    let t = tmp();
    for path in &t.paths {
        let (_, stem, _) = core::split_name(path);
        let cmd = core::expand(
            "printf \"%s\\0\" \"{stem} copy.{ext}\" \"$HOME\"",
            &vals(&[
                ("stem", Val::One(stem.clone())),
                ("ext", Val::One("txt".into())),
            ]),
        )
        .unwrap();
        let out = sh(&cmd, None);
        assert_eq!(out[0], format!("{stem} copy.txt"));
        assert_eq!(
            out[1],
            std::env::var("HOME").unwrap(),
            "shell syntax outside the value still works"
        );
    }
    no_pwned(&t);
}

#[test]
fn single_quotes_quote_for_an_inner_shell() {
    let t = tmp();
    let cmd = core::expand(
        "sh -c 'printf \"%s\\0\" {files}'",
        &vals(&[("files", Val::Many(t.paths.clone()))]),
    )
    .unwrap();
    assert_eq!(sh(&cmd, Some(t.dir.path())), t.paths);
    no_pwned(&t);
}

#[test]
fn concatenation_and_names() {
    let t = tmp();
    let dir = t.dir.path().to_string_lossy().into_owned();
    let cmd = core::expand(
        "printf '%s\\0' {dir}/{stem}-50%.{ext} {names}",
        &vals(&[
            ("dir", Val::One(dir.clone())),
            ("stem", Val::One("a b".into())),
            ("ext", Val::One("png".into())),
            ("names", Val::Many(vec!["a b.png".into(), "c".into()])),
        ]),
    )
    .unwrap();
    assert_eq!(
        sh(&cmd, None),
        vec![format!("{dir}/a b-50%.png"), "a b.png".into(), "c".into()]
    );
}

#[test]
fn unknown_braces_are_kept() {
    let t = "awk '/w/ {print $2}' {file} | { cat; } && echo {nope} ${HOME}";
    let out = core::expand(t, &vals(&[("file", Val::One("/x y".into()))])).unwrap();
    assert_eq!(
        out,
        "awk '/w/ {print $2}' '/x y' | { cat; } && echo {nope} ${HOME}"
    );
}

#[test]
fn escaped_quotes_dont_confuse_the_scanner() {
    let out = core::expand(
        "echo \\' {file} \"a\\\"b {file}\"",
        &vals(&[("file", Val::One("it's".into()))]),
    )
    .unwrap();
    assert_eq!(out, "echo \\' 'it'\"'\"'s' \"a\\\"b it's\"");
}

#[test]
fn missing_value() {
    let r = core::expand(
        "cp {files} {dest}/",
        &vals(&[("files", Val::Many(vec!["/a".into()]))]),
    );
    assert!(r.is_err());
    with_lang(Lang::En, || {
        let e = core::expand("cp {dest}/", &HashMap::new()).unwrap_err();
        assert_eq!(e.0, "{dest} has no value here");
    });
}

#[test]
fn count_is_a_number() {
    assert_eq!(
        core::expand("echo {count}", &vals(&[("count", Val::Count(3))])).unwrap(),
        "echo 3"
    );
}

#[test]
fn shell_quote_matches_python() {
    assert_eq!(core::shell_quote(""), "''");
    assert_eq!(
        core::shell_quote("a-b_c.d/e:f,g@h%i+j=k"),
        "a-b_c.d/e:f,g@h%i+j=k"
    );
    assert_eq!(core::shell_quote("a b"), "'a b'");
    assert_eq!(core::shell_quote("it's"), "'it'\"'\"'s'");
    assert_eq!(core::shell_quote("ü"), "'ü'");
}

/// Every default action expanded for every hostile name: the command
/// parses and no name is executed. (Only `sh -n`: the tools may be missing.)
#[test]
fn default_actions_quote_hostile_names() {
    let t = tmp();
    let (actions, errors) = core::parse_actions(core::DEFAULT_ACTIONS).unwrap();
    assert!(errors.is_empty());
    let items: Vec<(String, bool)> = t.paths.iter().map(|p| (p.clone(), false)).collect();
    let folder = t.dir.path().to_string_lossy().into_owned();
    for a in &actions {
        for (cmd, _) in core::plan(a, &items, &folder, Some("/tmp/dest $(x)")).unwrap() {
            let ok = std::process::Command::new("/bin/sh")
                .arg("-n")
                .arg("-c")
                .arg(&cmd)
                .status()
                .unwrap();
            assert!(ok.success(), "{}: {cmd}", a.id);
        }
    }
    no_pwned(&t);
}

// ------------------------------------------------------------------ config

#[test]
fn defaults_parse() {
    let (actions, errors) = core::parse_actions(core::DEFAULT_ACTIONS).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let ids: Vec<&str> = actions.iter().map(|a| a.id.as_str()).collect();
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), ids.len());
    for want in [
        "resize-50",
        "to-jpeg",
        "to-png",
        "zip",
        "zip-many",
        "extract",
        "vscode",
        "terminal-app",
        "copy-path",
        "reveal",
        "chmod-x",
        "count-lines",
        "sha256",
        "rsync-other",
    ] {
        assert!(ids.contains(&want), "{want}");
    }
    let rsync = actions.iter().find(|a| a.id == "rsync-other").unwrap();
    assert_eq!(rsync.mode, core::Mode::Terminal);
    assert!(rsync.confirm.is_some());
}

/// The defaults are byte-identical to the Python plugin's, so actions.json
/// files it wrote still count as "unchanged defaults" (translated titles).
#[test]
fn defaults_are_the_python_plugins() {
    assert!(core::DEFAULT_ACTIONS
        .starts_with("// Quick Actions for Manta: your own commands for the selected files.\n"));
    assert!(core::DEFAULT_ACTIONS.contains(r#""command": "printf '%s\\n' {files} | awk 'NR > 1 {printf \"\\n\"} {printf \"%s\", $0}' | pbcopy","#));
}

#[test]
fn comments_and_trailing_commas() {
    let text = r#"// top
        {"actions": [ /* block */
          {"id": "a", "title": "A // not a comment", "command": "echo 'http://x' /* kept */",},
          {"//": "a comment entry"},
        ],}"#;
    let (actions, errors) = core::parse_actions(text).unwrap();
    assert!(errors.is_empty());
    let a = &actions[0];
    assert_eq!(a.title, "A // not a comment");
    assert_eq!(a.command, "echo 'http://x' /* kept */");
    assert_eq!(a.applies_to, vec!["any".to_string()]);
    assert_eq!((a.min, a.max, a.mode), (1, None, core::Mode::Background));
}

#[test]
fn bad_entries_are_reported() {
    let text = r#"[{"id": "ok", "title": "OK", "command": "true"},
                   {"id": "bad id", "title": "X", "command": "true"},
                   {"id": "nocmd", "title": "X"},
                   {"id": "mode", "title": "X", "command": "x", "mode": "loud"},
                   {"id": "ok", "title": "Dup", "command": "true"},
                   {"id": "mix", "title": "X", "command": "cp {file} {files}"}]"#;
    let (actions, errors) = core::parse_actions(text).unwrap();
    assert_eq!(
        actions.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        ["ok"]
    );
    assert_eq!(errors.len(), 5, "{errors:?}");
    assert!(core::parse_actions("{not json").is_err());
    assert!(core::parse_actions("42").is_err());
}

#[test]
fn field_rules() {
    let bad = |raw: serde_json::Value| core::normalize_action(&raw).is_err();
    assert!(bad(
        json!({"id": "a", "title": "A", "command": "x", "min": -1})
    ));
    assert!(bad(
        json!({"id": "a", "title": "A", "command": "x", "min": 2, "max": 1})
    ));
    assert!(bad(
        json!({"id": "a", "title": "A", "command": "x", "max": 0})
    ));
    assert!(bad(
        json!({"id": "a", "title": "A", "command": "x", "output": "loud"})
    ));
    assert!(bad(
        json!({"id": "a", "title": "A", "command": "x", "applies_to": []})
    ));
    assert!(bad(
        json!({"id": "a".repeat(65), "title": "A", "command": "x"})
    ));
    let a = core::normalize_action(&json!({
        "id": "a", "title": "  A  ", "command": "x", "min": 0, "refresh": 1, "applies_to": "Folder",
        "title_de": "  ", "message_de": " Fertig ", "output": "panel", "mode": "silent"
    }))
    .unwrap();
    assert_eq!((a.title.as_str(), a.min, a.refresh), ("A", 0, true));
    assert_eq!(a.applies_to, vec!["folders".to_string()]);
    assert_eq!(
        (a.title_de, a.message_de.as_deref()),
        (None, Some("Fertig"))
    );
    assert_eq!(
        (a.output, a.mode),
        (core::Output::Panel, core::Mode::Silent)
    );
}

#[test]
fn kinds_extensions_and_counts() {
    let it = |v: &[(&str, bool)]| {
        v.iter()
            .map(|(p, d)| (p.to_string(), *d))
            .collect::<Vec<_>>()
    };
    let img = action("x {file}", json!({"applies_to": ["JPG", ".png", "tar.gz"]}));
    assert!(core::applies(&img, &it(&[("/a/b.jpg", false), ("/a/c.PNG", false)]), None).is_ok());
    assert!(core::applies(&img, &it(&[("/a/b.tar.gz", false)]), None).is_ok());
    let why = with_lang(Lang::En, || {
        core::applies(&img, &it(&[("/a/b.gif", false)]), None).unwrap_err()
    });
    assert!(why.contains("gif"), "{why}");
    assert!(
        core::applies(&img, &it(&[("/a/pics.jpg", true)]), None).is_err(),
        "a folder named .jpg isn't an image"
    );
    let folders = action("x {files}", json!({"applies_to": ["folders"]}));
    assert!(core::applies(&folders, &it(&[("/a", true)]), None).is_ok());
    assert!(core::applies(&folders, &it(&[("/a", true), ("/b", false)]), None).is_err());
    let files = action("x {files}", json!({"applies_to": "files"}));
    assert!(core::applies(&files, &it(&[("/a", true)]), None).is_err());
    let many = action("x {files}", json!({"min": 2, "max": 3}));
    assert!(core::applies(&many, &it(&[("/a", false)]), None).is_err());
    assert!(core::applies(&many, &it(&[("/a", false), ("/b", false)]), None).is_ok());
    assert!(core::applies(&many, &it(&[("/a", false); 4]), None).is_err());
    assert!(
        core::applies(&action("x {files}", json!({})), &[], None).is_err(),
        "min defaults to 1"
    );
    let dest = action("cp {files} {dest}/", json!({}));
    assert!(core::applies(&dest, &it(&[("/a", false)]), None).is_err());
    assert!(core::applies(&dest, &it(&[("/a", false)]), Some("/b")).is_ok());
}

#[test]
fn split_name_matches_python() {
    let s = |p: &str| core::split_name(p);
    assert_eq!(
        s("/a/b.tar.gz"),
        ("b.tar.gz".into(), "b.tar".into(), "gz".into())
    );
    assert_eq!(
        s("/a/.bashrc"),
        (".bashrc".into(), ".bashrc".into(), "".into())
    );
    assert_eq!(s("/a/dir/"), ("dir".into(), "dir".into(), "".into()));
    assert_eq!(s("/a/x."), ("x.".into(), "x".into(), "".into()));
    assert_eq!(s("/a/..a.b"), ("..a.b".into(), "..a".into(), "b".into()));
}

#[test]
fn messages() {
    assert_eq!(
        core::fill_message("Copied {count} path{s}", 1, "T"),
        "Copied 1 path"
    );
    assert_eq!(
        core::fill_message("Copied {count} path{s} ({title})", 2, "T"),
        "Copied 2 paths (T)"
    );
}

// ------------------------------------------------------------------ language

fn resize(extra: serde_json::Value) -> core::Action {
    let mut raw = json!({"id": "resize-50", "title": "Resize Images to 50%", "command": "x {file}",
        "message": "Resized {count} image{s}"});
    raw.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    core::normalize_action(&raw).unwrap()
}

#[test]
fn default_titles_follow_the_language() {
    let a = resize(json!({}));
    with_lang(Lang::En, || {
        assert_eq!(core::title(&a), "Resize Images to 50%");
        assert_eq!(core::message(&a, 2).unwrap(), "Resized 2 images");
    });
    with_lang(Lang::De, || {
        assert_eq!(core::title(&a), "Bilder auf 50 % verkleinern");
        assert_eq!(core::message(&a, 1).unwrap(), "1 Bild verkleinert");
        assert_eq!(core::message(&a, 1200).unwrap(), "1.200 Bilder verkleinert");
    });
}

#[test]
fn user_titles_are_kept() {
    let a = resize(json!({"title": "Halbieren", "message": "Done: {count}"}));
    with_lang(Lang::De, || {
        assert_eq!(core::title(&a), "Halbieren");
        assert_eq!(core::message(&a, 3).unwrap(), "Done: 3");
    });
}

#[test]
fn title_de() {
    let a = resize(
        json!({"id": "mine", "title": "Shrink", "title_de": "Verkleinern", "message": "Shrunk {count}",
        "message_de": "{count} verkleinert", "confirm": "Sure?", "confirm_de": "{count} verkleinern?"}),
    );
    with_lang(Lang::En, || {
        assert_eq!(core::title(&a), "Shrink");
        assert_eq!(core::confirm_text(&a, 2).unwrap(), "Sure?");
    });
    with_lang(Lang::De, || {
        assert_eq!(core::title(&a), "Verkleinern");
        assert_eq!(core::message(&a, 4).unwrap(), "4 verkleinert");
        assert_eq!(core::confirm_text(&a, 2).unwrap(), "2 verkleinern?");
    });
}

#[test]
fn default_confirm_is_translated() {
    let (actions, _) = core::parse_actions(core::DEFAULT_ACTIONS).unwrap();
    let rsync = actions.iter().find(|a| a.id == "rsync-other").unwrap();
    with_lang(Lang::De, || {
        assert!(core::confirm_text(rsync, 1)
            .unwrap()
            .starts_with("1 Objekt mit rsync"));
        assert_eq!(core::title(rsync), "Per rsync in anderen Bereich");
    });
    with_lang(Lang::En, || {
        assert!(core::confirm_text(rsync, 3)
            .unwrap()
            .starts_with("Copy 3 items"))
    });
}

#[test]
fn reasons_and_errors_in_german() {
    with_lang(Lang::De, || {
        let a = resize(json!({"applies_to": ["jpg"]}));
        let it = vec![("/a/b.gif".to_string(), false)];
        assert_eq!(
            core::applies(&a, &it, None).unwrap_err(),
            "sie funktioniert nicht mit „gif“-Dateien"
        );
        assert_eq!(
            core::applies(&a, &[], None).unwrap_err(),
            "wähle mindestens 1 Objekt aus"
        );
        let (_, errors) = core::parse_actions(r#"[{"id": "x", "command": "true"}]"#).unwrap();
        assert_eq!(errors, vec!["Aktion 'x': „title“ fehlt".to_string()]);
    });
}
