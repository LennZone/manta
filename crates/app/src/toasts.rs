//! Non-blocking notifications in the bottom-right corner. Info and success
//! toasts disappear on their own; errors stay until dismissed (or until
//! pushed out by newer ones).

use std::time::{Duration, Instant};

use eframe::egui::{self, CornerRadius, RichText, Sense};

use crate::icons;
use crate::theme::{self, radius, space, text};
use crate::widgets;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Toast {
    id: u64,
    pub kind: ToastKind,
    pub title: String,
    pub detail: Option<String>,
    /// A button (label, plugin command `plugin:cmd`), e.g. "Undo".
    pub action: Option<(String, String)>,
    at: Instant,
    /// Shown at least once (for the fade-in).
    seen: bool,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next: u64,
}

const MAX_VISIBLE: usize = 4;
const INFO_TTL: Duration = Duration::from_secs(4);
const ERROR_TTL: Duration = Duration::from_secs(30);

impl Toasts {
    /// Test seam: dismiss every toast.
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn push(&mut self, kind: ToastKind, title: impl Into<String>, detail: Option<String>) {
        self.push_with_action(kind, title, detail, None);
    }

    /// A toast with a button; [`Toasts::ui`] returns its command when the
    /// button is clicked. Stays twice as long as a plain info toast.
    pub fn push_with_action(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        detail: Option<String>,
        action: Option<(String, String)>,
    ) {
        let title = title.into();
        // Replace an identical toast instead of stacking duplicates.
        self.items.retain(|t| !(t.title == title && t.kind == kind));
        self.next += 1;
        self.items.push(Toast {
            id: self.next,
            kind,
            title,
            detail,
            action,
            at: Instant::now(),
            seen: false,
        });
        while self.items.len() > MAX_VISIBLE {
            // Drop the oldest non-error first.
            match self.items.iter().position(|t| t.kind != ToastKind::Error) {
                Some(i) => self.items.remove(i),
                None => self.items.remove(0),
            };
        }
    }

    #[cfg(test)]
    pub fn titles(&self) -> Vec<String> {
        self.items.iter().map(|t| t.title.clone()).collect()
    }

    #[cfg(test)]
    pub fn actions(&self) -> Vec<(String, String)> {
        self.items.iter().filter_map(|t| t.action.clone()).collect()
    }

    /// `bottom_right`: anchor point (above the status bar). Returns the
    /// command of a clicked toast button.
    pub fn ui(&mut self, ctx: &egui::Context, bottom_right: egui::Pos2) -> Option<String> {
        let now = Instant::now();
        self.items.retain(|t| {
            let ttl = match t.kind {
                ToastKind::Error => ERROR_TTL,
                ToastKind::Warning => INFO_TTL * 2,
                _ if t.action.is_some() => INFO_TTL * 2,
                _ => INFO_TTL,
            };
            now.duration_since(t.at) < ttl
        });
        if self.items.is_empty() {
            return None;
        }
        let mut run: Option<(u64, String)> = None;
        let p = theme::pal_ctx(ctx);
        let mut dismiss: Option<u64> = None;
        let mut y = bottom_right.y;
        let width = 340.0_f32.min(ctx.content_rect().width() - 2.0 * space::XL);
        for t in self.items.iter_mut().rev() {
            let anim_id = egui::Id::unique(("toast", t.id));
            let alpha = ctx.animate_bool_with_time(anim_id, t.seen, crate::a11y::anim(0.18));
            t.seen = true;
            let area = egui::Area::new(anim_id)
                .order(egui::Order::Foreground)
                .pivot(egui::Align2::RIGHT_BOTTOM)
                .fixed_pos(egui::pos2(bottom_right.x, y + (1.0 - alpha) * 8.0))
                .interactable(true)
                // Clicks on the body (one response per id: a second
                // `interact` with the area's id is a widget-id clash).
                .sense(Sense::CLICK)
                .show(ctx, |ui| {
                    ui.set_opacity(alpha);

                    let (glyph, color) = match t.kind {
                        ToastKind::Info => (icons::INFO, p.accent_text),
                        ToastKind::Success => (icons::CHECK_CIRCLE, p.success),
                        ToastKind::Warning => (icons::WARNING, p.warning),
                        ToastKind::Error => (icons::WARNING_CIRCLE, p.danger),
                    };
                    egui::Frame::new()
                        .fill(p.elevated)
                        .stroke(egui::Stroke::new(1.0, p.separator))
                        .corner_radius(CornerRadius::same(radius::PANEL + 2))
                        .shadow(ui.visuals().popup_shadow)
                        .inner_margin(egui::Margin::symmetric(
                            space::LG as i8,
                            space::MD as i8 + 2,
                        ))
                        .show(ui, |ui| {
                            ui.set_width(width - 2.0 * space::LG);
                            ui.horizontal_top(|ui| {
                                widgets::icon(ui, glyph, 18.0, color);
                                ui.vertical(|ui| {
                                    // Room for the action button (as wide as its label,
                                    // see `widgets::button`) so longer labels such as
                                    // "Widerrufen" don't push the toast wider.
                                    let extra = match &t.action {
                                        Some((label, _)) => {
                                            let w = ui
                                                .painter()
                                                .layout_no_wrap(
                                                    label.clone(),
                                                    theme::regular(text::BODY),
                                                    p.text,
                                                )
                                                .size()
                                                .x;
                                            (w + 2.0 * space::LG + 4.0).max(76.0)
                                                + ui.spacing().item_spacing.x
                                        }
                                        None => 0.0,
                                    };
                                    ui.set_width(ui.available_width() - 26.0 - extra);
                                    ui.spacing_mut().item_spacing.y = space::XXS;
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&t.title)
                                                .font(theme::medium(text::BODY))
                                                .color(p.text),
                                        )
                                        .wrap(),
                                    );
                                    if let Some(d) = &t.detail {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(d).color(p.text_secondary),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                                if let Some((label, cmd)) = &t.action {
                                    let b = widgets::button(
                                        ui,
                                        label,
                                        widgets::ButtonKind::Secondary,
                                        true,
                                    );
                                    if b.clicked() {
                                        run = Some((t.id, cmd.clone()));
                                    }
                                }
                                let close =
                                    widgets::mini_icon_button(ui, icons::X, p.text_tertiary);
                                if widgets::tooltip(close, t!("toast.dismiss"), "").clicked() {
                                    dismiss = Some(t.id);
                                }
                            });
                        });
                });
            // VoiceOver reads a new toast at once (errors interrupt).
            let spoken = match &t.detail {
                Some(d) => format!("{}. {d}", t.title),
                None => t.title.clone(),
            };
            let urgent = t.kind == ToastKind::Error;
            let role = if urgent {
                egui::accesskit::Role::Alert
            } else {
                egui::accesskit::Role::Status
            };
            crate::a11y::live(ctx, area.response.id, role, &spoken, urgent);
            // A click on the toast body dismisses info toasts.
            if area.response.clicked() && t.kind != ToastKind::Error {
                dismiss = Some(t.id);
            }
            y = area.response.rect.top() - space::SM - (1.0 - alpha) * 8.0;
        }
        if let Some(id) = dismiss {
            self.items.retain(|t| t.id != id);
        }
        // Keep animating / expire on time.
        ctx.request_repaint_after(Duration::from_millis(250));
        let (id, cmd) = run?;
        self.items.retain(|t| t.id != id);
        Some(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_and_cap() {
        let mut t = Toasts::default();
        t.push(ToastKind::Info, "a", None);
        t.push(ToastKind::Info, "a", None);
        assert_eq!(t.titles(), ["a"]);
        t.push(ToastKind::Error, "e", None);
        for n in 0..5 {
            t.push(ToastKind::Info, format!("i{n}"), None);
        }
        assert_eq!(t.titles().len(), MAX_VISIBLE);
        assert!(
            t.titles().contains(&"e".to_string()),
            "errors are kept longest"
        );
    }
}
