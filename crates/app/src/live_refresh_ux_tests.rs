//! Live folder refresh: files created, removed or renamed in the shown
//! folder by another program appear / disappear without any user action,
//! and the selection, cursor and type-ahead stay. Real FSEvents; the test
//! only waits (bounded) for the result, it doesn't time anything.

use super::*;

fn live(fx: &Fixture) -> H {
    harness_init(
        fx,
        WIDE,
        Theme::Light,
        false,
        Box::new(|ctx: &egui::Context| PluginHost::new(ctx.clone(), Vec::new(), None)),
        |init| init.live_refresh = Some(true),
    )
}

/// Step until `cond` holds (the watcher, debounce and listing thread run
/// in the background); fails after a generous bound.
fn wait_until(h: &mut H, what: &str, cond: impl Fn(&MantaApp) -> bool) {
    let start = Instant::now();
    while !cond(h.state()) {
        assert!(
            start.elapsed() < crate::test_support::wait(),
            "timed out waiting for {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(2);
}

fn names(a: &MantaApp, side: Side) -> Vec<String> {
    a.pane(side)
        .tab()
        .entries
        .iter()
        .map(|e| e.name.clone())
        .collect()
}

#[test]
fn outside_changes_show_up_and_the_selection_stays() {
    let fx = fixture();
    let mut h = live(&fx);
    assert!(app(&mut h).live.enabled(), "FSEvents watcher available");
    select(&mut h, Side::Left, &["README.md", "notes.txt"]);
    {
        let tab = app(&mut h).pane_mut(Side::Left).tab_mut();
        tab.type_ahead = "rea".into();
    }
    let listings = app(&mut h).pane(Side::Left).tab().listing_gen;

    // Another program creates a file.
    std::fs::write(fx.root.join("from-outside.txt"), "hello").unwrap();
    wait_until(&mut h, "the new file", |a| {
        names(a, Side::Left).iter().any(|n| n == "from-outside.txt")
    });
    let tab = app(&mut h).pane(Side::Left).tab();
    assert!(tab.listing_gen > listings, "reloaded by itself");
    assert!(!tab.loading, "quiet reload");
    assert_eq!(
        tab.selected.iter().cloned().collect::<Vec<_>>(),
        vec!["README.md".to_string(), "notes.txt".to_string()]
    );
    assert_eq!(
        tab.cursor_entry().map(|e| e.name.as_str()),
        Some("notes.txt"),
        "cursor stays on its file"
    );
    assert_eq!(tab.type_ahead, "rea");

    // ... removes one of the selected files and renames another file.
    std::fs::remove_file(fx.root.join("notes.txt")).unwrap();
    std::fs::rename(fx.root.join("data.json"), fx.root.join("data-renamed.json")).unwrap();
    wait_until(&mut h, "the removal and rename", |a| {
        let n = names(a, Side::Left);
        !n.iter().any(|x| x == "notes.txt") && n.iter().any(|x| x == "data-renamed.json")
    });
    assert!(!names(app(&mut h), Side::Left)
        .iter()
        .any(|n| n == "data.json"));
    let tab = app(&mut h).pane(Side::Left).tab();
    let tab_sel: Vec<String> = tab.selected.iter().cloned().collect();
    assert_eq!(
        tab_sel,
        vec!["README.md".to_string()],
        "the removed file leaves the selection, the rest stays"
    );

    // The right pane shows Design/: a change there updates only it.
    let right = app(&mut h).pane(Side::Right).tab().listing_gen;
    std::fs::write(fx.root.join("Design/new-mockup.png"), "x").unwrap();
    wait_until(&mut h, "the right pane", |a| {
        names(a, Side::Right).iter().any(|n| n == "new-mockup.png")
    });
    assert!(app(&mut h).pane(Side::Right).tab().listing_gen > right);
}

#[test]
fn no_reload_while_a_name_is_typed() {
    let fx = fixture();
    let mut h = live(&fx);
    // Rename dialog open for the left tab.
    let a = app(&mut h);
    let tab = a.pane(Side::Left).tab();
    let form = crate::dialogs::NameForm::new(
        tab.id,
        tab.loc.fs.clone(),
        tab.loc.path.clone(),
        Some("README.md".into()),
        Vec::new(),
    );
    a.dialog = Some(Dialog::Name(form));
    let gen = a.pane(Side::Left).tab().listing_gen;
    std::fs::write(fx.root.join("while-renaming.txt"), "x").unwrap();
    // The change is noticed (deferred), but the listing stays as it was.
    wait_until(&mut h, "the event", |a| a.live.has_pending());
    for _ in 0..20 {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            app(&mut h).pane(Side::Left).tab().listing_gen,
            gen,
            "reloaded under the rename"
        );
    }
    // Done typing: the folder catches up.
    app(&mut h).dialog = None;
    wait_until(&mut h, "the deferred reload", |a| {
        names(a, Side::Left)
            .iter()
            .any(|n| n == "while-renaming.txt")
    });
}
