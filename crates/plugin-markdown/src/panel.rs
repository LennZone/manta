//! The Markdown panel: a header (Back, file name, README label, table of
//! contents, Open in Editor, Reveal), notices, empty states and the
//! scrolling document.

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};
use manta_core::VfsPath;

use crate::i18n::tr;
use crate::loader::{Content, MAX_DOC_BYTES};
use crate::render::{self, Env};
use crate::style::{self, icon, size, space, Colors};
use crate::{View, Want};

/// What the panel asks the plugin to do.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    Render(render::Action),
    Back,
    /// Show the first 2 MB of a too-large file.
    ShowPartial(VfsPath),
    OpenInEditor(VfsPath),
    Reveal(VfsPath),
}

const HEADER_H: f32 = 36.0;
const SIDE_PAD: f32 = 20.0;

pub fn panel_ui(view: &mut View, web_images: bool, ui: &mut Ui) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let col = Colors::of(ui);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let current = view.current().cloned();
    let shown = view.shown().cloned();
    let local = matches!(
        current,
        Some(Want::File { local: true, .. } | Want::Readme { local: true, .. })
    );

    // ---------------------------------------------------------- header
    let doc_path = shown
        .as_ref()
        .filter(|l| !matches!(l.content, Content::NoReadme))
        .map(|l| l.path.clone());
    let readme = shown.as_ref().is_some_and(|l| l.readme) && view.history.is_empty();
    let title = match (&doc_path, &current) {
        (Some(p), _) => p.file_name().to_string(),
        (None, Some(Want::File { path, .. })) => path.file_name().to_string(),
        _ => String::new(),
    };
    let headings = match shown.as_ref().map(|l| &l.content) {
        Some(Content::Doc { doc, .. }) => doc.headings.clone(),
        _ => Vec::new(),
    };
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    let cy = bar.center().y;
    let mut x = bar.left() + space::MD;
    let mut right = bar.right() - space::MD;
    if !view.history.is_empty() {
        let r = Rect::from_center_size(pos2(x + 13.0, cy), vec2(26.0, 24.0));
        if icon_button(ui, r, "md-back", icon::ARROW_LEFT, tr!("back"), true, col).clicked() {
            actions.push(UiAction::Back);
        }
        x = r.right() + space::XS;
    }
    // Right side buttons: [TOC] [Open in Editor] [Reveal]
    if let Some(p) = &doc_path {
        let r = Rect::from_center_size(pos2(right - 13.0, cy), vec2(26.0, 24.0));
        if icon_button(
            ui,
            r,
            "md-reveal",
            icon::FOLDER_OPEN,
            tr!("reveal"),
            true,
            col,
        )
        .clicked()
        {
            actions.push(UiAction::Reveal(p.clone()));
        }
        right = r.left() - 2.0;
        let r = Rect::from_center_size(pos2(right - 13.0, cy), vec2(26.0, 24.0));
        let tip = if local {
            tr!("edit")
        } else {
            tr!("edit.local_only")
        };
        if icon_button(ui, r, "md-edit", icon::PENCIL_SIMPLE, tip, local, col).clicked() {
            actions.push(UiAction::OpenInEditor(p.clone()));
        }
        right = r.left() - 2.0;
    }
    let mut toc_button = None;
    if !headings.is_empty() {
        let r = Rect::from_center_size(pos2(right - 19.0, cy), vec2(38.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with("md-toc"), Sense::click());
        let on = view.toc_open;
        if on || resp.hovered() {
            ui.painter().rect_filled(
                r,
                CornerRadius::same(5),
                if on { col.accent_soft } else { col.hover },
            );
        }
        let fg = if on { col.link } else { col.text2 };
        style::paint_icon(
            ui,
            Rect::from_center_size(pos2(r.left() + 13.0, cy), vec2(16.0, 16.0)),
            icon::LIST_BULLETS,
            15.0,
            fg,
        );
        style::paint_icon(
            ui,
            Rect::from_center_size(pos2(r.right() - 10.0, cy), vec2(10.0, 10.0)),
            icon::CARET_DOWN,
            10.0,
            fg,
        );
        let resp = resp.on_hover_text(tr!("contents"));
        if resp.clicked() {
            view.toc_open = !view.toc_open;
        }
        toc_button = Some(resp);
        right = r.left() - space::XS;
    }
    // File icon, name, README label.
    if !title.is_empty() {
        let ir = Rect::from_center_size(pos2(x + 9.0, cy), vec2(18.0, 18.0));
        style::paint_icon(ui, ir, icon::FILE_TEXT, 15.0, col.text2);
        x = ir.right() + space::SM;
        let pill_w = if readme { 58.0 } else { 0.0 };
        let max_w = (right - x - pill_w - space::SM).max(20.0);
        let g = galley_line(ui, &title, style::semibold(ui, size::BODY), col.text, max_w);
        let gw = g.size().x;
        ui.painter()
            .galley(pos2(x, cy - g.size().y / 2.0), g, col.text);
        x += gw + space::SM;
        if readme {
            let pg =
                ui.painter()
                    .layout_no_wrap("README".into(), style::medium(ui, 10.0), col.text2);
            let pr = Rect::from_min_size(pos2(x, cy - 8.0), vec2(pg.size().x + 12.0, 16.0));
            ui.painter()
                .rect_filled(pr, CornerRadius::same(4), col.neutral_soft);
            ui.painter()
                .galley(pr.center() - pg.size() / 2.0, pg, col.text2);
            ui.interact(pr, ui.scope_id().with("md-readme"), Sense::hover())
                .on_hover_text(tr!("readme.tip"));
        }
    }
    ui.painter().hline(
        bar.x_range(),
        bar.bottom() - 0.5,
        Stroke::new(1.0, col.separator),
    );

    // ---------------------------------------------------------- body
    let Some(loaded) = shown else {
        match &current {
            Some(Want::File { .. } | Want::Readme { .. }) => {
                loading(ui, col);
            }
            Some(Want::NotMarkdown { name }) => {
                empty_state(
                    ui,
                    col,
                    icon::FILE_TEXT,
                    tr!("empty.title"),
                    &tr!("empty.not_markdown", name = name),
                    None,
                );
            }
            None => {
                empty_state(
                    ui,
                    col,
                    icon::FILE_TEXT,
                    tr!("empty.title"),
                    tr!("empty.body"),
                    None,
                );
            }
        }
        return actions;
    };
    match &loaded.content {
        Content::NoReadme => {
            empty_state(
                ui,
                col,
                icon::FILE_TEXT,
                tr!("empty.title"),
                tr!("empty.body.readme"),
                None,
            );
        }
        Content::Unreadable(e) => {
            empty_state(
                ui,
                col,
                icon::WARNING_CIRCLE,
                &tr!("error.cant_read", name = loaded.path.file_name()),
                crate::i18n::show_msg(e),
                None,
            );
        }
        Content::TooLarge { size } => {
            let msg = tr!(
                "error.too_large.body",
                size = format_size(*size),
                max = format_size(MAX_DOC_BYTES)
            );
            if empty_state(
                ui,
                col,
                icon::FILE_TEXT,
                &tr!("error.too_large.title", name = loaded.path.file_name()),
                &msg,
                Some(tr!("error.too_large.button")),
            ) {
                actions.push(UiAction::ShowPartial(loaded.path.clone()));
            }
        }
        Content::Doc { doc, partial } => {
            if let Some((shown_b, total)) = partial {
                notice(
                    ui,
                    col,
                    &tr!(
                        "partial",
                        shown = format_size(*shown_b),
                        total = format_size(*total)
                    ),
                );
            }
            let dir = loaded.path.parent().unwrap_or_else(VfsPath::root);
            let doc = doc.clone();
            let mut env = Env::new(ui, &doc, dir, &view.images, web_images);
            env.anchor = view.anchor.clone();
            let id = ui.scope_id().with(("md-scroll", loaded.path.as_str()));
            let heights = &mut view.heights;
            let out = egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, vp| {
                    let avail = ui.available_width();
                    let w = (avail - 2.0 * SIDE_PAD).clamp(80.0, size::MEASURE);
                    let side = ((avail - w) / 2.0).max(space::LG);
                    ui.add_space(space::XL + 2.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            env.show(ui, vp, heights);
                        });
                    });
                    vp.top()
                });
            view.scroll_y = out.inner;
            // Everything is drawn while an anchor is pending, so one that
            // is still pending doesn't exist: drop it.
            view.anchor = None;
            view.anchor_y = env.anchor_y.clone();
            actions.extend(env.actions.into_iter().map(UiAction::Render));
        }
    }

    // ---------------------------------------------------------- TOC popup
    if view.toc_open {
        if let Some(btn) = toc_button {
            toc_popup(ui, view, &headings, &btn, col);
        }
    }
    actions
}

fn toc_popup(
    ui: &mut Ui,
    view: &mut View,
    headings: &[crate::doc::Heading],
    btn: &egui::Response,
    col: Colors,
) {
    let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
    // The section in view: the last heading above the top edge.
    let current = headings
        .iter()
        .rfind(|h| {
            view.anchor_y
                .get(&h.slug)
                .is_some_and(|y| *y <= view.scroll_y + 24.0)
        })
        .map(|h| h.slug.clone());
    let width = 280.0f32.min(ui.ctx().content_rect().width() - 24.0);
    let pos = pos2(
        (btn.rect.right() - width).max(ui.max_rect().left() + 4.0),
        btn.rect.bottom() + 4.0,
    );
    let area = egui::Area::new(ui.scope_id().with("md-toc-popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(if col.dark {
                    Color32::from_rgb(44, 44, 47)
                } else {
                    Color32::from_rgb(252, 252, 253)
                })
                .stroke(Stroke::new(1.0, col.code_border))
                .corner_radius(CornerRadius::same(8))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_black_alpha(if col.dark { 90 } else { 34 }),
                })
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.set_width(width - 12.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let cap = ui.painter().layout_no_wrap(
                        tr!("contents.caption").into(),
                        style::medium(ui, 10.0),
                        col.text3,
                    );
                    let (r, _) = ui.allocate_exact_size(vec2(width - 12.0, 22.0), Sense::hover());
                    ui.painter().galley(
                        pos2(r.left() + 8.0, r.center().y - cap.size().y / 2.0),
                        cap,
                        col.text3,
                    );
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for h in headings {
                                let indent = (h.level - min_level) as f32 * 14.0;
                                let (r, resp) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 26.0),
                                    Sense::click(),
                                );
                                let here = current.as_deref() == Some(h.slug.as_str());
                                if resp.hovered() {
                                    ui.painter()
                                        .rect_filled(r, CornerRadius::same(5), col.hover);
                                }
                                if here {
                                    let bar = Rect::from_min_size(
                                        pos2(r.left() + 2.0, r.top() + 6.0),
                                        vec2(2.5, 14.0),
                                    );
                                    ui.painter().rect_filled(
                                        bar,
                                        CornerRadius::same(1),
                                        col.accent,
                                    );
                                }
                                let top = h.level == min_level;
                                let font = if top {
                                    style::medium(ui, size::BODY)
                                } else {
                                    style::regular(size::BODY)
                                };
                                let color = if here {
                                    col.link
                                } else if top {
                                    col.text
                                } else {
                                    col.text2
                                };
                                let g = galley_line(
                                    ui,
                                    &h.text,
                                    font,
                                    color,
                                    r.width() - 20.0 - indent,
                                );
                                ui.painter().galley(
                                    pos2(r.left() + 10.0 + indent, r.center().y - g.size().y / 2.0),
                                    g,
                                    color,
                                );
                                if resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    view.anchor = Some(h.slug.clone());
                                    view.toc_open = false;
                                }
                            }
                        });
                });
        });
    // Click elsewhere or Esc closes it.
    let clicked_out = ui.input(|i| i.pointer.any_pressed())
        && ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !area.response.rect.contains(p) && !btn.rect.contains(p));
    if clicked_out || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        view.toc_open = false;
    }
}

fn icon_button(
    ui: &mut Ui,
    r: Rect,
    id: &str,
    glyph: &str,
    tip: &str,
    enabled: bool,
    col: Colors,
) -> egui::Response {
    let resp = ui.interact(
        r,
        ui.scope_id().with(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(r, CornerRadius::same(5), col.hover);
    }
    let fg = if enabled { col.text2 } else { col.text3 };
    style::paint_icon(ui, r, glyph, 15.0, fg);
    resp.on_hover_text(tip)
}

fn galley_line(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_w.max(8.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

fn loading(ui: &mut Ui, col: Colors) {
    ui.add_space(space::XL * 2.0);
    ui.vertical_centered(|ui| {
        ui.add(egui::Spinner::new().size(14.0).color(col.text3));
    });
}

/// A centred message; returns true if its button was clicked.
fn empty_state(
    ui: &mut Ui,
    col: Colors,
    glyph: &str,
    title: &str,
    text: &str,
    button: Option<&str>,
) -> bool {
    let mut clicked = false;
    let h = ui.available_height();
    ui.add_space((h * 0.28).clamp(space::XL, 160.0));
    ui.vertical_centered(|ui| {
        let w = (ui.available_width() - 48.0).clamp(120.0, 300.0);
        ui.set_max_width(w);
        let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
        ui.painter()
            .circle_filled(r.center(), 22.0, col.neutral_soft);
        style::paint_icon(ui, r, glyph, 22.0, col.text3);
        ui.add_space(space::LG);
        ui.label(
            RichText::new(title)
                .font(style::semibold(ui, size::BODY))
                .color(col.text),
        );
        ui.add_space(space::XS);
        ui.label(
            RichText::new(text)
                .size(size::SMALL)
                .color(col.text2)
                .line_height(Some(17.0)),
        );
        if let Some(b) = button {
            ui.add_space(space::LG);
            let g =
                ui.painter()
                    .layout_no_wrap(b.to_string(), style::medium(ui, size::BODY), col.text);
            let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 24.0, 24.0), Sense::click());
            ui.painter().rect_filled(
                r,
                CornerRadius::same(6),
                if resp.hovered() { col.hover } else { col.bg },
            );
            ui.painter().rect_stroke(
                r,
                CornerRadius::same(6),
                Stroke::new(1.0, col.code_border),
                StrokeKind::Inside,
            );
            ui.painter()
                .galley(r.center() - g.size() / 2.0, g, col.text);
            clicked = resp.clicked();
        }
    });
    clicked
}

fn notice(ui: &mut Ui, col: Colors, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let fill = if col.dark {
        Color32::from_rgb(70, 52, 18)
    } else {
        Color32::from_rgb(255, 242, 214)
    };
    ui.painter().rect_filled(r, CornerRadius::ZERO, fill);
    let ir = Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(16.0, 16.0));
    style::paint_icon(ui, ir, icon::INFO, 13.0, col.warning);
    ui.painter().text(
        pos2(ir.right() + space::SM, r.center().y),
        Align2::LEFT_CENTER,
        text,
        style::regular(size::SMALL),
        col.warning,
    );
}

pub fn format_size(b: u64) -> String {
    const UNITS: [&str; 4] = ["", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1000.0 && u + 1 < UNITS.len() {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        tr!("bytes", count = b)
    } else if v >= 10.0 || (v.fract() < 0.05) {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{} {}", manta_plugin::i18n::format_decimal(v, 1), UNITS[u])
    }
}
