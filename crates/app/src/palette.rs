//! Command palette (⇧⌘P / ⌘P): type to find any command, see its shortcut,
//! run it with Enter.

use eframe::egui::{self, CornerRadius, Key, Modifiers, RichText, Sense};

use crate::commands::{self, Cmd};

/// What a palette row runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteId {
    Cmd(Cmd),
    /// `plugin_id:command_id`.
    Plugin(String),
    /// Open or close a plugin panel: (plugin id, panel id).
    TogglePanel(String, String),
    /// Reveal the diagnostics log in Finder.
    ShowLogFile,
    /// Settings → General → Default file manager (macOS).
    DefaultViewer,
    /// Use Manta as the default file manager, or give the role back.
    ToggleDefaultViewer,
    /// Manta → Check for Updates….
    CheckForUpdates,
}

/// One row of the palette.
pub struct PaletteEntry {
    pub id: PaletteId,
    pub label: String,
    pub category: String,
    pub shortcut: String,
    pub icon: &'static str,
    pub keywords: String,
    pub enabled: bool,
}

impl PaletteEntry {
    pub fn from_cmd(cmd: Cmd, enabled: bool) -> Self {
        PaletteEntry {
            id: PaletteId::Cmd(cmd),
            label: cmd.label().into(),
            category: cmd.category().into(),
            shortcut: cmd.shortcut().into(),
            icon: cmd.icon(),
            keywords: cmd.keywords().into(),
            enabled,
        }
    }
}
use crate::theme::{self, pal, radius, space, text};
use crate::widgets;

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    query: String,
    selected: usize,
    just_opened: bool,
}

const MAX_ROWS: usize = 9;
const ROW_H: f32 = 32.0;

impl CommandPalette {
    pub fn show(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.just_opened = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    fn results<'a>(&self, entries: &'a [PaletteEntry]) -> Vec<(&'a PaletteEntry, bool)> {
        let mut v: Vec<(&PaletteEntry, i32)> = entries
            .iter()
            .filter_map(|e| {
                commands::match_text(&e.label, &e.keywords, &e.category, &self.query)
                    .map(|s| (e, s))
            })
            .collect();
        if !self.query.trim().is_empty() {
            // Stable: equal scores keep built-ins before plugin commands.
            v.sort_by_key(|(_, s)| -s);
        }
        v.into_iter().map(|(e, _)| (e, e.enabled)).collect()
    }

    /// Draw the palette; returns the entry to run.
    pub fn ui(&mut self, ctx: &egui::Context, entries: &[PaletteEntry]) -> Option<PaletteId> {
        if !self.open {
            return None;
        }
        let results = self.results(entries);
        let mut run: Option<PaletteId> = None;

        // Keys handled before the text field sees them.
        let (up, down, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            self.close();
            return None;
        }
        if !results.is_empty() {
            if down {
                self.selected = (self.selected + 1) % results.len();
            }
            if up {
                self.selected = (self.selected + results.len() - 1) % results.len();
            }
        }
        self.selected = self.selected.min(results.len().saturating_sub(1));
        if enter {
            if let Some((c, true)) = results.get(self.selected) {
                run = Some(c.id.clone());
            }
        }

        let screen = ctx.content_rect();
        let width = (screen.width() - 2.0 * space::XL).min(560.0);
        let p = theme::pal_ctx(ctx);

        // Dim the app behind; a click outside closes.
        let backdrop = egui::Area::new(egui::Id::unique("palette-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // A click beside the sheet closes it (VoiceOver: "Close").
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::CLICK);
                crate::a11y::name(&resp, egui::accesskit::Role::Button, t!("common.close"));
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    crate::a11y::scrim(p.dark, 40, 90),
                );
                resp
            });
        if backdrop.inner.clicked() {
            self.close();
            return None;
        }

        egui::Area::new(egui::Id::unique("command-palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(
                screen.center().x - width / 2.0,
                screen.top() + (screen.height() * 0.14).max(48.0),
            ))
            .show(ctx, |ui| {
                let p = pal(ui);
                egui::Frame::new()
                    .fill(p.elevated)
                    .stroke(egui::Stroke::new(1.0, p.separator))
                    .corner_radius(CornerRadius::same(radius::WINDOW))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(egui::Margin::same(space::SM as i8))
                    .show(ui, |ui| {
                        ui.set_width(width - 2.0 * space::SM);
                        // Search row: icon and query share one vertical centre.
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 36.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add_space(space::SM);
                                widgets::icon(
                                    ui,
                                    crate::icons::MAGNIFYING_GLASS,
                                    17.0,
                                    p.text_secondary,
                                );
                                let id = egui::Id::unique("palette-query");
                                let resp = widgets::show_with_hint(
                                    ui,
                                    egui::TextEdit::singleline(&mut self.query)
                                        .id(id)
                                        .frame(
                                            egui::Frame::NONE
                                                .inner_margin(egui::Margin::symmetric(2, 0)),
                                        )
                                        .font(theme::regular(text::TITLE + 2.0))
                                        .min_size(egui::vec2(0.0, 36.0))
                                        .align(egui::Align2::LEFT_CENTER)
                                        .desired_width(f32::INFINITY),
                                    theme::regular(text::TITLE + 2.0),
                                    t!("palette.placeholder"),
                                );
                                if self.just_opened || !resp.has_focus() {
                                    resp.request_focus();
                                    self.just_opened = false;
                                }
                                if resp.changed() {
                                    self.selected = 0;
                                }
                            },
                        );
                        ui.add_space(space::XS);
                        ui.painter().hline(
                            ui.min_rect().x_range(),
                            ui.cursor().top(),
                            egui::Stroke::new(1.0, p.separator),
                        );
                        ui.add_space(space::XS);

                        if results.is_empty() {
                            ui.add_space(space::MD);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(t!("palette.no_match")).color(p.text_secondary),
                                );
                            });
                            ui.add_space(space::MD);
                            return;
                        }
                        let rows = results.len().min(MAX_ROWS);
                        egui::ScrollArea::vertical()
                            .max_height(rows as f32 * ROW_H + 2.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (idx, (cmd, en)) in results.iter().enumerate() {
                                    // Rows are picked with ↑/↓ in the search field, not
                                    // Tab; VoiceOver follows the selected one (the
                                    // field's active descendant).
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), ROW_H),
                                        if *en { Sense::CLICK } else { Sense::hover() },
                                    );
                                    let mut spoken = format!("{}, {}", cmd.label, cmd.category);
                                    if !cmd.shortcut.is_empty() {
                                        spoken.push_str(", ");
                                        spoken.push_str(&cmd.shortcut);
                                    }
                                    resp.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::Role::ListBoxOption,
                                            *en,
                                            &spoken,
                                        )
                                    });
                                    resp.ctx.accesskit_node_builder(resp.id, |n| {
                                        n.set_selected(idx == self.selected);
                                        n.set_position_in_set(idx + 1);
                                        n.set_size_of_set(results.len());
                                    });
                                    if idx == self.selected {
                                        let row = resp.id.accesskit_id();
                                        resp.ctx.accesskit_node_builder(
                                            egui::Id::unique("palette-query"),
                                            |n| n.set_active_descendant(row),
                                        );
                                    }
                                    if resp.hovered()
                                        && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                                    {
                                        self.selected = idx;
                                    }
                                    let sel = idx == self.selected;
                                    if sel {
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(radius::ROW + 2),
                                            p.selection,
                                        );
                                        if up || down {
                                            ui.scroll_to_rect(rect, None);
                                        }
                                    }
                                    let fg = if sel {
                                        p.text_on_accent
                                    } else if *en {
                                        p.text
                                    } else {
                                        p.text_tertiary
                                    };
                                    let sub = if sel {
                                        p.text_on_accent.gamma_multiply(0.8)
                                    } else if *en {
                                        p.text_secondary
                                    } else {
                                        p.text_tertiary
                                    };
                                    let ir = egui::Rect::from_min_size(
                                        rect.min + egui::vec2(space::SM, 0.0),
                                        egui::vec2(26.0, ROW_H),
                                    );
                                    widgets::paint_icon(
                                        ui,
                                        ir,
                                        cmd.icon,
                                        16.0,
                                        if sel { fg } else { p.text_secondary },
                                        false,
                                    );
                                    let painter = ui.painter();
                                    let label = painter.layout_no_wrap(
                                        cmd.label.clone(),
                                        theme::regular(text::BODY),
                                        fg,
                                    );
                                    let lw = label.size().x;
                                    painter.galley(
                                        egui::pos2(
                                            ir.right() + space::XS,
                                            rect.center().y - label.size().y / 2.0,
                                        ),
                                        label,
                                        fg,
                                    );
                                    let cat = painter.layout_no_wrap(
                                        cmd.category.clone(),
                                        theme::regular(text::SMALL),
                                        sub,
                                    );
                                    painter.galley(
                                        egui::pos2(
                                            ir.right() + space::XS + lw + space::MD,
                                            rect.center().y - cat.size().y / 2.0,
                                        ),
                                        cat,
                                        sub,
                                    );
                                    let sc = painter.layout_no_wrap(
                                        cmd.shortcut.clone(),
                                        theme::regular(text::BODY),
                                        sub,
                                    );
                                    painter.galley(
                                        egui::pos2(
                                            rect.right() - space::MD - sc.size().x,
                                            rect.center().y - sc.size().y / 2.0,
                                        ),
                                        sc,
                                        sub,
                                    );
                                    if resp.clicked() {
                                        run = Some(cmd.id.clone());
                                    }
                                }
                            });
                        ui.add_space(space::XS);
                        ui.horizontal(|ui| {
                            ui.add_space(space::SM);
                            ui.label(
                                RichText::new(t!("palette.footer", run = crate::icons::RETURN))
                                    .font(theme::regular(text::CAPTION))
                                    .color(p.text_tertiary),
                            );
                        });
                    });
            });

        if run.is_some() {
            self.close();
        }
        run
    }
}
