//! Drawing plugin contributions in Manta's design language: decoration
//! badges, path-bar chips, the declarative block renderer for external
//! plugin panels, and the panel title bar.

use eframe::egui::{self, Color32, CornerRadius, Rect, RichText, Sense, Stroke, Ui};
use manta_plugin::protocol::Block;
use manta_plugin::{Badge, Chip, Tone};

use crate::icons;
use crate::theme::{self, pal, radius, space, text, Palette};
use crate::widgets::{self, ButtonKind};

/// Phosphor glyph for a chip / list-row icon name (unknown names: none).
pub fn named_glyph(name: &str) -> Option<&'static str> {
    Some(match name {
        "branch" => icons::GIT_BRANCH,
        "terminal" => icons::TERMINAL_WINDOW,
        "info" => icons::INFO,
        "warning" => icons::WARNING,
        "check" => icons::CHECK_CIRCLE,
        "cloud" => icons::CLOUD,
        "plug" => icons::PLUG,
        "file" => icons::FILE,
        "folder" => icons::FOLDER_SIMPLE,
        "code" => icons::CODE,
        "image" => icons::FILE_IMAGE,
        "archive" => icons::FILE_ARCHIVE,
        "text" => icons::FILE_TEXT,
        "copy" => icons::COPY,
        "link" => icons::LINK,
        "eye" => icons::EYE,
        "key" => icons::KEY,
        "lock" => icons::LOCK_SIMPLE,
        "clock" => icons::CLOCK,
        "stop" => icons::STOP,
        "error" => icons::X_CIRCLE,
        "package" => icons::PACKAGE,
        "search" => icons::MAGNIFYING_GLASS,
        "gear" => icons::GEAR_SIX,
        "pencil" => icons::PENCIL_SIMPLE,
        "download" => icons::DOWNLOAD_SIMPLE,
        "open" => icons::ARROW_SQUARE_OUT,
        "play" => icons::PLAY,
        "database" => icons::DATABASE,
        "cube" => icons::CUBE,
        "stack" => icons::STACK,
        "hammer" => icons::HAMMER,
        "flask" => icons::FLASK,
        "rocket" => icons::ROCKET_LAUNCH,
        "lightning" => icons::LIGHTNING,
        "list" => icons::LIST_BULLETS,
        "wrench" => icons::WRENCH,
        "bug" => icons::BUG,
        "trash" => icons::TRASH,
        "globe" => icons::GLOBE,
        "refresh" => icons::ARROW_CLOCKWISE,
        "cpu" => icons::CPU,
        "gpu" => icons::GRAPHICS_CARD,
        "memory" => icons::MEMORY,
        _ => return None,
    })
}

/// Icon for a plugin in Settings and panel title bars.
pub fn plugin_glyph(id: &str, builtin: bool) -> &'static str {
    match id {
        "manta.git" => icons::GIT_BRANCH,
        "manta.terminal" => icons::TERMINAL_WINDOW,
        "manta.markdown" => icons::FILE_TEXT,
        "manta.quickactions" => icons::LIGHTNING,
        "manta.projectinfo" => icons::ROCKET_LAUNCH,
        "manta.duplicates" => icons::COPY,
        "manta.imagetools" => icons::FILE_IMAGE,
        "manta.sysstats" => icons::CPU,
        _ if builtin => icons::PLUG,
        _ => icons::PUZZLE_PIECE,
    }
}

// ------------------------------------------------------------------ badges

pub const BADGE_H: f32 = 15.0;
const BADGE_GAP: f32 = 3.0;

fn badge_font() -> egui::FontId {
    theme::semibold(10.0)
}

fn badge_galley(ui: &Ui, b: &Badge, color: Color32) -> std::sync::Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(b.text.clone(), badge_font(), color)
}

/// Total width of a row of badges (0 if none).
pub fn badges_width(ui: &Ui, badges: &[Badge]) -> f32 {
    if badges.is_empty() {
        return 0.0;
    }
    badges
        .iter()
        .map(|b| (badge_galley(ui, b, Color32::WHITE).size().x + 8.0).max(BADGE_H))
        .sum::<f32>()
        + BADGE_GAP * (badges.len() - 1) as f32
}

/// Paint badges left to right starting at `x`, vertically centred on `cy`.
/// Returns the rect they cover.
pub fn paint_badges(ui: &Ui, x: f32, cy: f32, badges: &[Badge], on_accent: bool, dim: f32) -> Rect {
    let p = pal(ui);
    let mut x = x;
    let start = x;
    for b in badges {
        let (mut fg, mut bg) = theme::tone_colors(p, b.tone);
        if on_accent {
            fg = p.text_on_accent;
            bg = Color32::from_white_alpha(52);
        } else if b.tone == Tone::Muted {
            bg = p.neutral_soft;
        }
        fg = fg.gamma_multiply(dim);
        bg = bg.gamma_multiply(dim);
        let g = badge_galley(ui, b, fg);
        let w = (g.size().x + 8.0).max(BADGE_H);
        let r = Rect::from_min_size(egui::pos2(x, cy - BADGE_H / 2.0), egui::vec2(w, BADGE_H));
        ui.painter()
            .rect_filled(r, CornerRadius::same(radius::CONTROL - 1), bg);
        ui.painter().galley(r.center() - g.size() / 2.0, g, fg);
        x += w + BADGE_GAP;
    }
    Rect::from_min_max(
        egui::pos2(start, cy - BADGE_H / 2.0),
        egui::pos2((x - BADGE_GAP).max(start), cy + BADGE_H / 2.0),
    )
}

// ------------------------------------------------------------------ chips

pub const CHIP_H: f32 = 20.0;

fn chip_font() -> egui::FontId {
    theme::medium(text::SMALL)
}

/// Width of a path-bar chip at full size.
pub fn chip_width(ui: &Ui, chip: &Chip) -> f32 {
    let g = ui
        .painter()
        .layout_no_wrap(chip.text.clone(), chip_font(), Color32::WHITE);
    let icon = chip
        .icon
        .as_deref()
        .and_then(named_glyph)
        .map_or(0.0, |_| 16.0);
    (g.size().x + icon + 2.0 * space::MD).min(CHIP_MAX_W)
}

/// Widest a chip gets; longer text is cut with "…".
const CHIP_MAX_W: f32 = 180.0;
/// Width of an icon-only chip (a circle).
pub const CHIP_ICON_W: f32 = CHIP_H;

/// The narrowest a chip with text can be: its icon and about three
/// characters plus "…" (never more than its full width).
pub fn chip_min_text_width(ui: &Ui, chip: &Chip) -> f32 {
    let icon = chip
        .icon
        .as_deref()
        .and_then(named_glyph)
        .map_or(0.0, |_| 16.0);
    let g = ui
        .painter()
        .layout_no_wrap("Mai…".into(), chip_font(), Color32::WHITE);
    (g.size().x + icon + 2.0 * space::MD).min(chip_width(ui, chip))
}

/// How a path-bar chip is drawn when space is short.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChipForm {
    /// Icon and full text.
    Full,
    /// Icon and text cut with "…" to this total width.
    Truncated(f32),
    /// Just the icon (or the text's first letter); the text is in the
    /// tooltip.
    Icon,
    /// Not even the icon fits.
    Hidden,
}

impl ChipForm {
    pub fn width(self, full: f32) -> f32 {
        match self {
            ChipForm::Full => full,
            ChipForm::Truncated(w) => w,
            ChipForm::Icon => CHIP_ICON_W,
            ChipForm::Hidden => 0.0,
        }
    }
}

/// Fit chips (left to right; `full[i]` / `min_text[i]` their full and
/// narrowest text widths) into `avail` points, `gap` after each chip.
/// Degrades gracefully: first the text of the widest chips is cut (all
/// chips shrink towards the same width), then chips turn into icons from
/// the left (the rightmost chip keeps its text longest), and only when not
/// even the icons fit are chips hidden, again from the left.
pub fn layout_chips(full: &[f32], min_text: &[f32], gap: f32, avail: f32) -> Vec<ChipForm> {
    let n = full.len();
    let gaps = gap * n as f32;
    if full.iter().sum::<f32>() + gaps <= avail {
        return vec![ChipForm::Full; n];
    }
    // `k` chips from the left as icons, the rest with (cut) text.
    for k in 0..n {
        let rest_avail = avail - gaps - CHIP_ICON_W * k as f32;
        let mins: f32 = (k..n).map(|i| min_text[i].min(full[i])).sum();
        if mins > rest_avail {
            continue;
        }
        // Water-filling: the widest chips give up width first, down to a
        // common cap `c`, never below their minimum.
        let width_at = |c: f32| -> f32 { (k..n).map(|i| full[i].min(c.max(min_text[i]))).sum() };
        let (mut lo, mut hi) = (0.0_f32, full[k..].iter().cloned().fold(0.0, f32::max));
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if width_at(mid) <= rest_avail {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return (0..n)
            .map(|i| {
                if i < k {
                    return ChipForm::Icon;
                }
                let w = full[i].min(lo.max(min_text[i]));
                if w >= full[i] - 0.5 {
                    ChipForm::Full
                } else {
                    ChipForm::Truncated(w.floor())
                }
            })
            .collect();
    }
    // Icons only; hide from the left until they fit.
    let mut forms = vec![ChipForm::Icon; n];
    let mut used = (CHIP_ICON_W + gap) * n as f32;
    for f in forms.iter_mut() {
        if used <= avail {
            break;
        }
        *f = ChipForm::Hidden;
        used -= CHIP_ICON_W + gap;
    }
    forms
}

/// Tooltip of a chip drawn in `form`: the full text when it's cut or only
/// the icon shows, plus the plugin's own tooltip.
pub fn chip_tooltip(chip: &Chip, form: ChipForm) -> Option<String> {
    let cut = matches!(form, ChipForm::Truncated(_) | ChipForm::Icon);
    match (&chip.tooltip, cut) {
        (Some(t), true) if !t.contains(chip.text.as_str()) => Some(format!("{}\n{t}", chip.text)),
        (Some(t), _) => Some(t.clone()),
        (None, true) => Some(chip.text.clone()),
        (None, false) => None,
    }
}

/// Paint a chip in `rect`; its text sits on `baseline` (the bar's shared
/// baseline). A rect of [`CHIP_ICON_W`] or less draws just the icon (or
/// the text's first letter for a chip without one).
pub fn paint_chip(ui: &Ui, rect: Rect, chip: &Chip, hovered: bool, baseline: f32) {
    let p = pal(ui);
    let (fg, mut bg) = theme::tone_colors(p, chip.tone);
    if chip.tone == Tone::Muted {
        bg = p.neutral_soft;
    }
    let r = CornerRadius::same(radius::PILL);
    ui.painter().rect_filled(rect, r, bg);
    if hovered {
        ui.painter().rect_filled(rect, r, p.hover);
        ui.painter().rect_stroke(
            rect,
            r,
            Stroke::new(1.0, fg.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
    }
    let glyph = chip.icon.as_deref().and_then(named_glyph);
    if rect.width() <= CHIP_ICON_W + 0.5 {
        match glyph {
            Some(g) => widgets::paint_icon(ui, rect, g, 12.0, fg, false),
            None => {
                let letter: String = chip.text.chars().take(1).collect::<String>().to_uppercase();
                let g = ui.painter().layout_no_wrap(letter, chip_font(), fg);
                widgets::paint_on_baseline(ui, rect.center().x - g.size().x / 2.0, baseline, g, fg);
            }
        }
        return;
    }
    let mut x = rect.left() + space::MD;
    if let Some(g) = glyph {
        let ir = Rect::from_min_size(
            egui::pos2(x - 2.0, rect.top()),
            egui::vec2(16.0, rect.height()),
        );
        widgets::paint_icon(ui, ir, g, 12.0, fg, false);
        x += 16.0;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(chip.text.clone(), chip_font(), fg);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - space::MD - x).max(8.0));
    let g = ui.painter().layout_job(job);
    widgets::paint_on_baseline(ui, x, baseline, g, fg);
}

// ------------------------------------------------------------------ panels

pub const TITLE_BAR_H: f32 = 28.0;

/// One tab of a panel title bar.
pub struct TitleTab<'a> {
    pub title: &'a str,
    pub glyph: &'static str,
    /// Shown as tooltip ("Hello Python").
    pub plugin_name: &'a str,
}

#[derive(Default)]
pub struct TitleBarOutcome {
    pub selected: Option<usize>,
    pub close: bool,
}

/// Thin title bar: icon + title, or tabs when several panels share the
/// area; a close button on the right when `closable` (docked panels; modal
/// sheets have none and close with Esc or their own buttons, like macOS
/// sheets).
pub fn title_bar(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    tabs: &[TitleTab<'_>],
    selected: usize,
    top_radius: u8,
    closable: bool,
) -> TitleBarOutcome {
    let p = pal(ui);
    let mut out = TitleBarOutcome::default();
    let corners = CornerRadius {
        nw: top_radius,
        ne: top_radius,
        sw: 0,
        se: 0,
    };
    ui.painter().rect_filled(rect, corners, p.stripe);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.separator),
    );
    let cy = rect.center().y;
    let mut right_limit = rect.right() - space::SM;
    if closable {
        let close =
            Rect::from_center_size(egui::pos2(rect.right() - 16.0, cy), egui::vec2(20.0, 20.0));
        let c = ui.interact(close, id.with("close"), Sense::click());
        if c.hovered() {
            ui.painter()
                .rect_filled(close, CornerRadius::same(radius::CONTROL), p.hover);
        }
        widgets::paint_icon(
            ui,
            close,
            icons::X,
            12.0,
            if c.hovered() {
                p.text
            } else {
                p.text_secondary
            },
            false,
        );
        let title = tabs.get(selected).map_or("", |t| t.title);
        if widgets::tooltip(c, &t!("plugin_ui.close_panel", title = title), "").clicked() {
            out.close = true;
        }
        right_limit = close.left() - space::SM;
    }

    let mut x = rect.left() + space::SM;
    for (i, t) in tabs.iter().enumerate() {
        let on = i == selected;
        let font = if on {
            theme::medium(text::SMALL)
        } else {
            theme::regular(text::SMALL)
        };
        let color = if on { p.text } else { p.text_secondary };
        let g = ui
            .painter()
            .layout_no_wrap(t.title.to_string(), font.clone(), color);
        let w = (g.size().x + 16.0 + 2.0 * space::MD).min(right_limit - x);
        if w < 40.0 {
            break;
        }
        let r = Rect::from_min_size(egui::pos2(x, cy - 10.0), egui::vec2(w, 20.0));
        if tabs.len() > 1 {
            let resp = ui.interact(r, id.with(("tab", i)), Sense::click());
            if on {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(radius::CONTROL), p.content);
                ui.painter().rect_stroke(
                    r,
                    CornerRadius::same(radius::CONTROL),
                    Stroke::new(1.0, p.separator),
                    egui::StrokeKind::Inside,
                );
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(radius::CONTROL), p.hover);
            }
            if resp.on_hover_text(t.plugin_name).clicked() {
                out.selected = Some(i);
            }
        } else {
            ui.interact(r, id.with(("tab", i)), Sense::hover())
                .on_hover_text(t.plugin_name);
        }
        let ir = Rect::from_min_size(
            egui::pos2(r.left() + space::SM - 2.0, r.top()),
            egui::vec2(16.0, r.height()),
        );
        widgets::paint_icon(
            ui,
            ir,
            t.glyph,
            13.0,
            if on {
                p.text_secondary
            } else {
                p.text_tertiary
            },
            false,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(t.title.to_string(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(
            (r.right() - ir.right() - space::SM).max(8.0),
        );
        let g = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(ir.right() + 2.0, cy - g.size().y / 2.0),
            g,
            color,
        );
        x = r.right() + space::XS;
    }
    out
}

/// A horizontal drag handle along the top edge of `rect`. Returns the
/// vertical drag delta and whether a drag just ended.
pub fn resize_handle_top(ui: &mut Ui, rect: Rect, id: egui::Id) -> (f32, bool) {
    let p = pal(ui);
    let h = Rect::from_x_y_ranges(rect.x_range(), (rect.top() - 3.0)..=(rect.top() + 3.0));
    let resp = ui.interact(h, id, Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        ui.painter().rect_filled(
            Rect::from_center_size(
                egui::pos2(rect.center().x, rect.top()),
                egui::vec2(36.0, 3.0),
            ),
            CornerRadius::same(2),
            p.border,
        );
    }
    let resp = widgets::tooltip(resp, t!("plugin_ui.drag_resize"), "");
    (resp.drag_delta().y, resp.drag_stopped())
}

// ------------------------------------------------------------------ blocks

const LIST_ROW_H: f32 = 26.0;

/// Render declarative panel blocks. A clicked row/button action is
/// written to `clicked`.
pub fn blocks_ui(ui: &mut Ui, blocks: &[Block], clicked: &mut Option<String>) {
    let p = pal(ui);
    ui.spacing_mut().item_spacing.y = space::SM;
    for (bi, b) in blocks.iter().enumerate() {
        ui.push_id(bi, |ui| match b {
            Block::Heading { text: t } => {
                ui.add(
                    egui::Label::new(
                        RichText::new(t)
                            .font(theme::semibold(text::TITLE - 1.0))
                            .color(p.text),
                    )
                    .wrap(),
                );
            }
            Block::Text { text: t, tone } => {
                let color = match tone {
                    Tone::Muted => p.text_secondary,
                    t => theme::tone_text(p, *t),
                };
                ui.add(egui::Label::new(RichText::new(t).color(color)).wrap());
            }
            Block::Code { text: t } => code_block(ui, p, t),
            Block::KeyValue { rows } => key_value(ui, p, rows),
            Block::List { rows } => list(ui, p, rows, clicked),
            Block::Buttons { buttons } => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
                    for (i, b) in buttons.iter().enumerate() {
                        let kind = if b.primary {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Secondary
                        };
                        let r = ui
                            .push_id(i, |ui| widgets::button(ui, &b.label, kind, true))
                            .inner;
                        if r.clicked() {
                            *clicked = Some(b.action.clone());
                        }
                    }
                });
            }
            Block::Separator => {
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter()
                    .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.separator));
            }
            Block::Section { text: t, detail } => section(ui, p, t, detail.as_deref(), bi > 0),
            Block::Tags { tags } => tags_row(ui, p, tags),
            Block::Image {
                path,
                max_height,
                caption,
            } => local_image::block(
                ui,
                p,
                path,
                max_height.unwrap_or(local_image::DEFAULT_HEIGHT),
                caption.as_deref(),
            ),
        });
    }
}

/// A group caption: small semibold secondary text, optional detail on the
/// right. Gets a little extra air above unless it is the first block.
fn section(ui: &mut Ui, p: &Palette, t: &str, detail: Option<&str>, gap: bool) {
    if gap {
        ui.add_space(space::XS);
    }
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), Sense::hover());
    let mut right = rect.right() - space::XS;
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        let mut job = egui::text::LayoutJob::simple_singleline(
            d.to_string(),
            theme::regular(text::SMALL),
            p.text_tertiary,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(w * 0.5);
        let g = ui.painter().layout_job(job);
        let pos = egui::pos2(right - g.size().x, rect.center().y - g.size().y / 2.0);
        right = pos.x - space::MD;
        ui.painter().galley(pos, g, p.text_tertiary);
    }
    let mut job = egui::text::LayoutJob::simple_singleline(
        t.to_string(),
        theme::semibold(text::SMALL),
        p.text_secondary,
    );
    job.wrap =
        egui::text::TextWrapping::truncate_at_width((right - rect.left() - space::XS).max(10.0));
    let g = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left() + space::XS, rect.center().y - g.size().y / 2.0),
        g,
        p.text_secondary,
    );
}

const TAG_H: f32 = 19.0;

/// Small pills in a wrapping row.
fn tags_row(ui: &mut Ui, p: &Palette, tags: &[manta_plugin::protocol::Tag]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::XS + 1.0, space::XS + 1.0);
        let font = theme::medium(text::SMALL - 0.5);
        for t in tags {
            let (fg, mut bg) = theme::tone_colors(p, t.tone);
            if t.tone == Tone::Muted || t.tone == Tone::Neutral {
                bg = p.neutral_soft;
            }
            let fg = if t.tone == Tone::Muted {
                p.text_secondary
            } else {
                fg
            };
            let glyph = t.icon.as_deref().and_then(named_glyph);
            let g = ui
                .painter()
                .layout_no_wrap(t.text.clone(), font.clone(), fg);
            let icon_w = if glyph.is_some() { 14.0 } else { 0.0 };
            let w = (g.size().x + icon_w + 2.0 * space::SM).min(ui.available_width().max(40.0));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, TAG_H), Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::PILL), bg);
            let mut x = rect.left() + space::SM;
            if let Some(gl) = glyph {
                let ir = Rect::from_min_size(
                    egui::pos2(x - 2.0, rect.top()),
                    egui::vec2(14.0, rect.height()),
                );
                widgets::paint_icon(ui, ir, gl, 11.0, fg, false);
                x += icon_w;
            }
            ui.painter()
                .galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        }
    });
}

fn code_block(ui: &mut Ui, p: &Palette, t: &str) {
    egui::Frame::new()
        .fill(p.stripe)
        .stroke(Stroke::new(1.0, p.separator))
        .corner_radius(CornerRadius::same(radius::PANEL - 2))
        .inner_margin(egui::Margin::symmetric(
            space::MD as i8 + 2,
            space::SM as i8 + 1,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(t)
                        .font(theme::mono(text::SMALL))
                        .color(p.text),
                )
                .wrap()
                .selectable(true),
            );
        });
}

fn key_value(ui: &mut Ui, p: &Palette, rows: &[manta_plugin::protocol::KeyValueRow]) {
    let font = theme::regular(text::SMALL + 0.5);
    // The key column fits the longest key (measured), up to 45 % of the
    // block; longer keys wrap onto a second line instead of being cut.
    let longest_key = rows
        .iter()
        .map(|r| {
            ui.painter()
                .layout_no_wrap(r.key.clone(), font.clone(), p.text_secondary)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let inner_w = ui.available_width() - 2.0 * (space::MD + 2.0);
    let key_w = (longest_key + 1.0).min(inner_w * 0.45) + space::MD;
    egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, p.separator))
        .corner_radius(CornerRadius::same(radius::PANEL - 2))
        .inner_margin(egui::Margin::symmetric(
            space::MD as i8 + 2,
            space::SM as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XS;
            for (i, r) in rows.iter().enumerate() {
                if i > 0 {
                    let y = ui.cursor().top() - space::XXS;
                    ui.painter().hline(
                        ui.max_rect().x_range(),
                        y,
                        Stroke::new(1.0, p.separator.gamma_multiply(0.6)),
                    );
                }
                ui.horizontal_top(|ui| {
                    let g = ui.painter().layout(
                        r.key.clone(),
                        font.clone(),
                        p.text_secondary,
                        key_w - space::MD,
                    );
                    let (kr, _) = ui.allocate_exact_size(
                        egui::vec2(key_w, (g.size().y + 1.0).max(17.0)),
                        Sense::hover(),
                    );
                    ui.painter()
                        .galley(kr.left_top() + egui::vec2(0.0, 1.0), g, p.text_secondary);
                    let color = theme::tone_text(p, r.tone);
                    ui.add(
                        egui::Label::new(RichText::new(&r.value).font(font.clone()).color(color))
                            .wrap()
                            .selectable(true),
                    );
                });
            }
        });
}

fn list(
    ui: &mut Ui,
    p: &Palette,
    rows: &[manta_plugin::protocol::ListRow],
    clicked: &mut Option<String>,
) {
    egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, p.separator))
        .corner_radius(CornerRadius::same(radius::PANEL - 2))
        .inner_margin(egui::Margin::same(space::XS as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, r) in rows.iter().enumerate() {
                let sense = if r.action.is_some() {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), LIST_ROW_H), sense);
                let resp = resp.on_hover_cursor(if r.action.is_some() {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                });
                let resp = match &r.tooltip {
                    Some(t) => resp.on_hover_text(t),
                    None => resp,
                };
                if r.action.is_some() && resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(radius::ROW), p.hover);
                }
                let color = theme::tone_text(p, r.tone);
                let mut x = rect.left() + space::SM;
                if let Some(g) = r.icon.as_deref().and_then(named_glyph) {
                    let ir = Rect::from_min_size(
                        egui::pos2(x, rect.top()),
                        egui::vec2(18.0, rect.height()),
                    );
                    let ic = if r.tone == Tone::Neutral {
                        p.text_secondary
                    } else {
                        color
                    };
                    widgets::paint_icon(ui, ir, g, 14.0, ic, false);
                    x = ir.right() + space::XS;
                }
                let detail_w = r.detail.as_ref().map_or(0.0, |d| {
                    ui.painter()
                        .layout_no_wrap(d.clone(), theme::regular(text::SMALL), p.text_secondary)
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
                let mut job = egui::text::LayoutJob::simple_singleline(
                    r.text.clone(),
                    theme::regular(text::BODY),
                    color,
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width((text_right - x).max(10.0));
                let g = ui.painter().layout_job(job);
                ui.painter()
                    .galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, color);
                if let Some(d) = &r.detail {
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        d.clone(),
                        theme::regular(text::SMALL),
                        p.text_secondary,
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(detail_w + 1.0);
                    let g = ui.painter().layout_job(job);
                    ui.painter().galley(
                        egui::pos2(
                            rect.right() - space::SM - g.size().x,
                            rect.center().y - g.size().y / 2.0,
                        ),
                        g,
                        p.text_secondary,
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
                    *clicked = r.action.clone();
                }
            }
        });
}

/// `image` blocks: a local file decoded in the background by an egui image
/// loader of our own (`file://…?mtime=…` URIs), capped in size, turned by
/// its EXIF orientation and reloaded when the file's mtime changes.
pub mod local_image {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::task::Poll;
    use std::time::{Duration, Instant, UNIX_EPOCH};

    use eframe::egui::{self, load, ColorImage, CornerRadius, Rect, Sense, Stroke, Ui};

    use crate::theme::{self, radius, space, text, Palette};

    pub const DEFAULT_HEIGHT: f32 = 200.0;
    /// Longest edge (pixels) an image is decoded at.
    pub const MAX_EDGE: u32 = 1024;
    /// Larger files are not read.
    const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
    /// How often the file's mtime is checked while it is shown.
    const RECHECK: Duration = Duration::from_millis(1500);
    /// Marks our URIs (other `file://` URIs are left to other loaders).
    const MARK: &str = "?manta-mtime=";

    /// The URI of `path` at modification time `mtime` (nanoseconds).
    pub fn uri(path: &Path, mtime: u128) -> String {
        format!("file://{}{MARK}{mtime}", path.display())
    }

    /// The path of one of our URIs.
    pub fn path_of(uri: &str) -> Option<PathBuf> {
        let rest = uri.strip_prefix("file://")?;
        let (path, _mtime) = rest.rsplit_once(MARK)?;
        Some(PathBuf::from(path))
    }

    // ------------------------------------------------------------ mtime

    #[derive(Clone, Copy)]
    struct Stat {
        /// `None`: the file is missing or unreadable.
        mtime: Option<u128>,
        /// At least one check has finished.
        known: bool,
        checked: Instant,
        busy: bool,
    }

    fn stats() -> &'static Mutex<HashMap<PathBuf, Stat>> {
        static S: OnceLock<Mutex<HashMap<PathBuf, Stat>>> = OnceLock::new();
        S.get_or_init(Default::default)
    }

    fn mtime_of(path: &Path) -> Option<u128> {
        let m = std::fs::metadata(path).ok()?;
        if !m.is_file() {
            return None;
        }
        Some(
            m.modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos(),
        )
    }

    /// The mtime of `path`: `None` until the first check is done,
    /// `Some(None)` if the file is missing. While the image is shown it is
    /// checked again every [`RECHECK`] on a background thread; when it
    /// changes, the old image is forgotten and the new one loads.
    fn current_mtime(ctx: &egui::Context, path: &Path) -> Option<Option<u128>> {
        let mut map = stats().lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.get(path).copied();
        let due = entry.is_none_or(|s| !s.busy && s.checked.elapsed() >= RECHECK);
        if due {
            let prev = entry.filter(|s| s.known).map(|s| s.mtime);
            map.insert(
                path.to_path_buf(),
                Stat {
                    mtime: prev.flatten(),
                    known: prev.is_some(),
                    checked: Instant::now(),
                    busy: true,
                },
            );
            let (ctx, path) = (ctx.clone(), path.to_path_buf());
            std::thread::spawn(move || {
                let mtime = mtime_of(&path);
                stats().lock().unwrap_or_else(|e| e.into_inner()).insert(
                    path.clone(),
                    Stat {
                        mtime,
                        known: true,
                        checked: Instant::now(),
                        busy: false,
                    },
                );
                if prev != Some(mtime) {
                    if let Some(Some(old)) = prev {
                        ctx.forget_image(&uri(&path, old));
                    }
                    ctx.request_repaint();
                }
            });
        } else if let Some(s) = entry.filter(|s| !s.busy) {
            // Come back for the next check while the image is on screen.
            ctx.request_repaint_after(RECHECK.saturating_sub(s.checked.elapsed()));
        }
        entry.filter(|s| s.known).map(|s| s.mtime)
    }

    // ------------------------------------------------------------ loader

    type Entry = Poll<Result<Arc<ColorImage>, String>>;

    /// Decodes our `file://…?manta-mtime=…` URIs on a background thread
    /// with the `image` crate, scaled to fit the size hint (at most
    /// [`MAX_EDGE`] pixels).
    #[derive(Default)]
    pub struct LocalImageLoader {
        cache: Arc<Mutex<HashMap<String, Entry>>>,
    }

    impl LocalImageLoader {
        pub const ID: &'static str = concat!(module_path!(), "::LocalImageLoader");
    }

    /// Install the loader once per context.
    pub fn install(ctx: &egui::Context) {
        if !ctx.is_loader_installed(LocalImageLoader::ID) {
            ctx.add_image_loader(Arc::new(LocalImageLoader::default()));
        }
    }

    fn hint_edge(hint: load::SizeHint) -> u32 {
        let edge = match hint {
            load::SizeHint::Size { width, height, .. } => width.max(height),
            load::SizeHint::Width(w) => w,
            load::SizeHint::Height(h) => h,
            load::SizeHint::Scale(_) => MAX_EDGE,
        };
        edge.clamp(64, MAX_EDGE)
    }

    /// Read and decode `path`, fitting it into `edge` × `edge` pixels.
    pub fn decode(path: &Path, edge: u32) -> Result<ColorImage, String> {
        use image::ImageDecoder as _;
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len > MAX_FILE_BYTES {
            return Err(t!("plugin_ui.image.too_large", size = len / 1_000_000));
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
        img.apply_orientation(orientation);
        let source = egui::vec2(img.width() as f32, img.height() as f32);
        if img.width() > edge || img.height() > edge {
            img = img.thumbnail(edge, edge);
        }
        let rgba = img.to_rgba8();
        let mut out = ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        out.source_size = source;
        Ok(out)
    }

    impl load::ImageLoader for LocalImageLoader {
        fn id(&self) -> &str {
            Self::ID
        }

        fn load(
            &self,
            ctx: &egui::Context,
            uri: &str,
            size_hint: load::SizeHint,
        ) -> load::ImageLoadResult {
            let Some(path) = path_of(uri) else {
                return Err(load::LoadError::NotSupported);
            };
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            match cache.get(uri) {
                Some(Poll::Ready(Ok(img))) => {
                    return Ok(load::ImagePoll::Ready { image: img.clone() })
                }
                Some(Poll::Ready(Err(e))) => return Err(load::LoadError::Loading(e.clone())),
                Some(Poll::Pending) => return Ok(load::ImagePoll::Pending { size: None }),
                None => {}
            }
            cache.insert(uri.to_string(), Poll::Pending);
            drop(cache);
            let (cache, ctx, uri, edge) = (
                self.cache.clone(),
                ctx.clone(),
                uri.to_string(),
                hint_edge(size_hint),
            );
            std::thread::Builder::new()
                .name("plugin-image".into())
                .spawn(move || {
                    let result = decode(&path, edge).map(Arc::new);
                    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
                    // Forgotten meanwhile (file changed): drop the result.
                    if map.contains_key(&uri) {
                        map.insert(uri, Poll::Ready(result));
                    }
                    drop(map);
                    ctx.request_repaint();
                })
                .map_err(|e| load::LoadError::Loading(e.to_string()))?;
            Ok(load::ImagePoll::Pending { size: None })
        }

        fn forget(&self, uri: &str) {
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(uri);
        }

        fn forget_all(&self) {
            self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }

        fn byte_size(&self) -> usize {
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .map(|e| match e {
                    Poll::Ready(Ok(img)) => img.pixels.len() * 4,
                    _ => 0,
                })
                .sum()
        }

        fn has_pending(&self) -> bool {
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .any(|e| e.is_pending())
        }
    }

    // ------------------------------------------------------------ drawing

    /// Draw an `image` block: the picture aspect-fit into the panel's width
    /// and `max_height`, rounded, on a quiet backdrop; a caption below.
    pub fn block(ui: &mut Ui, p: &Palette, path: &str, max_height: f32, caption: Option<&str>) {
        let max_h = if max_height.is_finite() {
            max_height.clamp(24.0, 800.0)
        } else {
            DEFAULT_HEIGHT
        };
        let width = ui.available_width();
        let Some(file) = manta_plugin::protocol::image_block_path(path) else {
            note(
                ui,
                p,
                width,
                crate::icons::WARNING,
                t!("plugin_ui.image.not_local"),
            );
            return;
        };
        install(ui.ctx());
        let state = match current_mtime(ui.ctx(), &file) {
            None => Err(None),
            Some(None) => Err(Some(t!("plugin_ui.image.not_found").to_string())),
            Some(Some(m)) => {
                let uri = uri(&file, m);
                let hint = load::SizeHint::Size {
                    width: MAX_EDGE,
                    height: MAX_EDGE,
                    maintain_aspect_ratio: true,
                };
                match ui
                    .ctx()
                    .try_load_texture(&uri, egui::TextureOptions::LINEAR, hint)
                {
                    Ok(load::TexturePoll::Ready { texture }) => Ok(texture),
                    Ok(load::TexturePoll::Pending { .. }) => Err(None),
                    Err(e) => Err(Some(match e {
                        load::LoadError::Loading(m) => {
                            t!("plugin_ui.image.cant_preview", error = m)
                        }
                        other => t!("plugin_ui.image.cant_preview", error = other),
                    })),
                }
            }
        };
        match state {
            Ok(texture) => {
                // Aspect-fit, never larger than one image pixel per point.
                let size = texture.size;
                let scale = (width / size.x).min(max_h / size.y).min(1.0);
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                let shown = (size * scale).max(egui::vec2(8.0, 8.0));
                let frame_h = shown.y;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(width, frame_h), Sense::hover());
                let img_rect = Rect::from_center_size(rect.center(), shown);
                let r = CornerRadius::same(radius::PANEL - 2);
                // A checkerboard shows transparency.
                checkerboard(ui, p, img_rect, r);
                egui::Image::from_texture(texture)
                    .corner_radius(r)
                    .paint_at(ui, img_rect);
                ui.painter().rect_stroke(
                    img_rect,
                    r,
                    Stroke::new(1.0, p.separator),
                    egui::StrokeKind::Outside,
                );
                resp.on_hover_text(path);
            }
            Err(None) => {
                let h = max_h.min(120.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
                let r = CornerRadius::same(radius::PANEL - 2);
                ui.painter().rect_filled(rect, r, p.stripe);
                ui.painter().rect_stroke(
                    rect,
                    r,
                    Stroke::new(1.0, p.separator),
                    egui::StrokeKind::Inside,
                );
                egui::Spinner::new()
                    .size(16.0)
                    .color(p.text_tertiary)
                    .paint_at(
                        ui,
                        Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
                    );
            }
            Err(Some(msg)) => note(ui, p, width, crate::icons::FILE_IMAGE, &msg),
        }
        if let Some(c) = caption.filter(|c| !c.is_empty()) {
            ui.add_space(-space::XS);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(c)
                        .font(theme::regular(text::SMALL))
                        .color(p.text_secondary),
                )
                .wrap(),
            );
        }
    }

    fn checkerboard(ui: &Ui, p: &Palette, rect: Rect, r: CornerRadius) {
        ui.painter().rect_filled(rect, r, p.content);
        let cell = 8.0;
        let dark = p.neutral_soft;
        let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
        let cols = (rect.width() / cell).ceil() as i32;
        let rows = (rect.height() / cell).ceil() as i32;
        if cols * rows > 40_000 {
            return;
        }
        for y in 0..rows {
            for x in 0..cols {
                if (x + y) % 2 == 0 {
                    let min = rect.min + egui::vec2(x as f32 * cell, y as f32 * cell);
                    painter.rect_filled(
                        Rect::from_min_size(min, egui::vec2(cell, cell)),
                        0.0,
                        dark,
                    );
                }
            }
        }
    }

    fn note(ui: &mut Ui, p: &Palette, width: f32, glyph: &str, msg: &str) {
        let h = 44.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
        let r = CornerRadius::same(radius::PANEL - 2);
        ui.painter().rect_filled(rect, r, p.stripe);
        ui.painter().rect_stroke(
            rect,
            r,
            Stroke::new(1.0, p.separator),
            egui::StrokeKind::Inside,
        );
        let ir = Rect::from_min_size(
            egui::pos2(rect.left() + space::MD, rect.top()),
            egui::vec2(18.0, h),
        );
        crate::widgets::paint_icon(ui, ir, glyph, 14.0, p.text_tertiary, false);
        let mut job = egui::text::LayoutJob::simple_singleline(
            msg.to_string(),
            theme::regular(text::SMALL),
            p.text_secondary,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(
            (rect.right() - ir.right() - space::MD).max(10.0),
        );
        let g = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(ir.right() + space::XS, rect.center().y - g.size().y / 2.0),
            g,
            p.text_secondary,
        );
    }
}

/// Placeholder for an external panel without content yet.
pub fn empty_panel(ui: &mut Ui, rect: Rect) {
    let p = pal(ui);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        t!("plugin_ui.waiting"),
        theme::regular(text::BODY),
        p.text_tertiary,
    );
}

#[cfg(test)]
mod chip_layout_tests {
    use super::{layout_chips, ChipForm, CHIP_ICON_W};

    const GAP: f32 = 6.0;

    #[test]
    fn everything_fits() {
        assert_eq!(
            layout_chips(&[80.0, 120.0], &[50.0, 50.0], GAP, 300.0),
            vec![ChipForm::Full; 2]
        );
        assert!(layout_chips(&[], &[], GAP, 0.0).is_empty());
    }

    #[test]
    fn widest_text_is_cut_first() {
        // 60 + 160 + gaps 12 = 232 > 200: only the wide chip gives way.
        let f = layout_chips(&[60.0, 160.0], &[50.0, 50.0], GAP, 200.0);
        assert_eq!(f[0], ChipForm::Full);
        let ChipForm::Truncated(w) = f[1] else {
            panic!("{f:?}")
        };
        assert!((w - 128.0).abs() <= 1.0, "{w}");
    }

    #[test]
    fn icons_from_the_left_then_hidden() {
        let full = [100.0, 140.0];
        let min = [55.0, 60.0];
        // Room for icon + truncated text, not for two truncated chips.
        let f = layout_chips(&full, &min, GAP, CHIP_ICON_W + 60.0 + 2.0 * GAP + 10.0);
        assert_eq!(f[0], ChipForm::Icon);
        assert!(matches!(f[1], ChipForm::Truncated(_)), "{f:?}");
        // Only icons.
        let f = layout_chips(&full, &min, GAP, 2.0 * (CHIP_ICON_W + GAP) + 5.0);
        assert_eq!(f, vec![ChipForm::Icon, ChipForm::Icon]);
        // Only one icon.
        let f = layout_chips(&full, &min, GAP, CHIP_ICON_W + GAP + 5.0);
        assert_eq!(f, vec![ChipForm::Hidden, ChipForm::Icon]);
        // Nothing.
        let f = layout_chips(&full, &min, GAP, 10.0);
        assert_eq!(f, vec![ChipForm::Hidden, ChipForm::Hidden]);
    }

    #[test]
    fn widths_never_exceed_the_space() {
        let full = [90.0, 170.0, 70.0];
        let min = [55.0, 60.0, 50.0];
        for avail in (0..400).step_by(7) {
            let avail = avail as f32;
            let f = layout_chips(&full, &min, GAP, avail);
            let used: f32 = f
                .iter()
                .zip(full)
                .filter(|(f, _)| **f != ChipForm::Hidden)
                .map(|(f, w)| f.width(w) + GAP)
                .sum();
            assert!(used <= avail + 0.5, "avail {avail}: {f:?} uses {used}");
        }
    }
}

#[cfg(test)]
#[path = "image_tools_tests.rs"]
mod image_tools_tests;

/// Development folders from the command line (`--plugin-dev <dir>`,
/// `--plugin-dev=<dir>`, repeatable) and from `MANTA_PLUGIN_DEV` (folders
/// separated by `:`). `~/` is expanded.
pub fn plugin_dev_dirs(
    args: impl IntoIterator<Item = String>,
    env: Option<&str>,
) -> Vec<std::path::PathBuf> {
    let expand = |s: &str| -> std::path::PathBuf {
        match (s.strip_prefix("~/"), dirs::home_dir()) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => std::path::PathBuf::from(s),
        }
    };
    let mut out = Vec::new();
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        if a == "--plugin-dev" {
            if let Some(dir) = args.next() {
                out.push(expand(&dir));
            }
        } else if let Some(dir) = a.strip_prefix("--plugin-dev=") {
            out.push(expand(dir));
        }
    }
    for dir in env
        .unwrap_or("")
        .split(':')
        .filter(|d| !d.trim().is_empty())
    {
        out.push(expand(dir.trim()));
    }
    out
}

#[cfg(test)]
mod dev_args_tests {
    use super::plugin_dev_dirs;
    use std::path::PathBuf;

    #[test]
    fn parses_plugin_dev_arguments() {
        let args = [
            "-psn_0_123",
            "--plugin-dev",
            "/a",
            "--plugin-dev=/b c",
            "--other",
            "--plugin-dev",
        ]
        .map(String::from);
        assert_eq!(
            plugin_dev_dirs(args, Some("/c::/d")),
            [
                PathBuf::from("/a"),
                PathBuf::from("/b c"),
                PathBuf::from("/c"),
                PathBuf::from("/d")
            ]
        );
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            plugin_dev_dirs(["--plugin-dev".to_string(), "~/p".to_string()], None),
            [home.join("p")]
        );
        assert!(plugin_dev_dirs(Vec::<String>::new(), None).is_empty());
    }
}
