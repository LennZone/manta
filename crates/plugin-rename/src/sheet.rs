//! The Batch Rename sheet: scope line and presets on top, the rule
//! pipeline on the left, the live preview on the right, the summary and
//! buttons in the footer. Drawn inside the host's modal frame (the host
//! draws the title bar and close button).

use egui::{
    pos2, text::LayoutJob, vec2, Align, Align2, Color32, CornerRadius, Id, Key, Layout, Margin,
    Rect, RectAlign, Sense, Stroke, StrokeKind, TextFormat, Ui, UiBuilder,
};

use crate::i18n::tr;
use crate::presets::Presets;
use crate::preview::{Preview, Row};
use crate::rules::{
    CaseMode, DateSource, ExtMode, InsertAt, Place, RemoveMode, Rule, RuleKind, RuleType,
    SpaceMode, Target, DATE_PRESETS,
};
use crate::style::{self, icon, pal, space, text, ButtonKind, Pal};

/// Preferred size of the sheet (the host clamps it to the window).
pub const SHEET_SIZE: [f32; 2] = [920.0, 620.0];

const ROW_H: f32 = 24.0;
fn small_text_w(ui: &Ui, s: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(s.to_string(), style::regular(text::SMALL), Color32::WHITE)
        .size()
        .x
}

/// Width of a [`style::segmented`] control with these labels.
fn segmented_w(ui: &Ui, labels: &[&str]) -> f32 {
    labels
        .iter()
        .map(|l| small_text_w(ui, l) + 2.0 * space::MD)
        .sum::<f32>()
        + 4.0
}

/// Width of the labels in front of a card's rows: the longest field label
/// in the current language (measured, so no language needs a constant).
fn label_w(ui: &Ui) -> f32 {
    [
        "field.find",
        "field.replace",
        "field.apply_to",
        "field.text",
        "field.at",
        "field.remove",
        "field.count",
        "field.case",
        "field.start",
        "field.place",
        "field.separator",
        "field.date",
        "field.format",
        "field.extension",
        "field.new",
        "field.spaces",
    ]
    .iter()
    .map(|k| small_text_w(ui, crate::i18n::CATALOG.tr_key(k)) + space::SM)
    .fold(56.0_f32, f32::max)
    .min(150.0)
}

/// Width of the rules column: enough for the widest control that cannot
/// wrap (segmented controls, single checkboxes) next to the labels, at most
/// `max` (the preview keeps the rest). Rows wrap when even that is short.
fn left_w(ui: &Ui, max: f32) -> f32 {
    let groups: [&[&str]; 9] = [
        &[
            tr!("target.name"),
            tr!("target.extension"),
            tr!("target.both"),
        ],
        &[tr!("at.start"), tr!("at.end"), tr!("at.position"), "000000"],
        &[tr!("remove.first"), tr!("remove.last"), tr!("remove.range")],
        &[
            CaseMode::Lower.label(),
            CaseMode::Upper.label(),
            CaseMode::Title.label(),
            CaseMode::Sentence.label(),
        ],
        &[
            tr!("place.prefix"),
            tr!("place.suffix"),
            tr!("place.replace"),
        ],
        &[
            ExtMode::Keep.label(),
            ExtMode::Change.label(),
            ExtMode::Lower.label(),
            ExtMode::Upper.label(),
            ExtMode::Remove.label(),
        ],
        &[tr!("spaces.keep"), "→ -", "→ _"],
        &[tr!("occurrence.first"), tr!("occurrence.all")],
        &[tr!("date.modified"), tr!("date.today")],
    ];
    let widest_seg = groups
        .iter()
        .map(|g| segmented_w(ui, g))
        .fold(0.0_f32, f32::max);
    let widest_check = [
        tr!("opt.regex"),
        tr!("opt.match_case"),
        tr!("clean.trim"),
        tr!("clean.collapse"),
        tr!("clean.umlauts"),
    ]
    .iter()
    .map(|l| 14.0 + space::SM + small_text_w(ui, l))
    .fold(0.0_f32, f32::max);
    // Card frame (margins 6 + 6, stroke), the row's lead-in and the two
    // item gaps around the label, the list's scroll reserve and some air.
    let chrome = 12.0 + 2.0 + space::XS + 2.0 * space::SM + 4.0 + 16.0;
    (label_w(ui) + widest_seg.max(widest_check) + chrome).clamp(372.0, max.max(372.0))
}

/// What is being renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The selected items.
    Selection(usize),
    /// Nothing was selected: every file shown in the folder.
    AllFiles(usize),
}

/// The folder listing behind the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status<'a> {
    Loading,
    Ready,
    Error(&'a str),
}

/// Everything the sheet shows.
pub struct View<'a> {
    pub rules: &'a mut Vec<Rule>,
    pub preview: &'a Preview,
    pub dir_name: &'a str,
    /// Backend label for remote folders ("deploy@web-01 (SFTP)").
    pub remote: Option<&'a str>,
    pub scope: Scope,
    pub status: Status<'a>,
    pub presets: &'a Presets,
    /// A rename is running.
    pub busy: bool,
    /// Why the last attempt was refused.
    pub banner: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetAction {
    Apply,
    Cancel,
    SavePreset(String),
    LoadPreset(String),
    DeletePreset(String),
}

/// UI state that lives across frames.
#[derive(Default)]
pub struct SheetUi {
    pub preset_name: String,
    /// The rule being dragged.
    pub(crate) drag: Option<usize>,
    /// Focus the first field of the first rule next frame.
    pub focus_first: bool,
    /// Open the presets menu next frame (tests and renders).
    pub open_presets: bool,
}

fn presets_popup_id() -> Id {
    Id::unique("manta.rename.presets")
}

fn add_popup_id() -> Id {
    Id::unique("manta.rename.add-rule")
}

/// Draw the sheet into `ui.max_rect()`.
pub fn sheet_ui(ui: &mut Ui, st: &mut SheetUi, v: View<'_>) -> Vec<SheetAction> {
    let p = pal(ui);
    let mut out = Vec::new();
    let full = ui.max_rect();
    let footer_h = 54.0;
    let footer = Rect::from_min_max(pos2(full.left(), full.bottom() - footer_h), full.max);
    let inner = Rect::from_min_max(
        full.min + vec2(space::XL, space::LG),
        pos2(full.right() - space::XL, footer.top() - space::LG),
    );
    let header = Rect::from_min_size(inner.min, vec2(inner.width(), 28.0));
    let top = header.bottom() + space::LG;
    let left = Rect::from_min_max(
        pos2(inner.left(), top),
        pos2(
            inner.left() + left_w(ui, (inner.width() * 0.58).round()),
            inner.bottom(),
        ),
    );
    let right = Rect::from_min_max(pos2(left.right() + space::XL, top), inner.max);

    let child = |ui: &mut Ui, r: Rect, salt: &str, f: &mut dyn FnMut(&mut Ui)| {
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(r)
                .id_salt(salt)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(r.intersect(ui.clip_rect()).expand(3.0));
                f(ui)
            },
        );
    };

    child(ui, header, "header", &mut |ui| {
        header_ui(ui, st, &v, &mut out)
    });
    child(ui, left, "rules", &mut |ui| {
        rules_ui(ui, st, v.rules, v.preview);
    });
    child(ui, right, "preview", &mut |ui| {
        preview_ui(ui, v.preview, v.status)
    });

    // Footer.
    ui.painter().hline(
        footer.x_range(),
        footer.top(),
        Stroke::new(1.0, p.separator),
    );
    let frow = Rect::from_min_max(
        pos2(footer.left() + space::XL, footer.top() + 1.0),
        pos2(footer.right() - space::XL, footer.bottom()),
    );
    child(ui, frow, "footer", &mut |ui| footer_ui(ui, &v, &mut out));

    // Enter applies (from a text field or with nothing focused), as the
    // default button of a sheet does.
    let any_popup = egui::Popup::is_id_open(ui.ctx(), presets_popup_id())
        || egui::Popup::is_id_open(ui.ctx(), add_popup_id());
    let focus = ui.ctx().memory(|m| m.focused());
    let enter_ok = focus.is_none() || ui.ctx().text_edit_focused();
    if !any_popup
        && enter_ok
        && ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.is_none())
        && v.preview.can_apply()
        && !v.busy
        && v.status == Status::Ready
    {
        out.push(SheetAction::Apply);
    }
    out
}

// ------------------------------------------------------------------ header

fn header_ui(ui: &mut Ui, st: &mut SheetUi, v: &View<'_>, out: &mut Vec<SheetAction>) {
    let p = pal(ui);
    let rect = ui.max_rect();
    // Scope pill.
    let (glyph, fg, bg, main, rest) = match v.scope {
        Scope::Selection(n) => (
            icon::FILE,
            p.accent_text,
            p.accent_soft,
            tr!("scope.selected", count = n),
            tr!("scope.in", name = v.dir_name),
        ),
        Scope::AllFiles(n) => (
            icon::INFO,
            p.warning,
            p.warning_soft,
            tr!("scope.nothing_selected").to_string(),
            tr!("scope.all_files", count = n, name = v.dir_name),
        ),
    };
    let main_font = style::medium(ui, text::SMALL + 0.5);
    let rest_font = style::regular(text::SMALL + 0.5);
    let gm = ui.painter().layout_no_wrap(main, main_font, fg);
    let mut job = LayoutJob::simple_singleline(rest, rest_font, p.text2);
    job.wrap.max_width = (rect.width() - 260.0 - gm.size().x).max(60.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let gr = ui.painter().layout_job(job);
    let w = 24.0 + gm.size().x + space::SM + gr.size().x + space::LG;
    let pill = Rect::from_min_size(pos2(rect.left(), rect.center().y - 12.0), vec2(w, 24.0));
    ui.painter().rect_filled(pill, CornerRadius::same(12), bg);
    style::paint_icon(
        ui,
        Rect::from_min_size(pill.min + vec2(6.0, 0.0), vec2(16.0, 24.0)),
        glyph,
        13.0,
        fg,
    );
    let x = pill.left() + 24.0;
    ui.painter()
        .galley(pos2(x, pill.center().y - gm.size().y / 2.0), gm.clone(), fg);
    let x2 = x + gm.size().x + space::SM;
    ui.painter()
        .galley(pos2(x2, pill.center().y - gr.size().y / 2.0), gr, p.text2);
    if let Some(remote) = v.remote {
        ui.painter().text(
            pos2(pill.right() + space::MD, pill.center().y),
            Align2::LEFT_CENTER,
            tr!("scope.remote", remote = remote),
            style::regular(text::SMALL),
            p.text3,
        );
    }

    // Presets menu button (right).
    let label = tr!("presets.button");
    let font = style::regular(text::SMALL + 0.5);
    let g = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let bw = 18.0 + g.size().x + 22.0 + space::MD;
    let br = Rect::from_min_size(
        pos2(rect.right() - bw, rect.center().y - 12.0),
        vec2(bw, 24.0),
    );
    let resp = ui.interact(br, ui.scope_id().with("presets-btn"), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), presets_popup_id());
    let fill = if open || resp.hovered() {
        p.control_hover
    } else {
        p.control
    };
    let r = CornerRadius::same(style::CONTROL_R + 1);
    ui.painter()
        .rect(br, r, fill, Stroke::new(1.0, p.border), StrokeKind::Inside);
    style::paint_icon(
        ui,
        Rect::from_min_size(br.min + vec2(5.0, 0.0), vec2(18.0, 24.0)),
        icon::STAR,
        12.0,
        p.text2,
    );
    ui.painter().galley(
        pos2(br.left() + 23.0, br.center().y - g.size().y / 2.0),
        g,
        p.text,
    );
    style::paint_icon(
        ui,
        Rect::from_min_size(pos2(br.right() - 20.0, br.top()), vec2(16.0, 24.0)),
        icon::CARET_DOWN,
        10.0,
        p.text2,
    );
    if resp.has_focus() {
        style::focus_ring(ui, br, r);
    }
    if std::mem::take(&mut st.open_presets) {
        egui::Popup::open_id(ui.ctx(), presets_popup_id());
    }
    let resp = resp.on_hover_text(tr!("presets.button.tip"));
    egui::Popup::from_toggle_button_response(&resp)
        .id(presets_popup_id())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(4.0)
        .width(268.0)
        .frame(popup_frame(p))
        .show(|ui| presets_menu(ui, st, v, out));
}

fn popup_frame(p: &Pal) -> egui::Frame {
    egui::Frame::new()
        .fill(p.elevated)
        .stroke(Stroke::new(1.0, p.separator))
        .corner_radius(CornerRadius::same(style::PANEL_R + 2))
        .shadow(egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }),
        })
        .inner_margin(Margin::same(space::SM as i8))
}

fn menu_section(ui: &mut Ui, title: &str) {
    let p = pal(ui);
    ui.add_space(space::XXS);
    ui.horizontal(|ui| {
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(title)
                .font(style::medium(ui, text::CAPTION))
                .color(p.text3),
        );
    });
}

fn presets_menu(ui: &mut Ui, st: &mut SheetUi, v: &View<'_>, out: &mut Vec<SheetAction>) {
    let p = pal(ui);
    ui.spacing_mut().item_spacing.y = space::XXS;
    menu_section(ui, tr!("presets.saved"));
    if v.presets.items.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(tr!("presets.none"))
                    .font(style::regular(text::SMALL))
                    .color(p.text2),
            );
        });
    }
    for preset in &v.presets.items {
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        let del =
            Rect::from_center_size(pos2(row.right() - 14.0, row.center().y), vec2(20.0, 20.0));
        let del_resp = ui.interact(
            del,
            ui.scope_id().with(("del", &preset.name)),
            Sense::click(),
        );
        if resp.hovered() || del_resp.hovered() {
            ui.painter()
                .rect_filled(row, CornerRadius::same(style::CONTROL_R), p.hover);
        }
        style::paint_icon(
            ui,
            Rect::from_min_size(row.min + vec2(4.0, 0.0), vec2(18.0, 26.0)),
            icon::STAR,
            12.0,
            p.accent_text,
        );
        let n = preset.rules.len();
        let mut job =
            LayoutJob::simple_singleline(preset.name.clone(), style::regular(text::BODY), p.text);
        job.wrap.max_width = row.width() - 110.0;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let g = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(row.left() + 26.0, row.center().y - g.size().y / 2.0),
            g,
            p.text,
        );
        ui.painter().text(
            pos2(del.left() - space::SM, row.center().y),
            Align2::RIGHT_CENTER,
            tr!("presets.rules", count = n),
            style::regular(text::CAPTION),
            p.text3,
        );
        let dcol = if del_resp.hovered() {
            p.danger
        } else {
            p.text3
        };
        if del_resp.hovered() {
            ui.painter()
                .rect_filled(del, CornerRadius::same(style::CONTROL_R), p.danger_soft);
        }
        style::paint_icon(ui, del, icon::TRASH, 12.0, dcol);
        if del_resp.on_hover_text(tr!("presets.delete")).clicked() {
            out.push(SheetAction::DeletePreset(preset.name.clone()));
        } else if resp.on_hover_text(tr!("presets.load")).clicked() {
            out.push(SheetAction::LoadPreset(preset.name.clone()));
            egui::Popup::close_id(ui.ctx(), presets_popup_id());
        }
    }
    ui.add_space(space::XS);
    let (sep, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(sep.x_range(), sep.center().y, Stroke::new(1.0, p.separator));
    ui.add_space(space::XS);
    menu_section(ui, tr!("presets.save_current"));
    ui.add_space(space::XXS);
    ui.horizontal(|ui| {
        ui.add_space(space::XS);
        let w = ui.available_width() - 64.0;
        let resp = style::text_field(ui, &mut st.preset_name, tr!("presets.name_hint"), w, false);
        let name = st.preset_name.trim().to_string();
        let can = !name.is_empty() && !v.rules.is_empty();
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let save = style::button_small(ui, tr!("presets.save"), can);
        if can && (save.clicked() || enter) {
            out.push(SheetAction::SavePreset(name));
            st.preset_name.clear();
        }
    });
    if v.presets
        .items
        .iter()
        .any(|x| x.name.eq_ignore_ascii_case(st.preset_name.trim()))
    {
        ui.horizontal(|ui| {
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(tr!("presets.replaces"))
                    .font(style::regular(text::CAPTION))
                    .color(p.text2),
            );
        });
    }
    ui.add_space(space::XXS);
}

// ------------------------------------------------------------------ rules

/// The rule pipeline. Returns true if a rule changed.
fn rules_ui(ui: &mut Ui, st: &mut SheetUi, rules: &mut Vec<Rule>, preview: &Preview) -> bool {
    let p = pal(ui);
    let rect = ui.max_rect();
    let mut changed = false;
    // Title row with the Add Rule menu.
    let title_h = 26.0;
    let n = rules.len();
    ui.painter().text(
        pos2(rect.left() + 2.0, rect.top() + title_h / 2.0),
        Align2::LEFT_CENTER,
        tr!("rules.title"),
        style::semibold(ui, text::BODY),
        p.text,
    );
    let rg = ui.painter().layout_no_wrap(
        tr!("rules.title").into(),
        style::semibold(ui, text::BODY),
        p.text,
    );
    ui.painter().text(
        pos2(
            rect.left() + 2.0 + rg.size().x + space::SM,
            rect.top() + title_h / 2.0,
        ),
        Align2::LEFT_CENTER,
        if n == 0 {
            String::new()
        } else {
            tr!("rules.order", n = n)
        },
        style::regular(text::SMALL),
        p.text3,
    );
    let add_font = style::medium(ui, text::SMALL + 0.5);
    let add_label =
        ui.painter()
            .layout_no_wrap(tr!("rules.add").into(), add_font.clone(), p.accent_text);
    let add_w = (24.0 + add_label.size().x + space::MD).max(96.0);
    let add = Rect::from_min_size(
        pos2(rect.right() - add_w, rect.top() + 1.0),
        vec2(add_w, 24.0),
    );
    let resp = ui.interact(add, ui.scope_id().with("add-rule"), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), add_popup_id());
    let r = CornerRadius::same(style::CONTROL_R + 1);
    ui.painter().rect_filled(
        add,
        r,
        if open || resp.hovered() {
            p.accent_soft
        } else {
            Color32::TRANSPARENT
        },
    );
    style::paint_icon(
        ui,
        Rect::from_min_size(add.min + vec2(4.0, 0.0), vec2(18.0, 24.0)),
        icon::PLUS,
        13.0,
        p.accent_text,
    );
    ui.painter().text(
        pos2(add.left() + 24.0, add.center().y),
        Align2::LEFT_CENTER,
        tr!("rules.add"),
        add_font,
        p.accent_text,
    );
    if resp.has_focus() {
        style::focus_ring(ui, add, r);
    }
    let resp = resp.on_hover_text(tr!("rules.add.tip"));
    egui::Popup::from_toggle_button_response(&resp)
        .id(add_popup_id())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::BOTTOM_END)
        .gap(4.0)
        .width(250.0)
        .frame(popup_frame(p))
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for t in RuleType::ALL {
                if rule_type_row(ui, t).clicked() {
                    rules.push(t.new_rule());
                    st.focus_first = false;
                    changed = true;
                }
            }
        });

    let list = Rect::from_min_max(
        pos2(rect.left(), rect.top() + title_h + space::SM),
        rect.max,
    );
    let mut rects: Vec<Rect> = Vec::new();
    let mut action: Option<CardAction> = None;
    ui.scope_builder(UiBuilder::new().max_rect(list).id_salt("rule-list"), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("rules-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - 4.0);
                ui.spacing_mut().item_spacing.y = space::SM;
                if rules.is_empty() {
                    empty_rules(ui);
                }
                let count = rules.len();
                for (i, rule) in rules.iter_mut().enumerate() {
                    let err = preview
                        .rule_errors
                        .iter()
                        .find(|(j, _)| *j == i)
                        .map(|(_, e)| e.as_str());
                    let focus = st.focus_first && i == 0;
                    let (r, a, ch) = ui
                        .push_id(("rule", i), |ui| {
                            card(ui, i, count, rule, err, focus, st.drag)
                        })
                        .inner;
                    if focus {
                        st.focus_first = false;
                    }
                    changed |= ch;
                    rects.push(r);
                    if let Some(a) = a {
                        action = Some(a);
                    }
                }
                ui.add_space(space::XS);
            });
    });

    // Drag to reorder: an insertion line follows the pointer.
    if let Some(a) = action {
        changed = true;
        match a {
            CardAction::Up(i) if i > 0 => rules.swap(i, i - 1),
            CardAction::Down(i) if i + 1 < rules.len() => rules.swap(i, i + 1),
            CardAction::Remove(i) => {
                rules.remove(i);
            }
            CardAction::DragStart(i) => {
                st.drag = Some(i);
                changed = false;
            }
            _ => changed = false,
        }
    }
    if let Some(d) = st.drag {
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let down = ui.input(|i| i.pointer.primary_down());
        let target = pointer.map(|pos| rects.iter().filter(|r| r.center().y < pos.y).count());
        if let Some(t) = target {
            let y = if t == 0 {
                rects
                    .first()
                    .map_or(list.top(), |r| r.top() - space::SM / 2.0)
            } else {
                rects
                    .get(t - 1)
                    .map_or(list.top(), |r| r.bottom() + space::SM / 2.0)
            };
            if t != d && t != d + 1 {
                let x0 = list.left() + 2.0;
                let x1 = list.right() - 6.0;
                ui.painter().with_clip_rect(list.expand(2.0)).hline(
                    x0..=x1,
                    y,
                    Stroke::new(2.0, p.accent),
                );
                ui.painter().with_clip_rect(list.expand(2.0)).circle_filled(
                    pos2(x0, y),
                    3.5,
                    p.accent,
                );
            }
            if let Some(r) = rects.get(d) {
                ui.painter().with_clip_rect(list).rect_filled(
                    *r,
                    CornerRadius::same(style::PANEL_R),
                    p.accent_soft.gamma_multiply(0.55),
                );
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if !down {
            st.drag = None;
            if let Some(t) = target {
                let to = if t > d { t - 1 } else { t };
                if to != d && d < rules.len() {
                    let r = rules.remove(d);
                    rules.insert(to.min(rules.len()), r);
                    changed = true;
                }
            }
        }
    }
    changed
}

fn empty_rules(ui: &mut Ui) {
    let p = pal(ui);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
    ui.painter().rect_stroke(
        r.shrink(1.0),
        CornerRadius::same(style::PANEL_R),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    ui.painter().text(
        r.center() - vec2(0.0, 9.0),
        Align2::CENTER_CENTER,
        tr!("rules.empty"),
        style::medium(ui, text::BODY),
        p.text2,
    );
    ui.painter().text(
        r.center() + vec2(0.0, 10.0),
        Align2::CENTER_CENTER,
        tr!("rules.empty.note"),
        style::regular(text::SMALL),
        p.text3,
    );
}

fn rule_glyph(t: RuleType) -> (&'static str, bool) {
    // (glyph, is an icon-font glyph)
    match t {
        RuleType::Replace => (icon::MAGNIFYING_GLASS, true),
        RuleType::Insert => (icon::PENCIL, true),
        RuleType::Remove => (icon::SCISSORS, true),
        RuleType::Case => ("Aa", false),
        RuleType::Number => ("#", false),
        RuleType::Date => (icon::CLOCK, true),
        RuleType::Extension => (icon::FILE, true),
        RuleType::Clean => (icon::BROOM, true),
    }
}

fn paint_rule_glyph(ui: &Ui, r: Rect, t: RuleType, color: Color32) {
    let (g, is_icon) = rule_glyph(t);
    if is_icon {
        style::paint_icon(ui, r, g, 13.0, color);
    } else {
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            g,
            style::semibold(ui, text::SMALL),
            color,
        );
    }
}

fn rule_type_row(ui: &mut Ui, t: RuleType) -> egui::Response {
    let p = pal(ui);
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(row, CornerRadius::same(style::CONTROL_R), p.accent_soft);
    }
    let ic = Rect::from_min_size(row.min + vec2(6.0, 8.0), vec2(22.0, 22.0));
    ui.painter()
        .rect_filled(ic, CornerRadius::same(5), p.neutral_soft);
    paint_rule_glyph(ui, ic, t, p.text2);
    ui.painter().text(
        pos2(ic.right() + space::MD, row.top() + 12.0),
        Align2::LEFT_CENTER,
        t.title(),
        style::medium(ui, text::BODY),
        p.text,
    );
    ui.painter().text(
        pos2(ic.right() + space::MD, row.top() + 27.0),
        Align2::LEFT_CENTER,
        t.hint(),
        style::regular(text::CAPTION),
        p.text2,
    );
    resp
}

enum CardAction {
    Up(usize),
    Down(usize),
    Remove(usize),
    DragStart(usize),
}

/// One rule card. Returns its rect, a structural action and whether the
/// rule changed.
fn card(
    ui: &mut Ui,
    i: usize,
    count: usize,
    rule: &mut Rule,
    err: Option<&str>,
    focus: bool,
    dragging: Option<usize>,
) -> (Rect, Option<CardAction>, bool) {
    let p = pal(ui);
    let mut action = None;
    let mut changed = false;
    let t = rule.kind.rule_type();
    let stroke = if err.is_some() {
        p.danger.gamma_multiply(0.7)
    } else {
        p.separator
    };
    let resp = egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(style::PANEL_R))
        .inner_margin(Margin {
            left: 6,
            right: 6,
            top: 5,
            bottom: if rule.enabled { 10 } else { 6 },
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(space::SM, space::SM);
            // Header.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let (h, hresp) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::drag());
                let hc = if hresp.hovered() || dragging == Some(i) {
                    p.text2
                } else {
                    p.text3
                };
                for row in 0..3 {
                    for col in 0..2 {
                        let c = pos2(
                            h.left() + 3.5 + col as f32 * 5.0,
                            h.center().y - 5.0 + row as f32 * 5.0,
                        );
                        ui.painter().circle_filled(c, 1.3, hc);
                    }
                }
                if hresp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if hresp.drag_started() {
                    action = Some(CardAction::DragStart(i));
                }
                let (ic, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                let on = rule.enabled;
                ui.painter().rect_filled(
                    ic,
                    CornerRadius::same(5),
                    if on { p.accent_soft } else { p.neutral_soft },
                );
                paint_rule_glyph(ui, ic, t, if on { p.accent_text } else { p.text3 });
                ui.add_space(space::XXS);
                let title_color = if on { p.text } else { p.text2 };
                ui.label(
                    egui::RichText::new(format!("{}. {}", i + 1, t.title()))
                        .font(style::semibold(ui, text::BODY - 0.5))
                        .color(title_color),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    if style::icon_button(ui, icon::X, 20.0, true, tr!("card.remove")).clicked() {
                        action = Some(CardAction::Remove(i));
                    }
                    if style::icon_button(
                        ui,
                        icon::CARET_DOWN,
                        20.0,
                        i + 1 < count,
                        tr!("card.move_down"),
                    )
                    .clicked()
                    {
                        action = Some(CardAction::Down(i));
                    }
                    if style::icon_button(ui, icon::CARET_UP, 20.0, i > 0, tr!("card.move_up"))
                        .clicked()
                    {
                        action = Some(CardAction::Up(i));
                    }
                    ui.add_space(space::XS);
                    let tip = if rule.enabled {
                        tr!("card.on")
                    } else {
                        tr!("card.off")
                    };
                    changed |= style::switch(ui, &mut rule.enabled, tip).changed();
                });
            });
            if !rule.enabled {
                ui.horizontal(|ui| {
                    ui.add_space(38.0);
                    ui.label(
                        egui::RichText::new(tr!(
                            "card.off_summary",
                            summary = crate::rules::summary(rule)
                        ))
                        .font(style::regular(text::SMALL))
                        .color(p.text3),
                    );
                });
                return;
            }
            ui.add_space(space::XXS);
            ui.indent(("body", i), |ui| {
                ui.spacing_mut().indent = 0.0;
                changed |= body(ui, &mut rule.kind, err, focus);
            });
        });
    (resp.response.rect, action, changed)
}

/// A labelled row inside a card.
fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal(ui);
    let lw = label_w(ui);
    ui.horizontal_top(|ui| {
        ui.add_space(space::XS);
        let (r, _) = ui.allocate_exact_size(vec2(lw, 22.0), Sense::hover());
        if !label.is_empty() {
            ui.painter().text(
                r.left_center(),
                Align2::LEFT_CENTER,
                label,
                style::regular(text::SMALL),
                p.text2,
            );
        }
        // The controls wrap below each other (still right of the label)
        // when the column is too narrow for them in one line.
        ui.horizontal_wrapped(|ui| add(ui)).inner
    })
    .inner
}

fn target_row(ui: &mut Ui, target: &mut Target) -> bool {
    row(ui, tr!("field.apply_to"), |ui| {
        style::segmented(
            ui,
            target,
            &[
                (Target::Name, tr!("target.name")),
                (Target::Extension, tr!("target.extension")),
                (Target::Both, tr!("target.both")),
            ],
        )
    })
}

fn num<T: egui::emath::Numeric>(
    ui: &mut Ui,
    v: &mut T,
    range: std::ops::RangeInclusive<T>,
    w: f32,
) -> bool {
    ui.add_sized(
        vec2(w, 22.0),
        egui::DragValue::new(v).range(range).speed(0.15),
    )
    .changed()
}

/// The fields of one rule. Returns true if something changed.
fn body(ui: &mut Ui, kind: &mut RuleKind, err: Option<&str>, focus: bool) -> bool {
    let p = pal(ui);
    let mut ch = false;
    let field_w = |ui: &Ui| (ui.available_width() - space::XS).max(60.0);
    match kind {
        RuleKind::Replace {
            find,
            replace,
            regex,
            case_sensitive,
            all,
            target,
        } => {
            row(ui, tr!("field.find"), |ui| {
                let w = field_w(ui);
                let r = style::text_field(
                    ui,
                    find,
                    if *regex {
                        tr!("hint.regex")
                    } else {
                        tr!("hint.text")
                    },
                    w,
                    *regex,
                );
                if focus {
                    r.request_focus();
                }
                ch |= r.changed();
            });
            if let Some(e) = err {
                row(ui, "", |ui| {
                    ui.label(
                        egui::RichText::new(format!("{}  {e}", icon::WARNING_CIRCLE))
                            .font(style::regular(text::SMALL))
                            .color(p.danger),
                    );
                });
            }
            row(ui, tr!("field.replace"), |ui| {
                let w = field_w(ui);
                let hint = if *regex {
                    tr!("hint.replacement_regex")
                } else {
                    tr!("hint.replacement")
                };
                ch |= style::text_field(ui, replace, hint, w, *regex).changed();
            });
            row(ui, "", |ui| {
                ch |= style::checkbox(ui, regex, tr!("opt.regex")).changed();
                ui.add_space(space::XS);
                ch |= style::checkbox(ui, case_sensitive, tr!("opt.match_case")).changed();
                ui.add_space(space::XS);
                ch |= style::segmented(
                    ui,
                    all,
                    &[
                        (false, tr!("occurrence.first")),
                        (true, tr!("occurrence.all")),
                    ],
                );
            });
            ch |= target_row(ui, target);
        }
        RuleKind::Insert {
            text: t,
            at,
            position,
            target,
        } => {
            row(ui, tr!("field.text"), |ui| {
                let w = field_w(ui);
                let r = style::text_field(ui, t, tr!("hint.text_to_insert"), w, false);
                if focus {
                    r.request_focus();
                }
                ch |= r.changed();
            });
            row(ui, tr!("field.at"), |ui| {
                ch |= style::segmented(
                    ui,
                    at,
                    &[
                        (InsertAt::Start, tr!("at.start")),
                        (InsertAt::End, tr!("at.end")),
                        (InsertAt::Position, tr!("at.position")),
                    ],
                );
                if *at == InsertAt::Position {
                    ch |= num(ui, position, 0..=255, 46.0);
                }
            });
            ch |= target_row(ui, target);
        }
        RuleKind::Remove {
            mode,
            count,
            from,
            target,
        } => {
            row(ui, tr!("field.remove"), |ui| {
                ch |= style::segmented(
                    ui,
                    mode,
                    &[
                        (RemoveMode::First, tr!("remove.first")),
                        (RemoveMode::Last, tr!("remove.last")),
                        (RemoveMode::Range, tr!("remove.range")),
                    ],
                );
            });
            row(ui, tr!("field.count"), |ui| {
                ch |= num(ui, count, 0..=255, 46.0);
                style::caption(ui, tr!("caption.characters"));
                if *mode == RemoveMode::Range {
                    ui.add_space(space::XS);
                    style::caption(ui, tr!("caption.from"));
                    // Shown 1-based.
                    let mut f1 = *from + 1;
                    if num(ui, &mut f1, 1..=255, 46.0) {
                        *from = f1 - 1;
                        ch = true;
                    }
                }
            });
            ch |= target_row(ui, target);
        }
        RuleKind::Case { mode, target } => {
            row(ui, tr!("field.case"), |ui| {
                ch |= style::segmented(
                    ui,
                    mode,
                    &[
                        (CaseMode::Lower, CaseMode::Lower.label()),
                        (CaseMode::Upper, CaseMode::Upper.label()),
                        (CaseMode::Title, CaseMode::Title.label()),
                        (CaseMode::Sentence, CaseMode::Sentence.label()),
                    ],
                );
            });
            ch |= target_row(ui, target);
        }
        RuleKind::Number {
            start,
            step,
            pad,
            place,
            sep,
        } => {
            row(ui, tr!("field.start"), |ui| {
                ch |= num(ui, start, -99_999..=999_999, 58.0);
                ui.add_space(space::XS);
                style::caption(ui, tr!("caption.step"));
                ch |= num(ui, step, -999..=999, 46.0);
                ui.add_space(space::XS);
                style::caption(ui, tr!("caption.digits"));
                ch |= num(ui, pad, 0..=8, 40.0);
            });
            row(ui, tr!("field.place"), |ui| {
                ch |= style::segmented(
                    ui,
                    place,
                    &[
                        (Place::Prefix, tr!("place.prefix")),
                        (Place::Suffix, tr!("place.suffix")),
                        (Place::Replace, tr!("place.replace")),
                    ],
                );
            });
            if *place != Place::Replace {
                row(ui, tr!("field.separator"), |ui| {
                    ch |= style::text_field(ui, sep, tr!("hint.none"), 64.0, true).changed()
                });
            }
        }
        RuleKind::Date {
            source,
            format,
            place,
            sep,
        } => {
            row(ui, tr!("field.date"), |ui| {
                ch |= style::segmented(
                    ui,
                    source,
                    &[
                        (DateSource::Modified, tr!("date.modified")),
                        (DateSource::Today, tr!("date.today")),
                    ],
                );
            });
            row(ui, tr!("field.format"), |ui| {
                ch |= style::text_field(ui, format, "YYYY-MM-DD", 132.0, true).changed();
                egui::ComboBox::from_id_salt("date-presets")
                    .selected_text(tr!("date.presets"))
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        for f in DATE_PRESETS {
                            if ui.selectable_label(format == f, *f).clicked() {
                                *format = f.to_string();
                                ch = true;
                            }
                        }
                    });
            });
            row(ui, tr!("field.place"), |ui| {
                let mut pl = if *place == Place::Prefix {
                    Place::Prefix
                } else {
                    Place::Suffix
                };
                if style::segmented(
                    ui,
                    &mut pl,
                    &[
                        (Place::Prefix, tr!("place.prefix")),
                        (Place::Suffix, tr!("place.suffix")),
                    ],
                ) {
                    *place = pl;
                    ch = true;
                }
                ui.add_space(space::XS);
                style::caption(ui, tr!("field.separator"));
                ch |= style::text_field(ui, sep, tr!("hint.none"), 48.0, true).changed();
            });
        }
        RuleKind::Extension { mode, new_ext } => {
            row(ui, tr!("field.extension"), |ui| {
                ch |= style::segmented(
                    ui,
                    mode,
                    &[
                        (ExtMode::Keep, ExtMode::Keep.label()),
                        (ExtMode::Change, ExtMode::Change.label()),
                        (ExtMode::Lower, ExtMode::Lower.label()),
                        (ExtMode::Upper, ExtMode::Upper.label()),
                        (ExtMode::Remove, ExtMode::Remove.label()),
                    ],
                );
            });
            if *mode == ExtMode::Change {
                row(ui, tr!("field.new"), |ui| {
                    let r = style::text_field(ui, new_ext, tr!("hint.new_ext"), 120.0, false);
                    if focus {
                        r.request_focus();
                    }
                    ch |= r.changed();
                });
            }
        }
        RuleKind::Clean {
            trim,
            collapse,
            spaces,
            umlauts,
            target,
        } => {
            row(ui, "", |ui| {
                ch |= style::checkbox(ui, trim, tr!("clean.trim")).changed();
                ui.add_space(space::XS);
                ch |= style::checkbox(ui, collapse, tr!("clean.collapse")).changed();
            });
            row(ui, tr!("field.spaces"), |ui| {
                ch |= style::segmented(
                    ui,
                    spaces,
                    &[
                        (SpaceMode::Keep, tr!("spaces.keep")),
                        (SpaceMode::Dash, "→ -"),
                        (SpaceMode::Underscore, "→ _"),
                    ],
                );
            });
            row(ui, "", |ui| {
                ch |= style::checkbox(ui, umlauts, tr!("clean.umlauts")).changed()
            });
            ch |= target_row(ui, target);
        }
    }
    ch
}

// ------------------------------------------------------------------ preview

fn preview_ui(ui: &mut Ui, preview: &Preview, status: Status<'_>) {
    let p = pal(ui);
    let rect = ui.max_rect();
    let title_h = 26.0;
    ui.painter().text(
        pos2(rect.left() + 2.0, rect.top() + title_h / 2.0),
        Align2::LEFT_CENTER,
        tr!("preview.title"),
        style::semibold(ui, text::BODY),
        p.text,
    );
    let n = preview.rows.len();
    if n > 0 {
        let pg = ui.painter().layout_no_wrap(
            tr!("preview.title").into(),
            style::semibold(ui, text::BODY),
            p.text,
        );
        ui.painter().text(
            pos2(
                rect.left() + 2.0 + pg.size().x + space::SM,
                rect.top() + title_h / 2.0,
            ),
            Align2::LEFT_CENTER,
            tr!("preview.count", count = n),
            style::regular(text::SMALL),
            p.text3,
        );
    }
    let table = Rect::from_min_max(
        pos2(rect.left(), rect.top() + title_h + space::SM),
        rect.max,
    );
    let r = CornerRadius::same(style::PANEL_R);
    ui.painter().rect_filled(table, r, p.content);
    let head = Rect::from_min_size(table.min, vec2(table.width(), 26.0));
    ui.painter().rect_filled(
        head,
        CornerRadius {
            nw: style::PANEL_R,
            ne: style::PANEL_R,
            sw: 0,
            se: 0,
        },
        p.stripe,
    );
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.separator),
    );
    let mid = table.left() + (table.width() * 0.47).round();
    let hf = style::medium(ui, text::SMALL);
    ui.painter().text(
        pos2(table.left() + 30.0, head.center().y),
        Align2::LEFT_CENTER,
        tr!("preview.original"),
        hf.clone(),
        p.text2,
    );
    ui.painter().text(
        pos2(mid + 20.0, head.center().y),
        Align2::LEFT_CENTER,
        tr!("preview.new"),
        hf,
        p.text2,
    );

    let body = Rect::from_min_max(
        pos2(table.left() + 1.0, head.bottom()),
        table.max - vec2(1.0, 1.0),
    );
    match status {
        Status::Loading => centered(ui, body, tr!("preview.reading"), p.text2),
        Status::Error(e) => centered(ui, body, &tr!("preview.error", error = e), p.danger),
        Status::Ready if n == 0 => centered(ui, body, tr!("preview.no_files"), p.text2),
        Status::Ready => {
            ui.scope_builder(UiBuilder::new().max_rect(body).id_salt("rows"), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::ScrollArea::vertical()
                    .id_salt("preview-scroll")
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_H, n, |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for i in range {
                            let (rr, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), ROW_H),
                                Sense::hover(),
                            );
                            preview_row(ui, rr, i, &preview.rows[i], mid);
                        }
                    });
            });
        }
    }
    ui.painter()
        .rect_stroke(table, r, Stroke::new(1.0, p.separator), StrokeKind::Inside);
}

fn centered(ui: &Ui, r: Rect, s: &str, color: Color32) {
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        s,
        style::regular(text::BODY),
        color,
    );
}

/// Spans as one line with highlighted changes, cut with "…".
fn spans_job(
    row_spans: &[crate::preview::Span],
    base: Color32,
    hi_fg: Color32,
    hi_bg: Color32,
    max_w: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for s in row_spans {
        let mut f = TextFormat {
            font_id: style::regular(text::BODY),
            color: base,
            ..Default::default()
        };
        if s.changed {
            f.color = hi_fg;
            f.background = hi_bg;
            f.expand_bg = 1.5;
        }
        job.append(&s.text, 0.0, f);
    }
    job.wrap.max_width = max_w.max(8.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job
}

fn preview_row(ui: &Ui, rr: Rect, i: usize, row: &Row, mid: f32) {
    let p = pal(ui);
    if row.issue.is_some() {
        ui.painter().rect_filled(
            rr,
            CornerRadius::ZERO,
            p.danger_soft
                .gamma_multiply(if p.dark { 0.55 } else { 0.7 }),
        );
    } else if i % 2 == 1 {
        ui.painter().rect_filled(rr, CornerRadius::ZERO, p.stripe);
    }
    let dim = !row.changed;
    let ic = Rect::from_min_size(pos2(rr.left() + 8.0, rr.top()), vec2(16.0, rr.height()));
    let (g, c) = if row.is_dir {
        (icon::FOLDER, p.folder)
    } else {
        (icon::FILE, p.generic)
    };
    style::paint_icon(ui, ic, g, 13.0, if dim { c.gamma_multiply(0.5) } else { c });
    let base = if dim { p.text3 } else { p.text };
    let old_x = rr.left() + 30.0;
    let job = spans_job(
        &row.old_spans,
        if dim { p.text3 } else { p.text2 },
        p.danger,
        p.del_bg,
        mid - 12.0 - old_x,
    );
    let g = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(old_x, rr.center().y - g.size().y / 2.0), g, base);
    let arrow_color = if row.issue.is_some() {
        p.danger
    } else if dim {
        p.text3.gamma_multiply(0.6)
    } else {
        p.accent_text
    };
    style::paint_icon(
        ui,
        Rect::from_center_size(pos2(mid + 4.0, rr.center().y), vec2(16.0, 16.0)),
        icon::ARROW_RIGHT,
        11.0,
        arrow_color,
    );
    let new_x = mid + 20.0;
    let mut right = rr.right() - 10.0;
    if let Some(issue) = &row.issue {
        let label = issue.label();
        let lg = ui
            .painter()
            .layout_no_wrap(label, style::medium(ui, text::CAPTION), p.danger);
        let w = lg.size().x + 26.0;
        let pill = Rect::from_min_size(pos2(right - w, rr.center().y - 9.0), vec2(w, 18.0));
        ui.painter().rect_filled(
            pill,
            CornerRadius::same(9),
            if p.dark { p.danger_soft } else { p.content },
        );
        ui.painter().rect_stroke(
            pill,
            CornerRadius::same(9),
            Stroke::new(1.0, p.danger.gamma_multiply(0.45)),
            StrokeKind::Inside,
        );
        style::paint_icon(
            ui,
            Rect::from_min_size(pill.min + vec2(4.0, 0.0), vec2(14.0, 18.0)),
            icon::WARNING_CIRCLE,
            11.0,
            p.danger,
        );
        ui.painter().galley(
            pos2(pill.left() + 19.0, pill.center().y - lg.size().y / 2.0),
            lg,
            p.danger,
        );
        right = pill.left() - space::SM;
    }
    let shown: Vec<crate::preview::Span>;
    let spans = if row.new.is_empty() {
        shown = vec![crate::preview::Span {
            text: tr!("preview.empty_name").into(),
            changed: false,
        }];
        &shown
    } else {
        &row.new_spans
    };
    let new_base = if row.new.is_empty() { p.text3 } else { base };
    let job = spans_job(spans, new_base, p.ins_text, p.ins_bg, right - new_x);
    let g = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(new_x, rr.center().y - g.size().y / 2.0), g, base);
}

// ------------------------------------------------------------------ footer

fn footer_ui(ui: &mut Ui, v: &View<'_>, out: &mut Vec<SheetAction>) {
    let p = pal(ui);
    let rect = ui.max_rect();
    let pv = v.preview;
    // Summary (left).
    let cy = rect.center().y;
    let mut job = LayoutJob::default();
    let f = |c: Color32| TextFormat {
        font_id: style::regular(text::BODY),
        color: c,
        ..Default::default()
    };
    let fm = |ui: &Ui, c: Color32| TextFormat {
        font_id: style::medium(ui, text::BODY),
        color: c,
        ..Default::default()
    };
    let (glyph, gcol) = if let Some(b) = v.banner {
        job.append(b, 0.0, f(p.danger));
        (icon::WARNING_CIRCLE, p.danger)
    } else if v.status == Status::Loading {
        job.append(tr!("preview.reading"), 0.0, f(p.text2));
        (icon::INFO, p.text3)
    } else if let Some((i, e)) = pv.rule_errors.first() {
        job.append(&tr!("footer.rule_error", n = i + 1), 0.0, fm(ui, p.danger));
        job.append(e, 0.0, f(p.danger));
        (icon::WARNING_CIRCLE, p.danger)
    } else if pv.changed == 0 && pv.conflicts == 0 {
        job.append(tr!("footer.no_change"), 0.0, f(p.text2));
        (icon::INFO, p.text3)
    } else {
        let dirs = pv.rows.iter().any(|r| r.changed && r.is_dir);
        let n = pv.changed;
        let summary = if dirs {
            tr!("footer.items", count = n)
        } else {
            tr!("footer.files", count = n)
        };
        job.append(&summary, 0.0, fm(ui, p.text));
        if pv.conflicts > 0 {
            job.append("  ·  ", 0.0, f(p.text3));
            let c = pv.conflicts;
            job.append(&tr!("footer.conflicts", count = c), 0.0, fm(ui, p.danger));
        }
        let same = pv.rows.iter().filter(|r| !r.changed).count();
        if same > 0 {
            job.append("  ·  ", 0.0, f(p.text3));
            job.append(&tr!("footer.unchanged", n = same), 0.0, f(p.text2));
        }
        if pv.conflicts > 0 {
            (icon::WARNING_CIRCLE, p.danger)
        } else {
            (icon::CHECK, p.success)
        }
    };
    job.wrap.max_width = rect.width() - 260.0;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    style::paint_icon(
        ui,
        Rect::from_center_size(pos2(rect.left() + 8.0, cy), vec2(16.0, 16.0)),
        glyph,
        14.0,
        gcol,
    );
    let g = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(rect.left() + 22.0, cy - g.size().y / 2.0), g, p.text);

    // Buttons (right): Cancel, Rename.
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::MD;
            let can = pv.can_apply() && !v.busy && v.status == Status::Ready;
            let label = if v.busy {
                tr!("btn.renaming")
            } else {
                tr!("btn.rename")
            };
            let r = style::button(ui, label, ButtonKind::Primary, can);
            let r = if !can && pv.conflicts > 0 {
                r.on_hover_text(tr!("btn.resolve_first"))
            } else {
                r.on_hover_text(tr!("btn.rename.tip"))
            };
            if r.clicked() {
                out.push(SheetAction::Apply);
            }
            if style::button(ui, tr!("btn.cancel"), ButtonKind::Secondary, true)
                .on_hover_text(tr!("btn.cancel.tip"))
                .clicked()
            {
                out.push(SheetAction::Cancel);
            }
        },
    );
}
