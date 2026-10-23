//! The conflict question (`ConflictPolicy::Ask`, Finder's way): a copy,
//! move or extract stopped at an item that already exists at the
//! destination and waits until the user decides.
//!
//! - One question at a time: the app opens it for the oldest waiting job
//!   when no other dialog is open; further jobs wait their turn.
//! - Both items with size and modification date; the newer one is marked.
//! - Files: Replace, Replace if newer, Keep both, Skip. Folders onto
//!   folders: Merge (the contents go into the existing folder, files there
//!   are asked about one by one), Keep both, Skip.
//! - "Apply to all further conflicts of this job" answers the rest too.
//! - ↩ = Keep both (the safe default), Esc = Cancel: the job stops, items
//!   already transferred stay.

use eframe::egui::{self, CornerRadius, Key, RichText};
use manta_ops::{ConflictChoice, ConflictItem, ConflictQuestion, JobId, JobKind};

use crate::dialogs::{dialog_header, dialog_modal, folder_name};
use crate::icons;
use crate::theme::{self, pal, radius, space, text};
use crate::widgets::{self, ButtonKind};

/// The open question of one job.
pub struct ConflictForm {
    pub job: JobId,
    pub kind: JobKind,
    /// The job's (engine) title, localized when shown.
    pub title: String,
    pub question: ConflictQuestion,
    /// "Apply to all further conflicts of this job".
    pub all: bool,
}

/// Button label of an answer.
pub fn choice_label(c: ConflictChoice) -> &'static str {
    match c {
        ConflictChoice::Replace => t!("dialog.conflict.replace"),
        ConflictChoice::ReplaceIfNewer => t!("dialog.conflict.if_newer"),
        ConflictChoice::KeepBoth => t!("dialog.conflict.keep_both"),
        ConflictChoice::Skip => t!("dialog.conflict.skip"),
        ConflictChoice::Merge => t!("conflict.merge"),
        ConflictChoice::Cancel => t!("common.cancel"),
    }
}

impl ConflictForm {
    /// The safe default (↩): keep both when offered.
    pub fn default_choice(&self) -> ConflictChoice {
        let c = &self.question.choices;
        if c.contains(&ConflictChoice::KeepBoth) {
            ConflictChoice::KeepBoth
        } else {
            c.first().copied().unwrap_or(ConflictChoice::Skip)
        }
    }

    /// Draw the dialog; `Some` once the user answered.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<ConflictChoice> {
        let mut answer = None;
        let p = theme::pal_ctx(ctx);
        let q = &self.question;
        let name = q.dst.path.file_name().to_string();
        let folder = q
            .dst
            .path
            .parent()
            .map(|d| folder_name(&d))
            .unwrap_or_else(|| "/".into());
        let folder_case = q.choices.contains(&ConflictChoice::Merge);
        let default = self.default_choice();
        dialog_modal("conflict-dialog", ctx).show(ctx, |ui| {
            ui.set_width(580.0);
            let title = t!("conflict.title", name = name, folder = folder);
            let subtitle = crate::i18n::localize_job_title(&self.title);
            dialog_header(
                ui,
                icons::WARNING,
                p.warning,
                p.warning_soft,
                &title,
                &subtitle,
            );
            ui.add_space(space::MD);
            let body = if folder_case {
                t!("conflict.body.folder")
            } else if q.src.is_dir != q.dst.is_dir {
                t!("conflict.body.kind")
            } else {
                t!("conflict.body.file")
            };
            ui.add(egui::Label::new(RichText::new(body).color(p.text_secondary)).wrap());
            ui.add_space(space::MD);
            let newer = q.source_newer();
            let incoming = match self.kind {
                JobKind::Move => t!("conflict.incoming.move"),
                JobKind::Extract => t!("conflict.incoming.extract"),
                _ => t!("conflict.incoming.copy"),
            };
            egui::Frame::new()
                .fill(p.stripe)
                .corner_radius(CornerRadius::same(radius::PANEL))
                .inner_margin(egui::Margin::symmetric(
                    space::MD as i8 + 2,
                    space::SM as i8 + 2,
                ))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    item_row(ui, incoming, &q.src, newer == Some(true));
                    ui.add_space(space::SM);
                    item_row(ui, t!("conflict.existing"), &q.dst, newer == Some(false));
                });
            ui.add_space(space::MD);
            ui.checkbox(&mut self.all, t!("conflict.apply_all"));
            ui.add_space(space::LG);
            ui.horizontal(|ui| {
                if widgets::button(ui, t!("common.cancel"), ButtonKind::Secondary, true).clicked() {
                    answer = Some(ConflictChoice::Cancel);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right to left: the default (primary) button sits on
                    // the right, as in macOS alerts.
                    let mut order: Vec<ConflictChoice> = vec![default];
                    order.extend(q.choices.iter().rev().copied().filter(|c| *c != default));
                    for c in order {
                        let kind = if c == default {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Secondary
                        };
                        if widgets::button(ui, choice_label(c), kind, true).clicked() {
                            answer = Some(c);
                        }
                    }
                });
            });
        });
        if answer.is_none() {
            ctx.input(|i| {
                if i.key_pressed(Key::Escape) {
                    answer = Some(ConflictChoice::Cancel);
                } else if i.key_pressed(Key::Enter) {
                    answer = Some(default);
                }
            });
        }
        answer
    }
}

/// "Being copied  [icon] report.pdf  1.2 MB · 12.10.2026, 14:02  [Newer]".
fn item_row(ui: &mut egui::Ui, role: &str, item: &ConflictItem, newer: bool) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        let name = item.path.file_name();
        let kind = icons::file_kind(name, item.is_dir);
        let (glyph, tint) = if kind == icons::FileKind::Other {
            (icons::FILE, p.generic)
        } else {
            (kind.icon(), kind.tint(p))
        };
        let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
        widgets::paint_icon(ui, r, glyph, 24.0, tint, false);
        ui.add_space(space::XS);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::XXS;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(role)
                        .font(theme::semibold(text::SMALL))
                        .color(p.text_secondary),
                );
                if newer {
                    let label = t!("conflict.newer");
                    widgets::chip(ui, None, label, p.success, p.success_soft)
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
                }
            });
            ui.add(
                egui::Label::new(
                    RichText::new(name)
                        .font(theme::medium(text::BODY))
                        .color(p.text),
                )
                .truncate(),
            );
            let size = match item.size {
                Some(s) if !item.is_dir => crate::util::format_size(s),
                _ => t!("conflict.folder").to_string(),
            };
            let date = item
                .modified
                .map(crate::views::format_date)
                .unwrap_or_else(|| "—".into());
            ui.label(
                RichText::new(t!("conflict.details", size = size, date = date))
                    .font(theme::regular(text::SMALL))
                    .color(p.text_secondary),
            );
        });
    });
}
