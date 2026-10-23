//! Design tokens and look & feel.
//!
//! Direction: native-feeling macOS. Calm neutrals, one accent (system blue),
//! SF Pro when the system has it, and a strict spacing and type scale. Every
//! colour used by the UI comes from [`Palette`], so light and dark stay in
//! sync and contrast can be checked in one place (see the tests at the
//! bottom: text tokens meet WCAG AA on the surfaces they are used on).
//!
//! Light/dark follows Settings → General → Appearance (System by default)
//! and the accent colour is one of the macOS accent colours; see [`apply`].

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use eframe::egui::{
    self, epaint::text::VariationCoords, Color32, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Margin, Stroke, Theme, Visuals,
};
use manta_ops::CompareStatus;

// ------------------------------------------------------------------ spacing

/// 4-pt spacing scale.
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
}

/// Corner radii, by hierarchy: small controls < rows < panels < windows.
pub mod radius {
    pub const CONTROL: u8 = 5;
    pub const ROW: u8 = 5;
    pub const PANEL: u8 = 8;
    pub const WINDOW: u8 = 12;
    pub const PILL: u8 = 99;
}

/// Fixed heights.
pub mod size {
    pub const ROW: f32 = 24.0;
    pub const HEADER: f32 = 24.0;
    pub const TOOLBAR_BUTTON: f32 = 28.0;
    pub const SIDEBAR_ROW: f32 = 26.0;
    pub const ICON: f32 = 16.0;
    /// Height of every single-line control in a bar or form (text fields,
    /// breadcrumb segments, chips, buttons), so their text centres line up.
    pub const FIELD: f32 = 24.0;
    /// Height of the pane's path bar.
    pub const PATH_BAR: f32 = 36.0;
}

// ------------------------------------------------------------------ type

/// Type scale (points), following the macOS text styles.
pub mod text {
    /// Captions, counts, keyboard hints.
    pub const CAPTION: f32 = 11.0;
    /// Secondary lines, table headers.
    pub const SMALL: f32 = 12.0;
    /// Body, table rows, buttons.
    pub const BODY: f32 = 13.0;
    /// Section titles inside windows.
    pub const TITLE: f32 = 15.0;
    /// Dialog titles.
    pub const HEADLINE: f32 = 17.0;
}

pub const MEDIUM: &str = "rl-medium";
pub const SEMIBOLD: &str = "rl-semibold";
/// Phosphor regular, icons only.
pub const ICONS: &str = "rl-icons";
/// Phosphor fill, icons only.
pub const ICONS_FILL: &str = "rl-icons-fill";

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

pub fn icon_fill_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS_FILL.into()))
}

// ------------------------------------------------------------------ colour

/// Semantic colours for one theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Window chrome: toolbar, status bar.
    pub chrome: Color32,
    pub sidebar: Color32,
    /// File lists, text areas.
    pub content: Color32,
    /// Every other row.
    pub stripe: Color32,
    /// Dialogs, popovers, toasts.
    pub elevated: Color32,
    /// Hairlines between regions.
    pub separator: Color32,
    /// Control borders.
    pub border: Color32,

    pub text: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub text_on_accent: Color32,

    pub accent: Color32,
    /// Accent for text/icons on content (darker in light mode for AA).
    pub accent_text: Color32,
    pub accent_soft: Color32,
    /// Selection in the focused pane.
    pub selection: Color32,
    /// Selection in the unfocused pane (Finder's grey).
    pub selection_inactive: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub focus_ring: Color32,

    pub danger: Color32,
    pub danger_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    /// Informational (plugin tone `info`): blue.
    pub info: Color32,
    pub info_soft: Color32,
    /// Plugin tone `accent`: purple, so it never looks like `info`.
    pub purple: Color32,
    pub purple_soft: Color32,
    /// Background of neutral badges and chips.
    pub neutral_soft: Color32,

    /// File type tints (icons).
    pub folder: Color32,
    pub image: Color32,
    pub code: Color32,
    pub archive: Color32,
    pub document: Color32,
    pub media: Color32,
    pub generic: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const LIGHT: Palette = Palette {
    dark: false,
    chrome: rgb(236, 236, 238),
    sidebar: rgb(232, 232, 235),
    content: rgb(255, 255, 255),
    stripe: rgb(245, 245, 247),
    elevated: rgb(252, 252, 253),
    separator: rgb(220, 220, 224),
    border: rgb(200, 200, 206),
    text: rgb(29, 29, 31),
    text_secondary: rgb(104, 104, 110),
    text_tertiary: rgb(150, 150, 156),
    text_on_accent: rgb(255, 255, 255),
    accent: rgb(0, 122, 255),
    accent_text: rgb(0, 95, 204),
    accent_soft: rgb(222, 236, 255),
    selection: rgb(0, 100, 225),
    selection_inactive: rgb(220, 220, 225),
    hover: Color32::from_black_alpha(13),
    pressed: Color32::from_black_alpha(26),
    focus_ring: rgb(0, 122, 255),
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
    folder: rgb(62, 151, 240),
    image: rgb(175, 82, 222),
    code: rgb(36, 146, 108),
    archive: rgb(181, 117, 32),
    document: rgb(224, 68, 60),
    media: rgb(232, 80, 132),
    generic: rgb(142, 142, 150),
};

pub const DARK: Palette = Palette {
    dark: true,
    chrome: rgb(44, 44, 46),
    sidebar: rgb(38, 38, 41),
    content: rgb(30, 30, 32),
    stripe: rgb(37, 37, 40),
    elevated: rgb(46, 46, 49),
    separator: rgb(58, 58, 62),
    border: rgb(78, 78, 84),
    text: rgb(242, 242, 245),
    text_secondary: rgb(160, 160, 167),
    text_tertiary: rgb(112, 112, 118),
    text_on_accent: rgb(255, 255, 255),
    accent: rgb(10, 132, 255),
    accent_text: rgb(100, 170, 255),
    accent_soft: rgb(23, 52, 90),
    selection: rgb(0, 88, 208),
    selection_inactive: rgb(70, 70, 75),
    hover: Color32::from_rgba_premultiplied(16, 16, 16, 16),
    pressed: Color32::from_rgba_premultiplied(28, 28, 28, 28),
    focus_ring: rgb(64, 156, 255),
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
    folder: rgb(90, 172, 250),
    image: rgb(204, 128, 245),
    code: rgb(80, 200, 150),
    archive: rgb(222, 162, 80),
    document: rgb(255, 120, 110),
    media: rgb(255, 120, 170),
    generic: rgb(150, 150, 158),
};

impl Palette {
    /// The palette for `dark` with the current accent colour.
    pub fn of(dark: bool) -> &'static Palette {
        Palette::with_accent(dark, current_accent())
    }

    pub fn with_accent(dark: bool, accent: Accent) -> &'static Palette {
        static ALL: OnceLock<Vec<Palette>> = OnceLock::new();
        let all = ALL.get_or_init(|| {
            Accent::ALL
                .iter()
                .flat_map(|&a| [a.palette(false), a.palette(true)])
                .collect()
        });
        &all[accent as usize * 2 + dark as usize]
    }
}

// ------------------------------------------------------------------ accent

/// The macOS accent colours (System Settings → Appearance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Blue = 0,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Graphite,
}

/// Accent shades for one theme: fill (buttons, toggles, badges with
/// `on` text), selection, text on content, focus ring.
struct Shades {
    fill: Color32,
    selection: Color32,
    text: Color32,
    ring: Color32,
    on: Color32,
}

impl Accent {
    pub const ALL: [Accent; 8] = [
        Accent::Blue,
        Accent::Purple,
        Accent::Pink,
        Accent::Red,
        Accent::Orange,
        Accent::Yellow,
        Accent::Green,
        Accent::Graphite,
    ];

    /// Catalog key of the colour's name.
    pub fn key(self) -> &'static str {
        match self {
            Accent::Blue => "accent.blue",
            Accent::Purple => "accent.purple",
            Accent::Pink => "accent.pink",
            Accent::Red => "accent.red",
            Accent::Orange => "accent.orange",
            Accent::Yellow => "accent.yellow",
            Accent::Green => "accent.green",
            Accent::Graphite => "accent.graphite",
        }
    }

    /// Swatch colour (the fill in the current theme).
    pub fn swatch(self, dark: bool) -> Color32 {
        self.shades(dark).fill
    }

    /// Shades tuned so text on the fill and on the selection meets WCAG AA
    /// (4.5:1), accent text on the content background too, and the focus
    /// ring 3:1 (see the tests).
    fn shades(self, dark: bool) -> Shades {
        const W: Color32 = rgb(255, 255, 255);
        const K: Color32 = rgb(29, 29, 31);
        let s = |fill, selection, text, ring| Shades {
            fill,
            selection,
            text,
            ring,
            on: W,
        };
        match (self, dark) {
            (Accent::Blue, false) => s(
                rgb(0, 110, 230),
                rgb(0, 100, 225),
                rgb(0, 95, 204),
                rgb(0, 122, 255),
            ),
            (Accent::Blue, true) => s(
                rgb(10, 110, 230),
                rgb(0, 88, 208),
                rgb(100, 170, 255),
                rgb(64, 156, 255),
            ),
            (Accent::Purple, false) => s(
                rgb(130, 60, 200),
                rgb(120, 50, 190),
                rgb(120, 50, 180),
                rgb(130, 60, 200),
            ),
            (Accent::Purple, true) => s(
                rgb(140, 70, 210),
                rgb(110, 50, 180),
                rgb(200, 150, 255),
                rgb(190, 130, 255),
            ),
            (Accent::Pink, false) => s(
                rgb(205, 40, 120),
                rgb(195, 35, 110),
                rgb(190, 30, 105),
                rgb(205, 40, 120),
            ),
            (Accent::Pink, true) => s(
                rgb(200, 45, 120),
                rgb(180, 35, 105),
                rgb(255, 130, 180),
                rgb(255, 100, 165),
            ),
            (Accent::Red, false) => s(
                rgb(210, 35, 40),
                rgb(200, 30, 35),
                rgb(195, 25, 30),
                rgb(210, 35, 40),
            ),
            (Accent::Red, true) => s(
                rgb(210, 50, 50),
                rgb(185, 35, 38),
                rgb(255, 120, 112),
                rgb(255, 95, 88),
            ),
            (Accent::Orange, false) => s(
                rgb(190, 85, 0),
                rgb(180, 80, 0),
                rgb(170, 75, 0),
                rgb(190, 85, 0),
            ),
            (Accent::Orange, true) => s(
                rgb(185, 82, 0),
                rgb(170, 78, 0),
                rgb(255, 165, 80),
                rgb(255, 150, 50),
            ),
            (Accent::Yellow, false) => Shades {
                on: K,
                ..s(
                    rgb(255, 200, 0),
                    rgb(250, 195, 0),
                    rgb(140, 100, 0),
                    rgb(180, 130, 0),
                )
            },
            (Accent::Yellow, true) => Shades {
                on: K,
                ..s(
                    rgb(255, 204, 10),
                    rgb(235, 185, 0),
                    rgb(255, 214, 80),
                    rgb(255, 204, 10),
                )
            },
            (Accent::Green, false) => s(
                rgb(30, 130, 55),
                rgb(28, 120, 50),
                rgb(24, 118, 50),
                rgb(30, 130, 55),
            ),
            (Accent::Green, true) => s(
                rgb(30, 128, 52),
                rgb(30, 120, 52),
                rgb(80, 210, 110),
                rgb(70, 200, 100),
            ),
            (Accent::Graphite, false) => s(
                rgb(110, 110, 116),
                rgb(100, 100, 106),
                rgb(95, 95, 100),
                rgb(110, 110, 116),
            ),
            (Accent::Graphite, true) => s(
                rgb(108, 108, 114),
                rgb(96, 96, 102),
                rgb(175, 175, 182),
                rgb(160, 160, 167),
            ),
        }
    }

    fn palette(self, dark: bool) -> Palette {
        let base = if dark { DARK } else { LIGHT };
        let sh = self.shades(dark);
        // Soft tint: the fill mixed into the content colour.
        let soft = mix(base.content, sh.fill, if dark { 0.28 } else { 0.13 });
        Palette {
            accent: sh.fill,
            accent_text: sh.text,
            accent_soft: soft,
            selection: sh.selection,
            focus_ring: sh.ring,
            text_on_accent: sh.on,
            ..base
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

static ACCENT: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// Under `cargo test` the accent is per thread (parallel renders).
    static TEST_ACCENT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

pub fn current_accent() -> Accent {
    let i = if cfg!(test) {
        TEST_ACCENT.with(|a| a.get())
    } else {
        ACCENT.load(Ordering::Relaxed)
    };
    Accent::ALL[i as usize % Accent::ALL.len()]
}

fn set_accent(a: Accent) {
    if cfg!(test) {
        TEST_ACCENT.with(|c| c.set(a as u8));
    } else {
        ACCENT.store(a as u8, Ordering::Relaxed);
    }
}

/// Light, dark or follow the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn key(self) -> &'static str {
        match self {
            Appearance::System => "appearance.system",
            Appearance::Light => "appearance.light",
            Appearance::Dark => "appearance.dark",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Appearance::System => egui::ThemePreference::System,
            Appearance::Light => egui::ThemePreference::Light,
            Appearance::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Apply appearance and accent: the palette, egui's visuals for both
/// themes, and the theme preference. Cheap; call when either changes.
pub fn apply(ctx: &egui::Context, appearance: Appearance, accent: Accent) {
    apply_accent(ctx, accent);
    ctx.set_theme(appearance.preference());
}

/// Switch the accent colour (palette and egui's visuals of both themes).
pub fn apply_accent(ctx: &egui::Context, accent: Accent) {
    set_accent(accent);
    for theme in [Theme::Light, Theme::Dark] {
        let p = Palette::of(theme == Theme::Dark);
        ctx.style_mut_of(theme, |style| widget_visuals(&mut style.visuals, p));
    }
}

/// Palette for the current theme of a `Ui`.
pub fn pal(ui: &egui::Ui) -> &'static Palette {
    Palette::of(ui.visuals().dark_mode)
}

/// Palette for the current theme of a `Context`.
pub fn pal_ctx(ctx: &egui::Context) -> &'static Palette {
    Palette::of(ctx.global_style().visuals.dark_mode)
}

// ------------------------------------------------------------------ plugin tones

/// Foreground and soft background for a plugin [`Tone`](manta_plugin::Tone)
/// (badges, chips, panel text). Every pair meets WCAG AA (see the tests).
pub fn tone_colors(p: &Palette, tone: manta_plugin::Tone) -> (Color32, Color32) {
    use manta_plugin::Tone as T;
    match tone {
        T::Neutral => (p.text_secondary, p.neutral_soft),
        T::Accent => (p.purple, p.purple_soft),
        T::Success => (p.success, p.success_soft),
        T::Warning => (p.warning, p.warning_soft),
        T::Danger => (p.danger, p.danger_soft),
        T::Info => (p.info, p.info_soft),
        T::Muted => (p.text_tertiary, Color32::TRANSPARENT),
    }
}

/// Text colour for a tone on the content background (name tints, panel
/// text). Neutral is the normal text colour.
pub fn tone_text(p: &Palette, tone: manta_plugin::Tone) -> Color32 {
    match tone {
        manta_plugin::Tone::Neutral => p.text,
        t => tone_colors(p, t).0,
    }
}

// ------------------------------------------------------------------ compare

pub fn compare_color(status: CompareStatus, dark: bool) -> Color32 {
    let p = Palette::of(dark);
    match status {
        CompareStatus::Identical => p.text_tertiary,
        CompareStatus::OnlyLeft | CompareStatus::LeftNewer => p.accent_text,
        CompareStatus::OnlyRight | CompareStatus::RightNewer => p.image,
        CompareStatus::Different => p.warning,
    }
}

/// The status label with panes named "A" / "B" (Compare & Sync with three
/// or more panes, where "left" / "right" would be ambiguous).
pub fn compare_label_for(status: CompareStatus, ab: bool) -> &'static str {
    if !ab {
        return compare_label(status);
    }
    match status {
        CompareStatus::OnlyLeft => t!("compare.only_a"),
        CompareStatus::OnlyRight => t!("compare.only_b"),
        CompareStatus::LeftNewer => t!("compare.a_newer"),
        CompareStatus::RightNewer => t!("compare.b_newer"),
        other => compare_label(other),
    }
}

pub fn compare_label(status: CompareStatus) -> &'static str {
    match status {
        CompareStatus::OnlyLeft => t!("compare.only_left"),
        CompareStatus::OnlyRight => t!("compare.only_right"),
        CompareStatus::Identical => t!("compare.identical"),
        CompareStatus::LeftNewer => t!("compare.left_newer"),
        CompareStatus::RightNewer => t!("compare.right_newer"),
        CompareStatus::Different => t!("compare.different"),
    }
}

/// A glyph per status so colour is never the only cue (WCAG 1.4.1).
pub fn compare_icon(status: CompareStatus) -> &'static str {
    use crate::icons as i;
    match status {
        CompareStatus::OnlyLeft => i::ARROW_LINE_LEFT,
        CompareStatus::OnlyRight => i::ARROW_LINE_RIGHT,
        CompareStatus::LeftNewer => i::CARET_LEFT,
        CompareStatus::RightNewer => i::CARET_RIGHT,
        CompareStatus::Different => i::NOT_EQUALS,
        CompareStatus::Identical => i::EQUALS,
    }
}

// ------------------------------------------------------------------ fonts

/// SF Pro (variable) and SF Mono, if this is a Mac. Read once; ~8 MB, only
/// at startup.
/// Font file bytes, shared between font definitions.
type FontBytes = Arc<Vec<u8>>;
/// SF Pro and (if present) SF Mono.
type SystemFonts = Option<(FontBytes, Option<FontBytes>)>;

fn system_fonts() -> &'static SystemFonts {
    static FONTS: OnceLock<SystemFonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let sans = std::fs::read("/System/Library/Fonts/SFNS.ttf").ok()?;
        let mono = std::fs::read("/System/Library/Fonts/SFNSMono.ttf").ok();
        Some((Arc::new(sans), mono.map(Arc::new)))
    })
}

fn weighted(bytes: &Arc<Vec<u8>>, wght: f32) -> FontData {
    let mut fd = FontData::from_blob(bytes.clone(), 0);
    fd.tweak.coords = VariationCoords::new([(b"wght", wght)]);
    fd
}

/// The system font for Simplified Chinese (PingFang SC on macOS), found
/// through the system's own font fallback for a zh-Hans locale. The file is
/// memory-mapped, not embedded; looked up once (the first lookup waits for
/// the font list, a few hundred ms), only when the UI is Chinese.
fn cjk_font() -> Option<FontData> {
    static CJK: OnceLock<Option<FontData>> = OnceLock::new();
    CJK.get_or_init(|| {
        use egui::epaint::text::{FallbackRequest, FontProvider};
        let provider =
            egui_system_font_provider::SystemFontProvider::new().with_locale(Some("zh-Hans"));
        let family = FontFamily::Proportional;
        let found = provider.font_for(&FallbackRequest {
            cluster: "简",
            family: &family,
        });
        if let Some(f) = &found {
            log::info!("Chinese UI font: {}", f.name);
        }
        found.map(|f| f.data)
    })
    .clone()
}

static PHOSPHOR: &[u8] = include_bytes!("../assets/fonts/Phosphor.ttf");
static PHOSPHOR_FILL: &[u8] = include_bytes!("../assets/fonts/Phosphor-Fill.ttf");

pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let base_prop = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let base_mono = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    fonts
        .font_data
        .insert("phosphor".into(), Arc::new(FontData::from_static(PHOSPHOR)));
    fonts.font_data.insert(
        "phosphor-fill".into(),
        Arc::new(FontData::from_static(PHOSPHOR_FILL)),
    );

    let (regular, medium, semibold, mono): (Vec<String>, Vec<String>, Vec<String>, Vec<String>) =
        match system_fonts() {
            Some((sans, mono)) => {
                fonts
                    .font_data
                    .insert("sf".into(), Arc::new(weighted(sans, 400.0)));
                fonts
                    .font_data
                    .insert("sf-medium".into(), Arc::new(weighted(sans, 510.0)));
                fonts
                    .font_data
                    .insert("sf-semibold".into(), Arc::new(weighted(sans, 600.0)));
                let mono_key = mono.as_ref().map(|m| {
                    fonts.font_data.insert(
                        "sf-mono".into(),
                        Arc::new(FontData::from_blob(m.clone(), 0)),
                    );
                    "sf-mono".to_string()
                });
                (
                    vec!["sf".into()],
                    vec!["sf-medium".into()],
                    vec!["sf-semibold".into()],
                    mono_key.into_iter().collect(),
                )
            }
            None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        };

    // Chinese UI: the system's Simplified Chinese font (PingFang SC) right
    // after SF, so Han characters never fall back to a Japanese font (the
    // system fallback follows the macOS locale, not Manta's language).
    let (mut regular, mut medium, mut semibold, mut mono) = (regular, medium, semibold, mono);
    if crate::i18n::locale() == crate::i18n::Locale::Zh {
        if let Some(cjk) = cjk_font() {
            fonts.font_data.insert("cjk".into(), Arc::new(cjk));
            for chain in [&mut regular, &mut medium, &mut semibold, &mut mono] {
                chain.push("cjk".into());
            }
        }
    }

    // Text font first, then Phosphor (so icons mix into labels), then egui's
    // defaults as fallback.
    let chain = |first: Vec<String>, rest: &[String]| -> Vec<String> {
        let mut v = first;
        v.push("phosphor".into());
        v.extend(rest.iter().cloned());
        v
    };
    fonts
        .families
        .insert(FontFamily::Proportional, chain(regular, &base_prop));
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), chain(medium, &base_prop));
    fonts.families.insert(
        FontFamily::Name(SEMIBOLD.into()),
        chain(semibold, &base_prop),
    );
    fonts
        .families
        .insert(FontFamily::Monospace, chain(mono, &base_mono));
    let mut icons = vec!["phosphor".to_string()];
    icons.extend(base_prop.iter().cloned());
    fonts.families.insert(FontFamily::Name(ICONS.into()), icons);
    let mut fill = vec!["phosphor-fill".to_string(), "phosphor".to_string()];
    fill.extend(base_prop.iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(ICONS_FILL.into()), fill);
    fonts
}

// ------------------------------------------------------------------ install

fn widget_visuals(v: &mut Visuals, p: &Palette) {
    let w = &mut v.widgets;
    let r = CornerRadius::same(radius::CONTROL);
    let control_fill = if p.dark {
        rgb(66, 66, 70)
    } else {
        rgb(255, 255, 255)
    };

    w.noninteractive.bg_fill = p.content;
    w.noninteractive.weak_bg_fill = p.content;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = control_fill;
    w.inactive.weak_bg_fill = control_fill;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = if p.dark {
        rgb(78, 78, 83)
    } else {
        rgb(246, 246, 248)
    };
    w.hovered.weak_bg_fill = w.hovered.bg_fill;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = if p.dark {
        rgb(92, 92, 98)
    } else {
        rgb(232, 232, 236)
    };
    w.active.weak_bg_fill = w.active.bg_fill;
    w.active.bg_stroke = Stroke::new(1.0, p.border);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open = w.active;

    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.text_on_accent);
    v.hyperlink_color = p.accent_text;
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = if p.dark {
        rgb(28, 28, 30)
    } else {
        rgb(255, 255, 255)
    };
    v.text_edit_bg_color = Some(v.extreme_bg_color);
    v.code_bg_color = p.stripe;
    v.window_fill = p.elevated;
    v.panel_fill = p.chrome;
    v.window_stroke = Stroke::new(1.0, p.separator);
    v.window_corner_radius = CornerRadius::same(radius::WINDOW);
    v.menu_corner_radius = CornerRadius::same(radius::PANEL);
    v.window_shadow = egui::Shadow {
        offset: [0, 12],
        blur: 36,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 140 } else { 60 }),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }),
    };
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_secondary);
    v.override_text_color = None;
    v.striped = true;
    v.indent_has_left_vline = false;
    v.slider_trailing_fill = true;
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
    for theme in [Theme::Light, Theme::Dark] {
        let p = Palette::of(theme == Theme::Dark);
        ctx.style_mut_of(theme, |style| {
            use egui::TextStyle as T;
            style.text_styles = [
                (T::Small, regular(text::CAPTION)),
                (T::Body, regular(text::BODY)),
                (T::Button, regular(text::BODY)),
                (T::Heading, semibold(text::HEADLINE)),
                (T::Monospace, mono(text::SMALL)),
            ]
            .into();
            let s = &mut style.spacing;
            s.item_spacing = egui::vec2(space::MD, space::SM);
            s.button_padding = egui::vec2(space::MD + 2.0, space::XS);
            s.interact_size.y = 24.0;
            s.window_margin = Margin::same(space::XL as i8);
            s.menu_margin = Margin::same(space::SM as i8);
            s.combo_height = 300.0;
            s.icon_width = 14.0;
            s.icon_width_inner = 8.0;
            s.scroll = egui::style::ScrollStyle::floating();
            widget_visuals(&mut style.visuals, p);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Color32) -> f64 {
        let f = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn compare_labels_name_a_and_b_with_three_panes() {
        use manta_ops::CompareStatus as C;
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            assert_eq!(compare_label_for(C::OnlyLeft, false), "Nur links");
            assert_eq!(compare_label_for(C::OnlyRight, false), "Nur rechts");
            assert_eq!(compare_label_for(C::OnlyLeft, true), "Nur in A");
            assert_eq!(compare_label_for(C::OnlyRight, true), "Nur in B");
            assert_eq!(compare_label_for(C::LeftNewer, true), "A neuer");
            assert_eq!(compare_label_for(C::RightNewer, true), "B neuer");
            assert_eq!(compare_label_for(C::Identical, true), "Identisch");
        });
        crate::i18n::with_locale(crate::i18n::Locale::En, || {
            assert_eq!(compare_label_for(C::OnlyLeft, true), "Only in A");
            assert_eq!(compare_label_for(C::RightNewer, false), "Right newer");
        });
    }

    #[test]
    fn text_tokens_meet_wcag_aa() {
        let all: Vec<&Palette> = Accent::ALL
            .iter()
            .flat_map(|&a| {
                [
                    Palette::with_accent(false, a),
                    Palette::with_accent(true, a),
                ]
            })
            .collect();
        for p in all {
            assert!(
                contrast(p.text_on_accent, p.accent) >= 4.5,
                "text on accent {:?} {:.2} (dark={})",
                p.accent,
                contrast(p.text_on_accent, p.accent),
                p.dark
            );
            for bg in [p.content, p.stripe, p.sidebar, p.chrome, p.elevated] {
                assert!(
                    contrast(p.text, bg) >= 7.0,
                    "text on {bg:?} (dark={})",
                    p.dark
                );
                assert!(
                    contrast(p.text_secondary, bg) >= 4.5,
                    "secondary on {bg:?} (dark={})",
                    p.dark
                );
            }
            assert!(
                contrast(p.text_on_accent, p.selection) >= 4.5,
                "selection text (dark={})",
                p.dark
            );
            assert!(
                contrast(p.text, p.selection_inactive) >= 4.5,
                "inactive selection (dark={})",
                p.dark
            );
            assert!(
                contrast(p.accent_text, p.content) >= 4.5
                    && contrast(p.accent_text, p.elevated) >= 4.5,
                "accent text {:?} (dark={})",
                p.accent_text,
                p.dark
            );
            assert!(
                contrast(p.danger, p.content) >= 4.5,
                "danger (dark={})",
                p.dark
            );
            assert!(
                contrast(p.danger, p.danger_soft) >= 4.5,
                "danger on soft (dark={})",
                p.dark
            );
            assert!(
                contrast(p.success, p.content) >= 4.5,
                "success (dark={})",
                p.dark
            );
            assert!(
                contrast(p.warning, p.content) >= 4.5,
                "warning (dark={})",
                p.dark
            );
            use manta_plugin::Tone as T;
            for tone in [
                T::Neutral,
                T::Accent,
                T::Success,
                T::Warning,
                T::Danger,
                T::Info,
            ] {
                let (fg, bg) = tone_colors(p, tone);
                assert!(
                    contrast(fg, bg) >= 4.5,
                    "{tone:?} badge {:.2} (dark={})",
                    contrast(fg, bg),
                    p.dark
                );
                assert!(
                    contrast(tone_text(p, tone), p.content) >= 4.5,
                    "{tone:?} text (dark={})",
                    p.dark
                );
            }
            // Non-text UI (icons, focus ring) needs 3:1.
            assert!(
                contrast(p.focus_ring, p.content) >= 3.0,
                "focus ring {:?} (dark={})",
                p.focus_ring,
                p.dark
            );
        }
    }
}
