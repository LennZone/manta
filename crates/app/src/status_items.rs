//! Plugin items in the status bar (`HostRequest::SetStatusItems`): icon,
//! text and a mini meter each, right of the transfer pills and left of the
//! command palette hint, and how they shrink in narrow windows.

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Ui};
use manta_plugin::{StatusItem, StatusLevel};

use crate::plugin_ui;
use crate::theme::{self, pal, text};
use crate::widgets;

/// How an item is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemForm {
    Full,
    /// Its `compact` text (often icon and meter only).
    Compact,
    Hidden,
}

/// How the "⇧⌘P Command Palette" hint is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintForm {
    Full,
    /// Only the shortcut.
    Short,
    Hidden,
}

/// Room between two items (and between the items and the hint).
pub const GAP: f32 = 14.0;
const H: f32 = 18.0;
const PAD: f32 = 4.0;
const ICON_W: f32 = 13.0;
const PART_GAP: f32 = 4.0;
const METER_W: f32 = 18.0;
const METER_H: f32 = 4.0;

/// The forms that fit into `avail` points. `full` and `compact` are the
/// item widths (`None`: no compact form), `hint` the hint's full and short
/// widths. In order, until it fits: the hint shrinks to its shortcut, then
/// goes; items without a compact form go (lowest priority first); the rest
/// switch to their compact form; then those go, lowest priority first.
pub fn fit(
    full: &[f32],
    compact: &[Option<f32>],
    priority: &[i32],
    hint: [f32; 2],
    avail: f32,
) -> (Vec<ItemForm>, HintForm) {
    let n = full.len();
    let mut forms = vec![ItemForm::Full; n];
    let width = |forms: &[ItemForm], h: HintForm| {
        let mut w = 0.0;
        let mut shown = 0;
        for (i, f) in forms.iter().enumerate() {
            let iw = match f {
                ItemForm::Full => full[i],
                ItemForm::Compact => compact[i].unwrap_or(0.0),
                ItemForm::Hidden => continue,
            };
            w += iw;
            shown += 1;
        }
        let hw = match h {
            HintForm::Full => hint[0],
            HintForm::Short => hint[1],
            HintForm::Hidden => 0.0,
        };
        if hw > 0.0 {
            w += hw;
            shown += 1;
        }
        w + GAP * (shown.max(1) - 1) as f32
    };
    for h in [HintForm::Full, HintForm::Short, HintForm::Hidden] {
        if width(&forms, h) <= avail {
            return (forms, h);
        }
    }
    let hint_form = HintForm::Hidden;
    // Lowest priority first; on a tie the item further right goes first.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (priority[i], std::cmp::Reverse(i)));
    for &i in order.iter().filter(|&&i| compact[i].is_none()) {
        forms[i] = ItemForm::Hidden;
        if width(&forms, hint_form) <= avail {
            return (forms, hint_form);
        }
    }
    for f in forms.iter_mut().filter(|f| **f == ItemForm::Full) {
        *f = ItemForm::Compact;
    }
    if width(&forms, hint_form) <= avail {
        return (forms, hint_form);
    }
    for &i in &order {
        forms[i] = ItemForm::Hidden;
        if width(&forms, hint_form) <= avail {
            break;
        }
    }
    (forms, hint_form)
}

fn font() -> egui::FontId {
    theme::regular(text::CAPTION + 0.5)
}

fn text_of(item: &StatusItem, form: ItemForm) -> &str {
    match form {
        ItemForm::Compact => item.compact.as_deref().unwrap_or(""),
        _ => &item.text,
    }
}

/// Width of the text with the widest digits, so the item doesn't twitch
/// as numbers change.
fn text_width(ui: &Ui, s: &str) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let stable: String = s
        .chars()
        .map(|c| if c.is_ascii_digit() { '8' } else { c })
        .collect();
    ui.painter()
        .layout_no_wrap(stable, font(), Color32::WHITE)
        .size()
        .x
        .ceil()
}

fn glyph(item: &StatusItem) -> Option<&'static str> {
    item.icon.as_deref().and_then(plugin_ui::named_glyph)
}

/// The width of `item` in `form`.
pub fn width(ui: &Ui, item: &StatusItem, form: ItemForm) -> f32 {
    let mut parts = Vec::new();
    if glyph(item).is_some() {
        parts.push(ICON_W);
    }
    let tw = text_width(ui, text_of(item, form));
    if tw > 0.0 {
        parts.push(tw);
    }
    if item.meter.is_some() {
        parts.push(METER_W);
    }
    if parts.is_empty() {
        return 0.0;
    }
    parts.iter().sum::<f32>() + PART_GAP * (parts.len() - 1) as f32 + 2.0 * PAD
}

/// Text / icon colour, meter fill.
fn colors(ui: &Ui, level: StatusLevel) -> (Color32, Color32) {
    let p = pal(ui);
    match level {
        StatusLevel::Normal => (p.text_secondary, p.text_tertiary),
        StatusLevel::Warn => (p.warning, p.warning),
        StatusLevel::Error => (p.danger, p.danger),
    }
}

/// Draw one item into `rect` (from [`width`]); the response is clickable
/// when the item has an action.
pub fn show(ui: &mut Ui, item: &StatusItem, form: ItemForm, id: egui::Id) -> egui::Response {
    let w = width(ui, item, form);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, H), Sense::hover());
    let sense = if item.action.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, id, sense);
    let p = pal(ui);
    if item.action.is_some() && resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::radius::PILL), p.hover);
    }
    let (fg, fill) = colors(ui, item.level);
    let mut x = rect.left() + PAD;
    let cy = rect.center().y;
    if let Some(g) = glyph(item) {
        let r = Rect::from_center_size(egui::pos2(x + ICON_W / 2.0, cy), egui::vec2(ICON_W, H));
        widgets::paint_icon(ui, r, g, 12.0, fg, false);
        x += ICON_W + PART_GAP;
    }
    let s = text_of(item, form);
    if !s.is_empty() {
        let galley = ui.painter().layout_no_wrap(s.to_string(), font(), fg);
        ui.painter()
            .galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, fg);
        x += text_width(ui, s) + PART_GAP;
    }
    if let Some(m) = item.meter {
        let track = Rect::from_min_size(
            egui::pos2(x, cy - METER_H / 2.0),
            egui::vec2(METER_W, METER_H),
        );
        let r = CornerRadius::same(2);
        ui.painter().rect_filled(track, r, p.separator);
        let m = m.clamp(0.0, 1.0);
        if m > 0.0 {
            let w = (METER_W * m).max(2.0);
            ui.painter().rect_filled(
                Rect::from_min_size(track.min, egui::vec2(w, METER_H)),
                r,
                fill,
            );
        }
    }
    let label = item.text.clone();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let tip = match (&item.tooltip, form) {
        // Shortened: the full text leads the tooltip.
        (Some(t), ItemForm::Compact) => Some(format!("{}\n\n{t}", item.text)),
        (Some(t), _) => Some(t.clone()),
        (None, ItemForm::Compact) => Some(item.text.clone()),
        (None, _) => None,
    };
    let resp = match tip {
        Some(t) => resp.on_hover_text(t),
        None => resp,
    };
    if item.action.is_some() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HINT: [f32; 2] = [120.0, 30.0];

    /// CPU (30, compact), GPU (10, no compact), RAM (20, compact).
    fn sys(avail: f32) -> (Vec<ItemForm>, HintForm) {
        fit(
            &[70.0, 60.0, 110.0],
            &[Some(30.0), None, Some(30.0)],
            &[30, 10, 20],
            HINT,
            avail,
        )
    }

    use ItemForm::{Compact as C, Full as F, Hidden as X};

    #[test]
    fn everything_fits() {
        assert_eq!(sys(1000.0), (vec![F, F, F], HintForm::Full));
        let all = 70.0 + 60.0 + 110.0 + 120.0 + 3.0 * GAP;
        assert_eq!(sys(all), (vec![F, F, F], HintForm::Full));
    }

    #[test]
    fn hint_shrinks_then_goes() {
        let items = 70.0 + 60.0 + 110.0 + 2.0 * GAP;
        assert_eq!(sys(items + GAP + 30.0), (vec![F, F, F], HintForm::Short));
        assert_eq!(sys(items + GAP + 29.0), (vec![F, F, F], HintForm::Hidden));
    }

    #[test]
    fn gpu_goes_first_then_icons_then_by_priority() {
        let items = 70.0 + 60.0 + 110.0 + 2.0 * GAP;
        assert_eq!(sys(items - 1.0), (vec![F, X, F], HintForm::Hidden));
        assert_eq!(sys(70.0 + 110.0 + GAP), (vec![F, X, F], HintForm::Hidden));
        assert_eq!(
            sys(70.0 + 110.0 + GAP - 1.0),
            (vec![C, X, C], HintForm::Hidden)
        );
        assert_eq!(sys(60.0 + GAP), (vec![C, X, C], HintForm::Hidden));
        assert_eq!(sys(59.0 + GAP), (vec![C, X, X], HintForm::Hidden));
        assert_eq!(sys(30.0), (vec![C, X, X], HintForm::Hidden));
        assert_eq!(sys(29.0), (vec![X, X, X], HintForm::Hidden));
    }

    #[test]
    fn no_items_only_the_hint() {
        assert_eq!(fit(&[], &[], &[], HINT, 200.0), (vec![], HintForm::Full));
        assert_eq!(fit(&[], &[], &[], HINT, 50.0), (vec![], HintForm::Short));
        assert_eq!(fit(&[], &[], &[], HINT, 10.0), (vec![], HintForm::Hidden));
    }

    #[test]
    fn ties_drop_the_rightmost_first() {
        let (f, _) = fit(&[50.0, 50.0], &[None, None], &[0, 0], [0.0, 0.0], 60.0);
        assert_eq!(f, vec![F, X]);
    }
}
