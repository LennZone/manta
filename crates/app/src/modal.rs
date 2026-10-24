//! Modality: what may still be used while a modal window, dialog or menu
//! is up.
//!
//! Three kinds of "modal", from the strongest to the lightest:
//!
//! * **App-modal windows** (Settings, Software Update): real native windows
//!   (see `aux_window.rs`). While one is open the main window takes no
//!   pointer, keyboard, scroll or file-drop input and is dimmed; a click or
//!   a key in it brings the modal window back to the front with the system
//!   alert sound, as macOS does for app-modal panels. Moving the main window
//!   by its title bar still works (that is AppKit's, not egui's), and ⌘Q
//!   quits (AppKit's menu). The Help window, Compare & Sync and the preview
//!   stay *non*-modal: people read the manual while they work, Compare &
//!   Sync is driven from the panes (the A/B buttons in their path bars) and
//!   the preview follows the selection like Quick Look.
//! * **In-window dialogs and sheets** (Connect, Delete, What's New, conflict
//!   and host key questions, the command palette, Go to Path, plugin
//!   sheets): egui modals or full-window backdrops; nothing behind them
//!   reacts to the pointer and the main window's shortcuts are off.
//! * **Popovers and menus** (view options, context menus, pop-up buttons):
//!   a click outside closes them and is used up by that, so it never
//!   selects or opens the item underneath.
//!
//! The app asks one question, `MantaApp::modal_open`, before it runs a
//! shortcut, a menu bar command, a toast's command or accepts a file drop.

use eframe::egui::{self, Event, RawInput};

/// The system alert sound ("you can't do that here").
pub fn beep() {
    #[cfg(test)]
    BEEPS.with(|b| b.set(b.get() + 1));
    #[cfg(all(target_os = "macos", not(test)))]
    {
        #[link(name = "AppKit", kind = "framework")]
        unsafe extern "C" {
            fn NSBeep();
        }
        // SAFETY: plain AppKit function without arguments; called on the
        // main thread (from the UI pass).
        unsafe { NSBeep() };
    }
}

#[cfg(test)]
thread_local! {
    static BEEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How often [`beep`] sounded on this thread (tests).
#[cfg(test)]
pub fn beeps() -> usize {
    BEEPS.with(|b| b.get())
}

/// Take every user input out of the main window's `raw` input while an
/// app-modal window is open: clicks, keys, text, scrolling, zooming, file
/// drops. Pointer moves become "pointer gone" so nothing shows hover
/// highlights or tooltips. Window focus, modifier changes and screen-reader
/// tree requests stay.
///
/// Returns true when the user tried to use the window (pressed a mouse
/// button or a key, or dropped files): the caller then brings the modal
/// window to the front and beeps.
pub fn block_input(raw: &mut RawInput) -> bool {
    let mut attempted = !raw.dropped_files.is_empty();
    raw.hovered_files.clear();
    raw.dropped_files.clear();
    let mut gone = false;
    let events = std::mem::take(&mut raw.events);
    for e in events {
        match e {
            Event::PointerButton { pressed: true, .. } => attempted = true,
            Event::Key {
                pressed: true,
                repeat: false,
                ..
            } => attempted = true,
            Event::PointerMoved(_) | Event::PointerGone => {
                if !gone {
                    gone = true;
                    raw.events.push(Event::PointerGone);
                }
            }
            Event::WindowFocused(_) | Event::ModifiersChanged(_) => raw.events.push(e),
            _ => {}
        }
    }
    attempted
}

/// Popovers and menus: a mouse press outside the open popup only closes it.
///
/// egui closes a popup on an outside click but also lets that click reach
/// the widget underneath (a row gets selected, a button fires). Here the
/// press (and its release) is removed from `raw` instead and the popup is
/// closed. `swallow_release` carries "the next release belongs to a
/// swallowed press" across frames.
pub fn dismiss_popup_on_outside_press(
    ctx: &egui::Context,
    raw: &mut RawInput,
    swallow_release: &mut bool,
) {
    let popup_open = egui::Popup::is_any_open(ctx);
    let mut close = false;
    raw.events.retain(|e| match e {
        Event::PointerButton {
            pos, pressed: true, ..
        } if popup_open && !close => {
            // Popups, menus and their sub menus are Foreground / Tooltip
            // layers; sheets put a Foreground backdrop over everything, so
            // a press inside a dialog (or on its backdrop) is not "outside".
            let over_popup = ctx
                .layer_id_at(*pos)
                .is_some_and(|l| l.order >= egui::Order::Foreground);
            if over_popup {
                true
            } else {
                close = true;
                *swallow_release = true;
                false
            }
        }
        Event::PointerButton { pressed: false, .. } if *swallow_release => {
            *swallow_release = false;
            false
        }
        _ => true,
    });
    if close {
        egui::Popup::close_all(ctx);
        ctx.request_repaint();
    }
}

/// The dim layer over the main window while an app-modal window is open
/// (below in-window sheets, which stay readable). `catch`: it also takes
/// the clicks meant for what is behind it (when the modal window is drawn
/// inside the main window; a native one gets them filtered out instead).
pub fn scrim(ctx: &egui::Context, catch: bool) -> egui::LayerId {
    let p = crate::theme::pal_ctx(ctx);
    let screen = ctx.content_rect();
    egui::Area::new(scrim_id())
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .interactable(catch)
        .show(ctx, |ui| {
            let sense = if catch {
                egui::Sense::CLICK | egui::Sense::DRAG
            } else {
                egui::Sense::hover()
            };
            let (rect, resp) = ui.allocate_exact_size(screen.size(), sense);
            // Only dims and catches clicks: not something VoiceOver should
            // stop at.
            resp.ctx.accesskit_node_builder(resp.id, |n| n.set_hidden());
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::ZERO,
                crate::a11y::scrim(p.dark, 36, 80),
            );
        })
        .response
        .layer_id
}

pub fn scrim_id() -> egui::Id {
    egui::Id::unique("manta-app-modal-scrim")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers, PointerButton, Pos2};

    fn key(key: Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn block_input_strips_user_input_and_reports_attempts() {
        let mut raw = RawInput {
            events: vec![
                Event::PointerMoved(Pos2::new(10.0, 10.0)),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 20.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                    source: egui::MouseWheelSource::Unknown,
                },
                Event::PointerMoved(Pos2::new(12.0, 10.0)),
                Event::Text("a".into()),
                Event::Copy,
                Event::WindowFocused(true),
                Event::ModifiersChanged(Modifiers::COMMAND),
            ],
            ..Default::default()
        };
        assert!(
            !block_input(&mut raw),
            "moving and scrolling are no attempt"
        );
        assert_eq!(
            raw.events,
            vec![
                Event::PointerGone,
                Event::WindowFocused(true),
                Event::ModifiersChanged(Modifiers::COMMAND),
            ]
        );

        let mut raw = RawInput {
            events: vec![Event::PointerButton {
                pos: Pos2::new(5.0, 5.0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        assert!(block_input(&mut raw), "a click is");
        assert!(raw.events.is_empty());

        let mut raw = RawInput {
            events: vec![key(Key::W, true, false), key(Key::W, false, false)],
            ..Default::default()
        };
        assert!(block_input(&mut raw), "a key is (⌘W too)");
        assert!(raw.events.is_empty());

        let mut raw = RawInput {
            events: vec![key(Key::A, true, true)],
            ..Default::default()
        };
        assert!(!block_input(&mut raw), "key repeat beeps only once");

        let mut raw = RawInput {
            hovered_files: vec![egui::HoveredFile::default()],
            dropped_files: vec![crate::native_drag::test_hooks::dropped(
                std::path::Path::new("/tmp/x"),
            )],
            ..Default::default()
        };
        assert!(block_input(&mut raw), "a file drop is");
        assert!(raw.hovered_files.is_empty() && raw.dropped_files.is_empty());
    }
}
