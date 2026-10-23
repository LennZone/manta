//! Manta's design tokens (mirrored from the app's theme, which plugins
//! can't depend on) and the small building blocks the panels use: section
//! captions, key/value and list cards, tags and push buttons. They look
//! like the app's own plugin blocks, in light and dark.

use egui::{
    pos2, text::LayoutJob, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, Ui,
};
use manta_plugin::Tone;

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

pub const CONTROL_R: u8 = 5;
pub const PANEL_R: u8 = 8;
const LIST_ROW_H: f32 = 26.0;
const TAG_H: f32 = 19.0;

/// Phosphor codepoints (the app's icon font, family `rl-icons`).
pub mod icon {
    pub const CHECK: &str = "\u{E182}";
    pub const CHECK_CIRCLE: &str = "\u{E184}";
    pub const COPY: &str = "\u{E1CA}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const INFO: &str = "\u{E2CE}";
    pub const GLOBE: &str = "\u{E288}";
    pub const FILE_IMAGE: &str = "\u{EA24}";
    pub const WRENCH: &str = "\u{E5D4}";
    pub const REFRESH: &str = "\u{E036}";
    pub const LIGHTNING: &str = "\u{E2DE}";
    pub const LOCK: &str = "\u{E308}";
    pub const CODE: &str = "\u{E1BC}";
    pub const FOLDER: &str = "\u{E25A}";
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
    pub const CLOUD: &str = "\u{E1AA}";
    pub const STACK: &str = "\u{E466}";
}

/// The colours, for the current theme.
#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub dark: bool,
    pub content: Color32,
    pub stripe: Color32,
    pub separator: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub on_accent: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub info: Color32,
    pub info_soft: Color32,
    pub purple: Color32,
    pub purple_soft: Color32,
    pub neutral_soft: Color32,
    pub focus: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const LIGHT: Pal = Pal {
    dark: false,
    content: rgb(255, 255, 255),
    stripe: rgb(245, 245, 247),
    separator: rgb(220, 220, 224),
    border: rgb(200, 200, 206),
    text: rgb(29, 29, 31),
    text2: rgb(104, 104, 110),
    text3: rgb(150, 150, 156),
    on_accent: rgb(255, 255, 255),
    accent: rgb(0, 122, 255),
    accent_text: rgb(0, 95, 204),
    accent_soft: rgb(222, 236, 255),
    hover: Color32::from_black_alpha(13),
    control: rgb(255, 255, 255),
    control_hover: rgb(246, 246, 248),
    danger: rgb(204, 32, 40),
    danger_soft: rgb(253, 232, 232),
    success: rgb(24, 118, 50),
    success_soft: rgb(226, 245, 231),
    warning: rgb(150, 88, 0),
    warning_soft: rgb(255, 242, 214),
    info: rgb(0, 95, 204),
    info_soft: rgb(222, 236, 255),
    purple: rgb(128, 48, 186),
    purple_soft: rgb(243, 232, 252),
    neutral_soft: rgb(234, 234, 238),
    focus: rgb(0, 122, 255),
};

pub const DARK: Pal = Pal {
    dark: true,
    content: rgb(30, 30, 32),
    stripe: rgb(37, 37, 40),
    separator: rgb(58, 58, 62),
    border: rgb(78, 78, 84),
    text: rgb(242, 242, 245),
    text2: rgb(160, 160, 167),
    text3: rgb(112, 112, 118),
    on_accent: rgb(255, 255, 255),
    accent: rgb(10, 132, 255),
    accent_text: rgb(100, 170, 255),
    accent_soft: rgb(23, 52, 90),
    hover: Color32::from_rgba_premultiplied(16, 16, 16, 16),
    control: rgb(66, 66, 70),
    control_hover: rgb(78, 78, 83),
    danger: rgb(255, 105, 97),
    danger_soft: rgb(74, 32, 32),
    success: rgb(48, 209, 88),
    success_soft: rgb(24, 60, 34),
    warning: rgb(255, 196, 80),
    warning_soft: rgb(70, 52, 18),
    info: rgb(100, 170, 255),
    info_soft: rgb(23, 52, 90),
    purple: rgb(212, 160, 255),
    purple_soft: rgb(60, 36, 82),
    neutral_soft: rgb(52, 52, 57),
    focus: rgb(64, 156, 255),
};

pub fn pal(ui: &Ui) -> &'static Pal {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

/// (foreground, soft background) of a tone, like the app's `tone_colors`.
pub fn tone_colors(p: &Pal, tone: Tone) -> (Color32, Color32) {
    match tone {
        Tone::Neutral => (p.text2, p.neutral_soft),
        Tone::Accent => (p.purple, p.purple_soft),
        Tone::Success => (p.success, p.success_soft),
        Tone::Warning => (p.warning, p.warning_soft),
        Tone::Danger => (p.danger, p.danger_soft),
        Tone::Info => (p.info, p.info_soft),
        Tone::Muted => (p.text3, Color32::TRANSPARENT),
    }
}

/// Text colour of a tone on the content background (neutral = normal text).
pub fn tone_text(p: &Pal, tone: Tone) -> Color32 {
    match tone {
        Tone::Neutral => p.text,
        Tone::Muted => p.text2,
        t => tone_colors(p, t).0,
    }
}

fn family(ui: &Ui, name: &str) -> FontFamily {
    let fam = FontFamily::Name(name.into());
    if ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&fam))
    {
        fam
    } else {
        FontFamily::Proportional
    }
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(ui: &Ui, size: f32) -> FontId {
    FontId::new(size, family(ui, "rl-medium"))
}

pub fn semibold(ui: &Ui, size: f32) -> FontId {
    FontId::new(size, family(ui, "rl-semibold"))
}

pub fn icon_font(ui: &Ui, size: f32) -> FontId {
    FontId::new(size, family(ui, "rl-icons"))
}

pub fn paint_icon(ui: &Ui, rect: Rect, glyph: &str, size: f32, color: Color32) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        icon_font(ui, size),
        color,
    );
}

fn focus_ring(ui: &Ui, rect: Rect, r: CornerRadius) {
    let p = pal(ui);
    ui.painter().rect_stroke(
        rect.expand(1.0),
        r,
        Stroke::new(2.0, p.focus.gamma_multiply(0.8)),
        StrokeKind::Outside,
    );
}

/// One line of text cut with "…" at `max_w`.
pub fn line_galley(
    ui: &Ui,
    s: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(s.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(10.0));
    ui.painter().layout_job(job)
}

/// The body of a panel: a vertical scroll area with the host's margins.
pub fn body<R>(ui: &mut Ui, id: &str, margin: egui::Margin, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = space::SM;
                    add(ui)
                })
                .inner
        })
        .inner
}

/// Margins of a side panel body.
pub fn sidebar_margin() -> egui::Margin {
    egui::Margin::symmetric(space::LG as i8, space::MD as i8 + 2)
}

/// Margins of a sheet body.
pub fn sheet_margin() -> egui::Margin {
    egui::Margin::symmetric(space::XL as i8, space::LG as i8)
}

pub fn heading(ui: &mut Ui, s: &str) {
    let p = pal(ui);
    ui.add(
        egui::Label::new(
            RichText::new(s)
                .font(semibold(ui, text::TITLE - 1.0))
                .color(p.text),
        )
        .wrap(),
    );
}

/// Wrapped text in a tone (`Muted` = secondary text).
pub fn note(ui: &mut Ui, s: &str, tone: Tone) {
    let p = pal(ui);
    ui.add(
        egui::Label::new(
            RichText::new(s)
                .font(regular(text::BODY))
                .color(tone_text(p, tone)),
        )
        .wrap(),
    );
}

/// A callout: a soft-tinted box with an icon and wrapped text.
pub fn callout(ui: &mut Ui, glyph: &str, s: &str, tone: Tone) {
    let p = pal(ui);
    let (fg, bg) = tone_colors(p, tone);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(PANEL_R - 2))
        .inner_margin(egui::Margin::symmetric(
            space::MD as i8 + 2,
            space::SM as i8 + 1,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 17.0), Sense::hover());
                paint_icon(ui, r, glyph, 13.0, fg);
                let color = if p.dark { p.text } else { fg };
                ui.add(
                    egui::Label::new(
                        RichText::new(s)
                            .font(regular(text::SMALL + 0.5))
                            .color(color),
                    )
                    .wrap(),
                );
            });
        });
}

/// A group caption: small semibold secondary text, optional detail on the
/// right. A little extra air above unless `first`.
pub fn section(ui: &mut Ui, title: &str, detail: Option<&str>, first: bool) {
    let p = pal(ui);
    if !first {
        ui.add_space(space::XS);
    }
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
    let mut right = rect.right() - space::XS;
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        let g = line_galley(ui, d, regular(text::SMALL), p.text3, w * 0.5);
        let pos = pos2(right - g.size().x, rect.center().y - g.size().y / 2.0);
        right = pos.x - space::MD;
        ui.painter().galley(pos, g, p.text3);
    }
    let g = line_galley(
        ui,
        title,
        semibold(ui, text::SMALL),
        p.text2,
        right - rect.left() - space::XS,
    );
    ui.painter().galley(
        pos2(rect.left() + space::XS, rect.center().y - g.size().y / 2.0),
        g,
        p.text2,
    );
}

/// A thin full-width separator line.
pub fn separator(ui: &mut Ui) {
    let p = pal(ui);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.separator));
}

/// A key/value card. Rows are (key, value, tone).
pub fn key_values(ui: &mut Ui, rows: &[(String, String, Tone)]) {
    let p = pal(ui);
    let font = regular(text::SMALL + 0.5);
    // The key column fits the longest key (measured), up to 45 % of the
    // card; longer keys wrap onto a second line instead of being cut.
    let longest_key = rows
        .iter()
        .map(|r| {
            ui.painter()
                .layout_no_wrap(r.0.clone(), font.clone(), p.text2)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let inner_w = ui.available_width() - 2.0 * (space::MD + 2.0);
    let key_w = (longest_key + 1.0).min(inner_w * 0.45) + space::MD;
    egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, p.separator))
        .corner_radius(CornerRadius::same(PANEL_R - 2))
        .inner_margin(egui::Margin::symmetric(
            space::MD as i8 + 2,
            space::SM as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XS;
            for (i, (k, v, tone)) in rows.iter().enumerate() {
                if i > 0 {
                    let y = ui.cursor().top() - space::XXS;
                    ui.painter().hline(
                        ui.max_rect().x_range(),
                        y,
                        Stroke::new(1.0, p.separator.gamma_multiply(0.6)),
                    );
                }
                ui.horizontal_top(|ui| {
                    let g =
                        ui.painter()
                            .layout(k.clone(), font.clone(), p.text2, key_w - space::MD);
                    let (kr, _) = ui.allocate_exact_size(
                        vec2(key_w, (g.size().y + 1.0).max(17.0)),
                        Sense::hover(),
                    );
                    ui.painter()
                        .galley(kr.left_top() + vec2(0.0, 1.0), g, p.text2);
                    let color = tone_text(p, *tone);
                    ui.add(
                        egui::Label::new(RichText::new(v).font(font.clone()).color(color))
                            .wrap()
                            .selectable(true),
                    );
                });
            }
        });
}

/// One row of a [`list`] card.
#[derive(Clone, Default)]
pub struct Row {
    pub glyph: Option<&'static str>,
    pub text: String,
    pub detail: Option<String>,
    pub tone: Tone,
    pub clickable: bool,
    pub tooltip: Option<String>,
}

impl Row {
    pub fn new(glyph: Option<&'static str>, text: impl Into<String>) -> Row {
        Row {
            glyph,
            text: text.into(),
            ..Row::default()
        }
    }
    pub fn detail(mut self, d: impl Into<String>) -> Row {
        self.detail = Some(d.into());
        self
    }
    pub fn tone(mut self, t: Tone) -> Row {
        self.tone = t;
        self
    }
    pub fn clickable(mut self) -> Row {
        self.clickable = true;
        self
    }
    pub fn tooltip(mut self, t: impl Into<String>) -> Row {
        self.tooltip = Some(t.into());
        self
    }
}

/// A list card; returns the index of a clicked row.
pub fn list(ui: &mut Ui, rows: &[Row]) -> Option<usize> {
    let p = pal(ui);
    let mut clicked = None;
    egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, p.separator))
        .corner_radius(CornerRadius::same(PANEL_R - 2))
        .inner_margin(egui::Margin::same(space::XS as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, r) in rows.iter().enumerate() {
                let sense = if r.clickable {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), LIST_ROW_H), sense);
                if r.clickable {
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &r.text)
                    });
                }
                let mut resp = if r.clickable {
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                } else {
                    resp
                };
                if let Some(t) = &r.tooltip {
                    resp = resp.on_hover_text(t);
                }
                if r.clickable && resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(CONTROL_R), p.hover);
                }
                let color = tone_text(p, r.tone);
                let mut x = rect.left() + space::SM;
                if let Some(g) = r.glyph {
                    let ir = Rect::from_min_size(pos2(x, rect.top()), vec2(18.0, rect.height()));
                    let ic = if r.tone == Tone::Neutral {
                        p.text2
                    } else {
                        color
                    };
                    paint_icon(ui, ir, g, 14.0, ic);
                    x = ir.right() + space::XS;
                }
                let detail_w = r.detail.as_ref().map_or(0.0, |d| {
                    ui.painter()
                        .layout_no_wrap(d.clone(), regular(text::SMALL), p.text2)
                        .size()
                        .x
                        .min(rect.width() * 0.45)
                });
                let text_right = rect.right()
                    - space::SM
                    - if detail_w > 0.0 {
                        detail_w + space::MD
                    } else {
                        0.0
                    };
                let g = line_galley(ui, &r.text, regular(text::BODY), color, text_right - x);
                ui.painter()
                    .galley(pos2(x, rect.center().y - g.size().y / 2.0), g, color);
                if let Some(d) = &r.detail {
                    let g = line_galley(ui, d, regular(text::SMALL), p.text2, detail_w + 1.0);
                    ui.painter().galley(
                        pos2(
                            rect.right() - space::SM - g.size().x,
                            rect.center().y - g.size().y / 2.0,
                        ),
                        g,
                        p.text2,
                    );
                }
                if i + 1 < rows.len() {
                    ui.painter().hline(
                        (rect.left() + space::SM)..=(rect.right() - space::SM),
                        rect.bottom(),
                        Stroke::new(1.0, p.separator.gamma_multiply(0.5)),
                    );
                }
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
        });
    clicked
}

/// Small pills in a wrapping row: (text, tone, glyph).
pub fn tags(ui: &mut Ui, tags: &[(String, Tone, Option<&'static str>)]) {
    let p = pal(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::XS + 1.0, space::XS + 1.0);
        let font = medium(ui, text::SMALL - 0.5);
        for (t, tone, glyph) in tags {
            let (fg, mut bg) = tone_colors(p, *tone);
            if matches!(tone, Tone::Muted | Tone::Neutral) {
                bg = p.neutral_soft;
            }
            let fg = if *tone == Tone::Muted { p.text2 } else { fg };
            let g = ui.painter().layout_no_wrap(t.clone(), font.clone(), fg);
            let icon_w = if glyph.is_some() { 14.0 } else { 0.0 };
            let w = (g.size().x + icon_w + 2.0 * space::SM).min(ui.available_width().max(40.0));
            let (rect, _) = ui.allocate_exact_size(vec2(w, TAG_H), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(99), bg);
            let mut x = rect.left() + space::SM;
            if let Some(gl) = glyph {
                let ir = Rect::from_min_size(pos2(x - 2.0, rect.top()), vec2(14.0, rect.height()));
                paint_icon(ui, ir, gl, 11.0, fg);
                x += icon_w;
            }
            ui.painter()
                .galley(pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonKind {
    Primary,
    Secondary,
    /// Red text on a neutral button: a destructive action.
    Danger,
}

/// Push button like the app's: accent-filled primary, neutral secondary.
pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    button_sized(ui, label, kind, enabled, false)
}

fn button_sized(
    ui: &mut Ui,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
    small: bool,
) -> Response {
    let p = pal(ui);
    let (fill, fg, stroke) = match kind {
        ButtonKind::Primary => (p.accent, p.on_accent, Stroke::NONE),
        ButtonKind::Secondary => (p.control, p.text, Stroke::new(1.0, p.border)),
        ButtonKind::Danger => (p.control, p.danger, Stroke::new(1.0, p.border)),
    };
    let fs = if small { text::SMALL } else { text::BODY };
    let font = if kind == ButtonKind::Primary {
        medium(ui, fs)
    } else {
        regular(fs)
    };
    let fg = if enabled { fg } else { p.text3 };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let size = if small {
        vec2(galley.size().x + 2.0 * space::MD + 2.0, 21.0)
    } else {
        vec2((galley.size().x + 2.0 * space::LG).max(64.0), 24.0)
    };
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let r = CornerRadius::same(CONTROL_R + 1);
    let (mut fill, mut stroke) = (fill, stroke);
    if !enabled {
        fill = p.neutral_soft;
        stroke = Stroke::NONE;
    } else if resp.hovered() {
        fill = if kind == ButtonKind::Primary {
            fill.gamma_multiply(0.93)
        } else {
            p.control_hover
        };
    }
    ui.painter().rect(rect, r, fill, stroke, StrokeKind::Inside);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    if resp.has_focus() {
        focus_ring(ui, rect, r);
    }
    resp
}

/// A wrapping row of buttons: (label, kind). Returns the clicked index.
pub fn buttons(ui: &mut Ui, items: &[(&str, ButtonKind)]) -> Option<usize> {
    button_row(ui, items, false)
}

/// Like [`buttons`], smaller (per-item actions inside a list).
pub fn buttons_small(ui: &mut Ui, items: &[(&str, ButtonKind)]) -> Option<usize> {
    button_row(ui, items, true)
}

fn button_row(ui: &mut Ui, items: &[(&str, ButtonKind)], small: bool) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing =
            vec2(if small { space::XS + 1.0 } else { space::SM }, space::SM);
        // No child scopes here: they would stop the row from wrapping.
        for (i, (label, kind)) in items.iter().enumerate() {
            if button_sized(ui, label, *kind, true, small).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// A slim progress bar (`frac` in 0..=1; `None` = indeterminate stripe).
pub fn progress(ui: &mut Ui, frac: Option<f32>) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
    let r = CornerRadius::same(2);
    ui.painter().rect_filled(rect, r, p.neutral_soft);
    match frac {
        Some(f) => {
            let w = rect.width() * f.clamp(0.0, 1.0);
            if w > 0.5 {
                ui.painter().rect_filled(
                    Rect::from_min_size(rect.min, vec2(w.max(4.0), rect.height())),
                    r,
                    p.accent,
                );
            }
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let seg = rect.width() * 0.3;
            let x = rect.left() + ((t * 0.8).fract() * (rect.width() + seg)) - seg;
            let a = Rect::from_min_max(
                pos2(x.max(rect.left()), rect.top()),
                pos2((x + seg).min(rect.right()), rect.bottom()),
            );
            if a.width() > 0.0 {
                ui.painter().rect_filled(a, r, p.accent);
            }
            ui.ctx().request_repaint();
        }
    }
}
