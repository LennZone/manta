//! Headless renders of the Git panel with `egui_kittest` + wgpu, against a
//! real temp repository. PNGs go to `$MANTA_GIT_RENDER_DIR` (default
//! `target/plugin-git-renders`). Skips when git or a wgpu adapter is missing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme};
use egui_kittest::Harness;
use manta_core::{Protocol, VfsPath};
use manta_plugin::{PaneId, PaneState, Plugin, PluginEvent, PluginHandle};

use crate::panel::{Group, Selection};
use crate::shared::lock;
use crate::GitPlugin;

fn wgpu_available() -> bool {
    // Screenshots are opt-in (slow, need a GPU): set MANTA_RENDER_DIR or
    // this crate's *_RENDER_DIR to produce them.
    if !std::env::vars_os().any(|(k, _)| k.to_string_lossy().ends_with("RENDER_DIR")) {
        return false;
    }
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let ok = std::panic::catch_unwind(|| {
            egui_kittest::wgpu::create_render_state(
                egui_kittest::wgpu::default_wgpu_setup(),
                egui_wgpu::RendererOptions::PREDICTABLE,
            )
        })
        .is_ok();
        if !ok {
            eprintln!("render_tests: no wgpu adapter, skipping");
        }
        ok
    })
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("MANTA_GIT_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-git-renders")
        });
    std::fs::create_dir_all(&dir).expect("render dir");
    dir
}

fn git(dir: &Path, args: &[&str], env: &[(&str, &str)]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(env.iter().copied())
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn commit(root: &Path, msg: &str, author: &str, ago: &str) {
    git(root, &["add", "-A"], &[]);
    let email = format!(
        "{}@example.org",
        author.split(' ').next().unwrap().to_lowercase()
    );
    git(
        root,
        &["commit", "-q", "-m", msg, "--date", ago],
        &[
            ("GIT_AUTHOR_NAME", author),
            ("GIT_AUTHOR_EMAIL", &email),
            ("GIT_COMMITTER_NAME", author),
            ("GIT_COMMITTER_EMAIL", &email),
        ],
    );
}

const MAIN_RS: &str = "use std::env;\n\nfn main() {\n    let args: Vec<String> = env::args().collect();\n    let name = args.get(1).map(String::as_str).unwrap_or(\"world\");\n    println!(\"Hello, {name}!\");\n}\n\nfn helper() -> u32 {\n    40 + 2\n}\n";
const MAIN_RS_NEW: &str = "use std::env;\nuse std::process::ExitCode;\n\nfn main() -> ExitCode {\n    let args: Vec<String> = env::args().collect();\n    let name = args.get(1).map(String::as_str).unwrap_or(\"Manta\");\n    println!(\"Hello, {name}!\");\n    ExitCode::SUCCESS\n}\n\nfn helper() -> u32 {\n    42\n}\n";

/// A repo that looks like real work in progress.
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("manta-demo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"], &[]);
    git(&root, &["config", "commit.gpgsign", "false"], &[]);
    git(&root, &["config", "core.hooksPath", "/dev/null"], &[]);
    write(&root, ".gitignore", "target/\n");
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write(&root, "README.md", "# Demo\n");
    commit(&root, "Initial commit", "Anna Berg", "40 days ago");
    write(&root, "src/main.rs", MAIN_RS);
    write(&root, "src/util.rs", "pub fn util() {}\n");
    commit(
        &root,
        "Add command line entry point",
        "Ben Ortiz",
        "9 days ago",
    );
    write(&root, "docs/notes.md", "Notes\n");
    commit(
        &root,
        "Write first notes on the sync design",
        "Anna Berg",
        "3 days ago",
    );
    write(&root, "README.md", "# Demo\n\nA small demo.\n");
    commit(
        &root,
        "Explain the project in the README\n\nThe README now says what the demo does and how to\nrun it locally.",
        "Chris Lee",
        "5 hours ago",
    );
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.2.0\"\n",
    );
    commit(
        &root,
        "Bump version to 0.2.0",
        "Ben Ortiz",
        "20 minutes ago",
    );

    // Work in progress.
    write(&root, "src/main.rs", MAIN_RS_NEW);
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.2.1\"\nedition = \"2021\"\n",
    );
    git(&root, &["add", "Cargo.toml"], &[]);
    git(&root, &["mv", "src/util.rs", "src/helpers.rs"], &[]);
    write(&root, "docs/notes.md", "Notes\n\nMore notes.\n");
    write(
        &root,
        "docs/design/sync plan.md",
        "# Sync plan\n\n- compare\n- copy\n",
    );
    write(&root, "CHANGELOG.md", "## 0.2.1\n- Exit codes\n");
    write(&root, "target/debug/demo", "bin");
    tmp
}

fn conflict_fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("website");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"], &[]);
    git(&root, &["config", "commit.gpgsign", "false"], &[]);
    write(&root, "index.html", "<h1>Hello</h1>\n");
    write(&root, "style.css", "body {}\n");
    commit(&root, "Initial page", "Anna Berg", "2 days ago");
    git(&root, &["checkout", "-q", "-b", "redesign"], &[]);
    write(&root, "index.html", "<h1>Welcome</h1>\n");
    commit(&root, "New headline", "Ben Ortiz", "1 day ago");
    git(&root, &["checkout", "-q", "main"], &[]);
    write(&root, "index.html", "<h1>Hi there</h1>\n");
    write(&root, "style.css", "body { margin: 0 }\n");
    commit(&root, "Friendlier headline", "Anna Berg", "3 hours ago");
    let _ = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["merge", "-q", "redesign"])
        .output();
    tmp
}

fn pane(dir: Option<&Path>) -> PaneState {
    PaneState {
        remote: None,
        pane: PaneId::Left,
        path: dir.map_or_else(
            || VfsPath::new("/srv/www"),
            |d| VfsPath::new(&d.to_string_lossy()),
        ),
        local_path: dir.map(Path::to_path_buf),
        protocol: if dir.is_some() {
            Protocol::Local
        } else {
            Protocol::Sftp
        },
        backend_label: "Local".into(),
        selection: Vec::new(),
        active: true,
    }
}

struct State {
    plugin: GitPlugin,
    pane: PaneState,
}

/// Approximate the app's theme so renders look like the real thing.
fn setup_ctx(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        let sf = Arc::new(sf);
        let base = fonts.families[&FontFamily::Proportional].clone();
        for (key, fam, w) in [
            ("sf", FontFamily::Proportional, 400.0),
            ("sf-medium", FontFamily::Name("rl-medium".into()), 510.0),
            ("sf-semibold", FontFamily::Name("rl-semibold".into()), 600.0),
        ] {
            let mut fd = FontData::from_blob(sf.clone(), 0);
            fd.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", w)]);
            fonts.font_data.insert(key.into(), Arc::new(fd));
            let mut chain = vec![key.to_string()];
            chain.extend(base.iter().cloned());
            fonts.families.insert(fam, chain);
        }
        if let Ok(mono) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
            fonts.font_data.insert(
                "sf-mono".into(),
                Arc::new(FontData::from_blob(Arc::new(mono), 0)),
            );
            let mut chain = vec!["sf-mono".to_string()];
            chain.extend(fonts.families[&FontFamily::Monospace].iter().cloned());
            fonts.families.insert(FontFamily::Monospace, chain);
        }
    }
    ctx.set_fonts(fonts);
    for theme in [Theme::Light, Theme::Dark] {
        let dark = theme == Theme::Dark;
        ctx.style_mut_of(theme, |s| {
            use egui::TextStyle as T;
            s.text_styles
                .insert(T::Body, egui::FontId::proportional(13.0));
            s.text_styles
                .insert(T::Button, egui::FontId::proportional(13.0));
            s.text_styles
                .insert(T::Monospace, egui::FontId::monospace(12.0));
            let v = &mut s.visuals;
            let rgb = Color32::from_rgb;
            v.panel_fill = if dark {
                rgb(38, 38, 41)
            } else {
                rgb(246, 246, 248)
            };
            v.extreme_bg_color = if dark {
                rgb(28, 28, 30)
            } else {
                rgb(255, 255, 255)
            };
            v.weak_text_color = Some(if dark {
                rgb(160, 160, 167)
            } else {
                rgb(104, 104, 110)
            });
            v.widgets.noninteractive.bg_stroke = Stroke::new(
                1.0,
                if dark {
                    rgb(58, 58, 62)
                } else {
                    rgb(220, 220, 224)
                },
            );
            v.widgets.noninteractive.fg_stroke = Stroke::new(
                1.0,
                if dark {
                    rgb(242, 242, 245)
                } else {
                    rgb(29, 29, 31)
                },
            );
            v.widgets.inactive.bg_stroke = Stroke::new(
                1.0,
                if dark {
                    rgb(78, 78, 84)
                } else {
                    rgb(200, 200, 206)
                },
            );
            v.window_fill = if dark {
                rgb(46, 46, 49)
            } else {
                rgb(252, 252, 253)
            };
        });
    }
}

fn harness(state: State, theme: Theme) -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(340.0, 820.0))
        .with_pixels_per_point(2.0)
        .with_theme(theme)
        .allow_missing_glyphs()
        .with_context_setup(setup_ctx)
        .wgpu()
        .build_ui_state(
            |ui, s: &mut State| {
                // Mimic the host's sidebar frame: title bar + body.
                let rect = ui.max_rect();
                ui.painter()
                    .rect_filled(rect, CornerRadius::ZERO, ui.visuals().panel_fill);
                let (bar, _) =
                    ui.allocate_exact_size(egui::vec2(rect.width(), 32.0), egui::Sense::hover());
                ui.painter().text(
                    bar.left_center() + egui::vec2(12.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "Git",
                    egui::FontId::new(13.0, FontFamily::Name("rl-semibold".into())),
                    ui.visuals().text_color(),
                );
                ui.painter().hline(
                    bar.x_range(),
                    bar.bottom(),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                let pane = s.pane.clone();
                s.plugin.panel_ui("git", &pane, ui);
            },
            state,
        )
}

fn start(h: &mut Harness<'static, State>) {
    let (tx, rx) = mpsc::channel();
    // Keep the receiver alive for the test's duration.
    std::mem::forget(rx);
    let handle = PluginHandle::new("manta.git".into(), tx, h.ctx.clone(), std::env::temp_dir());
    let st = h.state_mut();
    st.plugin.enable(handle);
    let ev = PluginEvent::DirectoryChanged(st.pane.clone());
    st.plugin.on_event(&ev);
}

/// Step until `pred` holds (background threads deliver), then settle.
fn wait(h: &mut Harness<'static, State>, mut pred: impl FnMut(&State) -> bool) {
    let t = Instant::now();
    while !pred(h.state()) && t.elapsed() < Duration::from_secs(10) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(4);
}

fn status_loaded(s: &State) -> bool {
    let sh = s.plugin.shared();
    let g = lock(&sh);
    !g.repos.is_empty() && g.repos.values().all(|r| r.status.is_some())
}

fn diff_loaded(s: &State) -> bool {
    lock(&s.plugin.shared())
        .diff
        .as_ref()
        .is_some_and(|d| d.result.is_some())
}

fn save(h: &mut Harness<'static, State>, name: &str) {
    h.run_steps(2);
    let img = h.render().expect("render");
    let path = out_dir().join(format!("{name}.png"));
    img.save(&path).expect("save");
    eprintln!("render_tests: wrote {}", path.display());
}

fn tname(t: Theme) -> &'static str {
    if t == Theme::Dark {
        "dark"
    } else {
        "light"
    }
}

/// A group header that is visible must have at least one visible row.
fn assert_no_orphan_headers(layout: &crate::panel::ChangesLayout) {
    let vp = layout.viewport;
    assert!(!layout.headers.is_empty());
    for (g, hr) in &layout.headers {
        let header_visible = hr.top() < vp.bottom() - 1.0 && hr.bottom() > vp.top() + 1.0;
        if !header_visible {
            continue;
        }
        let has_row = layout
            .rows
            .iter()
            .any(|(rg, _, r)| rg == g && vp.contains_rect(*r));
        assert!(
            has_row,
            "header {g:?} visible without rows (viewport {vp:?}, header {hr:?})"
        );
    }
}

fn git_ok() -> bool {
    crate::git::available() && wgpu_available()
}

#[test]
fn render_git_panel() {
    if !git_ok() {
        return;
    }
    let tmp = fixture();
    let root = tmp.path().join("manta-demo");
    for theme in [Theme::Light, Theme::Dark] {
        let state = State {
            plugin: GitPlugin::new(),
            pane: pane(Some(&root)),
        };
        let mut h = harness(state, theme);
        start(&mut h);
        wait(&mut h, status_loaded);
        save(&mut h, &format!("panel_changes_{}", tname(theme)));

        let repo_root = lock(&h.state().plugin.shared())
            .repos
            .keys()
            .next()
            .cloned()
            .unwrap();
        let sel = Selection {
            root: repo_root,
            group: Group::Changes,
            path: "src/main.rs".into(),
            orig: None,
        };
        h.state_mut().plugin.select_for_test(Some(sel));
        wait(&mut h, diff_loaded);
        h.state_mut().plugin.panel_state_mut().commit_msg = "Return an exit code from main".into();
        save(&mut h, &format!("panel_diff_{}", tname(theme)));
        assert_no_orphan_headers(&h.state().plugin.panel_state().layout);

        // Every group stays reachable: scroll the list to its end.
        assert!(
            h.state().plugin.panel_state().layout.more_below,
            "fixture should overflow"
        );
        h.state_mut().plugin.panel_state_mut().scroll_changes_to_end = true;
        h.run_steps(3);
        let layout = h.state().plugin.panel_state().layout.clone();
        for path in ["CHANGELOG.md", "docs/design/sync plan.md"] {
            let (_, _, r) = layout
                .rows
                .iter()
                .find(|(g, p, _)| *g == Group::Untracked && p == path)
                .unwrap_or_else(|| panic!("no row for {path}"));
            assert!(
                layout.viewport.contains_rect(*r),
                "{path} not visible after scrolling: {r:?} vs {:?}",
                layout.viewport
            );
        }
        assert_no_orphan_headers(&layout);
        save(&mut h, &format!("panel_diff_scrolled_{}", tname(theme)));
        h.state_mut().plugin.disable();
    }
}

#[test]
fn render_git_panel_states() {
    if !git_ok() {
        return;
    }
    let conflict = conflict_fixture();
    let croot = conflict.path().join("website");
    let clean = tempfile::tempdir().unwrap();
    let clean_root = clean.path().join("notes");
    std::fs::create_dir_all(&clean_root).unwrap();
    git(&clean_root, &["init", "-q", "-b", "main"], &[]);
    git(&clean_root, &["config", "commit.gpgsign", "false"], &[]);
    write(&clean_root, "todo.md", "- [ ] ship\n");
    commit(
        &clean_root,
        "Start a todo list",
        "Anna Berg",
        "2 minutes ago",
    );
    let plain = tempfile::tempdir().unwrap();

    for theme in [Theme::Light, Theme::Dark] {
        let cases: [(&str, Option<&Path>, bool); 4] = [
            ("conflict", Some(&croot), true),
            ("clean", Some(&clean_root), true),
            ("not_repo", Some(plain.path()), false),
            ("remote", None, false),
        ];
        for (name, dir, repo) in cases {
            let state = State {
                plugin: GitPlugin::new(),
                pane: pane(dir),
            };
            let mut h = harness(state, theme);
            start(&mut h);
            if repo {
                wait(&mut h, status_loaded);
            } else {
                wait(&mut h, |s| {
                    lock(&s.plugin.shared()).panes.contains_key(&PaneId::Left)
                });
            }
            if name == "conflict" {
                let repo_root = lock(&h.state().plugin.shared())
                    .repos
                    .keys()
                    .next()
                    .cloned()
                    .unwrap();
                let sel = Selection {
                    root: repo_root,
                    group: Group::Conflicts,
                    path: "index.html".into(),
                    orig: None,
                };
                h.state_mut().plugin.select_for_test(Some(sel));
                wait(&mut h, diff_loaded);
            }
            save(&mut h, &format!("panel_{name}_{}", tname(theme)));
            h.state_mut().plugin.disable();
        }
    }
}

/// The panel in German (light and dark): changes with an upstream that is
/// ahead, a diff, a merge conflict, a clean repo and the empty states.
#[test]
fn render_git_panel_german() {
    use manta_plugin::i18n::{set_thread_lang, Lang};
    if !git_ok() {
        return;
    }
    set_thread_lang(Some(Lang::De));
    let tmp = fixture();
    let root = tmp.path().join("manta-demo");
    // A tracked upstream one commit behind, so the header shows "↑1".
    git(
        &root,
        &["update-ref", "refs/remotes/origin/main", "HEAD~1"],
        &[],
    );
    git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "https://example.org/demo.git",
        ],
        &[],
    );
    git(
        &root,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        &[],
    );
    git(&root, &["config", "branch.main.remote", "origin"], &[]);
    git(
        &root,
        &["config", "branch.main.merge", "refs/heads/main"],
        &[],
    );
    let conflict = conflict_fixture();
    let croot = conflict.path().join("website");
    let clean = tempfile::tempdir().unwrap();
    let clean_root = clean.path().join("notes");
    std::fs::create_dir_all(&clean_root).unwrap();
    git(&clean_root, &["init", "-q", "-b", "main"], &[]);
    git(&clean_root, &["config", "commit.gpgsign", "false"], &[]);
    write(&clean_root, "todo.md", "- [ ] ship\n");
    commit(
        &clean_root,
        "Start a todo list",
        "Anna Berg",
        "2 minutes ago",
    );
    let plain = tempfile::tempdir().unwrap();

    for theme in [Theme::Light, Theme::Dark] {
        let t = tname(theme);
        let state = State {
            plugin: GitPlugin::new(),
            pane: pane(Some(&root)),
        };
        let mut h = harness(state, theme);
        start(&mut h);
        wait(&mut h, status_loaded);
        save(&mut h, &format!("git_de_changes_{t}"));
        let repo_root = lock(&h.state().plugin.shared())
            .repos
            .keys()
            .next()
            .cloned()
            .unwrap();
        let sel = Selection {
            root: repo_root,
            group: Group::Staged,
            path: "Cargo.toml".into(),
            orig: None,
        };
        h.state_mut().plugin.select_for_test(Some(sel));
        wait(&mut h, diff_loaded);
        save(&mut h, &format!("git_de_diff_{t}"));
        h.state_mut().plugin.disable();

        let cases: [(&str, Option<&Path>, bool); 4] = [
            ("conflict", Some(&croot), true),
            ("clean", Some(&clean_root), true),
            ("not_repo", Some(plain.path()), false),
            ("remote", None, false),
        ];
        for (name, dir, repo) in cases {
            let state = State {
                plugin: GitPlugin::new(),
                pane: pane(dir),
            };
            let mut h = harness(state, theme);
            start(&mut h);
            if repo {
                wait(&mut h, status_loaded);
            } else {
                wait(&mut h, |s| {
                    lock(&s.plugin.shared()).panes.contains_key(&PaneId::Left)
                });
            }
            save(&mut h, &format!("git_de_{name}_{t}"));
            h.state_mut().plugin.disable();
        }
    }
    set_thread_lang(None);
}
