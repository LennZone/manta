//! Bottom panel listing transfer jobs from the `TransferManager`: one compact
//! row per job with a kind icon, a thin progress line and a status line.
//!
//! Jobs can be paused and resumed (per job and all at once). A job whose
//! network dropped waits ("Waiting for connection … attempt 3, next in
//! 12 s") and continues by itself; a job restored after a restart whose
//! server isn't connected offers "Reconnect". While the Mac is offline a
//! banner says so.

use std::collections::HashSet;

use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke};
use manta_ops::{Endpoint, JobId, JobKind, JobState, JobStatus, TransferManager};

use crate::commands::Cmd;
use crate::icons;
use crate::theme::{self, pal, radius, space, text, Palette};
use crate::util;
use crate::widgets;

/// Phosphor "pause".
const PAUSE: &str = "\u{E39E}";
/// Phosphor "wifi-slash".
const WIFI_SLASH: &str = "\u{E4F2}";

#[derive(Default)]
pub struct TransfersView {
    /// Jobs whose error / note list is expanded.
    expanded: HashSet<JobId>,
    /// "Reconnect" clicks on jobs that need a connection; the app takes
    /// them ([`Self::take_reconnect_requests`]).
    reconnect_requests: Vec<Endpoint>,
    /// Show the offline banner regardless of the network monitor (tests).
    pub force_offline: bool,
}

pub fn is_finished(state: &JobState) -> bool {
    state.is_finished()
}

fn kind_style(kind: JobKind, p: &Palette) -> (&'static str, Color32) {
    match kind {
        JobKind::Copy => (icons::COPY, p.accent_text),
        JobKind::Move => (icons::ARROW_LINE_RIGHT, p.image),
        JobKind::Delete => (icons::TRASH, p.danger),
        JobKind::Extract => (icons::ARCHIVE, p.archive),
        JobKind::Compress => (icons::FILE_ARCHIVE, p.archive),
    }
}

fn fraction(job: &JobStatus) -> f32 {
    let f = if job.bytes_total > 0 {
        job.bytes_done as f32 / job.bytes_total as f32
    } else if job.files_total > 0 {
        job.files_done as f32 / job.files_total as f32
    } else if job.state == JobState::Done {
        1.0
    } else {
        0.0
    };
    f.clamp(0.0, 1.0)
}

/// Can the user pause this job right now?
fn can_pause(job: &JobStatus) -> bool {
    job.resume.can_pause
        && matches!(
            job.state,
            JobState::Queued | JobState::Running | JobState::WaitingForNetwork { .. }
        )
}

/// Can the user resume this job right now?
fn can_resume(job: &JobStatus) -> bool {
    matches!(
        job.state,
        JobState::Paused | JobState::WaitingForNetwork { .. }
    )
}

/// Bytes progress "1.2 GB of 3.0 GB · 2 of 5 files" (or entries for deletes).
fn progress_text(job: &JobStatus) -> String {
    if job.kind == JobKind::Delete {
        return t!(
            "transfers.deleted_of",
            done = util::format_count(job.files_done),
            total = util::format_count(job.files_total)
        );
    }
    t!(
        "transfers.progress",
        count = job.files_total,
        done = util::format_size(job.bytes_done),
        total = util::format_size(job.bytes_total),
        files_done = util::format_count(job.files_done)
    )
}

/// The secondary line under a job's title.
pub(crate) fn detail_line(job: &JobStatus) -> String {
    match &job.state {
        JobState::Queued => {
            if job.bytes_total > 0 {
                t!(
                    "transfers.waiting_size",
                    size = util::format_size(job.bytes_total)
                )
            } else {
                t!("transfers.waiting").into()
            }
        }
        JobState::Running if job.kind == JobKind::Delete => progress_text(job),
        JobState::Running => {
            let remaining = job.bytes_total.saturating_sub(job.bytes_done);
            let mut s = String::new();
            if let Some(from) = job.resume.resumed_from {
                s.push_str(&t!(
                    "transfers.resuming_from",
                    size = util::format_size(from)
                ));
                s.push_str(" · ");
            }
            s.push_str(&progress_text(job));
            if job.speed > 0.0 {
                s.push_str(&t!(
                    "transfers.speed_left",
                    speed = util::format_speed(job.speed),
                    eta = util::format_eta(remaining, job.speed)
                ));
            }
            s
        }
        JobState::Paused if job.bytes_total > 0 => t!(
            "transfers.paused",
            done = util::format_size(job.bytes_done),
            total = util::format_size(job.bytes_total)
        ),
        JobState::Paused => t!("transfers.paused_plain").into(),
        JobState::WaitingForNetwork { attempt, .. } => match job.state.retry_in_secs() {
            Some(secs) if secs > 0 => {
                t!("transfers.waiting_network", attempt = attempt, secs = secs)
            }
            _ => t!("transfers.waiting_network_now", attempt = attempt),
        },
        JobState::NeedsConnection(server) => {
            t!(
                "transfers.needs_connection",
                server = crate::i18n::fs_label(server)
            )
        }
        JobState::WaitingForDecision(q) => {
            t!("transfers.waiting_decision", name = q.dst.path.file_name())
        }
        JobState::Done => {
            let what = if job.kind == JobKind::Delete {
                t!("transfers.items", count = job.files_done)
            } else {
                format!(
                    "{} · {}",
                    t!("transfers.files", count = job.files_done),
                    util::format_size(job.bytes_done)
                )
            };
            t!("transfers.done", what = what)
        }
        JobState::Failed(e) => t!("transfers.failed", error = crate::i18n::localize_error(e)),
        JobState::Cancelled => t!("transfers.stopped").into(),
    }
}

/// The transfer manager's resume notes ("PATH: what happened") in the UI
/// language; unknown notes are shown as they are.
fn localize_note(note: &str) -> String {
    let path = note.split(": ").next().unwrap_or(note);
    if note.ends_with("the source changed since the interruption; copying it again from the start")
    {
        t!("transfers.note.source_changed", path = path)
    } else if note.ends_with("can't continue partial files; copying it again from the start") {
        t!("transfers.note.no_append", path = path)
    } else if note
        .ends_with("the partial file is larger than the source; copying it again from the start")
    {
        t!("transfers.note.part_larger", path = path)
    } else if note.ends_with("the upload session expired; uploading again from the start") {
        t!("transfers.note.session_expired", path = path)
    } else {
        note.to_string()
    }
}

impl TransfersView {
    /// "Reconnect" requests since the last call.
    pub fn take_reconnect_requests(&mut self) -> Vec<Endpoint> {
        std::mem::take(&mut self.reconnect_requests)
    }

    /// Returns true when the user asked to close the panel.
    pub fn ui(&mut self, ui: &mut egui::Ui, manager: &TransferManager, jobs: &[JobStatus]) -> bool {
        let p = pal(ui);
        let mut close = false;
        let active = jobs.iter().filter(|j| !is_finished(&j.state)).count();
        let offline = self.force_offline || !manager.is_online();

        // Header.
        egui::Frame::new()
            .fill(p.chrome)
            .inner_margin(egui::Margin::symmetric(space::LG as i8, space::SM as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t!("transfers.title"))
                            .font(theme::semibold(text::BODY))
                            .color(p.text),
                    );
                    if active > 0 {
                        widgets::chip(
                            ui,
                            None,
                            &t!("transfers.active", count = active),
                            p.accent_text,
                            p.accent_soft,
                        );
                    } else if !jobs.is_empty() {
                        ui.label(
                            RichText::new(t!("transfers.all_done"))
                                .font(theme::regular(text::SMALL))
                                .color(p.text_secondary),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = widgets::mini_icon_button(ui, icons::X, p.text_secondary);
                        if widgets::tooltip(r, t!("transfers.hide"), Cmd::Transfers.shortcut())
                            .clicked()
                        {
                            close = true;
                        }
                        crate::help::help_button(ui, "05-dateien.md#pausieren");
                        let any_finished = jobs.iter().any(|j| is_finished(&j.state));
                        let r = ui.add_enabled(
                            any_finished,
                            egui::Button::new(
                                RichText::new(format!(
                                    "{}  {}",
                                    icons::BROOM,
                                    t!("transfers.clear_finished")
                                ))
                                .font(theme::regular(text::SMALL)),
                            )
                            .frame(false),
                        );
                        if r.clicked() {
                            manager.clear_finished();
                            self.expanded.retain(|id| {
                                jobs.iter().any(|j| j.id == *id && !is_finished(&j.state))
                            });
                        }
                        if jobs.iter().any(can_resume) {
                            let r = ui.add(
                                egui::Button::new(
                                    RichText::new(format!(
                                        "{}  {}",
                                        icons::PLAY,
                                        t!("transfers.resume_all")
                                    ))
                                    .font(theme::regular(text::SMALL)),
                                )
                                .frame(false),
                            );
                            if r.clicked() {
                                manager.resume_all();
                            }
                        }
                        if jobs.iter().any(can_pause) {
                            let r = ui.add(
                                egui::Button::new(
                                    RichText::new(format!(
                                        "{PAUSE}  {}",
                                        t!("transfers.pause_all")
                                    ))
                                    .font(theme::regular(text::SMALL)),
                                )
                                .frame(false),
                            );
                            if r.clicked() {
                                manager.pause_all();
                            }
                        }
                    });
                });
            });
        let top = ui.cursor().top();
        ui.painter()
            .hline(ui.max_rect().x_range(), top, Stroke::new(1.0, p.separator));

        if offline {
            offline_banner(ui, p);
        }

        if jobs.is_empty() {
            let rect = ui.available_rect_before_wrap();
            widgets::empty_state(
                ui,
                rect.translate(egui::vec2(0.0, 18.0)),
                icons::ARROWS_DOWN_UP,
                p.text_tertiary,
                t!("transfers.empty"),
                t!("transfers.empty.note"),
                &[],
            );
            return close;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, job) in jobs.iter().enumerate() {
                    self.job_row(ui, p, manager, job, i);
                }
            });
        close
    }

    fn job_row(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        manager: &TransferManager,
        job: &JobStatus,
        index: usize,
    ) {
        let w = ui.available_width();
        let h = 52.0;
        let (rect, row) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
        if index % 2 == 1 {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, p.stripe);
        }
        let (glyph, tint) = kind_style(job.kind, p);
        let finished = is_finished(&job.state);
        let waiting = matches!(job.state, JobState::WaitingForNetwork { .. });
        let needs = matches!(job.state, JobState::NeedsConnection(_));
        let deciding = matches!(job.state, JobState::WaitingForDecision(_));

        // Kind badge.
        let badge = egui::Rect::from_center_size(
            egui::pos2(rect.left() + space::LG + 14.0, rect.center().y),
            egui::vec2(28.0, 28.0),
        );
        ui.painter().rect_filled(
            badge,
            CornerRadius::same(radius::PANEL),
            tint.gamma_multiply(if p.dark { 0.22 } else { 0.12 }),
        );
        widgets::paint_icon(ui, badge, glyph, 15.0, tint, false);

        // Right side: state / actions.
        let right_w = if needs { 170.0 } else { 136.0 };
        let action_rect = egui::Rect::from_min_max(
            egui::pos2(rect.right() - right_w, rect.top()),
            egui::pos2(rect.right() - space::MD, rect.bottom()),
        );
        let text_left = badge.right() + space::MD + 2.0;
        let text_right = action_rect.left() - space::MD;
        let line_w = (text_right - text_left).max(40.0);

        // Title.
        let mut job_t = egui::text::LayoutJob::simple_singleline(
            crate::i18n::localize_job_title(&job.title),
            theme::medium(text::BODY),
            p.text,
        );
        job_t.wrap = egui::text::TextWrapping::truncate_at_width(line_w);
        let g = ui.painter().layout_job(job_t);
        ui.painter()
            .galley(egui::pos2(text_left, rect.top() + 8.0), g, p.text);

        // Progress line (only while the job isn't finished).
        let bar_y = rect.top() + 27.0;
        if !finished {
            let track =
                egui::Rect::from_min_size(egui::pos2(text_left, bar_y), egui::vec2(line_w, 4.0));
            ui.painter()
                .rect_filled(track, CornerRadius::same(2), p.separator);
            let f = fraction(job);
            if f > 0.0 {
                let mut fill = track;
                fill.set_width(track.width() * f);
                let color = match job.state {
                    JobState::Running => p.accent,
                    JobState::WaitingForNetwork { .. }
                    | JobState::NeedsConnection(_)
                    | JobState::WaitingForDecision(_) => p.warning,
                    _ => p.text_tertiary,
                };
                ui.painter().rect_filled(fill, CornerRadius::same(2), color);
            }
        }

        // Detail line.
        let (detail_color, detail_y) = match &job.state {
            JobState::Failed(_) => (p.danger, rect.top() + 28.0),
            _ if finished => (p.text_secondary, rect.top() + 28.0),
            _ if waiting || needs || deciding => (p.warning, bar_y + 8.0),
            _ => (p.text_secondary, bar_y + 8.0),
        };
        let mut detail = detail_line(job);
        if job.state == JobState::Running {
            if let Some(cur) = &job.current_file {
                detail.push_str(&format!(" · {cur}"));
            }
        }
        let mut dj = egui::text::LayoutJob::simple_singleline(
            detail,
            theme::regular(text::CAPTION + 0.5),
            detail_color,
        );
        dj.wrap = egui::text::TextWrapping::truncate_at_width(line_w);
        let g = ui.painter().layout_job(dj);
        ui.painter()
            .galley(egui::pos2(text_left, detail_y), g, detail_color);
        // The network error behind a wait, on hover.
        if let JobState::WaitingForNetwork { error, .. } = &job.state {
            row.on_hover_text(crate::i18n::localize_error(error));
        }

        // Actions / state on the right.
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(action_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                match &job.state {
                    JobState::Done => {
                        widgets::icon(ui, icons::CHECK_CIRCLE, 16.0, p.success);
                    }
                    JobState::Failed(_) => {
                        widgets::icon(ui, icons::WARNING_CIRCLE, 16.0, p.danger);
                    }
                    JobState::Cancelled => {
                        widgets::icon(ui, icons::STOP, 15.0, p.text_tertiary);
                    }
                    state => {
                        let r = widgets::mini_icon_button(ui, icons::X_CIRCLE, p.text_secondary);
                        if widgets::tooltip(r, t!("transfers.stop"), "").clicked() {
                            manager.cancel(job.id);
                        }
                        if can_resume(job) {
                            let (glyph, tip) = if waiting {
                                (icons::ARROW_CLOCKWISE, t!("transfers.retry_now"))
                            } else {
                                (icons::PLAY, t!("transfers.resume"))
                            };
                            let r = widgets::mini_icon_button(ui, glyph, p.text_secondary);
                            if widgets::tooltip(r, tip, "").clicked() {
                                manager.resume(job.id);
                            }
                        }
                        if can_pause(job) {
                            let r = widgets::mini_icon_button(ui, PAUSE, p.text_secondary);
                            if widgets::tooltip(r, t!("transfers.pause"), "").clicked() {
                                manager.pause(job.id);
                            }
                        }
                        if let JobState::NeedsConnection(server) = state {
                            let server = crate::i18n::fs_label(server);
                            let label = t!("transfers.reconnect");
                            let r = widgets::chip(
                                ui,
                                Some(icons::PLUGS),
                                label,
                                p.accent_text,
                                p.accent_soft,
                            )
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, label)
                            });
                            if widgets::tooltip(
                                r,
                                &t!("transfers.reconnect.tip", server = server),
                                "",
                            )
                            .clicked()
                            {
                                self.reconnect_requests
                                    .extend(job.resume.needs.iter().cloned());
                            }
                        }
                        if *state == JobState::Running {
                            ui.label(
                                RichText::new(t!(
                                    "transfers.percent",
                                    n = format!("{:.0}", fraction(job) * 100.0)
                                ))
                                .font(theme::medium(text::SMALL))
                                .color(p.text),
                            );
                        }
                    }
                }
                let open = self.expanded.contains(&job.id);
                let caret = if open {
                    icons::CARET_DOWN
                } else {
                    icons::CARET_RIGHT
                };
                let mut toggle = false;
                if !job.errors.is_empty() {
                    let label = t!("transfers.issues", count = job.errors.len());
                    toggle |= widgets::chip(ui, Some(caret), &label, p.warning, p.warning_soft)
                        .interact(Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                } else if !job.resume.log.is_empty() {
                    let label = t!("transfers.notes", count = job.resume.log.len());
                    toggle |= widgets::chip(ui, Some(caret), &label, p.text_secondary, p.hover)
                        .interact(Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                }
                if toggle {
                    if open {
                        self.expanded.remove(&job.id);
                    } else {
                        self.expanded.insert(job.id);
                    }
                }
            },
        );

        if self.expanded.contains(&job.id) {
            let bg = if job.errors.is_empty() {
                p.hover
            } else {
                p.warning_soft.gamma_multiply(0.6)
            };
            egui::Frame::new()
                .fill(bg)
                .inner_margin(egui::Margin {
                    left: (text_left - rect.left()) as i8,
                    right: 12,
                    top: 4,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for e in &job.errors {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("•  {}", crate::i18n::localize_error(e)))
                                    .font(theme::regular(text::SMALL))
                                    .color(p.text),
                            )
                            .wrap(),
                        );
                    }
                    for n in &job.resume.log {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}  {}", icons::INFO, localize_note(n)))
                                    .font(theme::regular(text::SMALL))
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                });
        }
    }
}

// ------------------------------------------------------------ launch prompt

/// One interrupted job in the launch prompt.
pub struct PromptRow {
    pub kind: JobKind,
    pub title: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_total: u64,
    /// A server it needs that isn't connected (label).
    pub needs: Option<String>,
}

/// What the user chose in the launch prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeChoice {
    Resume,
    Discard,
    /// Closed without choosing: ask again next time.
    Later,
}

/// "Resume interrupted transfers?" at launch (jobs from the journal).
pub struct ResumePrompt {
    pub rows: Vec<PromptRow>,
}

/// Rows shown before "and N more".
const PROMPT_ROWS: usize = 4;

impl ResumePrompt {
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<ResumeChoice> {
        let p = theme::pal_ctx(ctx);
        let mut choice = None;
        let n = self.rows.len();
        let modal = crate::dialogs::dialog_modal("resume-transfers", ctx).show(ctx, |ui| {
            ui.set_width(460.0);
            crate::dialogs::dialog_header(
                ui,
                icons::ARROWS_CLOCKWISE,
                p.accent_text,
                p.accent_soft,
                t!("resume.title"),
                &t!("resume.subtitle", count = n),
            );
            ui.add_space(space::MD);
            egui::Frame::new()
                .fill(p.content)
                .stroke(Stroke::new(1.0, p.separator))
                .corner_radius(CornerRadius::same(radius::PANEL))
                .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = space::SM;
                    for row in self.rows.iter().take(PROMPT_ROWS) {
                        prompt_row(ui, p, row);
                    }
                    if n > PROMPT_ROWS {
                        ui.label(
                            RichText::new(t!("resume.more", count = n - PROMPT_ROWS))
                                .font(theme::regular(text::SMALL))
                                .color(p.text_secondary),
                        );
                    }
                });
            ui.add_space(space::MD);
            ui.add(
                egui::Label::new(RichText::new(t!("resume.body")).color(p.text_secondary)).wrap(),
            );
            ui.add_space(space::LG);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(
                    ui,
                    t!("resume.continue"),
                    widgets::ButtonKind::Primary,
                    true,
                )
                .clicked()
                {
                    choice = Some(ResumeChoice::Resume);
                }
                if widgets::button(
                    ui,
                    t!("resume.discard"),
                    widgets::ButtonKind::Secondary,
                    true,
                )
                .clicked()
                {
                    choice = Some(ResumeChoice::Discard);
                }
            });
        });
        if choice.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            choice = Some(ResumeChoice::Resume);
        }
        if choice.is_none() && modal.should_close() {
            choice = Some(ResumeChoice::Later);
        }
        choice
    }
}

fn prompt_row(ui: &mut egui::Ui, p: &Palette, row: &PromptRow) {
    let (glyph, tint) = kind_style(row.kind, p);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::hover());
        ui.painter().rect_filled(
            r,
            CornerRadius::same(radius::CONTROL),
            tint.gamma_multiply(if p.dark { 0.22 } else { 0.12 }),
        );
        widgets::paint_icon(ui, r, glyph, 13.0, tint, false);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(
                egui::Label::new(
                    RichText::new(crate::i18n::localize_job_title(&row.title))
                        .font(theme::medium(text::BODY))
                        .color(p.text),
                )
                .truncate(),
            );
            let mut detail = if row.bytes_total > 0 {
                t!(
                    "resume.progress",
                    done = util::format_size(row.bytes_done),
                    total = util::format_size(row.bytes_total)
                )
            } else {
                t!("resume.files", count = row.files_total)
            };
            if let Some(server) = &row.needs {
                detail.push_str(" · ");
                detail.push_str(&t!("resume.needs", server = server));
            }
            ui.add(
                egui::Label::new(
                    RichText::new(detail)
                        .font(theme::regular(text::SMALL))
                        .color(if row.needs.is_some() {
                            p.warning
                        } else {
                            p.text_secondary
                        }),
                )
                .truncate(),
            );
        });
    });
}

/// "No internet connection" strip under the header.
fn offline_banner(ui: &mut egui::Ui, p: &Palette) {
    egui::Frame::new()
        .fill(if p.dark {
            p.warning_soft.gamma_multiply(0.7)
        } else {
            p.warning_soft
        })
        .inner_margin(egui::Margin::symmetric(space::LG as i8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                widgets::icon(ui, WIFI_SLASH, 15.0, p.warning);
                ui.label(
                    RichText::new(t!("transfers.offline"))
                        .font(theme::semibold(text::SMALL))
                        .color(p.text),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(t!("transfers.offline.note"))
                            .font(theme::regular(text::SMALL))
                            .color(p.text_secondary),
                    )
                    .truncate(),
                );
            });
        });
    let y = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), y, Stroke::new(1.0, p.separator));
}

#[cfg(test)]
#[path = "transfers_ux_tests.rs"]
mod ux_tests;

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn job(state: JobState) -> JobStatus {
        JobStatus {
            id: 1,
            kind: JobKind::Copy,
            title: "Copy".into(),
            state,
            current_file: None,
            bytes_done: 500,
            bytes_total: 1000,
            files_done: 1,
            files_total: 2,
            speed: 100.0,
            errors: vec![],
            resume: manta_ops::ResumeInfo {
                can_pause: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn detail_lines() {
        assert_eq!(
            detail_line(&job(JobState::Running)),
            "500 bytes of 1.0 KB · 1 of 2 files · 100 bytes/s · 5s left"
        );
        assert_eq!(
            detail_line(&job(JobState::Done)),
            "Done · 1 file · 500 bytes"
        );
        assert_eq!(detail_line(&job(JobState::Cancelled)), "Stopped");
        assert!((fraction(&job(JobState::Running)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn resume_states() {
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            assert_eq!(
                detail_line(&job(JobState::Paused)),
                "Pausiert · 500 Byte von 1,0 KB"
            );
            let next = std::time::Instant::now() + std::time::Duration::from_millis(11_500);
            let w = job(JobState::WaitingForNetwork {
                attempt: 3,
                next_retry: Some(next),
                error: "reset".into(),
            });
            assert_eq!(
                detail_line(&w),
                "Wartet auf Verbindung … (Versuch 3, nächster in 12 s)"
            );
            let mut r = job(JobState::Running);
            r.bytes_done = 1_500_000_000;
            r.bytes_total = 3_000_000_000;
            r.resume.resumed_from = Some(1_200_000_000);
            assert!(
                detail_line(&r).starts_with("Wird fortgesetzt ab 1,2 GB · "),
                "{}",
                detail_line(&r)
            );
            assert_eq!(
                detail_line(&job(JobState::NeedsConnection("Media (SFTP)".into()))),
                "Benötigt eine Verbindung zu Media (SFTP)"
            );
            assert_eq!(
                localize_note("/a/b.iso: the source changed since the interruption; copying it again from the start"),
                "/a/b.iso: Die Quelle hat sich seit der Unterbrechung geändert; wird von vorn kopiert"
            );
            assert_eq!(localize_note("something else"), "something else");
        });
        // Pause / resume availability.
        assert!(can_pause(&job(JobState::Running)));
        assert!(can_pause(&job(JobState::Queued)));
        assert!(!can_pause(&job(JobState::Paused)));
        assert!(can_resume(&job(JobState::Paused)));
        assert!(!can_resume(&job(JobState::Running)));
        let mut compress = job(JobState::Running);
        compress.resume.can_pause = false;
        assert!(!can_pause(&compress));
    }
}
