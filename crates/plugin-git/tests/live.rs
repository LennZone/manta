//! Integration tests against real temp repositories.

mod common;

use std::time::{Duration, Instant};

use common::*;
use manta_plugin::{PaneId, Plugin, Tone};
use manta_plugin_git::model::InProgress;
use manta_plugin_git::panel::{Group, PanelAction, Selection};
use manta_plugin_git::shared::{lock, PaneRepo};

const L: PaneId = PaneId::Left;
const R: PaneId = PaneId::Right;

/// A repo with every kind of change in it.
fn mixed_repo() -> Repo {
    let r = Repo::new();
    r.write(".gitignore", "target/\n*.log\n");
    r.write("a.txt", "one\n");
    r.write("b.txt", "two\n");
    r.write("sub/c.txt", "three\n");
    r.write("old name.txt", "rename me\n");
    r.commit_all("Initial commit");
    r.write("a.txt", "one changed\n"); // modified, unstaged
    r.write("b.txt", "two changed\n");
    r.git(&["add", "b.txt"]); // staged
    r.write("new file.txt", "fresh\n"); // untracked
    r.write("sub/c.txt", "three changed\n"); // folder aggregate
    r.write("target/debug/app", "bin"); // ignored dir
    r.write("build.log", "log"); // ignored file
    r.git(&["mv", "old name.txt", "new name.txt"]); // renamed
    r
}

#[test]
fn decorations_and_chip_for_mixed_repo() {
    if !git_available() {
        return;
    }
    let r = mixed_repo();
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(
        h.wait_until(WAIT, |h| h.chip(L, &p).is_some()
            && h.deco(L, &p, "build.log").is_some()),
        "no decorations: {:?}",
        h.decorations
    );
    assert_eq!(h.badge(L, &p, "a.txt"), Some(("M".into(), Tone::Warning)));
    assert_eq!(
        h.deco(L, &p, "a.txt").unwrap().tooltip.as_deref(),
        Some("Modified, not staged")
    );
    assert_eq!(h.badge(L, &p, "b.txt"), Some(("M".into(), Tone::Success)));
    assert_eq!(
        h.deco(L, &p, "b.txt").unwrap().tooltip.as_deref(),
        Some("Staged for commit")
    );
    assert_eq!(
        h.badge(L, &p, "new file.txt"),
        Some(("?".into(), Tone::Info))
    );
    assert_eq!(
        h.badge(L, &p, "new name.txt"),
        Some(("R".into(), Tone::Accent))
    );
    assert_eq!(h.badge(L, &p, "sub"), Some(("•".into(), Tone::Warning)));
    assert_eq!(
        h.deco(L, &p, "sub").unwrap().tooltip.as_deref(),
        Some("1 changed file inside")
    );
    for ignored in ["target", "build.log"] {
        let d = h
            .deco(L, &p, ignored)
            .unwrap_or_else(|| panic!("{ignored} not decorated"));
        assert!(d.dimmed && d.badge.is_none(), "{ignored}: {d:?}");
        assert_eq!(d.tooltip.as_deref(), Some("Ignored by .gitignore"));
    }
    assert!(h.deco(L, &p, ".gitignore").is_none());
    assert!(h.deco(L, &p, ".git").is_none());

    let chip = h.chip(L, &p).unwrap();
    assert_eq!(chip.text, "main · 5");
    assert_eq!(chip.tone, Tone::Warning);
    assert_eq!(chip.icon.as_deref(), Some("branch"));
    assert_eq!(chip.command.as_deref(), Some("toggle-panel"));
    let tip = chip.tooltip.unwrap();
    assert!(tip.contains("Last commit: Initial commit"), "{tip}");
    assert!(tip.contains("No upstream"), "{tip}");

    // Subfolder in the other pane, sharing the repo worker.
    let sub = r.path.join("sub");
    h.show(R, Some(&sub));
    assert!(h.wait_until(WAIT, |h| h.badge(R, &sub, "c.txt").is_some()));
    assert_eq!(h.badge(R, &sub, "c.txt"), Some(("M".into(), Tone::Warning)));
    // Ignored folder contents are dimmed too.
    let dbg = r.path.join("target/debug");
    h.show(R, Some(&dbg));
    assert!(h.wait_until(WAIT, |h| h.deco(R, &dbg, "app").is_some_and(|d| d.dimmed)));
}

#[test]
fn clean_repo_is_success() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write("readme.md", "hi\n");
    r.commit_all("First");
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    let chip = h.chip(L, &p).unwrap();
    assert_eq!(chip.text, "main");
    assert_eq!(chip.tone, Tone::Success);
    assert!(chip.tooltip.unwrap().contains("Working tree clean"));
}

#[test]
fn non_repo_and_remote_panes_are_cleared() {
    if !git_available() {
        return;
    }
    let r = mixed_repo();
    let outside = tempfile::tempdir().unwrap();
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));

    h.show(L, Some(outside.path()));
    let o = outside.path().to_path_buf();
    assert!(h.wait_until(WAIT, |h| {
        h.decorations
            .get(&(L, vfs(&o)))
            .is_some_and(|d| d.is_empty())
            && h.chips.get(&(L, vfs(&o))).is_some_and(|c| c.is_empty())
    }));
    assert_eq!(
        lock(&h.plugin.shared()).panes.get(&L),
        Some(&PaneRepo::NotRepo)
    );
    // No pane in the repo anymore: the worker and its snapshot are dropped.
    assert!(h.wait_until(WAIT, |h| lock(&h.plugin.shared()).repos.is_empty()));

    h.show(R, None);
    let remote = common::pane_state(R, None).path;
    assert!(h.wait_until(WAIT, |h| h
        .chips
        .get(&(R, remote.clone()))
        .is_some_and(|c| c.is_empty())));
    assert_eq!(
        lock(&h.plugin.shared()).panes.get(&R),
        Some(&PaneRepo::NotLocal)
    );
}

#[test]
fn upstream_ahead_behind() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let remote = tmp.path().join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);

    let r = Repo::new();
    r.write("a.txt", "a\n");
    r.commit_all("Base");
    r.git(&["remote", "add", "origin", &remote.to_string_lossy()]);
    r.git(&["push", "-q", "-u", "origin", "main"]);

    // Another clone pushes one commit (→ we are 1 behind after fetch).
    let other = tmp.path().join("other");
    git(
        tmp.path(),
        &["clone", "-q", &remote.to_string_lossy(), "other"],
    );
    configure(&other);
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "Theirs"]);
    git(&other, &["push", "-q"]);

    r.write("c.txt", "c\n");
    r.commit_all("Mine 1");
    r.write("d.txt", "d\n");
    r.commit_all("Mine 2");
    r.git(&["fetch", "-q"]);

    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    let chip = h.chip(L, &p).unwrap();
    assert_eq!(chip.text, "main ↑2 ↓1");
    assert_eq!(chip.tone, Tone::Success);
    let tip = chip.tooltip.unwrap();
    assert!(
        tip.contains("Tracking origin/main (2 ahead, 1 behind)"),
        "{tip}"
    );
    assert!(tip.contains("Last commit: Mine 2"), "{tip}");
}

#[test]
fn merge_conflict() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write("shared.txt", "base\n");
    r.write("dir/inner.txt", "base\n");
    r.commit_all("Base");
    r.git(&["checkout", "-q", "-b", "other"]);
    r.write("shared.txt", "theirs\n");
    r.write("dir/inner.txt", "theirs\n");
    r.commit_all("Theirs");
    r.git(&["checkout", "-q", "main"]);
    r.write("shared.txt", "ours\n");
    r.write("dir/inner.txt", "ours\n");
    r.commit_all("Ours");
    assert!(
        !git_try(&r.path, &["merge", "-q", "other"]),
        "merge should conflict"
    );

    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    assert_eq!(
        h.badge(L, &p, "shared.txt"),
        Some(("U".into(), Tone::Danger))
    );
    assert_eq!(
        h.deco(L, &p, "shared.txt").unwrap().tooltip.as_deref(),
        Some("Merge conflict")
    );
    assert_eq!(h.badge(L, &p, "dir"), Some(("•".into(), Tone::Danger)));
    let chip = h.chip(L, &p).unwrap();
    assert_eq!(chip.tone, Tone::Danger);
    assert!(chip.tooltip.unwrap().contains("Merge in progress"));
    let sh = h.plugin.shared();
    let snap = lock(&sh).repos.values().next().cloned().unwrap();
    assert_eq!(snap.in_progress, Some(InProgress::Merge));
    assert_eq!(snap.status.as_ref().unwrap().conflicts(), 2);
}

#[test]
fn detached_head_and_initial_commit() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write("x.txt", "x\n");
    r.git(&["add", "x.txt"]);
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    let chip = h.chip(L, &p).unwrap();
    assert_eq!(chip.text, "main · 1");
    assert!(chip.tooltip.unwrap().contains("No commits yet"));
    assert_eq!(h.badge(L, &p, "x.txt"), Some(("A".into(), Tone::Success)));

    r.git(&["commit", "-q", "-m", "one"]);
    let sha = r
        .git(&["rev-parse", "--short=7", "HEAD"])
        .trim()
        .to_string();
    r.git(&["checkout", "-q", "--detach"]);
    assert!(
        h.wait_until(WAIT, |h| h.chip(L, &p).is_some_and(|c| c.text == sha)),
        "chip: {:?}",
        h.chip(L, &p)
    );
}

/// The status generation of the (only) watched repository.
fn generation(sh: &manta_plugin_git::shared::SharedRef) -> u64 {
    lock(sh)
        .repos
        .values()
        .next()
        .map(|s| s.generation)
        .unwrap_or(0)
}

#[test]
fn live_update_after_file_save() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write("notes.txt", "a\n");
    r.write("other.txt", "b\n");
    r.commit_all("Base");
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h
        .chip(L, &p)
        .is_some_and(|c| c.tone == Tone::Success)));
    // Let the watcher settle (FSEvents may replay the setup writes).
    let sh = h.plugin.shared();
    h.wait_quiet(Duration::from_millis(1000), || generation(&sh));

    let start = Instant::now();
    r.write("notes.txt", "a\nedited in an editor\n");
    assert!(
        h.wait_until(WAIT, |h| h.badge(L, &p, "notes.txt").is_some()),
        "no live update"
    );
    let took = start.elapsed();
    eprintln!("live update took {took:?}");
    assert_eq!(
        h.badge(L, &p, "notes.txt"),
        Some(("M".into(), Tone::Warning))
    );
    assert_eq!(h.chip(L, &p).unwrap().tone, Tone::Warning);

    // Staging from a terminal (index change inside .git) also updates.
    r.git(&["add", "notes.txt"]);
    assert!(h.wait_until(WAIT, |h| {
        h.badge(L, &p, "notes.txt") == Some(("M".into(), Tone::Success))
    }));
    // Commit → clean again.
    r.git(&["commit", "-q", "-m", "Edit"]);
    assert!(h.wait_until(WAIT, |h| {
        h.badge(L, &p, "notes.txt").is_none()
            && h.chip(L, &p).is_some_and(|c| c.tone == Tone::Success)
    }));
}

#[test]
fn ignored_build_output_does_not_trigger_status() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write(".gitignore", "target/\n");
    r.write("src.txt", "a\n");
    r.commit_all("Base");
    std::fs::create_dir_all(r.path.join("target")).unwrap();
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    let sh = h.plugin.shared();
    let generation = || generation(&sh);
    // Settled: no status run for the setup any more.
    h.wait_quiet(Duration::from_millis(1500), generation);
    let g0 = generation();
    for i in 0..20 {
        r.write(&format!("target/out{i}.o"), "obj");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(generation(), g0, "status ran for ignored files");
    r.write("src.txt", "changed\n");
    assert!(h.wait_until(WAIT, |h| h.badge(L, &p, "src.txt").is_some()));
    assert!(generation() > g0);
}

#[test]
fn burst_of_changes_is_coalesced() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write("a.txt", "a\n");
    r.commit_all("Base");
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    let sh = h.plugin.shared();
    let generation = || generation(&sh);
    h.wait_quiet(Duration::from_millis(1500), generation);
    let g0 = generation();
    // One burst, as fast as the machine writes (a paced burst stretches
    // under load and then legitimately needs more runs).
    let burst = Instant::now();
    for i in 0..50 {
        r.write(&format!("f{i}.txt"), "x");
    }
    let took = burst.elapsed();
    assert!(h.wait_until(WAIT, |h| h.badge(L, &p, "f49.txt").is_some()));
    h.wait_quiet(Duration::from_millis(1500), generation);
    let runs = generation() - g0;
    eprintln!("50 writes in {took:?} → {runs} status runs");
    // At most one run per MAX_DELAY window the burst spans, plus the last.
    let allowed = 2 + (took.as_millis() / 1000) as u64;
    assert!(
        runs <= allowed,
        "too many status runs: {runs} (allowed {allowed})"
    );
}

#[test]
fn stage_unstage_commit_and_diff() {
    if !git_available() {
        return;
    }
    let r = Repo::new();
    r.write("a.txt", "line 1\nline 2\n");
    r.commit_all("Base");
    r.write("a.txt", "line 1\nline two\n");
    r.write("new.txt", "hello\nworld\n");
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.badge(L, &p, "a.txt").is_some()));
    let root = lock(&h.plugin.shared())
        .repos
        .keys()
        .next()
        .cloned()
        .unwrap();

    // Diff of the unstaged change.
    let sel = Selection {
        root: root.clone(),
        group: Group::Changes,
        path: "a.txt".into(),
        orig: None,
    };
    h.plugin.select_for_test(Some(sel));
    let sh = h.plugin.shared();
    assert!(h.wait_until(WAIT, |_| lock(&sh)
        .diff
        .as_ref()
        .is_some_and(|d| d.result.is_some())));
    let d = lock(&sh).diff.clone().unwrap().result.unwrap().unwrap();
    assert_eq!((d.adds, d.dels), (1, 1));
    // Untracked file shows as added lines.
    let sel = Selection {
        root: root.clone(),
        group: Group::Untracked,
        path: "new.txt".into(),
        orig: None,
    };
    h.plugin.select_for_test(Some(sel));
    assert!(h.wait_until(WAIT, |_| lock(&sh)
        .diff
        .as_ref()
        .is_some_and(|d| d.result.is_some())));
    let d = lock(&sh).diff.clone().unwrap().result.unwrap().unwrap();
    assert_eq!(d.adds, 2);
    assert_eq!(d.lines[1].text, "hello");

    h.plugin
        .apply(PanelAction::Stage(root.clone(), vec!["a.txt".into()]));
    assert!(h.wait_until(WAIT, |h| h.badge(L, &p, "a.txt")
        == Some(("M".into(), Tone::Success))));
    h.plugin
        .apply(PanelAction::Unstage(root.clone(), vec!["a.txt".into()]));
    assert!(h.wait_until(WAIT, |h| h.badge(L, &p, "a.txt")
        == Some(("M".into(), Tone::Warning))));
    h.plugin.apply(PanelAction::StageAll(root.clone()));
    assert!(h.wait_until(WAIT, |h| h.badge(L, &p, "new.txt")
        == Some(("A".into(), Tone::Success))));

    let seq = lock(&sh).commit_seq;
    h.plugin
        .apply(PanelAction::Commit(root.clone(), "Second commit".into()));
    assert!(h.wait_until(WAIT, |h| h
        .chip(L, &p)
        .is_some_and(|c| c.tone == Tone::Success)));
    assert_eq!(lock(&sh).commit_seq, seq + 1);
    assert!(
        h.toasts.iter().any(|t| t.contains("Second commit")),
        "{:?}",
        h.toasts
    );
    let log = r.git(&["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "Second commit");
    let snap = lock(&sh).repos.values().next().cloned().unwrap();
    assert_eq!(
        snap.commits.first().map(|c| c.subject.as_str()),
        Some("Second commit")
    );

    // An error becomes a toast.
    h.plugin
        .apply(PanelAction::Commit(root, "Nothing to commit".into()));
    assert!(
        h.wait_until(WAIT, |h| h
            .toasts
            .iter()
            .any(|t| t.starts_with("Commit failed"))),
        "{:?}",
        h.toasts
    );
}

#[test]
fn disable_stops_promptly_and_clears() {
    if !git_available() {
        return;
    }
    let r = mixed_repo();
    let mut h = FakeHost::new();
    h.show(L, Some(&r.path));
    h.show(R, Some(&r.path.join("sub")));
    let p = r.path.clone();
    assert!(h.wait_until(WAIT, |h| h.chip(L, &p).is_some()));
    let start = Instant::now();
    h.plugin.disable();
    // Promptly: not waiting for a running `git status` (generous for
    // loaded machines; a hang would take far longer).
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "disable took {:?}",
        start.elapsed()
    );
    {
        let sh = h.plugin.shared();
        let s = lock(&sh);
        assert!(s.repos.is_empty() && s.panes.is_empty());
    }
    // No more updates after disable.
    h.pump();
    let before = h.decoration_sends;
    r.write("a.txt", "after disable\n");
    std::thread::sleep(Duration::from_millis(1200));
    h.pump();
    assert_eq!(h.decoration_sends, before);
}

#[test]
fn toggle_command_toggles_panel() {
    let mut h = FakeHost::new();
    let state = common::pane_state(L, None);
    h.plugin.run_command("toggle-panel", &state);
    h.pump();
    assert_eq!(h.panel_toggles, 1);
    assert!(h.panel_requests.is_empty());
    let cmds = h.plugin.commands();
    assert!(cmds
        .iter()
        .any(|c| c.title == "Toggle Git Panel" && c.shortcut.as_deref() == Some("⇧⌘B")));
    assert!(cmds.iter().any(|c| c.title == "Refresh Git Status"));
    let panels = h.plugin.panels();
    assert_eq!(panels[0].id, "git");
    assert!(!panels[0].default_open);
}
