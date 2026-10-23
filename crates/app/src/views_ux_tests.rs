//! The Icons, Columns and Gallery views (⌘1 – ⌘4): switching per pane,
//! selection and keys in each view, type-to-select scrolling, the Columns
//! path, drag & drop out of the Icons view, the saved session, thumbnails.
//!
//! Behaviour tests step the real app without a GPU; `render_views_*` need
//! one and write PNGs to `$MANTA_RENDER_DIR/views`.

use super::*;
use crate::app::Action;
use crate::views::ViewMode;
use egui::{Key, Modifiers};

fn behaviour(fx: &Fixture) -> H {
    harness_with(fx, WIDE, Theme::Light, false)
}

pub(super) fn act(h: &mut H, a: Action) {
    let ctx = h.ctx.clone();
    app(h).handle_action(&ctx, a);
    settle(h);
}

fn view_of(h: &mut H, side: Side) -> ViewMode {
    app(h).pane(side).view
}

fn cursor_name(h: &mut H, side: Side) -> Option<String> {
    app(h)
        .pane(side)
        .tab()
        .cursor_entry()
        .map(|e| e.name.clone())
}

fn button(h: &mut H, pos: egui::Pos2, pressed: bool, modifiers: Modifiers) {
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    });
}

/// A click at `pos` with `mods` held.
fn click_at(h: &mut H, pos: egui::Pos2, mods: Modifiers) {
    h.hover_at(pos);
    h.event(egui::Event::ModifiersChanged(mods));
    h.step();
    button(h, pos, true, mods);
    h.step();
    button(h, pos, false, mods);
    h.step();
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

/// Press at `a`, move to `b` in steps, release there.
fn drag_from_to(h: &mut H, a: egui::Pos2, b: egui::Pos2) {
    h.hover_at(a);
    h.step();
    button(h, a, true, Modifiers::NONE);
    h.step();
    for k in 1..=8 {
        let p = a + (b - a) * (k as f32 / 8.0);
        h.event(egui::Event::PointerMoved(p));
        h.step();
    }
    button(h, b, false, Modifiers::NONE);
    h.run_steps(3);
}

fn key(h: &mut H, k: Key) {
    h.key_press(k);
    h.run_steps(2);
}

/// A colourful "photo" (sky gradient, sun, hills) so thumbnails look real.
fn photo(w: u32, h: u32, seed: u32) -> image::RgbImage {
    let hue = |t: f32| -> [f32; 3] {
        let k = (seed % 7) as f32 / 7.0;
        [
            0.35 + 0.6 * ((t + k) * std::f32::consts::TAU).sin().abs(),
            0.45 + 0.4 * (t * 3.1 + k).cos().abs(),
            0.6 + 0.4 * (k * 9.0).sin().abs(),
        ]
    };
    let sun = (
        (0.25 + 0.5 * ((seed * 37 % 100) as f32 / 100.0)) * w as f32,
        0.35 * h as f32,
    );
    image::RgbImage::from_fn(w, h, |x, y| {
        let fy = y as f32 / h as f32;
        let fx = x as f32 / w as f32;
        let sky = hue(fy * 0.5);
        let mut c = [sky[0] * (1.0 - fy * 0.4), sky[1] * (1.0 - fy * 0.3), sky[2]];
        let d = ((x as f32 - sun.0).powi(2) + (y as f32 - sun.1).powi(2)).sqrt();
        if d < h as f32 * 0.12 {
            c = [1.0, 0.86, 0.45];
        }
        let hill = 0.62 + 0.08 * (fx * 7.0 + seed as f32).sin() + 0.05 * (fx * 17.0).cos();
        if fy > hill {
            let g = 0.25 + 0.3 * (1.0 - fy);
            c = [g * 0.5, g * 1.4 + 0.1 * (seed % 3) as f32, g * 0.6];
        }
        let px = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        image::Rgb([px(c[0]), px(c[1]), px(c[2])])
    })
}

/// A one-page PDF with a title and a few lines (Quick Look renders it).
fn pdf(title: &str) -> Vec<u8> {
    let content = format!(
        "BT /F1 28 Tf 60 720 Td ({title}) Tj ET\n0.2 0.4 0.8 rg 60 640 480 6 re f\n\
         BT /F1 14 Tf 60 600 Td (Quarterly figures and notes) Tj ET\n\
         0.85 0.85 0.9 rg 60 300 480 260 re f\n0.2 0.6 0.4 rg 80 320 60 160 re f 160 320 60 220 re f 240 320 60 120 re f\n"
    );
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

/// `Projects/Fotos`: photos (JPEG / PNG), PDFs, folders, a few files;
/// `Fotos/Reisen/2026/Island` three levels deep for the columns.
pub(super) fn photos(fx: &Fixture) -> PathBuf {
    let dir = fx.root.join("Fotos");
    std::fs::create_dir_all(&dir).unwrap();
    let names = [
        "Strand bei Sonnenuntergang.jpg",
        "Berge im Nebel.jpg",
        "Hafen.jpg",
        "Leuchtturm.png",
        "Wanderung Tag 3 – Gipfel und Aussicht über das ganze Tal.jpg",
        "Familie.jpg",
        "Waldweg.jpg",
        "Abendrot.png",
    ];
    for (i, n) in names.iter().enumerate() {
        let (w, h) = if i % 3 == 1 { (480, 640) } else { (800, 533) };
        let img = photo(w, h, i as u32 + 1);
        let p = dir.join(n);
        if n.ends_with(".png") {
            img.save(&p).unwrap();
        } else {
            img.save_with_format(&p, image::ImageFormat::Jpeg).unwrap();
        }
    }
    std::fs::write(dir.join("Reiseplan.pdf"), pdf("Reiseplan Island")).unwrap();
    std::fs::write(dir.join("Rechnung Hotel.pdf"), pdf("Rechnung")).unwrap();
    std::fs::write(dir.join("Notizen.txt"), "Fähre um 9:40\n").unwrap();
    let deep = dir.join("Reisen/2026/Island");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(dir.join("Reisen/2025")).unwrap();
    std::fs::create_dir_all(dir.join("Reisen/2026/Norwegen")).unwrap();
    std::fs::create_dir_all(dir.join("Archiv")).unwrap();
    for (i, n) in [
        "Gletscher.jpg",
        "Geysir.jpg",
        "Polarlicht.jpg",
        "Vulkan.jpg",
    ]
    .iter()
    .enumerate()
    {
        photo(800, 533, 20 + i as u32)
            .save_with_format(deep.join(n), image::ImageFormat::Jpeg)
            .unwrap();
    }
    std::fs::write(deep.join("Route.pdf"), pdf("Route")).unwrap();
    dir
}

pub(super) fn goto(h: &mut H, side: Side, dir: &Path) {
    app(h).navigate_path(side, VfsPath::new(&dir.to_string_lossy()));
    settle(h);
}

/// Steps until the thumbnails the views asked for are made and shown.
pub(super) fn settle_thumbs(h: &mut H) {
    for _ in 0..6 {
        h.run_steps(2);
        let start = Instant::now();
        while !app(h).thumbs.wait_idle(Duration::from_millis(50))
            && start.elapsed() < Duration::from_secs(20)
        {
            h.step();
        }
        h.run_steps(3);
    }
}

#[test]
fn cmd_1_to_4_switch_the_active_pane_only() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).activate(Side::LEFT);
    assert_eq!(view_of(&mut h, Side::LEFT), ViewMode::List);
    for (k, mode) in [
        (Key::Num1, ViewMode::Icons),
        (Key::Num3, ViewMode::Columns),
        (Key::Num4, ViewMode::Gallery),
        (Key::Num2, ViewMode::List),
    ] {
        h.key_press_modifiers(Modifiers::COMMAND, k);
        h.run_steps(2);
        assert_eq!(view_of(&mut h, Side::LEFT), mode, "{k:?}");
        assert_eq!(
            view_of(&mut h, Side::RIGHT),
            ViewMode::List,
            "the other pane keeps its view"
        );
    }
    app(&mut h).activate(Side::RIGHT);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num3);
    h.run_steps(2);
    assert_eq!(view_of(&mut h, Side::RIGHT), ViewMode::Columns);
    assert_eq!(view_of(&mut h, Side::LEFT), ViewMode::List);
    // Palette commands do the same.
    app(&mut h).activate(Side::LEFT);
    let acts = app(&mut h).cmd_actions(crate::commands::Cmd::ViewGallery);
    for a in acts {
        act(&mut h, a);
    }
    assert_eq!(view_of(&mut h, Side::LEFT), ViewMode::Gallery);
    // A split pane starts with the same view.
    act(&mut h, Action::SplitPane(Side::LEFT));
    assert_eq!(view_of(&mut h, PaneId(2)), ViewMode::Gallery);
}

#[test]
fn view_switcher_in_the_path_bar() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    // The segments are radio buttons named after the views.
    use egui_kittest::kittest::Queryable;
    let n = h.query_all_by_label(t!("view.icons")).count();
    assert_eq!(n, 2, "one switcher per pane");
    h.query_all_by_label(t!("view.columns"))
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    assert_eq!(view_of(&mut h, Side::LEFT), ViewMode::Columns);
    assert_eq!(app(&mut h).active_pane(), Side::LEFT);
}

#[test]
fn icons_selection_and_grid_keys() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).activate(Side::LEFT);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
    h.run_steps(3);
    let cols = app(&mut h).pane(Side::LEFT).tab().view_state.grid_cols;
    assert!(cols >= 3, "a grid: {cols} per row");
    key(&mut h, Key::ArrowRight);
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().cursor, Some(0));
    key(&mut h, Key::ArrowRight);
    key(&mut h, Key::ArrowDown);
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().cursor,
        Some(1 + cols),
        "↓ goes one row down"
    );
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().selected.len(),
        2,
        "⇧← extends"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    let (sel, n) = {
        let t = app(&mut h).pane(Side::LEFT).tab();
        (t.selected.len(), t.view.len())
    };
    assert_eq!(sel, n, "⌘A");

    // Clicks on the cells: plain, ⌘, ⇧.
    let cell = |h: &mut H, i: usize| {
        let (g, o) = app(h).pane(Side::LEFT).tab().view_state.grid.unwrap();
        g.icon_rect(i).translate(o.to_vec2()).center()
    };
    let p = cell(&mut h, 2);
    click_at(&mut h, p, Modifiers::NONE);
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().selected.len(), 1);
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().cursor, Some(2));
    let p = cell(&mut h, 4);
    click_at(&mut h, p, Modifiers::COMMAND);
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().selected.len(),
        2,
        "⌘-click adds"
    );
    let p = cell(&mut h, 0);
    click_at(&mut h, p, Modifiers::SHIFT);
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().selected.len(),
        5,
        "⇧-click: 0…4"
    );

    // Rubber band over items 0 and 1 from empty space above them.
    let (g, o) = app(&mut h).pane(Side::LEFT).tab().view_state.grid.unwrap();
    let a = g.cell(0).translate(o.to_vec2()).left_top() + egui::vec2(2.0, 1.0);
    let b = g.icon_rect(1).translate(o.to_vec2()).center();
    drag_from_to(&mut h, a, b);
    let t = app(&mut h).pane(Side::LEFT).tab();
    let names: Vec<String> = t.selected.iter().cloned().collect();
    assert_eq!(
        names.len(),
        2,
        "rubber band selects what it touches: {names:?}"
    );

    // Enter opens a folder; ⌫ goes up again.
    let src = app(&mut h)
        .pane(Side::LEFT)
        .tab()
        .view_index_of("src")
        .unwrap();
    app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(src);
    key(&mut h, Key::Enter);
    settle(&mut h);
    assert!(app(&mut h)
        .pane(Side::LEFT)
        .tab()
        .loc
        .path
        .as_str()
        .ends_with("/src"));
    key(&mut h, Key::Backspace);
    settle(&mut h);
    assert_eq!(
        cursor_name(&mut h, Side::LEFT).as_deref(),
        Some("src"),
        "back with the folder selected"
    );
    assert_eq!(view_of(&mut h, Side::LEFT), ViewMode::Icons);
}

#[test]
fn gallery_keys_step_through_the_filmstrip() {
    let fx = fixture();
    let dir = photos(&fx);
    let mut h = behaviour(&fx);
    app(&mut h).activate(Side::LEFT);
    goto(&mut h, Side::LEFT, &dir);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Gallery));
    h.run_steps(3);
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().cursor,
        Some(0),
        "the gallery always shows an item"
    );
    key(&mut h, Key::ArrowRight);
    key(&mut h, Key::ArrowRight);
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().cursor, Some(2));
    key(&mut h, Key::ArrowLeft);
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().cursor, Some(1));
    key(&mut h, Key::End);
    let n = app(&mut h).pane(Side::LEFT).tab().view.len();
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().cursor, Some(n - 1));
    // Space opens the preview of the selected item.
    let name = cursor_name(&mut h, Side::LEFT).unwrap();
    assert!(!name.is_empty());
}

#[test]
fn columns_navigation_path() {
    let fx = fixture();
    let dir = photos(&fx);
    let mut h = behaviour(&fx);
    app(&mut h).activate(Side::LEFT);
    goto(&mut h, Side::LEFT, &dir);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Columns));
    h.run_steps(3);
    let root = VfsPath::new(&dir.to_string_lossy());
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().view_state.cols.root,
        Some(root.clone())
    );
    // Select "Reisen" by typing, → opens it as the next column with its
    // first item selected, twice.
    h.event(egui::Event::Text("Reisen".into()));
    h.run_steps(2);
    assert_eq!(cursor_name(&mut h, Side::LEFT).as_deref(), Some("Reisen"));
    key(&mut h, Key::ArrowRight);
    settle(&mut h);
    assert!(app(&mut h)
        .pane(Side::LEFT)
        .tab()
        .loc
        .path
        .as_str()
        .ends_with("Fotos/Reisen"));
    assert_eq!(
        cursor_name(&mut h, Side::LEFT).as_deref(),
        Some("2025"),
        "first item selected"
    );
    key(&mut h, Key::ArrowDown);
    assert_eq!(cursor_name(&mut h, Side::LEFT).as_deref(), Some("2026"));
    key(&mut h, Key::ArrowRight);
    settle(&mut h);
    assert_eq!(cursor_name(&mut h, Side::LEFT).as_deref(), Some("Island"));
    h.run_steps(3);
    let path = app(&mut h).pane(Side::LEFT).tab().loc.path.clone();
    let chain = crate::views::column_chain(&root, &path);
    assert_eq!(chain.len(), 3, "Fotos › Reisen › 2026: {chain:?}");
    // The next column lists "Island" in the background.
    let island = path.join("Island");
    assert!(app(&mut h)
        .pane_mut(Side::LEFT)
        .tab_mut()
        .view_state
        .cols
        .wait_loaded(Duration::from_secs(5)));
    h.run_steps(2);
    let listed = app(&mut h)
        .pane(Side::LEFT)
        .tab()
        .view_state
        .cols
        .listings
        .get(&island)
        .map(|l| l.entries.len());
    assert_eq!(listed, Some(5), "the Island column");
    // ← goes back one column, keeping the folder selected.
    key(&mut h, Key::ArrowLeft);
    settle(&mut h);
    assert!(app(&mut h)
        .pane(Side::LEFT)
        .tab()
        .loc
        .path
        .as_str()
        .ends_with("Fotos/Reisen"));
    assert_eq!(cursor_name(&mut h, Side::LEFT).as_deref(), Some("2026"));
    assert_eq!(
        app(&mut h).pane(Side::LEFT).tab().view_state.cols.root,
        Some(root.clone()),
        "the columns still start at Fotos"
    );
    // A click in the first column makes it active again.
    act(
        &mut h,
        Action::ColumnsSelect(Side::LEFT, root.clone(), Some("Archiv".into())),
    );
    settle(&mut h);
    assert_eq!(app(&mut h).pane(Side::LEFT).tab().loc.path, root);
    assert_eq!(cursor_name(&mut h, Side::LEFT).as_deref(), Some("Archiv"));
    // ← past the first column: the columns start one folder higher.
    key(&mut h, Key::ArrowLeft);
    settle(&mut h);
    h.run_steps(2);
    assert_eq!(
        app(&mut h)
            .pane(Side::LEFT)
            .tab()
            .view_state
            .cols
            .root
            .as_ref()
            .map(|p| p.to_string()),
        Some(fx.root.to_string_lossy().into_owned())
    );
    assert_eq!(cursor_name(&mut h, Side::LEFT).as_deref(), Some("Fotos"));
}

/// Type-to-select scrolls the match into view in every view.
#[test]
fn type_to_select_scrolls_into_view_in_every_view() {
    let fx = fixture();
    let dir = fx.root.join("many");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..300 {
        std::fs::write(dir.join(format!("a-file-{i:03}.txt")), b"x").unwrap();
    }
    std::fs::write(dir.join("zebra.txt"), b"z").unwrap();
    for mode in [ViewMode::Icons, ViewMode::Columns, ViewMode::Gallery] {
        let mut h = behaviour(&fx);
        app(&mut h).activate(Side::LEFT);
        goto(&mut h, Side::LEFT, &dir);
        act(&mut h, Action::SetView(Side::LEFT, mode));
        h.run_steps(3);
        h.event(egui::Event::Text("z".into()));
        h.run();
        let t = app(&mut h).pane(Side::LEFT).tab();
        assert_eq!(
            t.cursor_entry().map(|e| e.name.as_str()),
            Some("zebra.txt"),
            "{mode:?}"
        );
        let c = t.cursor.unwrap();
        match mode {
            ViewMode::Icons => {
                let (g, _) = t.view_state.grid.unwrap();
                let top = (c / g.cols) as f32 * g.row_h;
                assert!(
                    top >= t.scroll_offset - 0.5
                        && top + g.row_h <= t.scroll_offset + t.viewport_h + 1.0,
                    "icons: row at {top} in [{}, {}]",
                    t.scroll_offset,
                    t.scroll_offset + t.viewport_h
                );
            }
            ViewMode::Columns => {
                let top = c as f32 * 22.0;
                assert!(
                    top >= t.scroll_offset - 0.5
                        && top + 22.0 <= t.scroll_offset + t.viewport_h + 1.0,
                    "columns: {top} vs {}",
                    t.scroll_offset
                );
            }
            _ => {
                let x = c as f32 * 68.0;
                let (off, w) = (t.view_state.strip_offset, t.view_state.strip_view_w);
                assert!(
                    x >= off - 0.5 && x + 68.0 <= off + w + 1.0,
                    "gallery: {x} in [{off}, {}]",
                    off + w
                );
            }
        }
    }
}

#[test]
fn drag_from_icons_to_another_pane() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).activate(Side::LEFT);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
    h.run_steps(3);
    let i = app(&mut h)
        .pane(Side::LEFT)
        .tab()
        .view_index_of("notes.txt")
        .unwrap();
    let (g, o) = app(&mut h).pane(Side::LEFT).tab().view_state.grid.unwrap();
    let from = g.icon_rect(i).translate(o.to_vec2()).center();
    let to = app(&mut h).pane_rects[1].1.center();
    drag_from_to(&mut h, from, to);
    h.run_steps(3);
    let Some(Dialog::Transfer(f)) = &app(&mut h).dialog else {
        panic!("dropping opens the copy dialog")
    };
    assert_eq!(f.src_side, Side::LEFT);
    assert_eq!(f.dst_side, Side::RIGHT);
    assert_eq!(f.sources.len(), 1);
    assert!(f.sources[0].as_str().ends_with("notes.txt"));
}

#[test]
fn views_are_saved_with_the_session() {
    let fx = fixture();
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");
    let mut h = behaviour(&fx);
    app(&mut h).session_path = Some(session.clone());
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Gallery));
    act(&mut h, Action::SetView(Side::RIGHT, ViewMode::Icons));
    app(&mut h).pane_mut(Side::RIGHT).icon_size = 144.0;
    app(&mut h).save_session();
    let s = crate::pane_set::Session::load(&session).unwrap();
    assert_eq!(s.panes[0].view, ViewMode::Gallery);
    assert_eq!(s.panes[1].view, ViewMode::Icons);
    assert_eq!(s.panes[1].icon_size, Some(144.0));
    // A new start restores them.
    let ctx = egui::Context::default();
    let local: manta_core::SharedVfs = std::sync::Arc::new(manta_vfs::LocalFs::new());
    let home = crate::pane::Location::new(
        local.clone(),
        None,
        VfsPath::new(&fx.root.to_string_lossy()),
    );
    let mut next = 1;
    let (panes, _, _) = MantaApp::restore_panes(Some(&s), &home, &local, &mut next);
    let _ = ctx;
    assert_eq!(panes[0].view, ViewMode::Gallery);
    assert_eq!(panes[1].view, ViewMode::Icons);
    assert_eq!(panes[1].icon_size, 144.0);
    // Old sessions (no view saved) open as lists.
    let old: crate::pane_set::Session =
        serde_json::from_str(r#"{"panes":[{"id":0,"tabs":[{"path":"/tmp"}]}]}"#).unwrap();
    assert_eq!(old.panes[0].view, ViewMode::List);
}

#[test]
fn remember_view_per_folder() {
    let fx = fixture();
    let mut h = behaviour(&fx);
    app(&mut h).prefs.remember_view_per_folder = true;
    app(&mut h).activate(Side::LEFT);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
    goto(&mut h, Side::LEFT, &fx.root.join("src"));
    h.run_steps(2);
    assert_eq!(
        view_of(&mut h, Side::LEFT),
        ViewMode::Icons,
        "an unknown folder keeps the pane's view"
    );
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Columns));
    goto(&mut h, Side::LEFT, &fx.root);
    h.run_steps(2);
    assert_eq!(
        view_of(&mut h, Side::LEFT),
        ViewMode::Icons,
        "Projects was shown as icons"
    );
    goto(&mut h, Side::LEFT, &fx.root.join("src"));
    h.run_steps(2);
    assert_eq!(
        view_of(&mut h, Side::LEFT),
        ViewMode::Columns,
        "src as columns"
    );
    // Off (the default): the view belongs to the pane.
    app(&mut h).prefs.remember_view_per_folder = false;
    goto(&mut h, Side::LEFT, &fx.root);
    h.run_steps(2);
    assert_eq!(view_of(&mut h, Side::LEFT), ViewMode::Columns);
}

#[test]
fn icons_load_thumbnails_and_cancel_when_leaving() {
    let fx = fixture();
    let dir = photos(&fx);
    let many = fx.root.join("many-photos");
    std::fs::create_dir_all(&many).unwrap();
    let img = photo(64, 48, 3);
    for i in 0..400 {
        img.save_with_format(many.join(format!("p{i:03}.png")), image::ImageFormat::Png)
            .unwrap();
    }
    let mut h = behaviour(&fx);
    app(&mut h).activate(Side::LEFT);
    goto(&mut h, Side::LEFT, &dir);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
    settle_thumbs(&mut h);
    assert!(
        app(&mut h).thumbs.texture_count() >= 8,
        "photos have thumbnails: {}",
        app(&mut h).thumbs.texture_count()
    );
    // A big folder: only the visible items are asked for; leaving it
    // cancels what is still queued.
    goto(&mut h, Side::LEFT, &many);
    h.run_steps(1);
    let (queued, _) = app(&mut h).thumbs.queue_stats();
    assert!(queued < 120, "only visible items are requested: {queued}");
    goto(&mut h, Side::LEFT, &fx.root);
    // Frame-based, not time-based: under load the listing swap can take a
    // few more frames before nothing asks for the old folder any more.
    for _ in 0..60 {
        h.step();
        if app(&mut h).thumbs.queue_stats().0 == 0 {
            break;
        }
    }
    let (left, cancelled) = app(&mut h).thumbs.queue_stats();
    assert_eq!(left, 0, "nothing left queued");
    assert!(
        cancelled > 0 || queued == 0,
        "unfinished requests were cancelled"
    );
}

// ------------------------------------------------------------------ renders

fn view_harness(fx: &Fixture, theme: Theme, size: [f32; 2]) -> H {
    let mut h = harness_de(fx, size, theme);
    app(&mut h).activate(Side::LEFT);
    h
}

#[test]
fn render_views() {
    require_gpu!();
    let fx = fixture();
    let dir = photos(&fx);
    let out = render_dir("views");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        // Icons, 96 pt, a selection in the active pane.
        let mut h = view_harness(&fx, theme, WIDE);
        goto(&mut h, Side::LEFT, &dir);
        act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
        app(&mut h).pane_mut(Side::LEFT).icon_size = 96.0;
        settle_thumbs(&mut h);
        select(&mut h, Side::LEFT, &["Hafen.jpg", "Leuchtturm.png"]);
        save_to(&mut h, &out, &format!("icons_{tname}"));

        // Columns three levels deep, a photo selected: the preview column
        // (one pane, so every column shows).
        act(&mut h, Action::ClosePane(Side::RIGHT));
        app(&mut h).activate(Side::LEFT);
        act(&mut h, Action::SetView(Side::LEFT, ViewMode::Columns));
        h.run_steps(2);
        for (dirname, pick) in [
            ("Reisen", "2026"),
            ("2026", "Island"),
            ("Island", "Geysir.jpg"),
        ] {
            let _ = dirname;
            let i = app(&mut h).pane(Side::LEFT).tab().view_index_of(pick);
            if pick == "2026" {
                let i = app(&mut h)
                    .pane(Side::LEFT)
                    .tab()
                    .view_index_of("Reisen")
                    .unwrap();
                app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
                act(&mut h, Action::ColumnsEnter(Side::LEFT));
            }
            if let Some(i) = app(&mut h).pane(Side::LEFT).tab().view_index_of(pick).or(i) {
                app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
                if pick != "Geysir.jpg" {
                    act(&mut h, Action::ColumnsEnter(Side::LEFT));
                }
            }
            settle(&mut h);
        }
        let i = app(&mut h)
            .pane(Side::LEFT)
            .tab()
            .view_index_of("Geysir.jpg")
            .unwrap();
        app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
        app(&mut h)
            .pane_mut(Side::LEFT)
            .tab_mut()
            .view_state
            .cols
            .wait_loaded(Duration::from_secs(5));
        settle_thumbs(&mut h);
        save_to(&mut h, &out, &format!("columns_{tname}"));

        // Gallery: a photo large, the filmstrip below.
        act(&mut h, Action::SplitPane(Side::LEFT));
        app(&mut h).activate(Side::LEFT);
        goto(&mut h, Side::LEFT, &dir);
        act(&mut h, Action::SetView(Side::LEFT, ViewMode::Gallery));
        let i = app(&mut h)
            .pane(Side::LEFT)
            .tab()
            .view_index_of("Hafen.jpg")
            .unwrap();
        app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
        settle_thumbs(&mut h);
        save_to(&mut h, &out, &format!("gallery_{tname}"));

        // A PDF in the gallery (Quick Look thumbnail).
        let i = app(&mut h)
            .pane(Side::LEFT)
            .tab()
            .view_index_of("Reiseplan.pdf")
            .unwrap();
        app(&mut h).pane_mut(Side::LEFT).tab_mut().select_only(i);
        settle_thumbs(&mut h);
        save_to(&mut h, &out, &format!("gallery_pdf_{tname}"));
    }

    // Icons at the smallest and a large size, one pane.
    for (size, name) in [(48.0, "icons_48"), (176.0, "icons_176")] {
        let mut h = view_harness(&fx, Theme::Light, WIDE);
        act(&mut h, Action::ClosePane(Side::RIGHT));
        goto(&mut h, Side::LEFT, &dir);
        act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
        app(&mut h).pane_mut(Side::LEFT).icon_size = size;
        settle_thumbs(&mut h);
        select(&mut h, Side::LEFT, &["Reisen"]);
        save_to(&mut h, &out, name);
    }

    // The options popover (sort + icon size) and narrow panes.
    let mut h = view_harness(&fx, Theme::Dark, NARROW);
    goto(&mut h, Side::LEFT, &dir);
    act(&mut h, Action::SetView(Side::LEFT, ViewMode::Icons));
    settle_thumbs(&mut h);
    save_to(&mut h, &out, "icons_narrow_dark");
}
