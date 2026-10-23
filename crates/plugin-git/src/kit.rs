//! Small drawing kit for the Git panel: colours derived from `ui.visuals()`
//! (plus semantic tones matching the app's palette), fonts that use the
//! app's weights when present, painted vector icons (no icon font needed),
//! and a few widgets.

use egui::{
    epaint::PathStroke, text::LayoutJob, Align2, Color32, CornerRadius, FontFamily, FontId, Galley,
    Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use manta_plugin::Tone;

use crate::i18n::tr;
use std::sync::Arc;

pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
}

pub mod text {
    pub const CAPTION: f32 = 11.0;
    pub const SMALL: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
}

pub const ROW_H: f32 = 24.0;

/// Colours for the current theme.
#[derive(Clone, Copy)]
pub struct Colors {
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub separator: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub field: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub purple: Color32,
    pub purple_soft: Color32,
    pub add_bg: Color32,
    pub del_bg: Color32,
    pub hunk_bg: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Colors {
    pub fn of(ui: &Ui) -> Colors {
        let v = ui.visuals();
        let dark = v.dark_mode;
        let text = v.text_color();
        let text2 = v.weak_text_color();
        let bg = v.panel_fill;
        let text3 = mix(text2, bg, 0.35);
        let separator = v.widgets.noninteractive.bg_stroke.color;
        let semantic = |l: Color32, d: Color32| if dark { d } else { l };
        Colors {
            text,
            text2,
            text3,
            separator,
            hover: if dark {
                Color32::from_white_alpha(14)
            } else {
                Color32::from_black_alpha(12)
            },
            selected: semantic(rgb(222, 236, 255), rgb(30, 60, 100)),
            field: v.extreme_bg_color,
            accent: semantic(rgb(0, 122, 255), rgb(10, 132, 255)),
            accent_text: semantic(rgb(0, 95, 204), rgb(100, 170, 255)),
            accent_soft: semantic(rgb(222, 236, 255), rgb(23, 52, 90)),
            success: semantic(rgb(30, 128, 56), rgb(48, 209, 88)),
            success_soft: semantic(rgb(226, 245, 231), rgb(24, 60, 34)),
            warning: semantic(rgb(166, 98, 0), rgb(255, 196, 80)),
            warning_soft: semantic(rgb(255, 242, 214), rgb(70, 52, 18)),
            danger: semantic(rgb(204, 32, 40), rgb(255, 105, 97)),
            danger_soft: semantic(rgb(253, 232, 232), rgb(74, 32, 32)),
            purple: semantic(rgb(150, 60, 200), rgb(204, 128, 245)),
            purple_soft: semantic(rgb(243, 230, 251), rgb(58, 36, 74)),
            add_bg: semantic(rgb(232, 248, 236), rgb(26, 52, 34)),
            del_bg: semantic(rgb(253, 236, 236), rgb(62, 30, 32)),
            hunk_bg: semantic(rgb(238, 243, 251), rgb(34, 42, 56)),
        }
    }

    /// (foreground, soft background) for a tone.
    pub fn tone(&self, t: Tone) -> (Color32, Color32) {
        match t {
            Tone::Success => (self.success, self.success_soft),
            Tone::Warning => (self.warning, self.warning_soft),
            Tone::Danger => (self.danger, self.danger_soft),
            Tone::Info => (self.accent_text, self.accent_soft),
            Tone::Accent => (self.purple, self.purple_soft),
            Tone::Neutral | Tone::Muted => (self.text2, self.hover),
        }
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

// ------------------------------------------------------------------ fonts

fn named(ui: &Ui, name: &str, size: f32) -> FontId {
    let fam = FontFamily::Name(name.into());
    let has = ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&fam));
    FontId::new(size, if has { fam } else { FontFamily::Proportional })
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// The app's medium weight if installed, else regular.
pub fn medium(ui: &Ui, size: f32) -> FontId {
    named(ui, "rl-medium", size)
}

pub fn semibold(ui: &Ui, size: f32) -> FontId {
    named(ui, "rl-semibold", size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// One line of text, cut with "…" to `max_w`.
pub fn galley(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_w.max(8.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

/// Paint `g` vertically centred in `rect` at `x`. Returns its right edge.
pub fn paint_galley(ui: &Ui, g: Arc<Galley>, x: f32, rect: Rect, color: Color32) -> f32 {
    let w = g.size().x;
    let y = rect.center().y - g.size().y / 2.0;
    ui.painter().galley(Pos2::new(x, y), g, color);
    x + w
}

// ------------------------------------------------------------------ icons

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Branch,
    Refresh,
    Plus,
    Minus,
    Check,
    Close,
    Warning,
    Folder,
    Cloud,
}

/// Paint an icon into a square `rect` (designed on a 16-pt grid).
pub fn paint_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let p = ui.painter();
    let s = rect.width().min(rect.height()) / 16.0;
    let c = rect.center();
    let at = |x: f32, y: f32| Pos2::new(c.x + (x - 8.0) * s, c.y + (y - 8.0) * s);
    let w = 1.4 * s.max(0.8);
    let stroke = Stroke::new(w, color);
    match icon {
        Icon::Branch => {
            p.circle_stroke(at(5.0, 3.5), 1.8 * s, stroke);
            p.circle_stroke(at(5.0, 12.5), 1.8 * s, stroke);
            p.circle_stroke(at(11.5, 5.0), 1.8 * s, stroke);
            p.line_segment([at(5.0, 5.3), at(5.0, 10.7)], stroke);
            let curve: Vec<Pos2> = (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    // from (11.5, 6.8) bending down to (5, 10.7)
                    let x = 11.5 + (5.0 - 11.5) * t * t;
                    let y = 6.8 + (10.2 - 6.8) * t;
                    at(x, y)
                })
                .collect();
            p.add(egui::Shape::line(curve, PathStroke::new(w, color)));
        }
        Icon::Refresh => {
            let r = 5.0 * s;
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = (-60.0 + 290.0 * i as f32 / 20.0).to_radians();
                    Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            let end = *pts.last().expect("points");
            p.add(egui::Shape::line(pts, PathStroke::new(w, color)));
            // Arrow head at the end of the arc (pointing clockwise).
            let a = (230.0f32).to_radians();
            let tangent = Vec2::new(-a.sin(), a.cos());
            let normal = Vec2::new(a.cos(), a.sin());
            let tip = end + tangent * 1.6 * s;
            p.add(egui::Shape::convex_polygon(
                vec![
                    tip + tangent * 1.2 * s,
                    end + normal * 2.4 * s - tangent * 1.2 * s,
                    end - normal * 2.4 * s - tangent * 1.2 * s,
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Plus => {
            p.line_segment([at(8.0, 3.5), at(8.0, 12.5)], stroke);
            p.line_segment([at(3.5, 8.0), at(12.5, 8.0)], stroke);
        }
        Icon::Minus => {
            p.line_segment([at(3.5, 8.0), at(12.5, 8.0)], stroke);
        }
        Icon::Check => {
            p.add(egui::Shape::line(
                vec![at(3.5, 8.5), at(6.5, 11.5), at(12.5, 4.5)],
                PathStroke::new(w * 1.15, color),
            ));
        }
        Icon::Close => {
            p.line_segment([at(4.5, 4.5), at(11.5, 11.5)], stroke);
            p.line_segment([at(11.5, 4.5), at(4.5, 11.5)], stroke);
        }
        Icon::Warning => {
            p.add(egui::Shape::closed_line(
                vec![at(8.0, 2.5), at(14.0, 13.0), at(2.0, 13.0)],
                PathStroke::new(w, color),
            ));
            p.line_segment([at(8.0, 6.5), at(8.0, 9.5)], stroke);
            p.circle_filled(at(8.0, 11.3), 0.8 * s, color);
        }
        Icon::Folder => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(2.0, 4.0),
                    at(6.0, 4.0),
                    at(7.5, 5.5),
                    at(14.0, 5.5),
                    at(14.0, 12.5),
                    at(2.0, 12.5),
                ],
                PathStroke::new(w, color),
            ));
        }
        Icon::Cloud => {
            p.circle_stroke(at(6.0, 9.0), 3.0 * s, stroke);
            p.circle_stroke(at(10.0, 8.0), 3.5 * s, stroke);
        }
    }
}

/// A small square icon button. `hint` shows as tooltip.
pub fn icon_button(
    ui: &mut Ui,
    id: egui::Id,
    rect: Rect,
    icon: Icon,
    hint: &str,
    color: Color32,
) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let col = Colors::of(ui);
    if resp.hovered() {
        let fill = if resp.is_pointer_button_down_on() {
            col.hover.gamma_multiply(2.0)
        } else {
            col.hover
        };
        ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    }
    let ir = Rect::from_center_size(rect.center(), Vec2::splat(rect.height().min(16.0) - 2.0));
    paint_icon(ui, ir, icon, color);
    resp.on_hover_text(hint)
}

/// Rounded status badge ("M", "?", …) centred in `rect`.
pub fn paint_badge(ui: &Ui, rect: Rect, text: &str, fg: Color32, bg: Color32) {
    ui.painter().rect_filled(rect, CornerRadius::same(4), bg);
    let font = medium(ui, 10.5);
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        text,
        font,
        fg,
    );
}

/// A pill with text, allocated in the layout.
pub fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    let font = medium(ui, text::CAPTION);
    let g = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = Vec2::new(g.size().x + 2.0 * space::SM + 2.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(99), bg);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
    resp
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    /// Text-only, accent coloured (e.g. "Stage All").
    Link,
}

/// A button matching the app's buttons.
pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool, height: f32) -> Response {
    let col = Colors::of(ui);
    let font = match kind {
        ButtonKind::Primary => medium(ui, text::BODY),
        ButtonKind::Link => medium(ui, text::SMALL),
    };
    let (fill, fg, stroke) = match kind {
        ButtonKind::Primary => (col.accent, Color32::WHITE, Stroke::NONE),
        ButtonKind::Link => (Color32::TRANSPARENT, col.accent_text, Stroke::NONE),
    };
    let g = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let pad = if kind == ButtonKind::Link {
        space::SM
    } else {
        space::LG
    };
    let min_w = if kind == ButtonKind::Primary {
        76.0
    } else {
        0.0
    };
    let size = Vec2::new((g.size().x + 2.0 * pad).max(min_w), height);
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let mut fill = fill;
    let mut fg = fg;
    if !enabled {
        // Neutral when disabled, so it never reads as clickable.
        if kind == ButtonKind::Primary {
            fill = col.hover.gamma_multiply(1.6);
            fg = col.text3;
        }
    } else if resp.is_pointer_button_down_on() {
        fill = if kind == ButtonKind::Primary {
            fill.gamma_multiply(0.85)
        } else {
            col.hover.gamma_multiply(2.0)
        };
    } else if resp.hovered() {
        fill = if kind == ButtonKind::Primary {
            fill.gamma_multiply(0.93)
        } else {
            col.hover
        };
    }
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    if !enabled && kind == ButtonKind::Link {
        fg = fg.gamma_multiply(0.6);
    }
    ui.painter()
        .galley_with_override_text_color(rect.center() - g.size() / 2.0, g, fg);
    resp
}

/// A stable widget id from any hashable key.
pub fn id(key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
    egui::Id::unique(key)
}

/// Thin horizontal separator across the full width.
pub fn separator(ui: &mut Ui) {
    let col = Colors::of(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, col.separator),
    );
}

/// Human relative time, e.g. "5 min ago".
pub fn relative_time(now: i64, t: i64) -> String {
    let d = (now - t).max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match d {
        _ if d < MIN => tr!("git.time.now").into(),
        _ if d < HOUR => tr!("git.time.min", n = d / MIN),
        _ if d < DAY => tr!("git.time.hours", n = d / HOUR),
        _ if d < 2 * DAY => tr!("git.time.yesterday").into(),
        _ if d < 14 * DAY => tr!("git.time.days", n = d / DAY),
        _ if d < 60 * DAY => tr!("git.time.weeks", n = d / (7 * DAY)),
        _ if d < 365 * DAY => tr!("git.time.months", n = d / (30 * DAY)),
        _ if d < 730 * DAY => tr!("git.time.year").into(),
        _ => tr!("git.time.years", n = d / (365 * DAY)),
    }
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times() {
        let n = 1_000_000_000;
        assert_eq!(relative_time(n, n - 5), "just now");
        assert_eq!(relative_time(n, n - 300), "5 min ago");
        assert_eq!(relative_time(n, n - 3 * 3600), "3 h ago");
        assert_eq!(relative_time(n, n - 30 * 3600), "yesterday");
        assert_eq!(relative_time(n, n - 4 * 86400), "4 days ago");
        assert_eq!(relative_time(n, n - 21 * 86400), "3 weeks ago");
        assert_eq!(relative_time(n, n - 90 * 86400), "3 months ago");
        assert_eq!(relative_time(n, n - 800 * 86400), "2 years ago");
        assert_eq!(relative_time(n, n + 50), "just now");
        manta_plugin::i18n::with_lang(manta_plugin::i18n::Lang::De, || {
            assert_eq!(relative_time(n, n - 300), "vor 5 Min.");
            assert_eq!(relative_time(n, n - 30 * 3600), "gestern");
            assert_eq!(relative_time(n, n - 4 * 86400), "vor 4 Tagen");
        });
    }
}
