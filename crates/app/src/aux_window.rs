//! Auxiliary windows (Settings, Compare & Sync, the built-in preview, Help) as
//! real native windows with the system title bar and traffic lights.
//!
//! They are egui *immediate* viewports: the content callback runs inside the
//! main window's frame with `&mut` access to the app state. Each viewport has
//! its own input, so shortcuts typed in one window never reach the other.
//!
//! macOS conventions: the red button or ⌘W closes the window (Esc does not),
//! the app keeps running, and the position is remembered for the next open.
//!
//! When the backend can't create native windows (`ctx.embed_viewports()`,
//! e.g. the headless test harness), the window is drawn as an in-window egui
//! window instead. There "keyboard focus" is emulated: the auxiliary window
//! that was opened or clicked last is the key window until the user clicks
//! outside it.

use eframe::egui::{
    self, Id, LayerId, Order, Pos2, Vec2, ViewportBuilder, ViewportCommand, ViewportId,
};

use crate::theme::{self, space};

/// The auxiliary windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuxKind {
    Settings,
    Sync,
    Preview,
    /// "Software Update".
    Update,
    /// The user manual ("Manta Help").
    Help,
}

impl AuxKind {
    #[cfg(test)]
    pub const ALL: [AuxKind; 5] = [
        AuxKind::Settings,
        AuxKind::Sync,
        AuxKind::Preview,
        AuxKind::Update,
        AuxKind::Help,
    ];

    pub fn viewport_id(self) -> ViewportId {
        ViewportId::from_hash_of(("manta-aux-window", self))
    }

    /// Id of the embedded egui window (fallback mode).
    pub fn window_id(self) -> Id {
        Id::unique(("manta-aux-embedded", self))
    }
}

/// How an auxiliary window looks.
pub struct Spec<'a> {
    pub kind: AuxKind,
    pub title: &'a str,
    /// Inner size when first opened.
    pub size: Vec2,
    pub min_size: Vec2,
    /// Content padding.
    pub margin: egui::Margin,
    /// Make the window key when it opens (the preview opens in the
    /// background, like Quick Look, so the file list keeps the keyboard).
    pub activate: bool,
    /// Show the green zoom button.
    pub zoomable: bool,
}

/// Placement and focus state of one auxiliary window (lives as long as the app).
#[derive(Default)]
pub struct AuxWindow {
    /// Outer position used when the window was created. Kept stable while
    /// it is open: egui turns builder changes into window commands, so a
    /// changing position would move the window every frame.
    spawn_pos: Option<Pos2>,
    /// Last known outer position, remembered across close / reopen.
    last_pos: Option<Pos2>,
    /// Bring the window to the front on the next frame.
    focus: bool,
    /// Drawn in the previous frame.
    shown: bool,
}

/// Was the window closed this frame (red button, ⌘W)?
#[must_use]
pub struct Closed(pub bool);

impl AuxWindow {
    /// Bring the window to the front (and make it key) next frame.
    pub fn request_focus(&mut self) {
        self.focus = true;
    }

    /// Test seam: a bring-to-front is pending.
    #[cfg(test)]
    pub fn focus_requested(&self) -> bool {
        self.focus
    }

    /// Call when the owner stops showing the window, so the next open is
    /// placed again (at the remembered position).
    pub fn hidden(&mut self, ctx: &egui::Context, kind: AuxKind) {
        if self.shown {
            self.shown = false;
            self.spawn_pos = None;
            self.focus = false;
            if embedded_key(ctx) == Some(kind) {
                set_embedded_key(ctx, None);
            }
        }
    }

    /// Draw the window (call every frame while it is open) with `add`
    /// filling the content area.
    pub fn show<T>(
        &mut self,
        ctx: &egui::Context,
        spec: &Spec<'_>,
        // FnMut, not FnOnce: egui may run an immediate viewport's callback
        // more than once per frame (a second layout pass after
        // `request_discard`, which grids and fresh text layouts trigger).
        mut add: impl FnMut(&mut egui::Ui) -> T,
    ) -> (Option<T>, Closed) {
        let first = !self.shown;
        self.shown = true;
        if first && spec.activate {
            self.focus = true;
        }
        if ctx.embed_viewports() {
            let (v, closed) = self.show_embedded(ctx, spec, first, add);
            return (Some(v), closed);
        }
        if first {
            self.spawn_pos = self.last_pos.or_else(|| centred_over_main(ctx, spec.size));
        }
        let mut builder = ViewportBuilder::default()
            .with_title(spec.title)
            .with_inner_size(spec.size)
            .with_min_inner_size(spec.min_size)
            .with_resizable(true)
            .with_maximize_button(spec.zoomable)
            .with_active(spec.activate);
        if let Some(pos) = self.spawn_pos {
            builder = builder.with_position(pos);
        }
        let focus = std::mem::take(&mut self.focus);
        let vid = spec.kind.viewport_id();
        if focus && !first {
            ctx.send_viewport_cmd_to(vid, ViewportCommand::Focus);
        }
        let margin = spec.margin;
        let mut last_pos = self.last_pos;
        let kind = spec.kind;
        let out = ctx.show_viewport_immediate(vid, builder, |ui, class| {
            if let Some(r) = ui.input(|i| i.viewport().outer_rect) {
                if last_pos.is_none() || first {
                    let native = matches!(class, egui::ViewportClass::Immediate);
                    log::info!("aux window {kind:?}: viewport {vid:?} (native window: {native}), outer rect {r:?}");
                }
                last_pos = Some(r.min);
            }
            // The red button, or ⌘W while this window is key (its input
            // only carries keys typed into it).
            // Self-test: force egui's second layout pass in this window
            // (what grids and fresh text layouts do on their own).
            if first && std::env::var_os("MANTA_AUX_FORCE_DISCARD").is_some() && ui.ctx().cumulative_pass_nr() % 2 == 0 {
                ui.ctx().request_discard("MANTA_AUX_FORCE_DISCARD");
            }
            let close = ui.input(|i| i.viewport().close_requested())
                || ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::W));
            let p = theme::pal(ui);
            // A panic in here would unwind into winit's run loop, which macOS
            // turns into an abort (the whole app dies). Catch it, log it and
            // close just this window instead.
            let inner = egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(p.elevated).inner_margin(margin))
                .show(ui, |ui| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| add(ui))))
                .inner;
            match inner {
                Ok(v) => (Some(v), close),
                Err(e) => {
                    let msg = e
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| e.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    log::error!("aux window {kind:?} crashed and was closed: {msg}");
                    (None, true)
                }
            }
        });
        self.last_pos = last_pos;
        if out.1 {
            log::debug!("aux window {:?} closed", spec.kind);
        }
        (out.0, Closed(out.1))
    }

    fn show_embedded<T>(
        &mut self,
        ctx: &egui::Context,
        spec: &Spec<'_>,
        first: bool,
        add: impl FnOnce(&mut egui::Ui) -> T,
    ) -> (T, Closed) {
        let wid = spec.kind.window_id();
        let layer = LayerId::new(Order::Middle, wid);
        if std::mem::take(&mut self.focus) || first && spec.activate {
            ctx.move_to_top(layer);
            set_embedded_key(ctx, Some(spec.kind));
        }
        let p = theme::pal_ctx(ctx);
        let screen = ctx.content_rect();
        let size = egui::vec2(
            spec.size.x.min(screen.width() - 2.0 * space::XL),
            spec.size.y.min(screen.height() - 2.0 * space::XL),
        );
        // ⌘W while this window is key (consumed before the content can see it).
        let close = embedded_key(ctx) == Some(spec.kind)
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::W));
        let resp = egui::Window::new(spec.title)
            .id(wid)
            .collapsible(false)
            .resizable(true)
            .default_size(size)
            .min_size(spec.min_size.min(size))
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .fill(p.elevated)
                    .inner_margin(spec.margin),
            )
            .show(ctx, add)
            .expect("window is open");
        let rect = resp.response.rect;
        ctx.data_mut(|d| d.insert_temp(wid.with("rect"), rect));
        (
            resp.inner.expect("window is not collapsible"),
            Closed(close),
        )
    }
}

/// Where a new window of inner `size` goes: centred over the main window.
fn centred_over_main(ctx: &egui::Context, size: Vec2) -> Option<Pos2> {
    let main = ctx.input_for(ViewportId::ROOT, |i| i.viewport().outer_rect)?;
    Some(centred_in(main, size))
}

/// Outer position of a window of inner `size` centred over `main` (never
/// above its top edge, where the menu bar may be).
fn centred_in(main: egui::Rect, size: Vec2) -> Pos2 {
    // The title bar adds ~28 pt to the outer height.
    let outer = size + egui::vec2(0.0, 28.0);
    let mut pos = main.center() - outer / 2.0;
    pos.y = pos.y.max(main.top());
    pos
}

fn key_id() -> Id {
    Id::unique("manta-aux-embedded-key")
}

/// Embedded mode only: the auxiliary window that has keyboard focus
/// (always `None` with native windows, where each window has its own input).
pub fn embedded_key(ctx: &egui::Context) -> Option<AuxKind> {
    if !ctx.embed_viewports() {
        return None;
    }
    ctx.data(|d| d.get_temp::<Option<AuxKind>>(key_id()))
        .flatten()
}

fn set_embedded_key(ctx: &egui::Context, kind: Option<AuxKind>) {
    ctx.data_mut(|d| d.insert_temp(key_id(), kind));
}

/// Embedded mode only: a click moves keyboard focus to the auxiliary window
/// under the pointer, or back to the main window. Call once per frame
/// before the main window's shortcuts run, with the windows that are open.
pub fn track_embedded_focus(ctx: &egui::Context, open: &[AuxKind]) {
    if !ctx.embed_viewports() {
        return;
    }
    if let Some(k) = embedded_key(ctx) {
        if !open.contains(&k) {
            set_embedded_key(ctx, None);
        }
    }
    let Some(pos) = ctx.input(|i| {
        i.pointer
            .primary_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    }) else {
        return;
    };
    match ctx.layer_id_at(pos) {
        // Popups, menus and sheets keep the focus where it is.
        Some(l) if l.order != Order::Middle && l.order != Order::Background => {}
        Some(l) => set_embedded_key(ctx, open.iter().copied().find(|k| l.id == k.window_id())),
        None => set_embedded_key(ctx, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_ids_are_distinct() {
        let ids: std::collections::HashSet<_> =
            AuxKind::ALL.iter().map(|k| k.viewport_id()).collect();
        assert_eq!(ids.len(), AuxKind::ALL.len());
        assert!(!ids.contains(&ViewportId::ROOT));
    }

    #[test]
    fn new_window_is_centred_over_the_main_window() {
        let main = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1280.0, 800.0));
        let pos = centred_in(main, egui::vec2(620.0, 500.0));
        assert_eq!(
            pos,
            egui::pos2(100.0 + (1280.0 - 620.0) / 2.0, 50.0 + (800.0 - 528.0) / 2.0)
        );
        // Taller than the main window: not above its top edge.
        assert_eq!(centred_in(main, egui::vec2(600.0, 2000.0)).y, 50.0);
    }
}
