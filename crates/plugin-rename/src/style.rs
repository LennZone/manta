//! Manta's design tokens (mirrored from the app's theme, which plugins
//! can't depend on) and the few small controls the sheet needs.

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Rect, Response, Sense, Stroke,
    StrokeKind, Ui,
};

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
}

pub const CONTROL_R: u8 = 5;
pub const PANEL_R: u8 = 8;

/// Phosphor codepoints (the app's icon font, family `rl-icons`).
pub mod icon {
    pub const ARROW_RIGHT: &str = "\u{E06C}";
    pub const ARROW_UNDO: &str = "\u{E038}";
    pub const BROOM: &str = "\u{EC54}";
    pub const CARET_DOWN: &str = "\u{E136}";
    pub const CARET_UP: &str = "\u{E13C}";
    pub const CHECK: &str = "\u{E182}";
    pub const CLOCK: &str = "\u{E19A}";
    pub const FILE: &str = "\u{E230}";
    pub const FOLDER: &str = "\u{E25A}";
    pub const INFO: &str = "\u{E2CE}";
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
    pub const PENCIL: &str = "\u{E3B4}";
    pub const PLUS: &str = "\u{E3D4}";
    pub const SCISSORS: &str = "\u{EAE0}";
    pub const STAR: &str = "\u{E46A}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const WARNING_CIRCLE: &str = "\u{E4E2}";
    pub const X: &str = "\u{E4F6}";
}

/// The colours the sheet uses, for the current theme.
#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub dark: bool,
    pub content: Color32,
    pub stripe: Color32,
    pub elevated: Color32,
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
    pub info_soft: Color32,
    pub neutral_soft: Color32,
    pub focus: Color32,
    pub folder: Color32,
    pub generic: Color32,
    /// Removed characters in the old name.
    pub del_bg: Color32,
    /// Inserted characters in the new name.
    pub ins_bg: Color32,
    pub ins_text: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const LIGHT: Pal = Pal {
    dark: false,
    content: rgb(255, 255, 255),
    stripe: rgb(245, 245, 247),
    elevated: rgb(252, 252, 253),
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
    info_soft: rgb(222, 236, 255),
    neutral_soft: rgb(234, 234, 238),
    focus: rgb(0, 122, 255),
    folder: rgb(62, 151, 240),
    generic: rgb(142, 142, 150),
    del_bg: rgb(253, 226, 226),
    ins_bg: rgb(212, 240, 220),
    ins_text: rgb(14, 92, 38),
};

pub const DARK: Pal = Pal {
    dark: true,
    content: rgb(30, 30, 32),
    stripe: rgb(37, 37, 40),
    elevated: rgb(46, 46, 49),
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
    info_soft: rgb(23, 52, 90),
    neutral_soft: rgb(52, 52, 57),
    focus: rgb(64, 156, 255),
    folder: rgb(90, 172, 250),
    generic: rgb(150, 150, 158),
    del_bg: rgb(92, 38, 40),
    ins_bg: rgb(26, 74, 42),
    ins_text: rgb(150, 236, 170),
};

pub fn pal(ui: &Ui) -> &'static Pal {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
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

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// The icon font if the host installed it.
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

pub fn focus_ring(ui: &Ui, rect: Rect, r: CornerRadius) {
    let p = pal(ui);
    ui.painter().rect_stroke(
        rect.expand(1.0),
        r,
        Stroke::new(2.0, p.focus.gamma_multiply(0.8)),
        StrokeKind::Outside,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
}

/// Push button like the app's: accent-filled primary, neutral secondary.
pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    let p = pal(ui);
    let (fill, fg, stroke) = match kind {
        ButtonKind::Primary => (p.accent, p.on_accent, Stroke::NONE),
        ButtonKind::Secondary => (p.control, p.text, Stroke::new(1.0, p.border)),
    };
    let font = if kind == ButtonKind::Primary {
        medium(ui, text::BODY)
    } else {
        regular(text::BODY)
    };
    let fg = if enabled { fg } else { p.text3 };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let size = vec2((galley.size().x + 2.0 * space::LG + 4.0).max(76.0), 26.0);
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let r = CornerRadius::same(CONTROL_R + 1);
    let (mut fill, mut stroke) = (fill, stroke);
    if !enabled {
        // Disabled: neutral, so it never reads as the active default.
        fill = p.neutral_soft;
        stroke = Stroke::NONE;
    } else if resp.hovered() {
        fill = if kind == ButtonKind::Secondary {
            p.control_hover
        } else {
            fill.gamma_multiply(0.93)
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

/// A borderless square icon button.
pub fn icon_button(ui: &mut Ui, glyph: &str, size: f32, enabled: bool, tip: &str) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(
        vec2(size, size),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_R), p.hover);
    }
    let color = if !enabled {
        p.text3.gamma_multiply(0.6)
    } else if resp.hovered() {
        p.text
    } else {
        p.text2
    };
    paint_icon(ui, rect, glyph, (size * 0.6).round(), color);
    if resp.has_focus() {
        focus_ring(ui, rect, CornerRadius::same(CONTROL_R));
    }
    if enabled && !tip.is_empty() {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}

/// A macOS-style checkbox with a label.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let p = pal(ui);
    let font = regular(text::SMALL);
    let g = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let w = 14.0
        + if label.is_empty() {
            0.0
        } else {
            space::SM + g.size().x
        };
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let b = Rect::from_center_size(pos2(rect.left() + 7.0, rect.center().y), vec2(14.0, 14.0));
    let r = CornerRadius::same(4);
    if *on {
        ui.painter().rect_filled(b, r, p.accent);
        paint_icon(ui, b, icon::CHECK, 10.0, p.on_accent);
    } else {
        let fill = if resp.hovered() {
            p.control_hover
        } else {
            p.control
        };
        ui.painter()
            .rect(b, r, fill, Stroke::new(1.0, p.border), StrokeKind::Inside);
    }
    if resp.has_focus() {
        focus_ring(ui, b, r);
    }
    if !label.is_empty() {
        ui.painter().galley(
            pos2(b.right() + space::SM, rect.center().y - g.size().y / 2.0),
            g,
            p.text,
        );
    }
    resp
}

/// A small on/off switch (rule enabled).
pub fn switch(ui: &mut Ui, on: &mut bool, tip: &str) -> Response {
    let p = pal(ui);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(26.0, 16.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let track = if *on {
        p.accent
    } else if p.dark {
        rgb(84, 84, 90)
    } else {
        rgb(212, 212, 218)
    };
    let r = CornerRadius::same(8);
    ui.painter().rect_filled(rect, r, track);
    let x = rect.left() + 8.0 + t * (rect.width() - 16.0);
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), 6.0, Color32::WHITE);
    if resp.has_focus() {
        focus_ring(ui, rect, r);
    }
    resp.on_hover_text(tip)
}

/// A segmented control. Returns true if the value changed. Each segment is
/// focusable (Tab) and toggles with Space.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = pal(ui);
    let font = regular(text::SMALL);
    let pad = space::MD;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), p.text)
                .size()
                .x
                + 2.0 * pad
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 22.0), Sense::hover());
    let r = CornerRadius::same(CONTROL_R + 1);
    ui.painter().rect_filled(
        rect,
        r,
        if p.dark {
            rgb(24, 24, 26)
        } else {
            rgb(232, 232, 236)
        },
    );
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, ((v, label), w)) in options.iter().zip(&widths).enumerate() {
        let seg = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(*w, rect.height() - 4.0));
        let id = ui.scope_id().with(("seg", options[0].1, i));
        let resp = ui.interact(seg, id, Sense::click());
        let on = *v == *value;
        if on {
            ui.painter().rect(
                seg,
                CornerRadius::same(CONTROL_R),
                if p.dark { rgb(92, 92, 98) } else { p.content },
                Stroke::new(
                    1.0,
                    if p.dark {
                        Color32::TRANSPARENT
                    } else {
                        p.separator
                    },
                ),
                StrokeKind::Inside,
            );
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(CONTROL_R), p.hover);
        }
        let color = if on { p.text } else { p.text2 };
        ui.painter().text(
            seg.center(),
            Align2::CENTER_CENTER,
            *label,
            font.clone(),
            color,
        );
        if resp.has_focus() {
            focus_ring(ui, seg, CornerRadius::same(CONTROL_R));
        }
        if resp.clicked() && !on {
            *value = *v;
            changed = true;
        }
        x += w;
    }
    changed
}

/// A single-line text field with the app's look.
pub fn text_field(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    mono_font: bool,
) -> Response {
    let p = pal(ui);
    let font = if mono_font {
        mono(text::SMALL)
    } else {
        regular(text::SMALL + 0.5)
    };
    let edit = egui::TextEdit::singleline(value)
        .hint_text(egui::RichText::new(hint).color(p.text3))
        .font(font)
        .desired_width(width)
        .margin(egui::Margin::symmetric(6, 3))
        .text_color(p.text);
    // The app's selection stroke is white (for selected rows), which makes
    // egui's focused frame invisible on light backgrounds: draw our own.
    let resp = ui
        .scope(|ui| {
            let w = &mut ui.visuals_mut().widgets;
            w.active.bg_stroke = Stroke::new(1.0, p.border);
            ui.visuals_mut().selection.stroke = Stroke::new(1.0, p.focus);
            ui.add_sized(vec2(width, 22.0), edit)
        })
        .inner;
    if resp.has_focus() {
        focus_ring(ui, resp.rect, CornerRadius::same(CONTROL_R));
    }
    resp
}

/// A small caption label.
pub fn caption(ui: &mut Ui, s: &str) {
    let p = pal(ui);
    ui.label(
        egui::RichText::new(s)
            .font(regular(text::SMALL))
            .color(p.text2),
    );
}

/// Mix two colours (`t` = 0 → a, 1 → b).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// A compact primary button (inside menus).
pub fn button_small(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let font = medium(ui, text::SMALL);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, p.on_accent);
    let size = vec2(galley.size().x + 2.0 * space::LG, 22.0);
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let r = CornerRadius::same(CONTROL_R);
    let mut fill = p.accent;
    if !enabled {
        fill = fill.gamma_multiply(0.45);
    } else if resp.hovered() {
        fill = fill.gamma_multiply(0.93);
    }
    ui.painter().rect_filled(rect, r, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.on_accent);
    if resp.has_focus() {
        focus_ring(ui, rect, r);
    }
    resp
}
