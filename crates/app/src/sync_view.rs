//! Compare & Sync window: compares the folders of two panes — left and
//! right with two panes; with three or more, the two the user picks
//! ("Pane A" / "Pane B", default: the active pane and its copy target).
//!
//! Every status has a glyph *and* a colour, and the legend doubles as the
//! filter, so the window reads at a glance without relying on colour alone.

use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke};
use manta_ops::{CompareItem, CompareOptions, CompareStatus, SyncDirection};

use crate::aux_window::{AuxKind, AuxWindow, Spec};
use crate::icons;
use crate::pane::{Location, SyncSlot};
use crate::theme::{self, pal, radius, space, text};
use crate::util;
use crate::widgets::{self, ButtonKind};

pub const ALL_STATUSES: [CompareStatus; 6] = [
    CompareStatus::OnlyLeft,
    CompareStatus::OnlyRight,
    CompareStatus::LeftNewer,
    CompareStatus::RightNewer,
    CompareStatus::Different,
    CompareStatus::Identical,
];

pub enum SyncRequest {
    Compare,
    Sync,
    /// Compare this pane as A or B (the picker with three or more panes).
    Pick(SyncSlot, crate::app::Side),
}

/// A pane offered by the A / B picker.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneChoice {
    pub id: crate::app::Side,
    /// Position, 1 = leftmost.
    pub number: usize,
    /// The folder's name.
    pub folder: String,
    /// "Local", "deploy@web (SFTP)" …
    pub backend: String,
    pub remote: bool,
}

impl PaneChoice {
    /// "2 · Downloads" for the dropdown.
    fn title(&self) -> String {
        format!("{} · {}", self.number, self.folder)
    }
}

pub struct SyncView {
    pub open: bool,
    /// The panes compared, (A, B); `None` until the window opens.
    pub pick: Option<(crate::app::Side, crate::app::Side)>,
    /// Three or more panes: show the A / B pickers.
    pub picker: bool,
    /// The panes to choose from (left to right).
    pub choices: Vec<PaneChoice>,
    pub left: Option<Location>,
    pub right: Option<Location>,
    pub left_label: String,
    pub right_label: String,
    pub items: Option<Vec<CompareItem>>,
    pub loading: bool,
    pub gen: u64,
    pub error: Option<String>,
    pub show: [bool; 6],
    pub direction: SyncDirection,
    pub delete_extra: bool,
    pub options: CompareOptions,
    /// The native window (placement, focus).
    pub(crate) window: AuxWindow,
}

impl Default for SyncView {
    fn default() -> Self {
        SyncView {
            open: false,
            pick: None,
            picker: false,
            choices: Vec::new(),
            left: None,
            right: None,
            left_label: String::new(),
            right_label: String::new(),
            items: None,
            loading: false,
            gen: 0,
            error: None,
            // Identical items hidden by default.
            show: [true, true, true, true, true, false],
            direction: SyncDirection::LeftToRight,
            delete_extra: false,
            options: CompareOptions::default(),
            window: AuxWindow::default(),
        }
    }
}

fn status_index(s: CompareStatus) -> usize {
    ALL_STATUSES.iter().position(|x| *x == s).unwrap_or(0)
}

fn size_text(s: Option<u64>, is_dir: bool) -> String {
    match (s, is_dir) {
        (None, _) => String::new(),
        (Some(_), true) => "—".into(),
        (Some(n), false) => util::format_size(n),
    }
}

/// What a sync would do with an item (mirrors `manta_ops::plan_sync`).
/// `None` = nothing.
fn planned_action(
    it: &CompareItem,
    dir: SyncDirection,
    delete_extra: bool,
) -> Option<&'static str> {
    use CompareStatus as C;
    use SyncDirection as D;
    if it.is_dir && !matches!(it.status, C::OnlyLeft | C::OnlyRight) {
        return None;
    }
    match (dir, it.status) {
        (_, C::Identical) => None,
        (D::LeftToRight, C::OnlyLeft | C::LeftNewer | C::Different) => {
            Some(t!("sync.action.copy_right"))
        }
        (D::LeftToRight, C::OnlyRight) if delete_extra => Some(t!("sync.action.delete_right")),
        (D::RightToLeft, C::OnlyRight | C::RightNewer | C::Different) => {
            Some(t!("sync.action.copy_left"))
        }
        (D::RightToLeft, C::OnlyLeft) if delete_extra => Some(t!("sync.action.delete_left")),
        (D::Bidirectional, C::OnlyLeft | C::LeftNewer) => Some(t!("sync.action.copy_right")),
        (D::Bidirectional, C::OnlyRight | C::RightNewer) => Some(t!("sync.action.copy_left")),
        _ => None,
    }
}

/// Split "conn /path" (as built by the app) into connection and path.
fn split_label(label: &str) -> (&str, &str) {
    match label.find(" /") {
        Some(i) => (&label[..i], &label[i + 1..]),
        None => ("", label),
    }
}

/// The backend icon of a pane in the picker.
fn pane_icon(c: &PaneChoice) -> &'static str {
    if c.remote {
        icons::GLOBE
    } else {
        icons::HARD_DRIVE
    }
}

/// Height of the A / B dropdown in a card.
const PICKER_H: f32 = 22.0;

/// Height of a location card: caption, path, connection.
const CARD_H: f32 = 64.0;

/// One side of the comparison, painted into `rect`: "Left" caption, the
/// folder path (start-truncated, full path on hover) and the connection.
/// `picker`: the caption row holds the pane dropdown, so it is taller.
fn location_card(ui: &mut egui::Ui, rect: egui::Rect, side_label: &str, label: &str, picker: bool) {
    let p = pal(ui);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::PANEL), p.stripe);
    let inner = rect.shrink2(egui::vec2(space::LG, space::MD));
    let (conn, path) = split_label(label);
    let caption = ui.painter().layout_no_wrap(
        side_label.to_string(),
        theme::semibold(text::CAPTION),
        p.text_secondary,
    );
    let path_g =
        widgets::truncate_start(ui, path, theme::medium(text::BODY), p.text, inner.width());
    let mut job = egui::text::LayoutJob::simple_singleline(
        conn.to_string(),
        theme::regular(text::SMALL),
        p.text_secondary,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(inner.width());
    let conn_g = ui.painter().layout_job(job);
    // Three lines, evenly spread over the card's inner height.
    let total = caption.size().y + path_g.size().y + conn_g.size().y;
    let mut gap = ((inner.height() - total) / 2.0).max(0.0);
    let mut y = inner.top();
    if picker {
        // The caption sits beside the dropdown (top + SM, 22 high); path and
        // connection go below it.
        y = rect.top() + space::SM + 11.0 - caption.size().y / 2.0;
        gap = PICKER_H - 11.0 - caption.size().y / 2.0 + 4.0;
    }
    let ch = caption.size().y;
    ui.painter()
        .galley(egui::pos2(inner.left(), y), caption, p.text_secondary);
    y += ch + gap;
    let ph = path_g.size().y;
    ui.painter()
        .galley(egui::pos2(inner.left(), y), path_g, p.text);
    y += ph + gap;
    ui.painter()
        .galley(egui::pos2(inner.left(), y), conn_g, p.text_secondary);
    ui.interact(
        rect,
        ui.scope_id().with(("sync-card", side_label)),
        Sense::hover(),
    )
    .on_hover_text(format!("{conn}  {path}"));
}

impl SyncView {
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<SyncRequest> {
        let mut request = None;
        if !self.open {
            self.window.hidden(ctx, AuxKind::Sync);
            return None;
        }
        let spec = Spec {
            kind: AuxKind::Sync,
            title: t!("sync.title"),
            size: egui::vec2(820.0, 540.0),
            min_size: egui::vec2(600.0, 360.0),
            margin: egui::Margin::same(space::XL as i8),
            activate: true,
            zoomable: true,
        };
        let mut window = std::mem::take(&mut self.window);
        let (_, closed) = window.show(ctx, &spec, |ui| {
            let p = pal(ui);
            // Left card — direction — right card: one row, equal-height
            // cards that share the remaining width, the direction
            // control centred between them.
            let seg_w = 168.0;
            let card_h = if self.picker { CARD_H + 18.0 } else { CARD_H };
            let row = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), card_h), Sense::hover())
                .0;
            let card_w = ((row.width() - seg_w) / 2.0 - space::MD).max(80.0);
            let left = egui::Rect::from_min_size(row.min, egui::vec2(card_w, card_h));
            let right = egui::Rect::from_min_size(
                egui::pos2(row.right() - card_w, row.top()),
                egui::vec2(card_w, card_h),
            );
            if self.picker {
                for (slot, rect, label) in [
                    (SyncSlot::A, left, &self.left_label),
                    (SyncSlot::B, right, &self.right_label),
                ] {
                    let caption = match slot {
                        SyncSlot::A => t!("sync.pane_a"),
                        SyncSlot::B => t!("sync.pane_b"),
                    };
                    location_card(ui, rect, caption, label, true);
                    if let Some(side) = self.pane_picker(ui, rect, slot) {
                        request = Some(SyncRequest::Pick(slot, side));
                    }
                }
            } else {
                location_card(ui, left, t!("sync.left"), &self.left_label, false);
                location_card(ui, right, t!("sync.right"), &self.right_label, false);
            }
            let mid = egui::Rect::from_x_y_ranges(
                egui::Rangef::new(left.right() + space::MD, right.left() - space::MD),
                row.y_range(),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(mid)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = space::XS;
                    ui.add_space(space::MD);
                    widgets::segmented(
                        ui,
                        "sync-direction",
                        &mut self.direction,
                        &[
                            (SyncDirection::LeftToRight, "→"),
                            (SyncDirection::Bidirectional, "↔"),
                            (SyncDirection::RightToLeft, "←"),
                        ],
                    );
                    let d = match self.direction {
                        SyncDirection::LeftToRight => t!("sync.mirror_ltr"),
                        SyncDirection::RightToLeft => t!("sync.mirror_rtl"),
                        SyncDirection::Bidirectional => t!("sync.newer_wins"),
                    };
                    ui.label(
                        RichText::new(d)
                            .font(theme::regular(text::CAPTION))
                            .color(p.text_secondary),
                    );
                },
            );
            // The scopes above (dropdowns, direction) leave the cursor
            // inside the row: continue below the cards.
            ui.advance_cursor_after_rect(row);

            ui.add_space(space::LG);
            // Legend = filter.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
                for s in ALL_STATUSES {
                    let i = status_index(s);
                    let count = self
                        .items
                        .as_ref()
                        .map_or(0, |it| it.iter().filter(|x| x.status == s).count());
                    let label = format!("{}  {count}", theme::compare_label_for(s, self.picker));
                    widgets::toggle_pill(
                        ui,
                        &mut self.show[i],
                        theme::compare_icon(s),
                        &label,
                        theme::compare_color(s, p.dark),
                    )
                    .on_hover_text(if self.show[i] {
                        t!("sync.click_hide")
                    } else {
                        t!("sync.click_show")
                    });
                }
            });
            if let Some(err) = &self.error {
                ui.add_space(space::SM);
                ui.label(
                    RichText::new(format!(
                        "{}  {}",
                        icons::WARNING_CIRCLE,
                        t!("sync.compare_failed", error = err)
                    ))
                    .color(p.danger),
                );
            }
            ui.add_space(space::SM);

            // Footer first (bottom-up) so the table gets the rest.
            egui::Panel::bottom("sync-footer")
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    top: 10,
                    ..Default::default()
                }))
                .show(ui, |ui| {
                    let (dir, del) = (self.direction, self.delete_extra);
                    let pending = self.items.as_ref().map_or(0, |it| {
                        it.iter()
                            .filter(|x| planned_action(x, dir, del).is_some())
                            .count()
                    });
                    let sync_label = if pending == 0 {
                        t!("sync.sync").to_string()
                    } else {
                        t!("sync.sync_n", count = pending)
                    };
                    // Plan the row from measured widths: options and
                    // buttons on one line when they fit, otherwise the
                    // options (wrapping) above right-aligned buttons.
                    let gap = ui.spacing().item_spacing.x;
                    let buttons_w = widgets::button_width(ui, &sync_label, ButtonKind::Primary)
                        + widgets::button_width(
                            ui,
                            t!("sync.compare_again"),
                            ButtonKind::Secondary,
                        )
                        + if self.loading { 14.0 + gap } else { 0.0 }
                        + gap;
                    let options_w = 20.0
                        + [
                            t!("sync.include_subfolders"),
                            t!("sync.hidden_files"),
                            t!("sync.delete_extra"),
                        ]
                        .iter()
                        .map(|l| widgets::checkbox_width(ui, l) + gap)
                        .sum::<f32>();
                    let one_row = options_w + buttons_w + space::LG <= ui.available_width();
                    let mut options = |ui: &mut egui::Ui| {
                        // The manual's chapter, bottom-left as in macOS windows.
                        let help = if self.picker {
                            "13-vergleichen-synchronisieren.md#a-b"
                        } else {
                            "13-vergleichen-synchronisieren.md"
                        };
                        crate::help::help_button(ui, help);
                        ui.checkbox(&mut self.options.recursive, t!("sync.include_subfolders"));
                        ui.checkbox(&mut self.options.include_hidden, t!("sync.hidden_files"));
                        let one_way = self.direction != SyncDirection::Bidirectional;
                        ui.add_enabled(
                            one_way,
                            egui::Checkbox::new(&mut self.delete_extra, t!("sync.delete_extra")),
                        )
                        .on_disabled_hover_text(t!("sync.delete_extra.one_way"));
                    };
                    let loading = self.loading;
                    let buttons = |ui: &mut egui::Ui, request: &mut Option<SyncRequest>| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::button(
                                ui,
                                &sync_label,
                                ButtonKind::Primary,
                                pending > 0 && !loading,
                            )
                            .clicked()
                            {
                                *request = Some(SyncRequest::Sync);
                            }
                            if widgets::button(
                                ui,
                                t!("sync.compare_again"),
                                ButtonKind::Secondary,
                                !loading,
                            )
                            .clicked()
                            {
                                *request = Some(SyncRequest::Compare);
                            }
                            if loading {
                                ui.add(egui::Spinner::new().size(14.0));
                            }
                        });
                    };
                    if one_row {
                        ui.horizontal(|ui| {
                            options(ui);
                            buttons(ui, &mut request);
                        });
                    } else {
                        ui.horizontal_wrapped(|ui| options(ui));
                        ui.add_space(space::SM);
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 26.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| buttons(ui, &mut request),
                        );
                    }
                });

            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(radius::PANEL),
                Stroke::new(1.0, p.separator),
                egui::StrokeKind::Inside,
            );
            let Some(items) = &self.items else {
                if self.loading {
                    ui.put(
                        egui::Rect::from_center_size(rect.center(), egui::vec2(24.0, 24.0)),
                        egui::Spinner::new().size(22.0),
                    );
                } else {
                    widgets::empty_state(
                        ui,
                        rect,
                        icons::ARROWS_LEFT_RIGHT,
                        p.text_tertiary,
                        t!("sync.not_compared"),
                        t!("sync.not_compared.note"),
                        &[],
                    );
                }
                return;
            };
            let visible: Vec<&CompareItem> = items
                .iter()
                .filter(|x| self.show[status_index(x.status)])
                .collect();
            if visible.is_empty() {
                let differences = items.iter().any(|x| x.status != CompareStatus::Identical);
                let (title, body) = if differences {
                    (t!("sync.no_match"), t!("sync.no_match.note"))
                } else {
                    (t!("sync.in_sync"), t!("sync.in_sync.note"))
                };
                widgets::empty_state(
                    ui,
                    rect,
                    icons::CHECK_CIRCLE,
                    if differences {
                        p.text_tertiary
                    } else {
                        p.success
                    },
                    title,
                    body,
                    &[],
                );
                return;
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(1.0)), |ui| {
                self.table(ui, &visible);
            });
        });
        self.window = window;
        if closed.0 {
            self.open = false;
            self.window.hidden(ctx, AuxKind::Sync);
        }
        request
    }

    /// The dropdown in the top-right corner of a card: which pane is A / B.
    /// Returns the pane picked (choosing the other slot's pane swaps them).
    fn pane_picker(
        &self,
        ui: &mut egui::Ui,
        card: egui::Rect,
        slot: SyncSlot,
    ) -> Option<crate::app::Side> {
        let (a, b) = self.pick?;
        let current = if slot == SyncSlot::A { a } else { b };
        let other = if slot == SyncSlot::A { b } else { a };
        let cur = self.choices.iter().find(|c| c.id == current)?;
        let w = (card.width() * 0.5).clamp(120.0, 190.0);
        let r = egui::Rect::from_min_size(
            egui::pos2(card.right() - space::MD - w, card.top() + space::SM),
            egui::vec2(w, PICKER_H),
        );
        let mut picked = None;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(r)
                .id_salt(("sync-pane-pick", slot == SyncSlot::A)),
            |ui| {
                egui::ComboBox::from_id_salt(("sync-pane", slot == SyncSlot::A))
                    .width(w)
                    .selected_text(format!("{}  {}", pane_icon(cur), cur.title()))
                    .show_ui(ui, |ui| {
                        for c in &self.choices {
                            let mut text = format!("{}  {}", pane_icon(c), c.title());
                            if c.id == other {
                                text.push_str(match slot {
                                    SyncSlot::A => "  (B)",
                                    SyncSlot::B => "  (A)",
                                });
                            }
                            let resp = ui
                                .selectable_label(c.id == current, text)
                                .on_hover_text(&c.backend);
                            if resp.clicked() && c.id != current {
                                picked = Some(c.id);
                            }
                        }
                    })
                    .response
                    .on_hover_text(t!("sync.pick_tip"));
            },
        );
        picked
    }

    /// Bring the window to the front (⇧⌘S while it is open).
    pub fn focus(&mut self) {
        self.window.request_focus();
    }

    fn table(&self, ui: &mut egui::Ui, visible: &[&CompareItem]) {
        let p = pal(ui);
        let rect = ui.max_rect();
        // The status column fits the longest badge in this language (up to
        // a third of the table; longer badges are truncated with a tooltip).
        let pill_max = ALL_STATUSES
            .iter()
            .map(|&s| {
                ui.painter()
                    .layout_no_wrap(
                        theme::compare_label_for(s, self.picker).to_string(),
                        theme::medium(text::SMALL),
                        p.text,
                    )
                    .size()
                    .x
                    + 30.0
            })
            .fold(0.0_f32, f32::max);
        let status_w = (pill_max + space::MD).clamp(130.0, (rect.width() / 3.0).max(130.0));
        let pill_text_max = status_w - space::MD - 30.0;
        let size_w = 84.0;
        let action_w = 96.0;
        let x_status = rect.left() + space::MD;
        let x_path = x_status + status_w;
        let x_action = rect.right() - space::MD - action_w;
        let x_right = x_action - space::MD - size_w;
        let x_left = x_right - space::MD - size_w;
        let row_h = 26.0;
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 24.0));
        let hfont = theme::medium(text::CAPTION);
        let hc = p.text_secondary;
        let pt = ui.painter();
        let cy = header.center().y;
        pt.text(
            egui::pos2(x_status, cy),
            egui::Align2::LEFT_CENTER,
            t!("sync.col.status"),
            hfont.clone(),
            hc,
        );
        pt.text(
            egui::pos2(x_path, cy),
            egui::Align2::LEFT_CENTER,
            t!("sync.col.path"),
            hfont.clone(),
            hc,
        );
        let (left_name, right_name) = if self.picker {
            (t!("sync.pane_a"), t!("sync.pane_b"))
        } else {
            (t!("sync.left"), t!("sync.right"))
        };
        pt.text(
            egui::pos2(x_left + size_w, cy),
            egui::Align2::RIGHT_CENTER,
            left_name,
            hfont.clone(),
            hc,
        );
        pt.text(
            egui::pos2(x_right + size_w, cy),
            egui::Align2::RIGHT_CENTER,
            right_name,
            hfont.clone(),
            hc,
        );
        pt.text(
            egui::pos2(x_action, cy),
            egui::Align2::LEFT_CENTER,
            t!("sync.col.action"),
            hfont,
            hc,
        );
        pt.hline(
            header.x_range(),
            header.bottom(),
            Stroke::new(1.0, p.separator),
        );

        let body =
            egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom() + 1.0), rect.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_h, visible.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for i in range {
                        let it = visible[i];
                        let (r, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_h),
                            Sense::hover(),
                        );
                        if i % 2 == 1 {
                            ui.painter().rect_filled(r, CornerRadius::ZERO, p.stripe);
                        }
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::ZERO, p.hover);
                        }
                        let color = theme::compare_color(it.status, p.dark);
                        let cy = r.center().y;
                        // Status pill.
                        let status_label = theme::compare_label_for(it.status, self.picker);
                        let mut job = egui::text::LayoutJob::simple_singleline(
                            status_label.to_string(),
                            theme::medium(text::SMALL),
                            color,
                        );
                        job.wrap = egui::text::TextWrapping::truncate_at_width(pill_text_max);
                        let g = ui.painter().layout_job(job);
                        if g.elided {
                            let pill = egui::Rect::from_min_size(
                                egui::pos2(x_status, cy - 10.0),
                                egui::vec2(g.size().x + 30.0, 20.0),
                            );
                            if resp.hover_pos().is_some_and(|h| pill.contains(h)) {
                                resp.clone().on_hover_text(status_label);
                            }
                        }
                        let pill = egui::Rect::from_min_size(
                            egui::pos2(x_status, cy - 10.0),
                            egui::vec2(g.size().x + 30.0, 20.0),
                        );
                        ui.painter().rect_filled(
                            pill,
                            CornerRadius::same(radius::PILL),
                            color.gamma_multiply(if p.dark { 0.2 } else { 0.1 }),
                        );
                        let ir = egui::Rect::from_min_size(
                            pill.min + egui::vec2(4.0, 0.0),
                            egui::vec2(16.0, 20.0),
                        );
                        widgets::paint_icon(
                            ui,
                            ir,
                            theme::compare_icon(it.status),
                            12.0,
                            color,
                            false,
                        );
                        ui.painter().galley(
                            egui::pos2(ir.right() + 2.0, cy - g.size().y / 2.0),
                            g,
                            color,
                        );
                        // Path with icon; the folder part is dimmed.
                        let kind = icons::file_kind(&it.rel_path, it.is_dir);
                        let ir = egui::Rect::from_center_size(
                            egui::pos2(x_path + 8.0, cy),
                            egui::vec2(16.0, 16.0),
                        );
                        widgets::paint_icon(ui, ir, kind.icon(), 15.0, kind.tint(p), it.is_dir);
                        let (dir, name) = match it.rel_path.rfind('/') {
                            Some(k) => (&it.rel_path[..=k], &it.rel_path[k + 1..]),
                            None => ("", it.rel_path.as_str()),
                        };
                        let mut job = egui::text::LayoutJob::default();
                        job.append(
                            dir,
                            0.0,
                            egui::TextFormat::simple(theme::regular(text::BODY), p.text_secondary),
                        );
                        job.append(
                            name,
                            0.0,
                            egui::TextFormat::simple(theme::regular(text::BODY), p.text),
                        );
                        job.wrap = egui::text::TextWrapping::truncate_at_width(
                            (x_left - x_path - 24.0).max(20.0),
                        );
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(
                            egui::pos2(x_path + 22.0, cy - g.size().y / 2.0),
                            g,
                            p.text,
                        );
                        let small = theme::regular(text::SMALL + 0.5);
                        ui.painter().text(
                            egui::pos2(x_left + size_w, cy),
                            egui::Align2::RIGHT_CENTER,
                            size_text(it.left_size, it.is_dir),
                            small.clone(),
                            p.text_secondary,
                        );
                        ui.painter().text(
                            egui::pos2(x_right + size_w, cy),
                            egui::Align2::RIGHT_CENTER,
                            size_text(it.right_size, it.is_dir),
                            small.clone(),
                            p.text_secondary,
                        );
                        let (action, ac) =
                            match planned_action(it, self.direction, self.delete_extra) {
                                Some(a)
                                    if a == t!("sync.action.delete_right")
                                        || a == t!("sync.action.delete_left") =>
                                {
                                    (a, p.danger)
                                }
                                Some(a) => (a, p.text),
                                None => ("—", p.text_tertiary),
                            };
                        ui.painter().text(
                            egui::pos2(x_action, cy),
                            egui::Align2::LEFT_CENTER,
                            action,
                            small,
                            ac,
                        );
                    }
                });
        });
    }
}
