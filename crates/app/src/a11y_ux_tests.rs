//! Accessibility: the AccessKit tree the app hands to VoiceOver (egui →
//! AccessKit → NSAccessibility), keyboard-only use (Tab order, focus that
//! returns after a dialog), announcements and the system display options.
//!
//! The tests query the tree by role and label like a screen reader would.
//! `MANTA_A11Y_DUMP=dir` also writes the whole tree as text.

use super::*;
use egui::accesskit::{Action as AkAction, Live, Role};
use egui::{Key, Modifiers};
use egui_kittest::kittest::{self, NodeT as _, Queryable as _};

fn behaviour(fx: &Fixture) -> H {
    harness_with(fx, WIDE, Theme::Light, false)
}

/// One line per node: role, name, value, description and states.
fn describe(n: &kittest::AccessKitNode<'_>) -> String {
    let d = n.data();
    let mut s = format!("{:?}", d.role());
    if let Some(l) = n.label() {
        s.push_str(&format!(" {l:?}"));
    }
    if let Some(v) = d.value() {
        s.push_str(&format!(" value={v:?}"));
    }
    if let Some(v) = d.placeholder() {
        s.push_str(&format!(" placeholder={v:?}"));
    }
    if let Some(v) = d.description() {
        s.push_str(&format!(" desc={v:?}"));
    }
    if let Some(v) = d.numeric_value() {
        s.push_str(&format!(" num={v}"));
    }
    if let Some(t) = d.toggled() {
        s.push_str(&format!(" toggled={t:?}"));
    }
    if let Some(sel) = d.is_selected() {
        s.push_str(&format!(" selected={sel}"));
    }
    if let Some(c) = d.size_of_set() {
        s.push_str(&format!(" size_of_set={c}"));
    }
    if let Some(c) = d.row_count() {
        s.push_str(&format!(" rows={c}"));
    }
    if d.live().is_some_and(|l| l != Live::Off) {
        s.push_str(&format!(" live={:?}", d.live().unwrap()));
    }
    if d.supports_action(AkAction::Focus) {
        s.push_str(" [focusable]");
    }
    if d.supports_action(AkAction::Click) {
        s.push_str(" [click]");
    }
    if d.is_disabled() {
        s.push_str(" [disabled]");
    }
    if d.role() == Role::Unknown {
        if let Some(b) = n.raw_bounds() {
            s.push_str(&format!(
                " @({:.0},{:.0} {:.0}x{:.0})",
                b.x0,
                b.y0,
                b.width(),
                b.height()
            ));
        }
    }
    s
}

fn dump_tree(h: &H) -> String {
    fn walk(n: &kittest::AccessKitNode<'_>, depth: usize, out: &mut String) {
        out.push_str(&"  ".repeat(depth));
        out.push_str(&describe(n));
        out.push('\n');
        for c in n.children() {
            walk(&c, depth + 1, out);
        }
    }
    let mut out = String::new();
    walk(&h.root().accesskit_node(), 0, &mut out);
    out
}

/// Write the tree to `$MANTA_A11Y_DUMP/<name>.txt` (evidence for the audit).
fn maybe_dump(h: &H, name: &str) {
    if let Some(dir) = std::env::var_os("MANTA_A11Y_DUMP") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.txt"));
        std::fs::write(&path, dump_tree(h)).unwrap();
        eprintln!("a11y: wrote {}", path.display());
    }
}

#[test]
fn dump_main_window_tree() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    select(&mut h, Side::Left, &["README.md"]);
    h.run_steps(2);
    maybe_dump(&h, "main_en");
    crate::i18n::set_locale(crate::i18n::Locale::De);
    h.run_steps(3);
    maybe_dump(&h, "main_de");
    crate::i18n::set_locale(crate::i18n::Locale::En);
}

fn has_icon(s: &str) -> bool {
    s.chars().any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c))
}

/// The drag area of a window that the test harness draws inside the main
/// window (Settings, Help); the real app shows native windows instead.
fn embedded_title_bar(n: &kittest::AccessKitNode<'_>) -> bool {
    if n.data().role() != Role::Unknown {
        return false;
    }
    let win = std::iter::successors(n.parent(), |p| p.parent())
        .find(|p| p.data().role() == Role::Window && p.parent().is_some());
    match (win.and_then(|w| w.raw_bounds()), n.raw_bounds()) {
        (Some(w), Some(b)) => (b.y0 - w.y0).abs() < 1.0 && (b.width() - w.width()).abs() < 1.0,
        _ => false,
    }
}

/// Actionable nodes VoiceOver could not describe: no role, no name, or a
/// name that is (or contains) an icon glyph.
fn problems(h: &H) -> Vec<String> {
    fn walk(n: &kittest::AccessKitNode<'_>, out: &mut Vec<String>) {
        let d = n.data();
        let actionable = d.supports_action(AkAction::Focus) || d.supports_action(AkAction::Click);
        let role = d.role();
        let name = n
            .label()
            .or_else(|| d.value().map(str::to_string))
            .or_else(|| d.placeholder().map(str::to_string))
            .unwrap_or_default();
        let structural = matches!(
            role,
            Role::GenericContainer
                | Role::Label
                | Role::TextRun
                | Role::Window
                | Role::Pane
                | Role::Heading
                | Role::ScrollView
        );
        let structural = structural || role == Role::ScrollBar || embedded_title_bar(n);
        if actionable
            && !n.is_hidden()
            && !structural
            && (role == Role::Unknown || name.trim().is_empty() || has_icon(&name))
        {
            out.push(describe(n));
        }
        for c in n.children() {
            walk(&c, out);
        }
    }
    let mut out = Vec::new();
    walk(&h.root().accesskit_node(), &mut out);
    out
}

fn set_view(h: &mut H, mode: crate::views::ViewMode) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, crate::app::Action::SetView(Side::Left, mode));
}

type Go = Box<dyn Fn(&mut H)>;

/// Every screen of the audit: (name, how to get there).
fn screens(fx: &Fixture) -> Vec<(&'static str, Go)> {
    let root = fx.root.clone();
    vec![
        ("main", Box::new(|_h: &mut H| {})),
        (
            "icons",
            Box::new(|h: &mut H| set_view(h, crate::views::ViewMode::Icons)),
        ),
        (
            "columns",
            Box::new(|h: &mut H| set_view(h, crate::views::ViewMode::Columns)),
        ),
        (
            "gallery",
            Box::new(|h: &mut H| set_view(h, crate::views::ViewMode::Gallery)),
        ),
        (
            "connect",
            Box::new(|h: &mut H| {
                app(h).dialog = Some(Dialog::Connect(ConnectForm::new(Side::Left)));
            }),
        ),
        (
            "connect_webdav",
            Box::new(|h: &mut H| {
                let mut f = ConnectForm::new(Side::Left);
                f.set_protocol_for_tests(manta_core::Protocol::WebDav);
                f.dav.set_preset("nextcloud", "anna");
                app(h).dialog = Some(Dialog::Connect(f));
            }),
        ),
        (
            "connect_smb",
            Box::new(|h: &mut H| {
                app(h).dialog = Some(Dialog::Connect(super::smb_webdav_ux::smb_form(true)));
            }),
        ),
        (
            "delete",
            Box::new(move |h: &mut H| {
                let a = app(h);
                let fs = a.local.clone();
                let paths = vec![VfsPath::new(&root.join("README.md").to_string_lossy())];
                a.dialog = Some(Dialog::Delete(DeleteForm {
                    side: Side::Left,
                    fs,
                    paths,
                    folders: Vec::new(),
                    no_trash: false,
                    in_trash: false,
                }));
            }),
        ),
        (
            "copy",
            Box::new(|h: &mut H| {
                let a = app(h);
                let fs = a.local.clone();
                let sources: Vec<VfsPath> = a
                    .pane(Side::Left)
                    .tab()
                    .selected_entries()
                    .into_iter()
                    .map(|e| e.path)
                    .collect();
                let dst = a.pane(Side::Right).tab().loc.path.clone();
                a.dialog = Some(Dialog::Transfer(TransferForm {
                    extract: false,
                    archive: None,
                    src_side: Side::Left,
                    dst_side: Side::Right,
                    targets: Vec::new(),
                    mv: false,
                    src_fs: fs.clone(),
                    sources,
                    dst_fs: fs,
                    dst_dir: dst,
                    dst_label: "Local".into(),
                    conflict: manta_ops::ConflictPolicy::Skip,
                    folders: Vec::new(),
                    choose: false,
                }));
            }),
        ),
        (
            "settings_general",
            Box::new(|h: &mut H| {
                app(h)
                    .settings
                    .show(Some(crate::settings::SettingsTab::General));
            }),
        ),
        (
            "settings_plugins",
            Box::new(|h: &mut H| {
                app(h)
                    .settings
                    .show(Some(crate::settings::SettingsTab::Plugins));
            }),
        ),
        (
            "help",
            Box::new(|h: &mut H| {
                let ctx = h.ctx.clone();
                app(h).handle_action(&ctx, crate::app::Action::ShowHelp(String::new()));
            }),
        ),
        ("palette", Box::new(|h: &mut H| app(h).palette.show())),
        (
            "toasts",
            Box::new(|h: &mut H| {
                let a = app(h);
                a.set_success("Copied 3 items");
                a.set_error_detail("Couldn't rename", "permission denied");
            }),
        ),
        (
            "transfers",
            Box::new(|h: &mut H| {
                let a = app(h);
                a.fake_jobs = Some(vec![
                    job(
                        1,
                        JobKind::Copy,
                        "Copy 3 items",
                        JobState::Running,
                        10,
                        50,
                        (1, 3),
                    ),
                    job(
                        2,
                        JobKind::Delete,
                        "Delete 4 items",
                        JobState::Done,
                        0,
                        0,
                        (4, 4),
                    ),
                    job(
                        3,
                        JobKind::Copy,
                        "Copy x",
                        JobState::Failed("denied".into()),
                        0,
                        0,
                        (0, 1),
                    ),
                ]);
                a.show_transfers = true;
            }),
        ),
        (
            "transfer_pill",
            Box::new(|h: &mut H| {
                let a = app(h);
                a.fake_jobs = Some(vec![job(
                    1,
                    JobKind::Copy,
                    "Copy 3 items",
                    JobState::Running,
                    10,
                    50,
                    (1, 3),
                )]);
                a.show_transfers = false;
            }),
        ),
        (
            "vcs_panel",
            Box::new(|h: &mut H| {
                app(h).plugins.set_panel_open(
                    "manta.git",
                    manta_plugin_git::PANEL_ID,
                    Some(PaneId::Left),
                    true,
                );
            }),
        ),
    ]
}

fn audit_harness(fx: &Fixture) -> H {
    harness_full(
        fx,
        WIDE,
        Theme::Light,
        false,
        Box::new(|ctx: &egui::Context| {
            PluginHost::new(
                ctx.clone(),
                vec![
                    Box::new(manta_plugin_git::GitPlugin::new()),
                    Box::new(manta_plugin_sysstats::SysStatsPlugin::new()),
                ],
                None,
            )
        }),
    )
}

/// The audit: every screen in German and English. With `MANTA_A11Y_DUMP`
/// the trees are written out; the test fails on unnamed or role-less
/// controls and on icon glyphs in names.
#[test]
fn every_control_has_a_role_and_a_name() {
    let fx = fixture();
    let mut all = Vec::new();
    for (locale, lname) in [
        (crate::i18n::Locale::De, "de"),
        (crate::i18n::Locale::En, "en"),
    ] {
        for (name, go) in screens(&fx) {
            let mut h = audit_harness(&fx);
            crate::i18n::set_locale(locale);
            select(&mut h, Side::Left, &["README.md"]);
            go(&mut h);
            settle(&mut h);
            h.run_steps(3);
            maybe_dump(&h, &format!("{name}_{lname}"));
            for p in problems(&h) {
                all.push(format!("{name}/{lname}: {p}"));
            }
        }
    }
    crate::i18n::set_thread_locale(None);
    assert!(
        all.is_empty(),
        "{} problem(s):\n{}",
        all.len(),
        all.join("\n")
    );
}

// ------------------------------------------------------------------ queries

/// All nodes, depth first.
fn nodes(h: &H) -> Vec<(Role, String, egui::accesskit::NodeId)> {
    fn walk(
        n: &kittest::AccessKitNode<'_>,
        out: &mut Vec<(Role, String, egui::accesskit::NodeId)>,
    ) {
        out.push((n.data().role(), n.label().unwrap_or_default(), n.locate().0));
        for c in n.children() {
            walk(&c, out);
        }
    }
    let mut out = Vec::new();
    walk(&h.root().accesskit_node(), &mut out);
    out
}

/// The accessible name of the widget holding egui's keyboard focus.
fn focused_label(h: &H) -> Option<String> {
    let id = h.ctx.memory(|m| m.focused())?.accesskit_id();
    nodes(h)
        .into_iter()
        .find(|(_, _, n)| *n == id)
        .map(|(_, l, _)| l)
}

/// A key press that first goes through `raw_input_hook`, as eframe does
/// it (the harness itself skips the hook; Tab switches panes there).
fn press(h: &mut H, mods: Modifiers, key: Key) {
    use eframe::App as _;
    let mut events = vec![egui::Event::ModifiersChanged(mods)];
    for pressed in [true, false] {
        events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: mods,
        });
    }
    events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.input_mut().events.extend(events);
    let ctx = h.ctx.clone();
    let mut raw = h.input_mut().clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
    h.run_steps(2);
}

/// Labels of the buttons inside the toolbar region.
fn toolbar_labels(h: &H) -> Vec<String> {
    fn walk(n: &kittest::AccessKitNode<'_>, inside: bool, out: &mut Vec<String>) {
        let inside = inside || n.data().role() == Role::Toolbar;
        if inside && n.data().role() == Role::Button {
            out.push(n.label().unwrap_or_default());
        }
        for c in n.children() {
            walk(&c, inside, out);
        }
    }
    let mut out = Vec::new();
    walk(&h.root().accesskit_node(), false, &mut out);
    out
}

// ------------------------------------------------------------------ names

#[test]
fn toolbar_buttons_have_localized_names() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    for (locale, expect) in [
        (crate::i18n::Locale::En, ["Refresh", "Settings…", "Connect"]),
        (
            crate::i18n::Locale::De,
            ["Aktualisieren", "Einstellungen …", "Verbinden"],
        ),
    ] {
        crate::i18n::set_locale(locale);
        h.run_steps(3);
        let labels = toolbar_labels(&h);
        assert!(labels.len() >= 15, "{locale:?}: {labels:?}");
        for l in &labels {
            assert!(!l.trim().is_empty(), "{locale:?}: unnamed toolbar button");
            assert!(!has_icon(l), "{locale:?}: icon glyph in {l:?}");
        }
        for e in expect {
            assert!(
                labels.iter().any(|l| l == e),
                "{locale:?}: no {e:?} in {labels:?}"
            );
        }
    }
    crate::i18n::set_thread_locale(None);
}

#[test]
fn file_rows_say_name_kind_size_and_selection() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    select(&mut h, Side::Left, &["report-q3.pdf"]);
    h.run_steps(2);
    let row = h
        .root()
        .query_all_by_role(Role::Row)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("report-q3.pdf"))
        })
        .expect("row for report-q3.pdf");
    let n = row.accesskit_node();
    let label = n.label().unwrap();
    assert!(label.contains("PDF document"), "{label}");
    assert!(label.contains("1.8 MB"), "{label}");
    assert!(label.contains("modified"), "{label}");
    assert_eq!(n.data().is_selected(), Some(true));
    // The list says how many rows it has.
    let table = h
        .root()
        .query_all_by_role(Role::Table)
        .find(|t| t.accesskit_node().label().as_deref() == Some("Files in Projects"))
        .expect("file table");
    assert_eq!(table.accesskit_node().data().row_count(), Some(13));
    crate::i18n::set_thread_locale(None);
}

#[test]
fn toasts_are_live_regions() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    app(&mut h).set_success("Copied 3 items");
    app(&mut h).set_error_detail("Couldn't rename", "permission denied");
    h.run_steps(2);
    let status = h
        .root()
        .query_all_by_role(Role::Status)
        .find(|n| n.accesskit_node().label().as_deref() == Some("Copied 3 items"))
        .expect("status node for the toast");
    assert_eq!(status.accesskit_node().data().live(), Some(Live::Polite));
    let alert = h
        .root()
        .query_by_role(Role::Alert)
        .expect("alert node for the error toast");
    assert_eq!(alert.accesskit_node().data().live(), Some(Live::Assertive));
    assert_eq!(
        alert.accesskit_node().label().as_deref(),
        Some("Couldn't rename. permission denied")
    );
    crate::i18n::set_thread_locale(None);
}

#[test]
fn cursor_row_is_what_voiceover_follows() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    select(&mut h, Side::Left, &["notes.txt"]);
    h.run_steps(2);
    // Nothing has egui focus: the window's active descendant is the row.
    let focused: Vec<String> = h
        .root()
        .query_all_by_role(Role::Row)
        .filter(|n| n.accesskit_node().is_focused())
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect();
    assert_eq!(focused.len(), 1, "{focused:?}");
    assert!(focused[0].starts_with("notes.txt"), "{focused:?}");
}

// ------------------------------------------------------------------ keyboard

#[test]
fn ctrl_f5_and_tab_walk_toolbar_sidebar_panes_and_status_bar() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    h.run_steps(2);
    press(&mut h, Modifiers::CTRL, Key::F5);
    let first = focused_label(&h).expect("⌃F5 focuses the toolbar");
    assert_eq!(first, "Enclosing Folder");
    let mut seen = vec![first.clone()];
    for _ in 0..200 {
        press(&mut h, Modifiers::NONE, Key::Tab);
        let l = focused_label(&h).expect("Tab never loses the focus");
        if l == first {
            break; // wrapped around: no trap
        }
        seen.push(l);
    }
    let at = |name: &str| {
        seen.iter()
            .position(|l| l == name)
            .unwrap_or_else(|| panic!("{name:?} not reached by Tab: {seen:?}"))
    };
    let order = [
        at("Refresh"),
        at("Connect"),
        at("Desktop"),
        at("Files in Projects"),
        at("Files in Design"),
        // The status bar's hint (the toolbar has a launcher of that name too).
        seen.iter().rposition(|l| l == "Command Palette").unwrap(),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "{order:?} in {seen:?}"
    );
    assert!(
        seen.len() < 120,
        "file rows are no Tab stops: {}",
        seen.len()
    );
    crate::i18n::set_thread_locale(None);
}

#[test]
fn file_list_tab_stop_keeps_the_list_keys() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    select(&mut h, Side::Left, &["README.md"]);
    press(&mut h, Modifiers::CTRL, Key::F5);
    for _ in 0..80 {
        if focused_label(&h).as_deref() == Some("Files in Design") {
            break;
        }
        press(&mut h, Modifiers::NONE, Key::Tab);
    }
    assert_eq!(focused_label(&h).as_deref(), Some("Files in Design"));
    assert_eq!(app(&mut h).active, Side::Right, "the list's pane is active");
    let before = app(&mut h).pane(Side::Right).tab().cursor;
    press(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_ne!(
        app(&mut h).pane(Side::Right).tab().cursor,
        before,
        "↓ moves the cursor"
    );
    assert_eq!(
        focused_label(&h).as_deref(),
        Some("Files in Design"),
        "… and not the focus"
    );
    // Esc leaves the controls; the list keeps the keyboard.
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(h.ctx.memory(|m| m.focused()), None);
    crate::i18n::set_thread_locale(None);
}

#[test]
fn shift_tab_in_the_file_list_goes_to_the_previous_pane() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    assert_eq!(app(&mut h).active, Side::Left);
    press(&mut h, Modifiers::NONE, Key::Tab);
    assert_eq!(app(&mut h).active, Side::Right);
    press(&mut h, Modifiers::SHIFT, Key::Tab);
    assert_eq!(app(&mut h).active, Side::Left);
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        None,
        "no control took the focus"
    );
}

#[test]
fn esc_from_a_control_keeps_the_selection() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    select(&mut h, Side::Left, &["README.md"]);
    press(&mut h, Modifiers::CTRL, Key::F5);
    assert!(h.ctx.memory(|m| m.focused()).is_some());
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(h.ctx.memory(|m| m.focused()), None);
    assert_eq!(
        app(&mut h).pane(Side::Left).tab().selected.len(),
        1,
        "Esc only left the toolbar"
    );
}

/// Focus `label` by AccessKit (as VoiceOver or Full Keyboard Access would).
fn focus(h: &mut H, label: &str) {
    h.get_by_label(label).focus();
    h.run_steps(2);
    assert_eq!(focused_label(h).as_deref(), Some(label));
}

#[test]
fn dialogs_keep_the_focus_inside_and_give_it_back() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    h.run_steps(2);
    focus(&mut h, "Connect");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(app(&mut h).dialog.is_some(), "Return pressed the button");
    // Tab stays inside the dialog and reaches its buttons.
    let mut seen = Vec::new();
    for _ in 0..40 {
        press(&mut h, Modifiers::NONE, Key::Tab);
        seen.push(focused_label(&h).unwrap_or_default());
    }
    for b in ["Connect", "Cancel", "Test Connection"] {
        assert!(seen.iter().any(|l| l == b), "{b:?} not reachable: {seen:?}");
    }
    for outside in ["Refresh", "Desktop", "Files in Projects", "Command Palette"] {
        assert!(
            !seen.iter().any(|l| l == outside),
            "focus escaped to {outside:?}"
        );
    }
    // Esc closes it; the focus is back on the toolbar button.
    press(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(2);
    assert!(app(&mut h).dialog.is_none());
    assert_eq!(focused_label(&h).as_deref(), Some("Connect"));
    crate::i18n::set_thread_locale(None);
}

#[test]
fn focus_returns_after_settings_close() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    h.run_steps(2);
    focus(&mut h, "Settings…");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(app(&mut h).settings.open);
    app(&mut h).settings.open = false;
    h.run_steps(3);
    assert_eq!(focused_label(&h).as_deref(), Some("Settings…"));
    crate::i18n::set_thread_locale(None);
}

#[test]
fn delete_dialog_buttons_are_reachable_by_tab() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    crate::i18n::set_locale(crate::i18n::Locale::De);
    let a = app(&mut h);
    let fs = a.local.clone();
    a.dialog = Some(Dialog::Delete(DeleteForm {
        side: Side::Left,
        fs,
        paths: vec![VfsPath::new(&fx.root.join("README.md").to_string_lossy())],
        folders: Vec::new(),
        no_trash: false,
        in_trash: false,
    }));
    h.run_steps(3);
    let mut seen = Vec::new();
    for _ in 0..6 {
        press(&mut h, Modifiers::NONE, Key::Tab);
        seen.push(focused_label(&h).unwrap_or_default());
    }
    assert!(
        seen.iter()
            .any(|l| l.contains("Papierkorb") || l == "Löschen"),
        "{seen:?}"
    );
    assert!(seen.iter().any(|l| l == "Abbrechen"), "{seen:?}");
    crate::i18n::set_thread_locale(None);
}

// ------------------------------------------------------------------ display options

#[test]
fn reduce_motion_stops_the_favourite_pulse() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let ctx = h.ctx.clone();
    crate::a11y::set_for_test(
        &ctx,
        crate::a11y::DisplayOptions {
            reduce_motion: true,
            ..Default::default()
        },
    );
    app(&mut h).fav_flash = Some((0, None));
    let a = app(&mut h).fav_flash_now(&ctx);
    assert_eq!(a, Some((0, 1.0)), "a steady highlight, no pulse");
    assert_eq!(crate::cloud::upload_pulse(0.7), 1.0);
    crate::a11y::set_for_test(&ctx, crate::a11y::DisplayOptions::default());
    assert!(crate::cloud::upload_pulse(0.7) < 1.0);
}

#[test]
fn increase_contrast_switches_the_palette() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    let ctx = h.ctx.clone();
    let normal = *crate::theme::pal_ctx(&ctx);
    crate::a11y::set_for_test(
        &ctx,
        crate::a11y::DisplayOptions {
            increase_contrast: true,
            ..Default::default()
        },
    );
    h.run_steps(1);
    let high = *crate::theme::pal_ctx(&ctx);
    assert_ne!(normal.separator, high.separator);
    assert_ne!(normal.text_secondary, high.text_secondary);
    // egui's own visuals follow (borders of stock widgets).
    assert_eq!(
        ctx.global_style().visuals.widgets.inactive.bg_stroke.color,
        high.border
    );
    crate::a11y::set_for_test(&ctx, crate::a11y::DisplayOptions::default());
}

// ------------------------------------------------------------------ renders

/// Focus rings (German, light and dark, normal and Increase contrast):
/// a toolbar button, a sidebar place and a file list; written to
/// `$MANTA_RENDER_DIR/a11y`.
#[test]
fn render_focus_rings() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("a11y");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        for (hc, cname) in [(false, ""), (true, "_contrast")] {
            let mut h = harness_de(&fx, WIDE, theme);
            let ctx = h.ctx.clone();
            crate::a11y::set_for_test(
                &ctx,
                crate::a11y::DisplayOptions {
                    increase_contrast: hc,
                    ..Default::default()
                },
            );
            select(&mut h, Side::Left, &["README.md"]);
            press(&mut h, Modifiers::CTRL, Key::F5);
            press(&mut h, Modifiers::NONE, Key::Tab);
            save_to(&mut h, &dir, &format!("focus_toolbar_{tname}{cname}"));
            for _ in 0..80 {
                if focused_label(&h).as_deref() == Some("Schreibtisch") {
                    break;
                }
                press(&mut h, Modifiers::NONE, Key::Tab);
            }
            save_to(&mut h, &dir, &format!("focus_sidebar_{tname}{cname}"));
            for _ in 0..80 {
                if focused_label(&h).as_deref() == Some("Dateien in Projects") {
                    break;
                }
                press(&mut h, Modifiers::NONE, Key::Tab);
            }
            save_to(&mut h, &dir, &format!("focus_list_{tname}{cname}"));
            crate::a11y::set_for_test(&ctx, crate::a11y::DisplayOptions::default());
        }
    }
    crate::i18n::set_thread_locale(None);
}
