//! Finder features in the real app: tags (dots, the Tags submenu, the
//! sidebar's tags, `tag:` filters), searching across folders (scope bar,
//! results as a virtual location, reveal, cancel) and Get Info windows.
//!
//! Everything runs on the temp fixture: tags are real xattrs on temp files,
//! Spotlight is a fake (`FakeSpotlight`), Get Info's system lookups too.
//! `render_finder_features` writes PNGs to `$MANTA_RENDER_DIR/finder`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Theme};
use manta_plugin::host::PluginHost;

use super::{
    app, fixture, harness_init, render_dir, save_to, select, settle, wgpu_available, Fixture, H,
    WIDE,
};
use crate::app::{Action, Side};
use crate::search::{FakeSpotlight, Query, Scope};
use crate::tags::{self, Tag, TagColor, TagEdit, TagStore};

fn red() -> Tag {
    Tag::new("Rot", TagColor::Red)
}

fn work() -> Tag {
    Tag::new("Arbeit", TagColor::None)
}

/// A harness with Finder's tag list (Red, Blue, "Arbeit") and a fake
/// Spotlight that answers with `spotlight` paths.
fn harness_finder(fx: &Fixture, gpu: bool, spotlight: Arc<FakeSpotlight>) -> H {
    harness_init(
        fx,
        WIDE,
        Theme::Light,
        gpu,
        Box::new(|ctx: &egui::Context| PluginHost::new(ctx.clone(), Vec::new(), None)),
        move |init| {
            init.tags = Some(TagStore::new(
                Box::new(|| {
                    vec![
                        Tag::new("Rot", TagColor::Red),
                        Tag::new("Blau", TagColor::Blue),
                        Tag::new("Arbeit", TagColor::None),
                    ]
                }),
                true,
            ));
            init.spotlight = Some(spotlight);
        },
    )
}

fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    h.run_steps(3);
}

/// Step until `f` holds (background threads).
fn wait_for(h: &mut H, what: &str, mut f: impl FnMut(&mut H) -> bool) {
    let start = Instant::now();
    while !f(h) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn names(h: &mut H, side: Side) -> Vec<String> {
    let tab = app(h).pane(side).tab();
    tab.view
        .iter()
        .filter_map(|&i| tab.entries.get(i))
        .map(|e| e.name.clone())
        .collect()
}

fn tag_file(fx: &Fixture, name: &str, t: &Tag) {
    tags::set_tag(&fx.root.join(name), t, true).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn tags_are_read_shown_filtered_and_changed() {
    let fx = fixture();
    tag_file(&fx, "README.md", &red());
    tag_file(&fx, "README.md", &work());
    tag_file(&fx, "src", &Tag::new("Blau", TagColor::Blue));
    let mut h = harness_finder(&fx, false, Arc::new(FakeSpotlight::default()));
    settle(&mut h);
    let tab = app(&mut h).pane(Side::Left).tab();
    assert_eq!(tab.tags_of("README.md"), &[red(), work()]);
    assert_eq!(tab.tags_of("src"), &[Tag::new("Blau", TagColor::Blue)]);
    assert!(tab.tags_of("notes.txt").is_empty());

    // `tag:` in the filter field: by name or by colour.
    app(&mut h).pane_mut(Side::Left).tab_mut().filter = "tag:rot".into();
    app(&mut h).pane_mut(Side::Left).tab_mut().rebuild_view();
    assert_eq!(names(&mut h, Side::Left), vec!["README.md"]);
    app(&mut h).pane_mut(Side::Left).tab_mut().filter = "tag:blue".into();
    app(&mut h).pane_mut(Side::Left).tab_mut().rebuild_view();
    assert_eq!(names(&mut h, Side::Left), vec!["src"]);
    app(&mut h).pane_mut(Side::Left).tab_mut().filter = "tag:arbeit read".into();
    app(&mut h).pane_mut(Side::Left).tab_mut().rebuild_view();
    assert_eq!(names(&mut h, Side::Left), vec!["README.md"]);
    app(&mut h).pane_mut(Side::Left).tab_mut().filter.clear();
    app(&mut h).pane_mut(Side::Left).tab_mut().rebuild_view();

    // The context menu offers the tags of the selection.
    select(&mut h, Side::Left, &["README.md", "notes.txt"]);
    let ctx = h.ctx.clone();
    let menu = crate::pane::MenuCtx::of(&ctx, app(&mut h).pane(Side::Left).tab(), false);
    let m = menu
        .tag_menu()
        .expect("local items have a Tags menu")
        .clone();
    assert_eq!(m.items, 2);
    assert_eq!(m.count(&red()), 1);
    assert_eq!(
        m.standard[0],
        Tag::new("Rot", TagColor::Red),
        "Finder's name for red"
    );
    assert!(m.custom.iter().any(|t| t.name == "Arbeit"));

    // Add red to both: written to the files, shown after the next read.
    act(
        &mut h,
        Action::SetTag(
            Side::Left,
            TagEdit {
                tag: red(),
                on: true,
            },
        ),
    );
    assert_eq!(
        tags::read_tags(&fx.root.join("notes.txt")).unwrap(),
        vec![red()]
    );
    assert_eq!(
        tags::read_tags(&fx.root.join("README.md")).unwrap(),
        vec![red(), work()]
    );
    // The listing changes on disk are picked up with the next listing.
    act(&mut h, Action::Refresh(Side::Left));
    settle(&mut h);
    assert_eq!(
        app(&mut h).pane(Side::Left).tab().tags_of("notes.txt"),
        &[red()]
    );
    // Remove it again.
    select(&mut h, Side::Left, &["notes.txt"]);
    act(
        &mut h,
        Action::SetTag(
            Side::Left,
            TagEdit {
                tag: red(),
                on: false,
            },
        ),
    );
    assert!(tags::read_tags(&fx.root.join("notes.txt"))
        .unwrap()
        .is_empty());
    // A server has no tags.
    let ctx = h.ctx.clone();
    let menu = crate::pane::MenuCtx::of(&ctx, app(&mut h).pane(Side::Left).tab(), false);
    assert!(menu.tag_menu().is_some());
}

#[test]
fn sidebar_tag_searches_this_mac_with_spotlight() {
    let fx = fixture();
    let fake = Arc::new(FakeSpotlight::default());
    *fake.results.lock().unwrap() = vec![fx.root.join("README.md"), fx.root.join("src/main.rs")];
    let mut h = harness_finder(&fx, false, fake.clone());
    // Finder's tag list is the sidebar's.
    assert_eq!(app(&mut h).tags.sidebar().len(), 3);
    act(&mut h, Action::ShowTag("Rot".into()));
    let side = app(&mut h).active_pane();
    wait_for(&mut h, "the tag search", |h| {
        app(h)
            .pane(side)
            .tab()
            .search
            .as_ref()
            .is_some_and(|s| !s.running)
    });
    h.run_steps(3);
    let q = fake.queries.lock().unwrap().clone();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].0, "kMDItemUserTags == \"Rot\"c");
    let tab = app(&mut h).pane(side).tab();
    assert_eq!(tab.search.as_ref().unwrap().query, Query::Tag("Rot".into()));
    assert_eq!(tab.title(), "Tag “Rot”");
    let mut got = names(&mut h, side);
    got.sort();
    let mut want = vec![
        fx.root.join("README.md").to_string_lossy().into_owned(),
        fx.root.join("src/main.rs").to_string_lossy().into_owned(),
    ];
    want.sort();
    assert_eq!(got, want, "named by their full paths");
    // Back: the folder again.
    act(&mut h, Action::Back(side));
    settle(&mut h);
    assert!(!app(&mut h).pane(side).tab().in_search());
    assert!(names(&mut h, side).contains(&"README.md".to_string()));
}

#[test]
fn subfolder_search_shows_results_reveals_and_ends() {
    let fx = fixture();
    let mut h = harness_finder(&fx, false, Arc::new(FakeSpotlight::default()));
    let side = Side::Left;
    // Typing with the Subfolders scope searches below the folder.
    act(&mut h, Action::SetSearchScope(side, Scope::Subfolders));
    {
        let tab = app(&mut h).pane_mut(side).tab_mut();
        assert_eq!(tab.scope, Scope::Subfolders);
        tab.filter = "main".into();
        tab.search_due = Some(Instant::now());
    }
    wait_for(&mut h, "the search", |h| {
        app(h)
            .pane(side)
            .tab()
            .search
            .as_ref()
            .is_some_and(|s| !s.running)
    });
    h.run_steps(3);
    assert_eq!(names(&mut h, side), vec!["src/main.rs"]);
    let tab = app(&mut h).pane(side).tab();
    assert!(tab.in_search());
    assert_eq!(tab.title(), "Search: “main”");
    assert_eq!(
        crate::search::status_text(tab.search.as_ref().unwrap()),
        "1 item found"
    );
    // Patterns, and hidden folders stay out.
    app(&mut h).pane_mut(side).tab_mut().filter = "*.rs".into();
    app(&mut h).pane_mut(side).tab_mut().search_due = Some(Instant::now());
    h.run_steps(2);
    wait_for(&mut h, "the second search", |h| {
        app(h)
            .pane(side)
            .tab()
            .search
            .as_ref()
            .is_some_and(|s| !s.running)
    });
    h.run_steps(3);
    assert_eq!(names(&mut h, side), vec!["src/lib.rs", "src/main.rs"]);

    // Show in Enclosing Folder: the folder, with the item selected.
    select(&mut h, side, &["src/main.rs"]);
    act(&mut h, Action::RevealResult(side));
    settle(&mut h);
    let tab = app(&mut h).pane(side).tab();
    assert!(!tab.in_search());
    assert_eq!(tab.loc.path.file_name(), "src");
    assert!(tab.selected.contains("main.rs"));
    assert_eq!(tab.scope, Scope::Folder, "navigating ends the search scope");

    // Clearing the query ends a search; the folder is listed again.
    act(&mut h, Action::Back(side));
    settle(&mut h);
    act(&mut h, Action::SetSearchScope(side, Scope::Subfolders));
    app(&mut h).pane_mut(side).tab_mut().filter = "notes".into();
    app(&mut h).pane_mut(side).tab_mut().search_due = Some(Instant::now());
    wait_for(&mut h, "the third search", |h| {
        app(h)
            .pane(side)
            .tab()
            .search
            .as_ref()
            .is_some_and(|s| !s.running)
    });
    act(&mut h, Action::EndSearch(side));
    settle(&mut h);
    assert!(!app(&mut h).pane(side).tab().in_search());
    assert!(names(&mut h, side).len() > 5, "{:?}", names(&mut h, side));

    // This Folder again: the text filters the folder (no search).
    act(&mut h, Action::SetSearchScope(side, Scope::Folder));
    app(&mut h).pane_mut(side).tab_mut().filter = "notes".into();
    app(&mut h).pane_mut(side).tab_mut().rebuild_view();
    assert_eq!(names(&mut h, side), vec!["notes.txt"]);
}

#[test]
fn search_results_are_pruned_after_file_operations() {
    let fx = fixture();
    let mut h = harness_finder(&fx, false, Arc::new(FakeSpotlight::default()));
    let side = Side::Left;
    act(&mut h, Action::SetSearchScope(side, Scope::Subfolders));
    app(&mut h).pane_mut(side).tab_mut().filter = ".rs".into();
    app(&mut h).pane_mut(side).tab_mut().search_due = Some(Instant::now());
    wait_for(&mut h, "the search", |h| {
        app(h)
            .pane(side)
            .tab()
            .search
            .as_ref()
            .is_some_and(|s| !s.running)
    });
    h.run_steps(3);
    assert_eq!(names(&mut h, side).len(), 2);
    std::fs::remove_file(fx.root.join("src/lib.rs")).unwrap();
    let fs = app(&mut h).pane(side).tab().loc.fs.clone();
    app(&mut h).prune_search_results(&fs);
    assert_eq!(names(&mut h, side), vec!["src/main.rs"]);
    // ⌘R runs the search again.
    act(&mut h, Action::Refresh(side));
    wait_for(&mut h, "the search again", |h| {
        app(h)
            .pane(side)
            .tab()
            .search
            .as_ref()
            .is_some_and(|s| !s.running)
    });
    h.run_steps(3);
    assert_eq!(names(&mut h, side), vec!["src/main.rs"]);
}

#[cfg(target_os = "macos")]
#[test]
fn get_info_windows() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture();
    tag_file(&fx, "notes.txt", &red());
    let mut h = harness_finder(&fx, false, Arc::new(FakeSpotlight::default()));
    let side = Side::Left;
    select(&mut h, side, &["notes.txt"]);
    act(&mut h, Action::GetInfo(side));
    // Again for the same item: brought to the front, not a second window.
    act(&mut h, Action::GetInfo(side));
    assert_eq!(app(&mut h).info.windows.len(), 1);
    app(&mut h).info.wait(Duration::from_secs(10));
    h.run_steps(3);
    let w = &app(&mut h).info.windows[0];
    let d = w.details().expect("details read");
    assert_eq!(d.tags, vec![red()]);
    assert!(d.owner.is_some());
    assert_eq!(w.name_edit, "notes.txt");

    // Permissions: chmod from the window.
    app(&mut h).info.windows[0].apply_mode_for_test(0o600);
    let mode = std::fs::metadata(fx.root.join("notes.txt"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // Hidden flag and tags from the window.
    app(&mut h).info.windows[0].apply_flag_for_test(crate::info::UF_HIDDEN, true);
    assert_ne!(
        crate::info::flags(&fx.root.join("notes.txt")).unwrap() & crate::info::UF_HIDDEN,
        0
    );
    app(&mut h).info.windows[0].apply_flag_for_test(crate::info::UF_HIDDEN, false);
    app(&mut h).info.windows[0].apply_tag_for_test(&work(), true);
    assert_eq!(
        tags::read_tags(&fx.root.join("notes.txt")).unwrap(),
        vec![red(), work()]
    );

    // Rename from the window.
    let fs = app(&mut h).pane(side).tab().loc.fs.clone();
    let from = app(&mut h).info.windows[0].target.items[0].path.clone();
    app(&mut h).rename_from_info(fs, from, "todo.txt".into());
    settle(&mut h);
    wait_for(&mut h, "the rename", |_| fx.root.join("todo.txt").exists());
    assert!(!fx.root.join("notes.txt").exists());

    // A multiple selection: one summary window with the total size.
    select(&mut h, side, &["README.md", "src"]);
    act(&mut h, Action::GetInfo(side));
    assert_eq!(app(&mut h).info.windows.len(), 2);
    app(&mut h).info.wait(Duration::from_secs(10));
    let w = &app(&mut h).info.windows[1];
    assert_eq!(w.target.items.len(), 2);
    let size = w.size();
    assert!(size.done);
    assert_eq!(size.files, 3, "README.md and the two files in src");

    // At most a few at a time: the oldest closes.
    for name in [
        "data.json",
        "install.sh",
        "report-q3.pdf",
        "Design",
        "photos",
        "Archive 2025",
    ] {
        select(&mut h, side, &[name]);
        act(&mut h, Action::GetInfo(side));
    }
    assert_eq!(app(&mut h).info.windows.len(), crate::info::MAX_WINDOWS);
    // The windows are auxiliary windows (focus tracking knows them).
    assert_eq!(
        app(&mut h).open_aux_windows().len(),
        crate::info::MAX_WINDOWS
    );
}

/// Screenshots: tag dots in the list and the Tags submenu, a subfolder
/// search with the scope bar and Path column, an Info window.
#[cfg(target_os = "macos")]
#[test]
fn render_finder_features() {
    if std::env::var_os("MANTA_RENDER_DIR").is_none() || !wgpu_available() {
        return;
    }
    let dir = render_dir("finder");
    let fx = fixture();
    tag_file(&fx, "README.md", &red());
    tag_file(&fx, "README.md", &Tag::new("Blau", TagColor::Blue));
    tag_file(&fx, "report-q3.pdf", &Tag::new("Grün", TagColor::Green));
    tag_file(&fx, "src", &Tag::new("Orange", TagColor::Orange));
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness_finder(&fx, true, Arc::new(FakeSpotlight::default()));
        h.ctx.set_theme(theme);
        crate::i18n::set_locale(crate::i18n::Locale::De);
        settle(&mut h);
        select(&mut h, Side::Left, &["README.md"]);
        save_to(&mut h, &dir, &format!("tags_list_{tname}"));
        // The context menu with its Tags submenu.
        let rect = app(&mut h)
            .pane_rects
            .iter()
            .find(|(s, _)| *s == Side::Left)
            .unwrap()
            .1;
        let idx = app(&mut h)
            .pane(Side::Left)
            .tab()
            .view_index_of("README.md")
            .unwrap();
        let y = rect.top() + 1.0 + 36.0 + 24.0 + idx as f32 * 24.0 + 12.0;
        let pos = egui::pos2(rect.left() + 120.0, y);
        h.hover_at(pos);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            });
        }
        h.run_steps(3);
        // Hover the Tags item (fourth in the menu) to open the submenu.
        h.hover_at(pos + egui::vec2(40.0, 14.0 + 3.0 * 30.0));
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("tags_menu_{tname}"));
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        // Icons view with dots.
        act(
            &mut h,
            Action::SetView(Side::Left, crate::views::ViewMode::Icons),
        );
        save_to(&mut h, &dir, &format!("tags_icons_{tname}"));
        act(
            &mut h,
            Action::SetView(Side::Left, crate::views::ViewMode::List),
        );

        // Subfolder search.
        act(
            &mut h,
            Action::SetSearchScope(Side::Left, Scope::Subfolders),
        );
        app(&mut h).pane_mut(Side::Left).tab_mut().filter = "r".into();
        app(&mut h).pane_mut(Side::Left).tab_mut().search_due = Some(Instant::now());
        wait_for(&mut h, "the search", |h| {
            app(h)
                .pane(Side::Left)
                .tab()
                .search
                .as_ref()
                .is_some_and(|s| !s.running)
        });
        save_to(&mut h, &dir, &format!("search_{tname}"));
        act(&mut h, Action::EndSearch(Side::Left));
        settle(&mut h);

        // Get Info.
        select(&mut h, Side::Left, &["README.md"]);
        act(&mut h, Action::GetInfo(Side::Left));
        app(&mut h).info.wait(Duration::from_secs(10));
        save_to(&mut h, &dir, &format!("info_{tname}"));
        crate::i18n::set_thread_locale(None);
    }
    let _ = Path::new("");
}
