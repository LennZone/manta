//! "Default file manager": open / reveal requests → tabs and selection in
//! the real app, plus renders of the first-run prompt and the Settings
//! section (German and English, light and dark). Child of `ux_tests`.

use std::path::{Path, PathBuf};

use eframe::egui::Theme;

use super::*;
use crate::default_viewer::{
    DefaultViewer, Holder, OpenRequest, RequestKind, Unavailable, ViewerStatus,
};
use crate::i18n::{self, Locale};

fn req(kind: RequestKind, paths: &[PathBuf]) -> OpenRequest {
    OpenRequest {
        kind,
        paths: paths.to_vec(),
    }
}

fn vpath(p: &Path) -> VfsPath {
    VfsPath::new(&p.to_string_lossy())
}

fn selected(h: &mut H, side: Side) -> Vec<String> {
    let mut v: Vec<String> = app(h).pane(side).tab().selected.iter().cloned().collect();
    v.sort();
    v
}

fn behaviour_harness(fx: &Fixture) -> H {
    let mut h = harness_with(fx, WIDE, Theme::Light, false);
    let a = app(&mut h);
    a.active = Side::Left;
    a.started = std::time::Instant::now();
    h
}

#[test]
fn cold_start_reveal_replaces_start_folder() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    let file = fx.root.join("Design/app-icon.svg");
    app(&mut h).apply_open_requests(vec![req(RequestKind::Reveal, &[file])]);
    settle(&mut h);
    let a = app(&mut h);
    let pane = a.pane(Side::Left);
    assert_eq!(pane.tabs.len(), 1, "cold start: no extra tab");
    assert_eq!(pane.tab().loc.path, vpath(&fx.root.join("Design")));
    assert!(pane.tab().back.is_empty(), "replaced, not navigated");
    assert_eq!(selected(&mut h, Side::Left), vec!["app-icon.svg"]);
}

#[test]
fn requests_while_running_open_new_tabs() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    // First request of the session: a folder replaces the start folder.
    app(&mut h).apply_open_requests(vec![req(RequestKind::Open, &[fx.root.join("src")])]);
    settle(&mut h);
    assert_eq!(app(&mut h).pane(Side::Left).tabs.len(), 1);
    assert_eq!(
        app(&mut h).pane(Side::Left).tab().loc.path,
        vpath(&fx.root.join("src"))
    );
    assert!(
        selected(&mut h, Side::Left).is_empty(),
        "a folder opens without selection"
    );

    // Then: several files → new tab on their folder, all selected.
    let files = [fx.root.join("README.md"), fx.root.join("notes.txt")];
    app(&mut h).apply_open_requests(vec![req(RequestKind::Reveal, &files)]);
    settle(&mut h);
    let pane = app(&mut h).pane(Side::Left);
    assert_eq!(pane.tabs.len(), 2);
    assert_eq!(pane.active, 1);
    assert_eq!(pane.tab().loc.path, vpath(&fx.root));
    assert_eq!(selected(&mut h, Side::Left), vec!["README.md", "notes.txt"]);

    // And a folder → another new tab.
    app(&mut h).apply_open_requests(vec![req(RequestKind::Open, &[fx.root.join("photos")])]);
    settle(&mut h);
    let pane = app(&mut h).pane(Side::Left);
    assert_eq!(pane.tabs.len(), 3);
    assert_eq!(pane.tab().loc.path, vpath(&fx.root.join("photos")));
}

#[test]
fn late_first_request_is_not_a_cold_start() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    app(&mut h).started = std::time::Instant::now() - std::time::Duration::from_secs(60);
    app(&mut h).apply_open_requests(vec![req(RequestKind::Reveal, &[fx.root.join("notes.txt")])]);
    settle(&mut h);
    let pane = app(&mut h).pane(Side::Left);
    assert_eq!(pane.tabs.len(), 2, "the user's start tab stays");
    assert_eq!(selected(&mut h, Side::Left), vec!["notes.txt"]);
}

#[test]
fn cold_start_into_used_pane_adds_tab() {
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    // The user already navigated in the active pane.
    app(&mut h).navigate_path(Side::Left, vpath(&fx.root.join("src")));
    settle(&mut h);
    app(&mut h).apply_open_requests(vec![req(RequestKind::Open, &[fx.root.join("photos")])]);
    settle(&mut h);
    assert_eq!(app(&mut h).pane(Side::Left).tabs.len(), 2);
}

// ------------------------------------------------------------------ renders

const LOCALES: [(Locale, &str); 2] = [(Locale::De, "de"), (Locale::En, "en")];
const THEMES: [(Theme, &str); 2] = [(Theme::Light, "light"), (Theme::Dark, "dark")];

fn forklift() -> Holder {
    Holder {
        id: "com.binarynights.ForkLift".into(),
        name: "ForkLift".into(),
    }
}

pub(super) fn status(available: Result<(), Unavailable>, is_default: bool) -> ViewerStatus {
    ViewerStatus {
        available,
        is_default,
        folders: is_default,
        holder: if is_default {
            Holder {
                id: "com.manta-filemanager.manta".into(),
                name: "Manta".into(),
            }
        } else {
            forklift()
        },
        restore_to: forklift(),
        busy: false,
    }
}

#[test]
fn render_default_viewer() {
    require_gpu!();
    let fx = fixture();
    for (locale, lname) in LOCALES {
        for (theme, tname) in THEMES {
            let tag = format!("{lname}_{tname}");
            let mut h = harness(&fx, WIDE, theme);
            i18n::set_locale(locale);

            // First-run prompt over the main window.
            let a = app(&mut h);
            a.viewer = DefaultViewer::with_status(status(Ok(()), false));
            a.viewer.prompt_open = true;
            h.run_steps(4);
            save(&mut h, &format!("viewer_prompt_{tag}"));
            app(&mut h).viewer.prompt_open = false;

            // Settings → General, off / on / unavailable.
            let cases = [
                ("off", status(Ok(()), false)),
                ("on", status(Ok(()), true)),
                (
                    "on_folders_refused",
                    ViewerStatus {
                        folders: false,
                        ..status(Ok(()), true)
                    },
                ),
                (
                    "unavailable",
                    status(
                        Err(Unavailable::NotInApplications(PathBuf::from(
                            "/Users/anna/Downloads/Manta.app",
                        ))),
                        false,
                    ),
                ),
            ];
            for (case, st) in cases {
                let a = app(&mut h);
                a.viewer = DefaultViewer::with_status(st);
                a.settings.show(Some(crate::settings::SettingsTab::General));
                h.run_steps(4);
                save(&mut h, &format!("viewer_settings_{case}_{tag}"));
            }
            app(&mut h).settings.open = false;
        }
    }
}

// ------------------------------------------------------- palette command

fn palette_toggle(h: &mut H) -> Option<(String, bool)> {
    app(h)
        .palette_entries()
        .into_iter()
        .find(|e| e.id == crate::palette::PaletteId::ToggleDefaultViewer)
        .map(|e| (e.label, e.enabled))
}

fn run_toggle(h: &mut H) {
    let ctx = h.ctx.clone();
    let actions = app(h).palette_action(crate::palette::PaletteId::ToggleDefaultViewer);
    for a in actions {
        app(h).handle_action(&ctx, a);
    }
    h.run_steps(2);
}

fn wait_switched(h: &mut H) {
    let start = std::time::Instant::now();
    while app(h).viewer.status.busy {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "switch timed out"
        );
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

/// "Use Manta as Default File Manager…" in the palette: asks with the
/// launch question (Cancel instead of Not Now), switches, and then offers
/// to give the role back. A fake macOS: the real NSFileViewer is never
/// touched.
#[test]
fn palette_command_switches_the_default_file_manager() {
    use crate::default_viewer::tests::{bundled, FORKLIFT, ME};
    use crate::default_viewer::ViewerOs;
    let fx = fixture();
    let mut h = behaviour_harness(&fx);
    let os = std::sync::Arc::new(bundled());
    app(&mut h).viewer = DefaultViewer::new(os.clone(), None);
    i18n::with_locale(Locale::En, || {
        assert_eq!(
            palette_toggle(&mut h),
            Some(("Use Manta as Default File Manager…".to_string(), true))
        );
        run_toggle(&mut h);
        assert!(app(&mut h).viewer.prompt_open && app(&mut h).viewer.prompt_manual);
        assert_eq!(
            os.file_viewer().as_deref(),
            Some(FORKLIFT),
            "nothing changes before confirming"
        );
        // Esc = Cancel.
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(!app(&mut h).viewer.prompt_open);
        assert_eq!(app(&mut h).viewer.prefs.not_now_count, 0);

        run_toggle(&mut h);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        wait_switched(&mut h);
        assert_eq!(os.file_viewer().as_deref(), Some(ME));
        assert!(
            app(&mut h)
                .toasts
                .titles()
                .iter()
                .any(|t| t.contains("default file manager")),
            "{:?}",
            app(&mut h).toasts.titles()
        );
        assert_eq!(
            palette_toggle(&mut h),
            Some((
                "Give the Default File Manager Back to ForkLift".to_string(),
                true
            ))
        );

        run_toggle(&mut h);
        assert!(!app(&mut h).viewer.prompt_open, "off without a question");
        wait_switched(&mut h);
        assert_eq!(os.file_viewer().as_deref(), Some(FORKLIFT));
        assert!(app(&mut h)
            .toasts
            .titles()
            .iter()
            .any(|t| t.contains("ForkLift")));
    });

    // Not in Applications: the command is greyed out.
    app(&mut h).viewer = DefaultViewer::with_status(status(
        Err(Unavailable::NotInApplications(PathBuf::from(
            "/Volumes/Manta/Manta.app",
        ))),
        false,
    ));
    assert_eq!(palette_toggle(&mut h).map(|t| t.1), Some(false));
}

/// The question as asked from the palette / menu bar (Cancel, no "Don't
/// Ask Again"), German light and dark, into `$MANTA_RENDER_DIR/viewer`.
#[test]
fn render_default_viewer_command_de() {
    require_gpu!();
    let fx = fixture();
    let dir = render_dir("viewer");
    for (theme, tname) in THEMES {
        let mut h = harness(&fx, WIDE, theme);
        i18n::set_locale(Locale::De);
        let a = app(&mut h);
        a.viewer = DefaultViewer::with_status(status(Ok(()), false));
        a.viewer.prompt_open = true;
        a.viewer.prompt_manual = true;
        h.run_steps(4);
        save_to(&mut h, &dir, &format!("viewer_prompt_command_de_{tname}"));
        app(&mut h).viewer.prompt_open = false;
        i18n::set_locale(Locale::En);
    }
}
