//! Quick Look–style preview window for text and image files.

use eframe::egui::{self, CornerRadius, RichText};
use manta_core::Entry;

use crate::aux_window::{AuxKind, AuxWindow, Spec};
use crate::icons;
use crate::theme::{self, pal, radius, space, text};
use crate::util;
use crate::widgets;
use crate::worker::{PreviewContent, TEXT_PREVIEW_LIMIT};

pub struct Preview {
    pub id: u64,
    pub entry: Entry,
    pub loading: bool,
    pub content: Option<PreviewContent>,
    pub error: Option<String>,
}

impl Preview {
    pub fn new(id: u64, entry: Entry) -> Self {
        Preview {
            id,
            entry,
            loading: true,
            content: None,
            error: None,
        }
    }

    /// Unique URI for egui's image cache (includes the extension so the
    /// loader can detect the format).
    pub fn image_uri(&self) -> String {
        format!("bytes://manta-preview-{}/{}", self.id, self.entry.name)
    }

    /// Draw the preview in its own window. Returns `false` when the
    /// window was closed (red button or ⌘W).
    pub fn ui(&mut self, ctx: &egui::Context, window: &mut AuxWindow) -> bool {
        let uri = self.image_uri();
        let kind = icons::file_kind(&self.entry.name, false);
        let title = self.entry.name.clone();
        let spec = Spec {
            kind: AuxKind::Preview,
            title: &title,
            size: egui::vec2(640.0, 480.0),
            min_size: egui::vec2(320.0, 240.0),
            margin: egui::Margin::same(space::LG as i8),
            // Opens behind the file list's keyboard focus, like Quick Look:
            // Space in the list closes it again.
            activate: false,
            zoomable: true,
        };
        let mut space = false;
        let (_, closed) = window.show(ctx, &spec, |ui| {
            // Space closes it from its own window too, like Quick Look
            // (in the fallback mode only while it has keyboard focus).
            let key = !ui.ctx().embed_viewports()
                || crate::aux_window::embedded_key(ui.ctx()) == Some(AuxKind::Preview);
            space = key
                && ui.memory(|m| m.focused().is_none())
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space));
            let p = pal(ui);
            ui.horizontal(|ui| {
                widgets::icon(ui, kind.icon(), 18.0, kind.tint(p));
                let mut meta = format!("{} · {}", kind.label(), util::format_size(self.entry.size));
                if self.entry.modified.is_some() {
                    meta.push_str(&format!(
                        " · {}",
                        util::format_time_relative(self.entry.modified, chrono::Local::now())
                    ));
                }
                ui.label(RichText::new(meta).color(p.text_secondary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(t!("preview.close_hint"))
                            .font(theme::regular(text::CAPTION))
                            .color(p.text_tertiary),
                    );
                });
            })
            .response
            .on_hover_text(self.entry.path.as_str());
            ui.add_space(space::SM);
            let rect = ui.available_rect_before_wrap();
            if self.loading {
                ui.put(
                    egui::Rect::from_center_size(rect.center(), egui::vec2(28.0, 28.0)),
                    egui::Spinner::new().size(26.0),
                );
                return;
            }
            if let Some(err) = &self.error {
                let body = err.clone();
                widgets::empty_state(
                    ui,
                    rect,
                    icons::WARNING_CIRCLE,
                    p.danger,
                    t!("preview.unreadable"),
                    &body,
                    &[],
                );
                return;
            }
            match &self.content {
                Some(PreviewContent::Text {
                    text: body,
                    truncated,
                }) => {
                    if *truncated {
                        ui.label(
                            RichText::new(t!(
                                "preview.truncated",
                                size = util::format_size(TEXT_PREVIEW_LIMIT)
                            ))
                            .font(theme::regular(text::CAPTION))
                            .color(p.warning),
                        );
                    }
                    egui::Frame::new()
                        .fill(p.content)
                        .stroke(egui::Stroke::new(1.0, p.separator))
                        .corner_radius(CornerRadius::same(radius::PANEL))
                        .inner_margin(egui::Margin::same(space::LG as i8))
                        .show(ui, |ui| {
                            egui::ScrollArea::both()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(body.as_str())
                                                .font(theme::mono(text::SMALL + 0.5))
                                                .color(p.text),
                                        )
                                        .selectable(true)
                                        .extend(),
                                    );
                                });
                        });
                }
                Some(PreviewContent::Image { bytes }) => {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.centered_and_justified(|ui| {
                                ui.add(
                                    egui::Image::from_bytes(
                                        uri.clone(),
                                        egui::load::Bytes::Shared(bytes.clone()),
                                    )
                                    .max_size(ui.available_size())
                                    .maintain_aspect_ratio(true)
                                    .shrink_to_fit()
                                    .corner_radius(CornerRadius::same(radius::CONTROL)),
                                );
                            });
                        });
                }
                Some(PreviewContent::Binary { size }) => {
                    let body = t!(
                        "preview.binary",
                        kind = kind.label(),
                        size = util::format_size(*size),
                        key = crate::icons::RETURN
                    );
                    widgets::empty_state(
                        ui,
                        rect,
                        kind.icon(),
                        kind.tint(p),
                        t!("preview.no_preview"),
                        &body,
                        &[],
                    );
                }
                None => {}
            }
        });
        !(closed.0 || space)
    }

    /// Free the decoded texture.
    pub fn release(&self, ctx: &egui::Context) {
        if matches!(self.content, Some(PreviewContent::Image { .. })) {
            ctx.forget_image(&self.image_uri());
        }
    }
}
