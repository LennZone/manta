//! The reader's look, in Manta's design language (macOS type scale,
//! calm neutrals, one accent), for light and dark.

use egui::{Color32, FontFamily, FontId, Ui};

pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
}

/// Type scale (points). Headings follow the macOS text styles the app
/// uses: Title 1 (22), Title 2 (17), Title 3 (15), Headline (13).
pub mod size {
    pub const CAPTION: f32 = 11.0;
    pub const SMALL: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const H1: f32 = 22.0;
    pub const H2: f32 = 17.0;
    pub const H3: f32 = 15.0;
    pub const H4: f32 = 13.0;
    pub const CODE: f32 = 12.0;
    /// Line height of body text (1.6 × 13 pt).
    pub const BODY_LINE: f32 = 21.0;
    pub const CODE_LINE: f32 = 18.0;
    /// The widest a text column gets (about 85 characters).
    pub const MEASURE: f32 = 640.0;
}

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub dark: bool,
    pub bg: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub link: Color32,
    pub separator: Color32,
    pub hover: Color32,
    pub code_bg: Color32,
    pub code_border: Color32,
    pub inline_code_bg: Color32,
    pub quote_bar: Color32,
    pub table_head: Color32,
    pub table_stripe: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub purple: Color32,
    pub neutral_soft: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

impl Colors {
    pub fn of(ui: &Ui) -> Colors {
        let v = ui.visuals();
        let dark = v.dark_mode;
        let pick = |l: Color32, d: Color32| if dark { d } else { l };
        let text2 = v.weak_text_color();
        // The app's content surface (panels sit on it, not on chrome).
        let bg = pick(rgb(255, 255, 255), rgb(30, 30, 32));
        Colors {
            dark,
            bg,
            text: v.text_color(),
            text2,
            text3: mix(text2, bg, 0.35),
            link: pick(rgb(0, 95, 204), rgb(100, 170, 255)),
            separator: v.widgets.noninteractive.bg_stroke.color,
            hover: pick(Color32::from_black_alpha(12), Color32::from_white_alpha(14)),
            code_bg: pick(rgb(246, 246, 248), rgb(38, 38, 41)),
            code_border: pick(rgb(229, 229, 234), rgb(52, 52, 56)),
            inline_code_bg: pick(rgb(237, 237, 241), rgb(52, 52, 56)),
            quote_bar: pick(rgb(208, 208, 214), rgb(78, 78, 84)),
            table_head: pick(rgb(244, 244, 246), rgb(44, 44, 48)),
            table_stripe: pick(rgb(250, 250, 251), rgb(35, 35, 38)),
            accent: pick(rgb(0, 122, 255), rgb(10, 132, 255)),
            accent_soft: pick(rgb(222, 236, 255), rgb(23, 52, 90)),
            success: pick(rgb(24, 118, 50), rgb(48, 209, 88)),
            warning: pick(rgb(150, 88, 0), rgb(255, 196, 80)),
            danger: pick(rgb(204, 32, 40), rgb(255, 105, 97)),
            purple: pick(rgb(128, 48, 186), rgb(204, 128, 245)),
            neutral_soft: pick(rgb(234, 234, 238), rgb(58, 58, 62)),
        }
    }
}

fn has_family(ui: &Ui, name: &str) -> bool {
    let fam = FontFamily::Name(name.into());
    ui.ctx()
        .fonts(|f| f.definitions().families.contains_key(&fam))
}

fn named(ui: &Ui, name: &str, size: f32) -> FontId {
    let fam = FontFamily::Name(name.into());
    FontId::new(
        size,
        if has_family(ui, name) {
            fam
        } else {
            FontFamily::Proportional
        },
    )
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(ui: &Ui, size: f32) -> FontId {
    named(ui, "rl-medium", size)
}

pub fn semibold(ui: &Ui, size: f32) -> FontId {
    named(ui, "rl-semibold", size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

// ------------------------------------------------------------------ icons

/// Phosphor glyphs (the app installs the font as `rl-icons`).
pub mod icon {
    pub const ARROW_LEFT: &str = "\u{E058}";
    pub const ARROW_SQUARE_OUT: &str = "\u{E5DE}";
    pub const CARET_DOWN: &str = "\u{E136}";
    pub const CHECK: &str = "\u{E182}";
    pub const COPY: &str = "\u{E1CA}";
    pub const FILE_TEXT: &str = "\u{E23A}";
    pub const FILE_IMAGE: &str = "\u{EA24}";
    pub const FOLDER_OPEN: &str = "\u{E256}";
    pub const GLOBE: &str = "\u{E288}";
    pub const INFO: &str = "\u{E2CE}";
    pub const LIST_BULLETS: &str = "\u{E2F2}";
    pub const PENCIL_SIMPLE: &str = "\u{E3B4}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const WARNING_CIRCLE: &str = "\u{E4E2}";
    pub const LIGHTBULB: &str = "\u{E2DC}";
    pub const CHAT_CIRCLE: &str = "\u{E168}";
}

/// The icon font, if the app installed it.
pub fn icon_font(ui: &Ui, size: f32) -> Option<FontId> {
    has_family(ui, "rl-icons").then(|| FontId::new(size, FontFamily::Name("rl-icons".into())))
}

/// Paint `glyph` centred in `rect` (nothing if the icon font is missing).
pub fn paint_icon(ui: &Ui, rect: egui::Rect, glyph: &str, size: f32, color: Color32) {
    if let Some(f) = icon_font(ui, size) {
        ui.painter()
            .text(rect.center(), egui::Align2::CENTER_CENTER, glyph, f, color);
    }
}
