//! Small reusable widgets drawn with the design tokens from [`crate::theme`].

use eframe::egui::{
    self, Color32, CornerRadius, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{self, pal, radius, space, text};

/// Paint an icon glyph centered in `rect`.
pub fn paint_icon(ui: &Ui, rect: egui::Rect, glyph: &str, size: f32, color: Color32, fill: bool) {
    let font = if fill {
        theme::icon_fill_font(size)
    } else {
        theme::icon_font(size)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        font,
        color,
    );
}

/// An icon as a non-interactive widget.
pub fn icon(ui: &mut Ui, glyph: &str, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 2.0), Sense::hover());
    paint_icon(ui, rect, glyph, size, color, false);
    resp
}

/// Tooltip with a label and an optional keyboard shortcut, Finder style.
///
/// Also names the widget for VoiceOver when nothing else did (icon-only
/// buttons): the tooltip text becomes its accessible name, the shortcut its
/// keyboard shortcut, and a painted click target without a role becomes a
/// button.
pub fn tooltip(resp: Response, label: &str, shortcut: &str) -> Response {
    describe(&resp, label, shortcut);
    if shortcut.is_empty() {
        return resp.on_hover_text(label);
    }
    let label = label.to_string();
    let shortcut = shortcut.to_string();
    resp.on_hover_ui(move |ui| {
        ui.horizontal(|ui| {
            ui.label(&label);
            ui.add_space(space::LG);
            ui.label(RichText::new(&shortcut).color(pal(ui).text_secondary));
        });
    })
}

/// Accessible name, role and shortcut from a tooltip (see [`tooltip`]).
fn describe(resp: &Response, label: &str, shortcut: &str) {
    if !resp.ctx.has_accesskit_node(resp.id) {
        return;
    }
    // The first line names the widget; further lines describe it.
    let label = crate::a11y::strip_icons(label);
    let (name, rest) = label.split_once('\n').unwrap_or((&label, ""));
    let senses_click = resp.sense.senses_click();
    resp.ctx.accesskit_node_builder(resp.id, |n| {
        use egui::accesskit::Role;
        if n.role() == Role::Unknown && senses_click {
            n.set_role(Role::Button);
        }
        if n.role() != Role::Label
            && n.labelled_by().is_empty()
            && n.label().is_none_or(|l| l.trim().is_empty())
        {
            n.set_label(name.trim());
        }
        if !rest.trim().is_empty() && n.description().is_none() {
            n.set_description(rest.trim());
        }
        if !shortcut.is_empty() {
            n.set_keyboard_shortcut(shortcut);
        }
    });
}

/// Toolbar-style borderless icon button. `selected` shows a toggled state.
pub fn icon_button(ui: &mut Ui, glyph: &str, enabled: bool, selected: bool) -> Response {
    let p = pal(ui);
    let size = egui::vec2(30.0, theme::size::TOOLBAR_BUTTON);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if !enabled {
        // Still on VoiceOver's map, dimmed ("dimmed button"); named by its
        // tooltip (see [`tooltip`]).
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, false, ""));
    }
    let hovered = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    let t =
        ui.ctx()
            .animate_bool_with_time(resp.id.with("hover"), hovered, crate::a11y::anim(0.08));
    let r = CornerRadius::same(radius::CONTROL + 1);
    if selected {
        ui.painter().rect_filled(rect, r, p.accent_soft);
    } else if pressed {
        ui.painter().rect_filled(rect, r, p.pressed);
    } else if t > 0.0 {
        ui.painter().rect_filled(rect, r, p.hover.gamma_multiply(t));
    }
    let color = if !enabled {
        p.text_disabled
    } else if selected {
        p.accent_text
    } else {
        p.text_secondary
    };
    paint_icon(ui, rect, glyph, 17.0, color, false);
    // A toggle (e.g. "Show hidden files") says whether it is on; the name
    // comes from the tooltip (see [`tooltip`]). The focus ring is drawn by
    // `a11y::paint_focus_ring`.
    if selected {
        resp.ctx.accesskit_node_builder(resp.id, |n| {
            n.set_role(egui::accesskit::Role::Button);
            n.set_toggled(egui::accesskit::Toggled::True);
        });
    }
    resp
}

/// Small borderless icon button for inline use (sidebar headers, rows).
pub fn mini_icon_button(ui: &mut Ui, glyph: &str, color: Color32) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::CONTROL), p.hover);
    }
    let c = if resp.hovered() { p.text } else { color };
    paint_icon(ui, rect, glyph, 13.0, c, false);
    resp
}

pub fn focus_ring(ui: &Ui, rect: egui::Rect, r: CornerRadius) {
    let p = pal(ui);
    ui.painter().rect_stroke(
        rect.expand(1.0),
        r,
        Stroke::new(2.0, p.focus_ring.gamma_multiply(0.8)),
        StrokeKind::Outside,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

/// Push button with a clear hierarchy: Primary (accent fill), Secondary
/// (neutral), Danger (red fill for destructive confirmations).
/// The width [`button`] takes for `label` (to plan a row before drawing it).
pub fn button_width(ui: &Ui, label: &str, kind: ButtonKind) -> f32 {
    let font = if kind == ButtonKind::Secondary {
        theme::regular(text::BODY)
    } else {
        theme::medium(text::BODY)
    };
    let w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, Color32::WHITE)
        .size()
        .x;
    (w + 2.0 * space::LG + 4.0).max(76.0)
}

/// The width an `egui::Checkbox` with `label` takes on one line.
pub fn checkbox_width(ui: &Ui, label: &str) -> f32 {
    let s = ui.spacing();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, Color32::WHITE)
        .size()
        .x;
    s.icon_width + s.icon_spacing + w + 2.0 * s.button_padding.x
}

pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    let p = pal(ui);
    let (fill, fg, stroke) = match kind {
        ButtonKind::Primary => (p.accent, p.text_on_accent, Stroke::NONE),
        ButtonKind::Danger => (
            if p.dark {
                Color32::from_rgb(220, 60, 55)
            } else {
                p.danger
            },
            Color32::WHITE,
            Stroke::NONE,
        ),
        ButtonKind::Secondary => (
            ui.visuals().widgets.inactive.bg_fill,
            p.text,
            Stroke::new(1.0, p.border),
        ),
    };
    let font = if kind == ButtonKind::Secondary {
        theme::regular(text::BODY)
    } else {
        theme::medium(text::BODY)
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let size = egui::vec2((galley.size().x + 2.0 * space::LG + 4.0).max(76.0), 26.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let spoken = crate::a11y::strip_icons(label);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &spoken));
    let r = CornerRadius::same(radius::CONTROL + 1);
    let mut fill = fill;
    if !enabled {
        fill = fill.gamma_multiply(0.45);
    } else if resp.is_pointer_button_down_on() {
        fill = if kind == ButtonKind::Secondary {
            ui.visuals().widgets.active.bg_fill
        } else {
            fill.gamma_multiply(0.85)
        };
    } else if resp.hovered() {
        fill = if kind == ButtonKind::Secondary {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            fill.gamma_multiply(0.93)
        };
    }
    ui.painter().rect(rect, r, fill, stroke, StrokeKind::Inside);
    let fg = if enabled { fg } else { fg.gamma_multiply(0.7) };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// A small rounded label ("chip") with optional icon.
pub fn chip(
    ui: &mut Ui,
    icon_glyph: Option<&str>,
    label: &str,
    fg: Color32,
    bg: Color32,
) -> Response {
    let font = theme::medium(text::CAPTION);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let icon_w = if icon_glyph.is_some() { 14.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + icon_w + 2.0 * space::SM + 2.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::PILL), bg);
    let mut x = rect.left() + space::SM + 1.0;
    if let Some(g) = icon_glyph {
        let r = egui::Rect::from_min_size(
            egui::pos2(x - 1.0, rect.top()),
            egui::vec2(14.0, rect.height()),
        );
        paint_icon(ui, r, g, 11.0, fg, false);
        x += icon_w;
    }
    ui.painter().galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    resp
}

/// Toggleable filter pill (used for the compare legend).
pub fn toggle_pill(
    ui: &mut Ui,
    on: &mut bool,
    glyph: &str,
    label: &str,
    color: Color32,
) -> Response {
    let p = pal(ui);
    let font = theme::medium(text::SMALL);
    let fg = if *on { color } else { p.text_tertiary };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let size = egui::vec2(galley.size().x + 18.0 + 2.0 * space::MD, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let r = CornerRadius::same(radius::PILL);
    let bg = if *on {
        color.gamma_multiply(if p.dark { 0.22 } else { 0.12 })
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if *on {
        Stroke::new(1.0, color.gamma_multiply(0.35))
    } else {
        Stroke::new(1.0, p.border)
    };
    ui.painter().rect(rect, r, bg, stroke, StrokeKind::Inside);
    if resp.hovered() {
        ui.painter().rect_filled(rect, r, p.hover);
    }
    let ir = egui::Rect::from_min_size(
        egui::pos2(rect.left() + space::MD - 2.0, rect.top()),
        egui::vec2(18.0, rect.height()),
    );
    paint_icon(ui, ir, glyph, 13.0, fg, false);
    ui.painter().galley(
        egui::pos2(ir.right() + 2.0, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    if resp.clicked() {
        *on = !*on;
    }
    resp
}

/// macOS-style segmented control. Returns true if the value changed.
///
/// `id_salt` must be unique within the parent `Ui`: the segments' ids derive
/// from it, so two controls on one row (or a control whose row changes
/// between frames) never share or swap ids.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let p = pal(ui);
    let font = theme::regular(text::BODY);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), p.text)
                .size()
                .x
                + 2.0 * space::LG
        })
        .collect();
    let seg_w = widths.iter().cloned().fold(56.0, f32::max);
    let total = egui::vec2(seg_w * options.len() as f32 + 4.0, 26.0);
    let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
    let base_id = ui.scope_id().with(("segmented", id_salt));
    let track = if p.dark {
        Color32::from_rgb(54, 54, 58)
    } else {
        Color32::from_rgb(228, 228, 232)
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::CONTROL + 2), track);
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 2.0 + seg_w * i as f32, rect.top() + 2.0),
            egui::vec2(seg_w, rect.height() - 4.0),
        );
        let resp = ui.interact(r, base_id.with(i), Sense::click());
        let on = *value == *v;
        let enabled = ui.is_enabled();
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, on, *label)
        });
        if on {
            let thumb = if p.dark {
                Color32::from_rgb(98, 98, 104)
            } else {
                Color32::WHITE
            };
            ui.painter()
                .rect_filled(r, CornerRadius::same(radius::CONTROL), thumb);
            ui.painter().rect_stroke(
                r,
                CornerRadius::same(radius::CONTROL),
                Stroke::new(0.5, Color32::from_black_alpha(30)),
                StrokeKind::Outside,
            );
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(r, CornerRadius::same(radius::CONTROL), p.hover);
        }
        let f = if on {
            theme::medium(text::BODY)
        } else {
            font.clone()
        };
        ui.painter()
            .text(r.center(), egui::Align2::CENTER_CENTER, *label, f, p.text);
        if resp.clicked() && !on {
            *value = *v;
            changed = true;
        }
    }
    changed
}

/// Standard single-line text field: body font, text vertically centred in a
/// [`theme::size::FIELD`]-high box, 8 pt side inset. Show it with
/// [`show_input`] so its placeholder lines up with the typed text.
pub fn input(value: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value)
        .font(theme::regular(text::BODY))
        .margin(egui::Margin::symmetric(space::MD as i8, 0))
        .min_size(egui::vec2(0.0, theme::size::FIELD))
        .align(egui::Align2::LEFT_CENTER)
}

/// Frameless single-line editor that fills `rect` and centres its text
/// vertically, starting `inset` points from the left edge. Used where the
/// text must line up with painted text (breadcrumbs, search fields).
pub fn bare_input<'a>(value: &'a mut String, rect: egui::Rect, inset: f32) -> egui::TextEdit<'a> {
    // Note: with an explicit `frame`, egui ignores `TextEdit::margin`, so the
    // inset has to be the frame's inner margin.
    egui::TextEdit::singleline(value)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: inset.round() as i8,
            right: 0,
            top: 0,
            bottom: 0,
        }))
        .font(theme::regular(text::BODY))
        .min_size(rect.size())
        .desired_width(rect.width() - inset)
        .align(egui::Align2::LEFT_CENTER)
}

/// Show a text edit and, while it's empty, its placeholder.
///
/// egui lays a `hint_text` out as a separate atom, which can land half a
/// point off the typed text and uses the default font unless told
/// otherwise. Here the placeholder is painted at the empty text galley's own
/// position, in the field's `font`, so placeholder, cursor and typed text
/// share one baseline and one left edge.
pub fn show_with_hint(
    ui: &mut Ui,
    edit: egui::TextEdit<'_>,
    font: egui::FontId,
    hint_text: &str,
) -> Response {
    let out = edit.show(ui);
    if out.galley.text().is_empty() && !hint_text.is_empty() {
        // `text_clip_rect` hugs the (empty) text, so clip to the field.
        let field = out.response.response.rect;
        let clip = egui::Rect::from_min_max(
            egui::pos2(field.left(), field.top()),
            egui::pos2(field.right() - space::XS, field.bottom()),
        )
        .intersect(ui.clip_rect());
        let p = pal(ui);
        let mut job =
            egui::text::LayoutJob::simple_singleline(hint_text.to_string(), font, p.text_tertiary);
        job.wrap =
            egui::text::TextWrapping::truncate_at_width((clip.right() - out.galley_pos.x).max(0.0));
        let galley = ui.painter().layout_job(job);
        ui.painter()
            .with_clip_rect(clip)
            .galley(out.galley_pos, galley, p.text_tertiary);
    }
    let resp = out.response.response;
    if !hint_text.is_empty() {
        // The placeholder is what VoiceOver reads for a field without a label.
        let hint_owned = hint_text.to_string();
        ui.ctx().accesskit_node_builder(resp.id, |node| {
            if node.label().is_none_or(|l| l.trim().is_empty()) && node.labelled_by().is_empty() {
                node.set_label(hint_owned.clone());
            }
            node.set_placeholder(hint_owned);
        });
    }
    resp
}

/// [`show_with_hint`] for a framed body-font field built with [`input`],
/// with the standard focus ring. (egui outlines a focused field in
/// `selection.stroke`, which is the white selected-text colour here.)
pub fn show_input(ui: &mut Ui, edit: egui::TextEdit<'_>, hint_text: &str) -> Response {
    let resp = show_with_hint(ui, edit, theme::regular(text::BODY), hint_text);
    if resp.has_focus() {
        focus_ring(ui, resp.rect, CornerRadius::same(radius::CONTROL));
    }
    resp
}

/// [`show_with_hint`] for a frameless body-font field placed exactly in
/// `rect` (the caller paints background and focus ring).
pub fn put_input(
    ui: &mut Ui,
    rect: egui::Rect,
    edit: egui::TextEdit<'_>,
    hint_text: &str,
) -> Response {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| show_with_hint(ui, edit, theme::regular(text::BODY), hint_text),
    )
    .inner
}

/// Y offset from a single-line galley's top to its first row's baseline.
pub fn baseline_offset(galley: &egui::Galley) -> f32 {
    galley
        .rows
        .first()
        .and_then(|r| r.glyphs.first().map(|g| r.pos.y + g.pos.y))
        .unwrap_or(galley.size().y * 0.78)
}

/// Baseline of body text vertically centred in a box spanning `center_y`,
/// the way [`input`], [`bare_input`] and the breadcrumbs place it. Painted
/// text in other sizes uses this so a whole bar shares one baseline.
pub fn body_baseline(ui: &Ui, center_y: f32) -> f32 {
    let g = ui
        .painter()
        .layout_no_wrap("Hg".into(), theme::regular(text::BODY), Color32::WHITE);
    center_y - g.size().y / 2.0 + baseline_offset(&g)
}

/// Paint `galley` with its baseline at `baseline`, starting at `x`.
pub fn paint_on_baseline(
    ui: &Ui,
    x: f32,
    baseline: f32,
    galley: std::sync::Arc<egui::Galley>,
    color: Color32,
) {
    let y = baseline - baseline_offset(&galley);
    ui.painter().galley(egui::pos2(x, y), galley, color);
}

/// Background of a search-style field (shared with the path editor).
pub fn field_background(ui: &Ui, rect: egui::Rect, focused: bool) {
    let p = pal(ui);
    let fill = if p.dark {
        Color32::from_rgb(52, 52, 56)
    } else {
        Color32::from_rgb(238, 238, 241)
    };
    let r = CornerRadius::same(radius::CONTROL + 1);
    ui.painter().rect_filled(rect, r, fill);
    if focused {
        focus_ring(ui, rect, r);
    }
}

/// Rounded search field with a magnifier and a clear button. Its text is
/// vertically centred in a [`theme::size::FIELD`]-high box.
pub fn search_field(
    ui: &mut Ui,
    value: &mut String,
    hint_text: &str,
    id: egui::Id,
    width: f32,
) -> Response {
    let p = pal(ui);
    let height = theme::size::FIELD;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    field_background(ui, rect, ui.memory(|m| m.has_focus(id)));
    let icon_rect = egui::Rect::from_min_size(rect.min, egui::vec2(24.0, height));
    paint_icon(
        ui,
        icon_rect,
        crate::icons::MAGNIFYING_GLASS,
        13.0,
        p.text_secondary,
        false,
    );
    let clear_w = if value.is_empty() { 0.0 } else { 22.0 };
    let edit_rect = egui::Rect::from_min_max(
        egui::pos2(icon_rect.right(), rect.top()),
        egui::pos2(rect.right() - clear_w - space::XS, rect.bottom()),
    );
    let resp = put_input(
        ui,
        edit_rect,
        bare_input(value, edit_rect, 0.0).id(id),
        hint_text,
    );
    if !value.is_empty() {
        let cr = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 22.0, rect.top()),
            egui::vec2(20.0, height),
        );
        let c = ui.interact(cr, id.with("clear"), Sense::click());
        paint_icon(
            ui,
            cr,
            crate::icons::X_CIRCLE,
            14.0,
            if c.hovered() { p.text } else { p.text_tertiary },
            false,
        );
        if c.clicked() {
            value.clear();
            let mut r = resp.clone();
            r.mark_changed();
            return tooltip(r, t!("widgets.clear"), "");
        }
    }
    resp
}

/// Centered empty / error state: big icon, title, body and up to a few
/// buttons. Returns the index of the clicked button.
pub fn empty_state(
    ui: &mut Ui,
    rect: egui::Rect,
    glyph: &str,
    color: Color32,
    title: &str,
    body: &str,
    buttons: &[(&str, ButtonKind)],
) -> Option<usize> {
    let p = pal(ui);
    let width = (rect.width() - 2.0 * space::XL).clamp(120.0, 360.0);
    let inner = egui::Rect::from_center_size(
        rect.center() - egui::vec2(0.0, 24.0),
        egui::vec2(width, 200.0),
    );
    let mut clicked = None;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), Sense::hover());
            paint_icon(ui, r, glyph, 40.0, color, false);
            ui.add_space(space::SM);
            ui.add(
                egui::Label::new(
                    RichText::new(title)
                        .font(theme::semibold(text::TITLE))
                        .color(p.text),
                )
                .wrap(),
            );
            if !body.is_empty() {
                ui.add_space(space::XXS);
                ui.add(
                    egui::Label::new(RichText::new(body).color(p.text_secondary))
                        .wrap()
                        .halign(egui::Align::Center),
                );
            }
            if !buttons.is_empty() {
                ui.add_space(space::LG);
                ui.horizontal(|ui| {
                    let font = theme::medium(text::BODY);
                    let total: f32 = buttons
                        .iter()
                        .map(|(l, _)| {
                            (ui.painter()
                                .layout_no_wrap(l.to_string(), font.clone(), p.text)
                                .size()
                                .x
                                + 2.0 * space::LG
                                + 4.0)
                                .max(76.0)
                        })
                        .sum::<f32>()
                        + space::MD * (buttons.len() as f32 - 1.0);
                    ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                    ui.spacing_mut().item_spacing.x = space::MD;
                    for (i, (label, kind)) in buttons.iter().enumerate() {
                        if button(ui, label, *kind, true).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            }
        },
    );
    clicked
}

/// `text` laid out in `font`, cut from the start ("…/b/c") to fit `width`:
/// for paths the end is what tells two folders apart.
pub fn truncate_start(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), color);
    if full.size().x <= width {
        return full;
    }
    let chars: Vec<char> = text.chars().collect();
    // Smallest cut that fits (binary search over the number of dropped chars).
    let (mut lo, mut hi) = (1, chars.len());
    let make = |k: usize| -> String {
        std::iter::once('…')
            .chain(chars[k..].iter().copied())
            .collect()
    };
    while lo < hi {
        let mid = (lo + hi) / 2;
        let w = ui
            .painter()
            .layout_no_wrap(make(mid), font.clone(), color)
            .size()
            .x;
        if w <= width {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    ui.painter().layout_no_wrap(make(lo), font, color)
}

/// macOS-style on/off switch. Returns the response; `clicked()` means the
/// user wants it flipped (the caller decides, e.g. after a confirmation).
pub fn toggle_switch(ui: &mut Ui, on: bool, enabled: bool) -> Response {
    let p = pal(ui);
    let size = egui::vec2(34.0, 20.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, ""));
    let t = ui
        .ctx()
        .animate_bool_with_time(resp.id, on, crate::a11y::anim(0.12));
    let off_track = if p.dark {
        Color32::from_rgb(72, 72, 78)
    } else {
        Color32::from_rgb(214, 214, 220)
    };
    let mut track = crate::pane::lerp_color(off_track, p.accent, t);
    if !enabled {
        track = track.gamma_multiply(0.5);
    } else if resp.hovered() {
        track = track.gamma_multiply(0.92);
    }
    let r = CornerRadius::same(radius::PILL);
    ui.painter().rect_filled(rect, r, track);
    let knob_r = rect.height() / 2.0 - 2.0;
    let x = egui::lerp(
        (rect.left() + 2.0 + knob_r)..=(rect.right() - 2.0 - knob_r),
        t,
    );
    let c = egui::pos2(x, rect.center().y);
    ui.painter().circle_filled(
        c + egui::vec2(0.0, 0.5),
        knob_r + 0.5,
        Color32::from_black_alpha(40),
    );
    ui.painter().circle_filled(c, knob_r, Color32::WHITE);
    resp.on_hover_cursor(if enabled {
        egui::CursorIcon::PointingHand
    } else {
        egui::CursorIcon::Default
    })
}

/// Section title in sidebars and panels: sentence case, semibold caption.
pub fn section_title(ui: &mut Ui, title: &str) -> Response {
    let p = pal(ui);
    let resp = ui.label(
        RichText::new(title)
            .font(theme::semibold(text::CAPTION))
            .color(p.text_secondary),
    );
    // A heading for VoiceOver's rotor (jump from section to section).
    resp.ctx.accesskit_node_builder(resp.id, |n| {
        n.set_role(egui::accesskit::Role::Heading);
        n.set_label(title);
    });
    resp
}

#[cfg(test)]
mod tests {
    use egui_kittest::{kittest::Queryable, Harness};

    /// Two segmented controls on one row must not share segment ids: a
    /// click in the second one changes only the second value.
    #[test]
    fn segmented_controls_on_one_row_are_independent() {
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum A {
            One,
            Two,
        }
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum B {
            Left,
            Right,
        }
        let state = (A::One, B::Left);
        let mut h = Harness::builder()
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::install)
            .build_ui_state(
                |ui, state: &mut (A, B)| {
                    ui.horizontal(|ui| {
                        super::segmented(
                            ui,
                            "a",
                            &mut state.0,
                            &[(A::One, "One"), (A::Two, "Two")],
                        );
                        super::segmented(
                            ui,
                            "b",
                            &mut state.1,
                            &[(B::Left, "Left"), (B::Right, "Right")],
                        );
                    });
                },
                state,
            );
        h.run();
        h.get_by_label("Right").click();
        h.run();
        assert_eq!(*h.state(), (A::One, B::Right));
        h.get_by_label("Two").click();
        h.run();
        assert_eq!(*h.state(), (A::Two, B::Right));
    }
}
