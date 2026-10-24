//! Accessibility helpers.
//!
//! * **System display options** (System Settings → Accessibility →
//!   Display): Reduce motion, Increase contrast and Reduce transparency are
//!   read from `NSWorkspace` at launch and again whenever macOS posts
//!   `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`. They
//!   switch off animations ([`anim`]), pick the high-contrast palette (see
//!   `theme::Palette::of`) and drop translucent scrims ([`scrim`]).
//! * **AccessKit nodes** for custom-painted widgets: egui hands its
//!   AccessKit tree to macOS (NSAccessibility), so VoiceOver reads what
//!   the nodes say. [`name`], [`file_item`], [`list`] and [`announce`]
//!   fill them in.
//! * **Keyboard focus**: [`paint_focus_ring`] draws one ring around
//!   whatever widget holds keyboard focus, like macOS does with Full
//!   Keyboard Access.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use eframe::egui::{self, accesskit, Color32, CornerRadius, Stroke, StrokeKind};

// ------------------------------------------------------------------ display options

/// The macOS display accommodations Manta follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayOptions {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub reduce_transparency: bool,
}

impl DisplayOptions {
    fn bits(self) -> u8 {
        self.reduce_motion as u8
            | (self.increase_contrast as u8) << 1
            | (self.reduce_transparency as u8) << 2
    }

    fn from_bits(b: u8) -> Self {
        DisplayOptions {
            reduce_motion: b & 1 != 0,
            increase_contrast: b & 2 != 0,
            reduce_transparency: b & 4 != 0,
        }
    }
}

static OPTIONS: AtomicU8 = AtomicU8::new(0);
/// Set by the macOS notification; the next frame re-reads the options.
static CHANGED: AtomicBool = AtomicBool::new(true);

thread_local! {
    /// Under `cargo test` the options are per thread (parallel tests).
    static TEST_OPTIONS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

pub fn options() -> DisplayOptions {
    DisplayOptions::from_bits(if cfg!(test) {
        TEST_OPTIONS.with(|o| o.get())
    } else {
        OPTIONS.load(Ordering::Relaxed)
    })
}

fn store(o: DisplayOptions) {
    if cfg!(test) {
        TEST_OPTIONS.with(|c| c.set(o.bits()));
    } else {
        OPTIONS.store(o.bits(), Ordering::Relaxed);
    }
}

pub fn reduce_motion() -> bool {
    options().reduce_motion
}

pub fn increase_contrast() -> bool {
    options().increase_contrast
}

pub fn reduce_transparency() -> bool {
    options().reduce_transparency
}

/// An animation's duration: `secs`, or 0 with Reduce motion (the change
/// then happens at once).
pub fn anim(secs: f32) -> f32 {
    if reduce_motion() {
        0.0
    } else {
        secs
    }
}

/// Apply `o`: remember it, switch egui's own animations and the palette.
pub fn apply(ctx: &egui::Context, o: DisplayOptions) {
    let before = options();
    store(o);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |s| {
            s.animation_time = if o.reduce_motion { 0.0 } else { 1.0 / 12.0 };
            s.scroll_animation = if o.reduce_motion {
                egui::style::ScrollAnimation::none()
            } else {
                egui::style::ScrollAnimation::default()
            };
        });
    }
    if before.increase_contrast != o.increase_contrast {
        // The palette depends on it: rebuild egui's visuals.
        crate::theme::apply_accent(ctx, crate::theme::current_accent());
    }
    if before != o {
        log::info!("display options: {o:?}");
        ctx.request_repaint();
    }
}

/// Once per frame: read the system options when they (may) have changed,
/// i.e. at launch and after macOS posted its notification.
pub fn frame(ctx: &egui::Context) {
    if cfg!(test) {
        return;
    }
    if CHANGED.swap(false, Ordering::Relaxed) {
        apply(ctx, system::read());
        system::observe(ctx);
    }
}

/// Test seam: pretend macOS reports `o`.
#[cfg(test)]
pub fn set_for_test(ctx: &egui::Context, o: DisplayOptions) {
    apply(ctx, o);
}

#[cfg(target_os = "macos")]
mod system {
    use super::*;
    use std::sync::Once;

    pub fn read() -> DisplayOptions {
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        DisplayOptions {
            reduce_motion: ws.accessibilityDisplayShouldReduceMotion(),
            increase_contrast: ws.accessibilityDisplayShouldIncreaseContrast(),
            reduce_transparency: ws.accessibilityDisplayShouldReduceTransparency(),
        }
    }

    /// Register for `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`
    /// (once): it marks the options as changed and wakes the UI.
    pub fn observe(ctx: &egui::Context) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let ctx = ctx.clone();
            let block = block2::RcBlock::new(
                move |_n: std::ptr::NonNull<objc2_foundation::NSNotification>| {
                    CHANGED.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                },
            );
            let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
            let center = ws.notificationCenter();
            // SAFETY: the notification name is AppKit's constant; the block
            // only touches an atomic and egui's thread-safe context. The
            // observer lives as long as the app (leaked on purpose).
            let observer = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(
                        objc2_app_kit::NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
                    ),
                    None,
                    None,
                    &block,
                )
            };
            std::mem::forget(observer);
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod system {
    use super::*;

    pub fn read() -> DisplayOptions {
        DisplayOptions::default()
    }

    pub fn observe(_ctx: &egui::Context) {}
}

/// A dimming backdrop colour, or none with Reduce transparency (macOS
/// then avoids translucent layers; the dialog's border separates it).
pub fn scrim(dark: bool, light_alpha: u8, dark_alpha: u8) -> Color32 {
    if reduce_transparency() {
        Color32::TRANSPARENT
    } else {
        Color32::from_black_alpha(if dark { dark_alpha } else { light_alpha })
    }
}

// ------------------------------------------------------------------ AccessKit

/// `text` without icon glyphs (Phosphor sits in the Private Use Area) and
/// the spaces around them: "\u{e288}  Connect" is read as "Connect".
pub fn strip_icons(text: &str) -> String {
    let is_icon = |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c);
    if !text.chars().any(is_icon) {
        return text.to_string();
    }
    text.lines()
        .map(|l| {
            l.chars()
                .filter(|c| !is_icon(*c))
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An egui plugin that cleans the AccessKit tree before it goes to macOS:
/// icon glyphs in names, values and descriptions (menu items such as
/// "\u{e256}   Open", buttons with an icon) are removed, so VoiceOver only
/// reads words. Only runs while assistive technology asks for the tree.
#[derive(Default)]
pub struct SpeechCleanup;

impl egui::Plugin for SpeechCleanup {
    fn debug_name(&self) -> &'static str {
        "manta-speech-cleanup"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        let Some(update) = output.platform_output.accesskit_update.as_mut() else {
            return;
        };
        for (_, node) in &mut update.nodes {
            // egui names the handles of resizable panels in English.
            if node.role() == accesskit::Role::Splitter && node.label() == Some("Resize panel") {
                node.set_label(t!("a11y.resize_panel"));
            }
            if let Some(l) = node.label().filter(|l| has_icon(l)) {
                let l = strip_icons(l);
                node.set_label(l);
            }
            if let Some(v) = node.value().filter(|v| has_icon(v)) {
                let v = strip_icons(v);
                node.set_value(v);
            }
            if let Some(d) = node.description().filter(|d| has_icon(d)) {
                let d = strip_icons(d);
                node.set_description(d);
            }
        }
    }
}

fn has_icon(text: &str) -> bool {
    text.chars().any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c))
}

/// Name a region of the window (toolbar, sidebar, a pane, the status
/// bar): VoiceOver lists them as landmarks to jump between.
pub fn region(ui: &egui::Ui, role: accesskit::Role, label: &str) {
    let label = label.to_string();
    ui.ctx().accesskit_node_builder(ui.unique_id(), |n| {
        n.set_role(role);
        n.set_label(label);
    });
}

/// Give `resp` a role and a name (icon-only buttons, painted controls).
pub fn name(resp: &egui::Response, role: accesskit::Role, label: &str) {
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(role, enabled, label));
}

/// A disclosure triangle ("▸ Log", a chapter in the manual's contents):
/// named after what it opens, with its expanded state.
pub fn disclosure(resp: &egui::Response, label: &str, open: bool) {
    name(resp, accesskit::Role::DisclosureTriangle, label);
    resp.ctx
        .accesskit_node_builder(resp.id, |n| n.set_expanded(open));
}

/// What a file row or cell says: "README.md, Markdown document, 214 bytes,
/// modified Today 09:41".
pub fn file_label(name: &str, kind: &str, size: Option<&str>, modified: Option<&str>) -> String {
    let mut s = name.to_string();
    for part in [Some(kind), size, modified].into_iter().flatten() {
        if !part.is_empty() {
            s.push_str(", ");
            s.push_str(part);
        }
    }
    s
}

/// The spoken description of a file: name, kind, size, modification
/// date, cloud state and what a plugin says about it (e.g. its VCS status).
pub fn entry_label(
    e: &manta_core::Entry,
    now: chrono::DateTime<chrono::Local>,
    dir_size: Option<&manta_plugin::SizeInfo>,
    sync: Option<manta_vfs::cloud_local::SyncState>,
    deco: Option<&manta_plugin::host::MergedDecoration>,
) -> String {
    let kind = match &e.link_target {
        Some(target) => t!("pane.alias_of", target = target),
        None => crate::icons::file_kind(&e.name, e.is_dir())
            .label()
            .to_string(),
    };
    let size = if e.is_dir() {
        dir_size.map(|d| crate::util::format_size(d.bytes))
    } else {
        Some(crate::util::format_size(e.size))
    };
    let modified = e.modified.map(|_| {
        t!(
            "a11y.modified",
            when = crate::util::format_time_relative(e.modified, now)
        )
    });
    let mut s = file_label(&e.name, &kind, size.as_deref(), modified.as_deref());
    if let Some((_, state)) = sync.and_then(crate::cloud::badge) {
        s.push_str(", ");
        s.push_str(state);
    }
    if let Some(tip) = deco.and_then(|d| d.tooltip.as_deref()) {
        for line in tip.lines().filter(|l| !l.trim().is_empty()) {
            s.push_str(", ");
            s.push_str(line.trim());
        }
    } else if let Some(d) = deco {
        for b in &d.badges {
            s.push_str(", ");
            s.push_str(&b.text);
        }
    }
    s
}

/// One file in a list, grid, column or filmstrip: a row with its full
/// description as name, selection and position ("3 of 13").
pub fn file_item(resp: &egui::Response, label: &str, selected: bool, index: usize, count: usize) {
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(accesskit::Role::Row, enabled, label));
    resp.ctx.accesskit_node_builder(resp.id, |n| {
        n.set_selected(selected);
        n.set_position_in_set(index + 1);
        n.set_size_of_set(count);
        n.set_row_index(index);
    });
}

/// Turn the node of `ui` (the scroll area content holding the rows) into a
/// named list with `rows` items.
pub fn list(ui: &egui::Ui, label: &str, rows: usize) {
    let label = label.to_string();
    ui.ctx().accesskit_node_builder(ui.unique_id(), |n| {
        n.set_role(accesskit::Role::Table);
        n.set_label(label);
        n.set_row_count(rows);
    });
}

/// The file row that VoiceOver should follow while the file list has the
/// keyboard (no egui widget is focused): becomes the window's active
/// descendant, so arrow keys announce the row under the cursor.
pub fn set_active_descendant(ctx: &egui::Context, id: egui::Id) {
    ctx.accesskit_node_builder(egui::accesskit_root_id(), |n| {
        n.set_active_descendant(id.accesskit_id());
    });
}

/// Make `id`'s node a live region: VoiceOver reads its name when it
/// appears or changes. Errors interrupt (`Assertive`), the rest is polite.
pub fn live(ctx: &egui::Context, id: egui::Id, role: accesskit::Role, text: &str, urgent: bool) {
    let text = text.to_string();
    ctx.accesskit_node_builder(id, |n| {
        n.set_role(role);
        n.set_label(text);
        n.set_live(if urgent {
            accesskit::Live::Assertive
        } else {
            accesskit::Live::Polite
        });
        n.set_live_atomic();
    });
}

// ------------------------------------------------------------------ focus ring

/// Draw the keyboard focus ring around the focused widget (one place for
/// every widget, painted or stock). Text fields show their own focus.
pub fn paint_focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|m| m.focused()) else {
        return;
    };
    if ctx.text_edit_focused() {
        return;
    }
    let Some(resp) = ctx.read_response(id) else {
        return;
    };
    if !resp.rect.is_positive() || resp.interact_rect.area() <= 0.0 {
        return;
    }
    let p = crate::theme::pal_ctx(ctx);
    let painter = ctx
        .layer_painter(resp.layer_id)
        .with_clip_rect(resp.interact_rect.expand(4.0));
    let width = if increase_contrast() { 3.0 } else { 2.5 };
    painter.rect_stroke(
        resp.rect.shrink(0.5),
        CornerRadius::same(crate::theme::radius::CONTROL + 1),
        Stroke::new(width, p.focus_ring),
        StrokeKind::Outside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip() {
        for b in 0..8u8 {
            assert_eq!(DisplayOptions::from_bits(b).bits(), b);
        }
    }

    #[test]
    fn reduce_motion_zeroes_animations() {
        let ctx = egui::Context::default();
        set_for_test(&ctx, DisplayOptions::default());
        assert_eq!(anim(0.2), 0.2);
        set_for_test(
            &ctx,
            DisplayOptions {
                reduce_motion: true,
                ..Default::default()
            },
        );
        assert_eq!(anim(0.2), 0.0);
        assert_eq!(ctx.global_style().animation_time, 0.0);
        set_for_test(&ctx, DisplayOptions::default());
    }

    #[test]
    fn reduce_transparency_drops_scrims() {
        let ctx = egui::Context::default();
        set_for_test(&ctx, DisplayOptions::default());
        assert_ne!(scrim(false, 40, 90), Color32::TRANSPARENT);
        set_for_test(
            &ctx,
            DisplayOptions {
                reduce_transparency: true,
                ..Default::default()
            },
        );
        assert_eq!(scrim(true, 40, 90), Color32::TRANSPARENT);
        set_for_test(&ctx, DisplayOptions::default());
    }

    #[test]
    fn file_label_joins_parts() {
        assert_eq!(
            file_label(
                "README.md",
                "Markdown",
                Some("214 bytes"),
                Some("Today 09:41")
            ),
            "README.md, Markdown, 214 bytes, Today 09:41"
        );
        assert_eq!(file_label("src", "Folder", None, Some("")), "src, Folder");
    }
}
