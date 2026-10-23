//! The status bar's progress pill: what the transfer jobs are doing while
//! the transfers panel (⌥⌘L) is hidden, and what a cloud provider still
//! uploads after a copy into its folder.
//!
//! - one job: "Kopiere 3 Objekte … 42 %" with a progress ring; several:
//!   "2 Übertragungen · 37 %"; paused / queued / waiting for the network
//!   as text ("Bewege „a.iso“ · pausiert"),
//! - when the last job is done: "Fertig" for a few seconds ([`DONE_LINGER`]),
//!   "Gestoppt" when it was cancelled,
//! - a failed job (or one that finished with skipped / failed items) keeps
//!   a red pill until the user looks at it (click, or opening the panel),
//! - with no job pill: "Google Drive lädt 2 Objekte hoch" (or "… 1
//!   Upload-Fehler", red) for the cloud folders the panes show.
//!
//! [`TransferPill`] is a small state machine over the job snapshots with an
//! explicit clock, so tests drive it without waiting.

use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Sense, Stroke};
use manta_ops::{JobId, JobKind, JobState, JobStatus};

use crate::icons;
use crate::theme::{self, pal, radius, text, Palette};
use crate::widgets;

/// How long "Fertig" stays after the last job finished.
pub const DONE_LINGER: Duration = Duration::from_secs(4);

/// What the pill shows (colour, icon / ring).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillKind {
    /// At least one job runs: accent, progress ring.
    Running,
    /// Queued or waiting for the network: warning tint, ring.
    Waiting,
    /// Everything paused: grey ring.
    Paused,
    /// The last jobs finished fine.
    Done,
    /// The last jobs were cancelled.
    Stopped,
    /// A job failed or finished with problems (stays until seen).
    Failed,
    /// A cloud provider still uploads items of a visible folder.
    Uploading,
    /// A cloud provider couldn't upload items of a visible folder.
    UploadError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    pub kind: PillKind,
    pub text: String,
    /// Progress 0–1 for the ring (`None`: an icon instead).
    pub fraction: Option<f32>,
}

/// Remembers which jobs it saw running, when the last one finished and
/// which failures the user hasn't seen yet.
pub struct TransferPill {
    seen: HashSet<JobId>,
    finished_at: Option<Instant>,
    /// Jobs that finished since the last idle moment were all cancelled.
    all_cancelled: bool,
    errors: BTreeSet<JobId>,
    pub linger: Duration,
}

impl Default for TransferPill {
    fn default() -> Self {
        TransferPill {
            seen: HashSet::new(),
            finished_at: None,
            all_cancelled: true,
            errors: BTreeSet::new(),
            linger: DONE_LINGER,
        }
    }
}

/// A finished job the user should look at.
fn has_problem(j: &JobStatus) -> bool {
    matches!(j.state, JobState::Failed(_)) || (j.state == JobState::Done && !j.errors.is_empty())
}

/// Progress of `jobs` (bytes, else files).
fn fraction(jobs: &[&JobStatus]) -> f32 {
    let (done, total) = jobs.iter().fold((0u64, 0u64), |(d, t), j| {
        (d + j.bytes_done, t + j.bytes_total)
    });
    let f = if total > 0 {
        done as f32 / total as f32
    } else {
        let (fd, ft) = jobs.iter().fold((0u64, 0u64), |(d, t), j| {
            (d + j.files_done, t + j.files_total)
        });
        if ft > 0 {
            fd as f32 / ft as f32
        } else {
            0.0
        }
    };
    f.clamp(0.0, 1.0)
}

/// "Kopiere 3 Objekte", "Bewege „a.iso“", ...
pub fn job_action(job: &JobStatus) -> String {
    let what = crate::i18n::job_object(&job.title)
        .unwrap_or_else(|| t!("pane.items", count = job.files_total.max(1)));
    match job.kind {
        JobKind::Copy => t!("pill.copy", what = what),
        JobKind::Move => t!("pill.move", what = what),
        JobKind::Delete => t!("pill.delete", what = what),
        JobKind::Extract => t!("pill.extract", what = what),
        JobKind::Compress => t!("pill.compress", what = what),
    }
}

impl TransferPill {
    /// Take a job snapshot (once per frame).
    pub fn update(&mut self, jobs: &[JobStatus], now: Instant) {
        let idle_before = self.seen.is_empty();
        for j in jobs {
            if !j.state.is_finished() {
                if idle_before && self.seen.is_empty() {
                    // A new batch: forget the last "Fertig".
                    self.finished_at = None;
                    self.all_cancelled = true;
                }
                self.seen.insert(j.id);
            } else if self.seen.remove(&j.id) {
                self.finished_at = Some(now);
                self.all_cancelled &= j.state == JobState::Cancelled;
                if has_problem(j) {
                    self.errors.insert(j.id);
                }
            }
        }
        // "Clear finished" (or a job gone otherwise): forget it.
        self.seen.retain(|id| jobs.iter().any(|j| j.id == *id));
        self.errors.retain(|id| jobs.iter().any(|j| j.id == *id));
    }

    /// The user looked at the transfers (clicked the pill, opened the
    /// panel): errors and "Fertig" go away.
    pub fn acknowledge(&mut self) {
        self.errors.clear();
        self.finished_at = None;
    }

    /// When the pill changes by itself next ("Fertig" expiring).
    pub fn repaint_in(&self, now: Instant) -> Option<Duration> {
        if !self.seen.is_empty() || !self.errors.is_empty() {
            return None;
        }
        let at = self.finished_at?;
        (self.linger)
            .checked_sub(now.duration_since(at))
            .map(|d| d + Duration::from_millis(20))
    }

    /// The pill for `jobs` at `now` (`None`: nothing to show).
    pub fn pill(&self, jobs: &[JobStatus], now: Instant) -> Option<Pill> {
        let active: Vec<&JobStatus> = jobs.iter().filter(|j| !j.state.is_finished()).collect();
        if !active.is_empty() {
            let frac = fraction(&active);
            let percent = format!("{:.0}", frac * 100.0);
            let running = active.iter().any(|j| j.state == JobState::Running);
            let network = active.iter().any(|j| {
                matches!(
                    j.state,
                    JobState::WaitingForNetwork { .. } | JobState::NeedsConnection(_)
                )
            });
            let paused = active.iter().all(|j| j.state == JobState::Paused);
            let action = match active.as_slice() {
                [one] => job_action(one),
                many => t!("pill.many", count = many.len()),
            };
            let (kind, text) = if running {
                let text = match active.len() {
                    1 => t!("pill.running", action = action, percent = percent),
                    n => t!("status.transfers", count = n, percent = percent),
                };
                (PillKind::Running, text)
            } else if network {
                (
                    PillKind::Waiting,
                    t!("pill.waiting_network", action = action),
                )
            } else if paused {
                (PillKind::Paused, t!("pill.paused", action = action))
            } else {
                (PillKind::Waiting, t!("pill.queued", action = action))
            };
            return Some(Pill {
                kind,
                text,
                fraction: Some(frac),
            });
        }
        if !self.errors.is_empty() {
            let failed: Vec<&JobStatus> = jobs
                .iter()
                .filter(|j| self.errors.contains(&j.id))
                .collect();
            let text = match failed.as_slice() {
                [one] if matches!(one.state, JobState::Failed(_)) => t!("pill.failed").to_string(),
                [_] => t!("pill.done_errors").to_string(),
                many => t!("pill.errors_many", count = many.len()),
            };
            return Some(Pill {
                kind: PillKind::Failed,
                text,
                fraction: None,
            });
        }
        let at = self.finished_at?;
        if now.duration_since(at) >= self.linger {
            return None;
        }
        Some(if self.all_cancelled {
            Pill {
                kind: PillKind::Stopped,
                text: t!("pill.stopped").into(),
                fraction: None,
            }
        } else {
            Pill {
                kind: PillKind::Done,
                text: t!("pill.done").into(),
                fraction: None,
            }
        })
    }
}

/// "Google Drive lädt 2 Objekte hoch" / "Google Drive: 1 Upload-Fehler"
/// for a visible cloud folder (`None`: nothing pending).
pub fn cloud_pill(provider: &str, uploading: usize, failed: usize) -> Option<Pill> {
    if failed > 0 {
        let text = t!("pill.upload_errors", count = failed, provider = provider);
        return Some(Pill {
            kind: PillKind::UploadError,
            text,
            fraction: None,
        });
    }
    (uploading > 0).then(|| Pill {
        kind: PillKind::Uploading,
        text: t!(
            "pill.uploading",
            provider = provider,
            what = t!("pane.items", count = uploading)
        ),
        fraction: None,
    })
}

/// The tooltip of a job pill: each unfinished job's title and state.
pub fn tooltip(jobs: &[JobStatus]) -> String {
    let mut lines: Vec<String> = jobs
        .iter()
        .filter(|j| !j.state.is_finished())
        .take(5)
        .map(|j| {
            format!(
                "{}\n{}",
                crate::i18n::localize_job_title(&j.title),
                crate::transfers::detail_line(j)
            )
        })
        .collect();
    lines.push(t!("pill.tip").to_string());
    lines.join("\n\n")
}

fn colors(kind: PillKind, p: &Palette) -> (Color32, Color32) {
    match kind {
        PillKind::Running | PillKind::Uploading => (p.accent_text, p.accent_soft),
        PillKind::Waiting => (p.warning, p.warning_soft),
        PillKind::Paused | PillKind::Stopped => (p.text_secondary, p.hover),
        PillKind::Done => (p.success, p.success_soft),
        PillKind::Failed | PillKind::UploadError => (p.danger, p.danger_soft),
    }
}

/// A ring that fills clockwise from the top with `frac` (0–1).
pub fn paint_ring(
    painter: &egui::Painter,
    center: egui::Pos2,
    r: f32,
    frac: f32,
    track: Color32,
    fill: Color32,
) {
    painter.circle_stroke(center, r, Stroke::new(2.0, track));
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.0 {
        return;
    }
    let steps = ((48.0 * frac).ceil() as usize).max(2);
    let start = -std::f32::consts::FRAC_PI_2;
    let pts: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let a = start + std::f32::consts::TAU * frac * i as f32 / steps as f32;
            center + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, fill)));
}

/// Paint `pill` (click: show the transfers). Screen readers (and tests)
/// see its text.
/// The width [`show`] takes for `pill`.
pub fn width(ui: &egui::Ui, pill: &Pill) -> f32 {
    let font = theme::medium(text::CAPTION + 0.5);
    ui.painter()
        .layout_no_wrap(pill.text.clone(), font, egui::Color32::WHITE)
        .size()
        .x
        + 32.0
}

pub fn show(ui: &mut egui::Ui, pill: &Pill) -> egui::Response {
    let p = pal(ui);
    let (fg, bg) = colors(pill.kind, p);
    let font = theme::medium(text::CAPTION + 0.5);
    let galley = ui.painter().layout_no_wrap(pill.text.clone(), font, fg);
    let size = egui::vec2(galley.size().x + 32.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() {
        bg.gamma_multiply(1.4)
    } else {
        bg
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::PILL), fill);
    let c = egui::pos2(rect.left() + 12.0, rect.center().y);
    let icon_rect = egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0));
    match (pill.kind, pill.fraction) {
        (_, Some(f)) => paint_ring(ui.painter(), c, 5.0, f, fg.gamma_multiply(0.25), fg),
        (PillKind::Uploading, None) => {
            let a = crate::cloud::upload_pulse(ui.input(|i| i.time));
            ui.ctx().request_repaint_after(Duration::from_millis(80));
            widgets::paint_icon(
                ui,
                icon_rect,
                icons::CLOUD_ARROW_UP,
                12.0,
                fg.gamma_multiply(a),
                false,
            );
        }
        (kind, None) => {
            let glyph = match kind {
                PillKind::Done => icons::CHECK_CIRCLE,
                PillKind::Stopped => icons::STOP,
                _ => icons::WARNING_CIRCLE,
            };
            widgets::paint_icon(ui, icon_rect, glyph, 12.0, fg, false);
        }
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    let text = pill.text.clone();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &text));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{with_locale, Locale};

    fn job(
        id: u64,
        kind: JobKind,
        title: &str,
        state: JobState,
        done: u64,
        total: u64,
    ) -> JobStatus {
        JobStatus {
            id,
            kind,
            title: title.into(),
            state,
            current_file: None,
            bytes_done: done,
            bytes_total: total,
            files_done: 1,
            files_total: 3,
            speed: 1000.0,
            errors: vec![],
            resume: manta_ops::ResumeInfo {
                can_pause: true,
                ..Default::default()
            },
        }
    }

    fn text(t: &TransferPill, jobs: &[JobStatus], now: Instant) -> Option<(PillKind, String)> {
        t.pill(jobs, now).map(|p| (p.kind, p.text))
    }

    #[test]
    fn running_jobs_name_kind_object_and_percent() {
        with_locale(Locale::De, || {
            let now = Instant::now();
            let mut t = TransferPill::default();
            let copy = job(
                1,
                JobKind::Copy,
                "Copying 3 items to /Drive",
                JobState::Running,
                420,
                1000,
            );
            t.update(std::slice::from_ref(&copy), now);
            let p = t.pill(std::slice::from_ref(&copy), now).unwrap();
            assert_eq!(
                (p.kind, p.text.as_str()),
                (PillKind::Running, "Kopiere 3 Objekte … 42 %")
            );
            assert!((p.fraction.unwrap() - 0.42).abs() < 1e-6);
            let mv = job(
                2,
                JobKind::Move,
                "Moving \"a.iso\" to /x",
                JobState::Paused,
                0,
                0,
            );
            assert_eq!(
                text(&t, std::slice::from_ref(&mv), now).unwrap(),
                (PillKind::Paused, "Bewege „a.iso“ · pausiert".into())
            );
            let both = [
                copy.clone(),
                job(
                    3,
                    JobKind::Copy,
                    "Copying 2 items to /y",
                    JobState::Running,
                    320,
                    1000,
                ),
            ];
            assert_eq!(
                text(&t, &both, now).unwrap(),
                (PillKind::Running, "2 Übertragungen · 37 %".into())
            );
            let queued = job(
                4,
                JobKind::Extract,
                "Extracting \"b.zip\" to /z",
                JobState::Queued,
                0,
                0,
            );
            assert_eq!(
                text(&t, &[queued], now).unwrap(),
                (PillKind::Waiting, "Entpacke „b.zip“ · wartet".into())
            );
            let net = job(
                5,
                JobKind::Copy,
                "Copying 2 items to /s",
                JobState::WaitingForNetwork {
                    attempt: 2,
                    next_retry: None,
                    error: "x".into(),
                },
                1,
                2,
            );
            let paused = job(
                6,
                JobKind::Copy,
                "Copying 2 items to /s",
                JobState::Paused,
                1,
                2,
            );
            assert_eq!(
                text(&t, &[net, paused], now).unwrap(),
                (
                    PillKind::Waiting,
                    "2 Übertragungen · wartet auf Verbindung".into()
                )
            );
        });
    }

    #[test]
    fn done_lingers_then_goes_and_errors_stay_until_seen() {
        with_locale(Locale::De, || {
            let t0 = Instant::now();
            let mut t = TransferPill::default();
            let mut jobs = vec![job(
                1,
                JobKind::Copy,
                "Copying 3 items to /d",
                JobState::Running,
                1,
                2,
            )];
            t.update(&jobs, t0);
            jobs[0].state = JobState::Done;
            let t1 = t0 + Duration::from_secs(1);
            t.update(&jobs, t1);
            assert_eq!(
                text(&t, &jobs, t1).unwrap(),
                (PillKind::Done, "Fertig".into())
            );
            assert!(t.repaint_in(t1).is_some(), "repaints to hide itself");
            assert_eq!(
                text(&t, &jobs, t1 + Duration::from_secs(3)).map(|p| p.0),
                Some(PillKind::Done)
            );
            assert!(
                t.pill(&jobs, t1 + DONE_LINGER).is_none(),
                "gone after a few seconds"
            );
            // Finished jobs from before don't bring it back.
            t.update(&jobs, t1 + Duration::from_secs(10));
            assert!(t.pill(&jobs, t1 + Duration::from_secs(10)).is_none());

            // A failing job: red, and it stays.
            jobs.push(job(
                2,
                JobKind::Move,
                "Moving 2 items to /e",
                JobState::Running,
                0,
                9,
            ));
            let t2 = t1 + Duration::from_secs(20);
            t.update(&jobs, t2);
            jobs[1].state = JobState::Failed("permission denied".into());
            t.update(&jobs, t2);
            let late = t2 + Duration::from_secs(60);
            assert_eq!(
                text(&t, &jobs, late).unwrap(),
                (PillKind::Failed, "Übertragung fehlgeschlagen".into())
            );
            assert_eq!(t.repaint_in(late), None);
            t.acknowledge();
            assert!(t.pill(&jobs, late).is_none(), "seen: gone");

            // Done with skipped items counts as a problem; cancelled is "Gestoppt".
            jobs.push(job(
                3,
                JobKind::Copy,
                "Copying 2 items to /f",
                JobState::Running,
                0,
                9,
            ));
            t.update(&jobs, late);
            jobs[2].state = JobState::Done;
            jobs[2].errors = vec!["skipped a.txt (exists)".into()];
            t.update(&jobs, late);
            assert_eq!(text(&t, &jobs, late).unwrap().1, "Fertig mit Fehlern");
            t.acknowledge();
            jobs.push(job(
                4,
                JobKind::Copy,
                "Copying 2 items to /g",
                JobState::Running,
                0,
                9,
            ));
            t.update(&jobs, late);
            jobs[3].state = JobState::Cancelled;
            t.update(&jobs, late);
            assert_eq!(
                text(&t, &jobs, late).unwrap(),
                (PillKind::Stopped, "Gestoppt".into())
            );
            // Cleared from the panel: forgotten.
            t.update(&[], late);
            assert!(t.errors.is_empty() && t.seen.is_empty());
        });
    }

    #[test]
    fn cloud_pills() {
        with_locale(Locale::De, || {
            assert_eq!(cloud_pill("Google Drive", 0, 0), None);
            let p = cloud_pill("Google Drive", 2, 0).unwrap();
            assert_eq!(
                (p.kind, p.text.as_str()),
                (PillKind::Uploading, "Google Drive lädt 2 Objekte hoch")
            );
            let p = cloud_pill("OneDrive", 3, 1).unwrap();
            assert_eq!(
                (p.kind, p.text.as_str()),
                (PillKind::UploadError, "OneDrive: 1 Upload-Fehler")
            );
        });
        with_locale(Locale::En, || {
            assert_eq!(
                cloud_pill("Dropbox", 1, 0).unwrap().text,
                "Dropbox is uploading 1 item"
            );
            assert_eq!(
                cloud_pill("Dropbox", 0, 2).unwrap().text,
                "Dropbox: 2 upload errors"
            );
        });
    }
}
