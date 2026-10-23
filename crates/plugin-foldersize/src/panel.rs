//! The "Disk Usage" panel: a squarified treemap of the active pane's
//! folder, a breadcrumb for zooming back out, a category legend and a
//! ranked list of the largest items.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{
    pos2, text::LayoutJob, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Galley, Rect,
    RichText, Sense, Stroke, StrokeKind, Ui,
};
use manta_plugin::PaneState;

use crate::engine::{lock, Item, PanelRef};
use crate::i18n::tr;
use crate::settings::format_size;
use crate::treemap::squarify;
use crate::walker::Category;
use manta_plugin::i18n::format_decimal;

/// What the panel asks the plugin to do.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    /// Measure this folder for the panel (`None`: nothing to show).
    Show(Option<PathBuf>),
    /// Open this folder in the pane.
    Navigate(PathBuf),
}

/// UI state of the panel.
#[derive(Default)]
pub struct PanelUi {
    /// The pane folder the treemap starts from.
    root: Option<PathBuf>,
    /// Folders zoomed into, below `root`.
    pub zoom: Vec<PathBuf>,
    /// The last single click on a folder: (folder, zoom depth before).
    last_click: Option<(PathBuf, usize)>,
    /// Tiles drawn in the last frame (for tests): (rect, path, is_dir).
    pub tiles: Vec<(Rect, PathBuf, bool)>,
    /// This frame's double-click was handled already.
    double_handled: bool,
}

impl PanelUi {
    /// The folder the treemap shows.
    pub fn view_dir(&self) -> Option<&Path> {
        self.zoom
            .last()
            .map(PathBuf::as_path)
            .or(self.root.as_deref())
    }
}

// ------------------------------------------------------------------ style

mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
}

const CAPTION: f32 = 11.0;
const SMALL: f32 = 12.0;
const BODY: f32 = 13.0;

#[derive(Clone, Copy)]
struct Colors {
    dark: bool,
    text: Color32,
    text2: Color32,
    text3: Color32,
    hover: Color32,
    track: Color32,
    accent_text: Color32,
}

impl Colors {
    fn of(ui: &Ui) -> Colors {
        let v = ui.visuals();
        let dark = v.dark_mode;
        let text2 = v.weak_text_color();
        Colors {
            dark,
            text: v.text_color(),
            text2,
            text3: mix(text2, v.panel_fill, 0.35),
            hover: if dark {
                Color32::from_white_alpha(14)
            } else {
                Color32::from_black_alpha(12)
            },
            track: if dark {
                Color32::from_white_alpha(16)
            } else {
                Color32::from_black_alpha(14)
            },
            accent_text: if dark {
                Color32::from_rgb(100, 170, 255)
            } else {
                Color32::from_rgb(0, 95, 204)
            },
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn named(ui: &Ui, name: &str, size: f32) -> FontId {
    let fam = FontFamily::Name(name.into());
    let has = ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&fam));
    FontId::new(size, if has { fam } else { FontFamily::Proportional })
}

fn medium(ui: &Ui, size: f32) -> FontId {
    named(ui, "rl-medium", size)
}

fn semibold(ui: &Ui, size: f32) -> FontId {
    named(ui, "rl-semibold", size)
}

/// One line, cut with "…" at `max_w`.
fn galley(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_w.max(4.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

/// Categorical fills (fixed order, validated for colour-vision deficiency
/// on adjacent pairs; stepped separately for the dark surface).
pub fn category_color(c: Category, dark: bool) -> Color32 {
    let (l, d) = match c {
        Category::Documents => ((0x2a, 0x78, 0xd6), (0x39, 0x87, 0xe5)),
        Category::Video => ((0xeb, 0x68, 0x34), (0xd9, 0x59, 0x26)),
        Category::Code => ((0x1b, 0xaf, 0x7a), (0x19, 0x9e, 0x70)),
        Category::Archives => ((0xed, 0xa1, 0x00), (0xc9, 0x85, 0x00)),
        Category::Images => ((0xe8, 0x7b, 0xa4), (0xd5, 0x51, 0x81)),
        Category::Audio => ((0x00, 0x83, 0x00), (0x00, 0x83, 0x00)),
        Category::Apps => ((0x4a, 0x3a, 0xa7), (0x90, 0x85, 0xe9)),
        Category::Other => ((0x9a, 0x9a, 0xa2), (0x6e, 0x6e, 0x76)),
    };
    let (r, g, b) = if dark { d } else { l };
    Color32::from_rgb(r, g, b)
}

/// Folders: the colour of what they mostly hold, shaded towards slate so
/// they read as containers next to plain files.
pub fn folder_color(dominant: Category, dark: bool) -> Color32 {
    let slate = if dark {
        Color32::from_rgb(84, 94, 112)
    } else {
        Color32::from_rgb(120, 134, 156)
    };
    if dominant == Category::Other {
        return slate;
    }
    mix(category_color(dominant, dark), slate, 0.42)
}

fn item_color(it: &Item, dark: bool) -> Color32 {
    if it.is_dir && it.category != Category::Apps {
        folder_color(it.dominant, dark)
    } else {
        category_color(it.category, dark)
    }
}

/// Text colour that reads on `fill`.
fn ink_on(fill: Color32) -> Color32 {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = 0.2126 * lin(fill.r()) + 0.7152 * lin(fill.g()) + 0.0722 * lin(fill.b());
    if lum > 0.36 {
        Color32::from_rgb(24, 24, 27)
    } else {
        Color32::WHITE
    }
}

pub fn size_text(it: &Item) -> String {
    if it.unknown {
        "—".into()
    } else if !it.complete {
        format!("≥ {}…", format_size(it.bytes))
    } else if it.unreadable > 0 {
        format!("≥ {}", format_size(it.bytes))
    } else {
        format_size(it.bytes)
    }
}

fn pct(part: u64, total: u64) -> String {
    if total == 0 {
        return String::new();
    }
    let p = part as f64 / total as f64 * 100.0;
    if p >= 10.0 {
        format!("{p:.0} %")
    } else if p >= 0.1 {
        format!("{} %", format_decimal(p, 1))
    } else {
        format!("< {} %", format_decimal(0.1, 1))
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

// ------------------------------------------------------------------ panel

/// One tile of the treemap: a real item or the "smaller items" rest.
struct Tile<'a> {
    item: Option<&'a Item>,
    bytes: u64,
    rest_count: usize,
}

/// Most tiles drawn; the rest becomes one "N smaller items" tile.
const MAX_TILES: usize = 80;
/// Items under this share of the total go into the rest tile.
const MIN_SHARE: f64 = 0.002;

pub fn panel_ui(
    st: &mut PanelUi,
    data: &PanelRef,
    pane: &PaneState,
    ui: &mut Ui,
) -> Vec<PanelAction> {
    let mut actions = Vec::new();
    let col = Colors::of(ui);
    st.tiles.clear();
    st.double_handled = false;
    let Some(root) = pane.local_path.clone() else {
        if st.root.take().is_some() {
            st.zoom.clear();
        }
        actions.push(PanelAction::Show(None));
        empty_state(ui, tr!("empty.local.title"), tr!("empty.local.body"));
        return actions;
    };
    if st.root.as_ref() != Some(&root) {
        st.root = Some(root.clone());
        st.zoom.clear();
        st.last_click = None;
    }
    st.zoom.retain(|z| z.starts_with(&root));
    // The second click of a double-click lands on the zoomed view (which
    // may still be loading): open the folder of the first click instead.
    let double = ui.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
            && i.pointer
                .interact_pos()
                .is_some_and(|p| ui.max_rect().contains(p))
    });
    if double {
        if let Some((path, depth)) = st.last_click.take() {
            st.zoom.truncate(depth);
            actions.push(PanelAction::Navigate(path));
            st.double_handled = true;
        }
    }
    let view = st.view_dir().map(Path::to_path_buf).unwrap_or(root.clone());
    actions.push(PanelAction::Show(Some(view.clone())));

    let (items, listed, error) = {
        let d = lock(data);
        if d.dir.as_ref() == Some(&view) {
            (d.items.clone(), d.listed, d.error.clone())
        } else {
            (Vec::new(), false, None)
        }
    };

    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            breadcrumb(ui, st, &root, col);
            ui.add_space(space::MD);
            if let Some(e) = error {
                ui.label(
                    RichText::new(tr!("error.unreadable"))
                        .font(semibold(ui, BODY))
                        .color(col.text),
                );
                ui.add_space(space::XS);
                ui.label(RichText::new(e).size(SMALL).color(col.text2));
                return;
            }
            if !listed {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0).color(col.text2));
                    ui.add_space(space::SM);
                    ui.label(RichText::new(tr!("reading")).size(SMALL).color(col.text2));
                });
                return;
            }
            let total: u64 = items.iter().map(|i| i.bytes).sum();
            let files: u64 = items.iter().map(|i| i.files).sum();
            let counting = items.iter().any(|i| i.is_dir && !i.complete);
            summary(ui, total, files, counting, items.is_empty(), col);
            ui.add_space(space::MD);
            if items.is_empty() {
                ui.label(
                    RichText::new(tr!("empty.folder"))
                        .size(SMALL)
                        .color(col.text2),
                );
                return;
            }
            let mut sorted: Vec<&Item> = items.iter().filter(|i| i.bytes > 0).collect();
            sorted.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
            if sorted.is_empty() {
                ui.label(
                    RichText::new(if counting {
                        tr!("measuring")
                    } else {
                        tr!("empty.no_space")
                    })
                    .size(SMALL)
                    .color(col.text2),
                );
                return;
            }

            // Treemap.
            let w = ui.available_width();
            let h = (w * 0.8).clamp(150.0, (ui.available_height() * 0.55).max(150.0));
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
            treemap(ui, st, &sorted, total, rect, &resp, col, &mut actions);
            ui.add_space(space::MD);
            legend(ui, &sorted, col);
            ui.add_space(space::LG);

            // Largest items.
            ui.label(
                RichText::new(tr!("largest"))
                    .font(semibold(ui, SMALL))
                    .color(col.text2),
            );
            ui.add_space(space::XS);
            let max = sorted.first().map_or(1, |i| i.bytes.max(1));
            egui::ScrollArea::vertical()
                .id_salt("fs-largest")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for it in sorted.iter().take(25) {
                        list_row(ui, st, it, max, total, col, &mut actions);
                    }
                });
        });
    actions
}

fn summary(ui: &mut Ui, total: u64, files: u64, counting: bool, empty: bool, col: Colors) {
    ui.horizontal(|ui| {
        let big = if counting {
            format!("≥ {}", format_size(total))
        } else {
            format_size(total)
        };
        ui.label(RichText::new(big).font(semibold(ui, 20.0)).color(col.text));
        ui.add_space(space::MD);
        ui.vertical(|ui| {
            ui.add_space(6.0);
            let detail = if empty {
                String::new()
            } else {
                tr!("files", count = files)
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(detail).size(SMALL).color(col.text2));
                if counting {
                    ui.add_space(space::MD);
                    ui.add(egui::Spinner::new().size(10.0).color(col.text3));
                    ui.add_space(space::XS);
                    ui.label(RichText::new(tr!("counting")).size(SMALL).color(col.text3));
                }
            });
        });
    });
}

fn breadcrumb(ui: &mut Ui, st: &mut PanelUi, root: &Path, col: Colors) {
    let mut segs: Vec<(String, usize)> = vec![(file_name(root), 0)];
    for (i, z) in st.zoom.iter().enumerate() {
        segs.push((file_name(z), i + 1));
    }
    let font_last = semibold(ui, BODY);
    let font = medium(ui, BODY);
    let avail = ui.available_width();
    let sep_w = 16.0;
    let widths: Vec<f32> = segs
        .iter()
        .enumerate()
        .map(|(i, (s, _))| {
            let f = if i + 1 == segs.len() {
                font_last.clone()
            } else {
                font.clone()
            };
            ui.painter()
                .layout_no_wrap(s.clone(), f, col.text)
                .size()
                .x
                .min(160.0)
                + 8.0
        })
        .collect();
    // Drop middle segments until it fits ("…" stands in for them).
    let mut first_shown = 1;
    let fits = |from: usize| -> bool {
        let mut w = widths[0] + widths[from..].iter().map(|w| w + sep_w).sum::<f32>();
        if from > 1 {
            w += 14.0 + sep_w;
        }
        w <= avail
    };
    while first_shown < segs.len().saturating_sub(1) && !fits(first_shown) {
        first_shown += 1;
    }
    let mut new_depth: Option<usize> = None;
    let (row, _) = ui.allocate_exact_size(vec2(avail, 22.0), Sense::hover());
    let mut x = row.left();
    let cy = row.center().y;
    let shown: Vec<usize> = std::iter::once(0).chain(first_shown..segs.len()).collect();
    for (k, &i) in shown.iter().enumerate() {
        if k > 0 {
            if k == 1 && first_shown > 1 {
                ui.painter().text(
                    pos2(x + 7.0, cy),
                    Align2::CENTER_CENTER,
                    "…",
                    font.clone(),
                    col.text3,
                );
                x += 14.0;
                chevron(ui, pos2(x + sep_w / 2.0, cy), col.text3);
                x += sep_w;
            }
            chevron(ui, pos2(x + sep_w / 2.0, cy), col.text3);
            x += sep_w;
        }
        let last = i + 1 == segs.len();
        let (name, depth) = &segs[i];
        let f = if last {
            font_last.clone()
        } else {
            font.clone()
        };
        let w = widths[i].min(row.right() - x).max(10.0);
        let r = Rect::from_min_size(pos2(x, row.top()), vec2(w, row.height()));
        let resp = ui.interact(
            r,
            egui::Id::unique(("fs-crumb", i)),
            if last { Sense::hover() } else { Sense::click() },
        );
        if resp.hovered() && !last {
            ui.painter()
                .rect_filled(r, CornerRadius::same(5), col.hover);
        }
        let color = if last {
            col.text
        } else if resp.hovered() {
            col.accent_text
        } else {
            col.text2
        };
        let g = galley(ui, name, f, color, w - 8.0);
        ui.painter()
            .galley(pos2(r.left() + 4.0, cy - g.size().y / 2.0), g, color);
        if !last {
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                new_depth = Some(*depth);
            }
        }
        x += w;
    }
    if let Some(d) = new_depth {
        st.zoom.truncate(d);
        st.last_click = None;
    }
}

fn chevron(ui: &Ui, c: egui::Pos2, color: Color32) {
    let s = 3.0;
    ui.painter().add(egui::Shape::line(
        vec![
            pos2(c.x - s / 2.0, c.y - s),
            pos2(c.x + s / 2.0, c.y),
            pos2(c.x - s / 2.0, c.y + s),
        ],
        Stroke::new(1.3, color),
    ));
}

#[allow(clippy::too_many_arguments)]
fn treemap(
    ui: &mut Ui,
    st: &mut PanelUi,
    sorted: &[&Item],
    total: u64,
    rect: Rect,
    resp: &egui::Response,
    col: Colors,
    actions: &mut Vec<PanelAction>,
) {
    // Small items fold into one tile.
    let mut tiles: Vec<Tile> = Vec::new();
    let mut rest = Tile {
        item: None,
        bytes: 0,
        rest_count: 0,
    };
    for (i, it) in sorted.iter().enumerate() {
        if i < MAX_TILES && (it.bytes as f64 / total.max(1) as f64 >= MIN_SHARE || i < 8) {
            tiles.push(Tile {
                item: Some(it),
                bytes: it.bytes,
                rest_count: 0,
            });
        } else {
            rest.bytes += it.bytes;
            rest.rest_count += 1;
        }
    }
    if rest.rest_count == 1 {
        // A single leftover item is drawn as itself.
        let it = sorted[tiles.len()];
        tiles.push(Tile {
            item: Some(it),
            bytes: it.bytes,
            rest_count: 0,
        });
    } else if rest.rest_count > 1 {
        tiles.push(rest);
    }
    let values: Vec<f64> = tiles.iter().map(|t| t.bytes as f64).collect();
    let rects = squarify(&values, rect);
    let painter = ui.painter_at(rect.expand(1.0));
    let hover = resp.hover_pos();
    let mut hovered: Option<usize> = None;
    for (i, (t, r)) in tiles.iter().zip(&rects).enumerate() {
        let r = r.shrink(1.0);
        if r.width() < 0.5 || r.height() < 0.5 {
            continue;
        }
        let rounding = CornerRadius::same(if r.width().min(r.height()) > 12.0 {
            4
        } else {
            1
        });
        let (mut fill, is_dir) = match t.item {
            Some(it) => (
                item_color(it, col.dark),
                it.is_dir && it.category != Category::Apps,
            ),
            None => (
                if col.dark {
                    Color32::from_rgb(72, 72, 78)
                } else {
                    Color32::from_rgb(196, 196, 202)
                },
                false,
            ),
        };
        let incomplete = t.item.is_some_and(|it| !it.complete);
        if incomplete {
            fill = mix(fill, ui.visuals().panel_fill, 0.3);
        }
        let is_hover = hover.is_some_and(|p| r.expand(1.0).contains(p));
        if is_hover {
            hovered = Some(i);
            fill = mix(
                fill,
                if col.dark {
                    Color32::WHITE
                } else {
                    Color32::BLACK
                },
                0.1,
            );
        }
        painter.rect_filled(r, rounding, fill);
        if is_dir {
            // Folder shading: a soft lighter band at the top edge and an
            // inner hairline, so containers read apart from flat files.
            let band = Rect::from_min_max(
                r.min,
                pos2(r.max.x, r.min.y + (r.height() * 0.35).min(18.0)),
            );
            let top = CornerRadius {
                nw: rounding.nw,
                ne: rounding.ne,
                sw: 0,
                se: 0,
            };
            painter.rect_filled(
                band,
                top,
                Color32::from_white_alpha(if col.dark { 14 } else { 30 }),
            );
            painter.rect_stroke(
                r,
                rounding,
                Stroke::new(
                    1.0,
                    Color32::from_white_alpha(if col.dark { 22 } else { 46 }),
                ),
                StrokeKind::Inside,
            );
        }
        if is_hover {
            painter.rect_stroke(
                r,
                rounding,
                Stroke::new(1.5, ink_on(fill)),
                StrokeKind::Inside,
            );
        }
        // Labels only where they fit.
        let ink = ink_on(fill);
        let (name, size) = match t.item {
            Some(it) => (it.name.clone(), size_text(it)),
            None => (tr!("smaller", count = t.rest_count), format_size(t.bytes)),
        };
        if r.width() >= 34.0 && r.height() >= 17.0 {
            let pad = 5.0;
            let maxw = r.width() - 2.0 * pad;
            let g = galley(ui, &name, medium(ui, CAPTION), ink, maxw);
            let gh = g.size().y;
            // A name cut to a stub says nothing: then no label at all.
            let full_w = ui
                .painter()
                .layout_no_wrap(name.clone(), medium(ui, CAPTION), ink)
                .size()
                .x;
            let shown_chars = (maxw / full_w.max(1.0) * name.chars().count() as f32) as usize;
            if full_w <= maxw + 0.5 || shown_chars >= 5 {
                painter.galley(pos2(r.left() + pad, r.top() + 3.0), g, ink);
                if r.height() >= 32.0 {
                    let sub = ink.gamma_multiply(0.82);
                    let sg = ui.painter().layout_no_wrap(
                        size.clone(),
                        egui::FontId::proportional(10.5),
                        sub,
                    );
                    // Sizes are never cut.
                    if sg.size().x <= maxw + 0.5 {
                        painter.galley(pos2(r.left() + pad, r.top() + 3.0 + gh), sg, sub);
                    }
                }
            }
        }
        if let Some(it) = t.item {
            st.tiles.push((r, it.path.clone(), it.is_dir));
        }
    }

    let Some(hi) = hovered else { return };
    let t = &tiles[hi];
    let parent = st.view_dir().map(file_name).unwrap_or_default();
    let resp = resp.clone().on_hover_ui_at_pointer(|ui| {
        ui.set_max_width(260.0);
        match t.item {
            Some(it) => {
                ui.label(
                    RichText::new(&it.name)
                        .font(semibold(ui, BODY))
                        .color(col.text),
                );
                ui.label(
                    RichText::new(tr!(
                        "share_of",
                        size = size_text(it),
                        pct = pct(it.bytes, total),
                        parent = parent
                    ))
                    .size(SMALL)
                    .color(col.text),
                );
                let mut detail = Vec::new();
                if it.is_dir {
                    detail.push(tr!("files", count = it.files));
                } else {
                    detail.push(it.category.label().to_string());
                }
                if it.on_disk > 0 {
                    detail.push(tr!("on_disk", size = format_size(it.on_disk)));
                }
                ui.label(
                    RichText::new(detail.join(" · "))
                        .size(SMALL)
                        .color(col.text2),
                );
                if it.unreadable > 0 {
                    ui.label(
                        RichText::new(tr!("some_unreadable"))
                            .size(SMALL)
                            .color(col.text2),
                    );
                }
                if it.is_dir {
                    ui.add_space(space::XS);
                    ui.label(
                        RichText::new(tr!("click_hint"))
                            .size(CAPTION)
                            .color(col.text3),
                    );
                }
            }
            None => {
                ui.label(
                    RichText::new(tr!("smaller", count = t.rest_count)).font(semibold(ui, BODY)),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        format_size(t.bytes),
                        pct(t.bytes, total)
                    ))
                    .size(SMALL)
                    .color(col.text),
                );
            }
        }
    });
    let target = t.item.filter(|it| it.is_dir).map(|it| it.path.clone());
    if target.is_some() {
        let _ = resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    click_folder(st, &resp, target, actions);
}

/// Click zooms into a folder, double-click opens it in the pane (and undoes
/// the zoom of its first click).
fn click_folder(
    st: &mut PanelUi,
    resp: &egui::Response,
    target: Option<PathBuf>,
    actions: &mut Vec<PanelAction>,
) {
    if resp.double_clicked() {
        if !st.double_handled {
            if let Some(t) = target {
                actions.push(PanelAction::Navigate(t));
            }
        }
    } else if resp.clicked() {
        if let Some(t) = target {
            st.last_click = Some((t.clone(), st.zoom.len()));
            st.zoom.push(t);
        }
    }
}

fn legend(ui: &mut Ui, sorted: &[&Item], col: Colors) {
    // Categories by share, folders counted by what they mostly hold.
    let mut by = [0u64; Category::COUNT];
    let mut has_folders = false;
    for it in sorted {
        let c = if it.is_dir && it.category != Category::Apps {
            has_folders = true;
            it.dominant
        } else {
            it.category
        };
        by[c.index()] += it.bytes;
    }
    let mut cats: Vec<Category> = Category::ALL
        .iter()
        .copied()
        .filter(|c| by[c.index()] > 0)
        .collect();
    cats.sort_by(|a, b| by[b.index()].cmp(&by[a.index()]));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 4.0);
        for c in cats {
            swatch_label(ui, category_color(c, col.dark), c.label(), col);
        }
        if has_folders {
            swatch_label(
                ui,
                folder_color(Category::Other, col.dark),
                tr!("legend.folders"),
                col,
            );
        }
    });
}

fn swatch_label(ui: &mut Ui, color: Color32, label: &str, col: Colors) {
    let font = egui::FontId::proportional(CAPTION);
    let g = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, col.text2);
    let (r, _) = ui.allocate_exact_size(vec2(12.0 + g.size().x, 14.0), Sense::hover());
    let sw = Rect::from_center_size(pos2(r.left() + 4.0, r.center().y), vec2(8.0, 8.0));
    ui.painter().rect_filled(sw, CornerRadius::same(2), color);
    ui.painter().galley(
        pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0),
        g,
        col.text2,
    );
}

fn list_row(
    ui: &mut Ui,
    st: &mut PanelUi,
    it: &Item,
    max: u64,
    total: u64,
    col: Colors,
    actions: &mut Vec<PanelAction>,
) {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(r, CornerRadius::same(5), col.hover);
    }
    let fill = item_color(it, col.dark);
    let is_folder = it.is_dir && it.category != Category::Apps;
    // Swatch: a folder tab shape for folders, a rounded square for files.
    let sw = Rect::from_center_size(pos2(r.left() + 12.0, r.top() + 12.0), vec2(12.0, 10.0));
    if is_folder {
        let tab = Rect::from_min_size(sw.min - vec2(0.0, 2.0), vec2(5.0, 3.0));
        ui.painter().rect_filled(tab, CornerRadius::same(1), fill);
    }
    ui.painter().rect_filled(sw, CornerRadius::same(2), fill);

    let left = r.left() + 24.0;
    let right = r.right() - 6.0;
    let size_s = size_text(it);
    let sg = ui
        .painter()
        .layout_no_wrap(size_s, egui::FontId::proportional(SMALL), col.text);
    let pg = ui.painter().layout_no_wrap(
        pct(it.bytes, total),
        egui::FontId::proportional(CAPTION),
        col.text3,
    );
    let pct_w = 40.0;
    let size_right = right - pct_w;
    let name_max = (size_right - sg.size().x - space::MD - left).max(20.0);
    let ng = galley(
        ui,
        &it.name,
        egui::FontId::proportional(BODY),
        col.text,
        name_max,
    );
    let ty = r.top() + 12.0;
    ui.painter()
        .galley(pos2(left, ty - ng.size().y / 2.0), ng, col.text);
    let sgw = sg.size();
    ui.painter()
        .galley(pos2(size_right - sgw.x, ty - sgw.y / 2.0), sg, col.text);
    let pgs = pg.size();
    ui.painter()
        .galley(pos2(right - pgs.x, ty - pgs.y / 2.0), pg, col.text3);
    // Size bar, relative to the largest item.
    let bar = Rect::from_min_max(pos2(left, r.top() + 23.0), pos2(size_right, r.top() + 26.0));
    ui.painter()
        .rect_filled(bar, CornerRadius::same(2), col.track);
    let frac = (it.bytes as f64 / max.max(1) as f64).clamp(0.0, 1.0) as f32;
    let mut fb = bar;
    fb.set_width((bar.width() * frac).max(2.0));
    ui.painter().rect_filled(
        fb,
        CornerRadius::same(2),
        if it.complete {
            fill
        } else {
            fill.gamma_multiply(0.6)
        },
    );

    let target = is_folder.then(|| it.path.clone());
    let resp = if is_folder {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr!("click_hint"))
    } else {
        resp
    };
    click_folder(st, &resp, target, actions);
}

fn empty_state(ui: &mut Ui, title: &str, body: &str) {
    let col = Colors::of(ui);
    let rect = ui.available_rect_before_wrap();
    let width = (rect.width() - 32.0).clamp(120.0, 280.0);
    let inner = Rect::from_center_size(rect.center() - vec2(0.0, 40.0), vec2(width, 170.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 22.0, col.hover);
            paint_disk(ui, r.shrink(12.0), col.text2);
            ui.add_space(space::MD);
            ui.label(
                RichText::new(title)
                    .font(semibold(ui, 15.0))
                    .color(col.text),
            );
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(RichText::new(body).size(SMALL).color(col.text2))
                    .wrap()
                    .halign(egui::Align::Center),
            );
        },
    );
}

/// A small treemap glyph.
fn paint_disk(ui: &Ui, r: Rect, color: Color32) {
    let p = ui.painter();
    let s = Stroke::new(1.4, color);
    p.rect_stroke(r, CornerRadius::same(3), s, StrokeKind::Inside);
    let x = r.left() + r.width() * 0.55;
    p.vline(x, r.y_range(), s);
    let y = r.top() + r.height() * 0.45;
    p.hline(egui::Rangef::new(x, r.right()), y, s);
}
