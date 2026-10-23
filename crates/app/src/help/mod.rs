//! The Help window ("Manta Help"): the user manual from `docs/manual`,
//! embedded in the binary, in a native window with a table of contents,
//! full-text search, back / forward and the Markdown Preview's renderer.
//!
//! Opens with ⌘? (Help menu, palette, the "?" buttons in Settings and
//! dialogs). Other parts of the UI ask for a chapter with [`request`]
//! (`"08-verbindungen.md#sftp"`); the app picks that up once per frame.

pub mod manual;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Color32, CornerRadius, Rect, Sense, Stroke, TextFormat,
};
use manta_plugin_markdown::links::{self, ImageSource};
use manta_plugin_markdown::render::{Action as MdAction, Env, Heights, ImageState};

use crate::aux_window::{AuxKind, AuxWindow, Spec};
use crate::icons;
use crate::theme::{self, pal, radius, space, text};
use crate::widgets;
use manual::{Hit, Manual, Place};

/// The licence notices of the components Manta is built on
/// (`docs/third-party-licenses.md`, embedded like the developer docs).
pub const THIRD_PARTY_LICENSES: &str = "third-party-licenses.md";

/// Manta's website: news, help and contact. Linked from the Help menu
/// ("Manta Website & Support…"), the palette and Settings › About Manta.
pub const WEBSITE_URL: &str = "https://manta-filemanager.com/";

/// [`WEBSITE_URL`] as shown on its link.
pub const WEBSITE_LABEL: &str = "manta-filemanager.com";

/// The source code repository. Linked from the Help menu ("Source Code on
/// GitHub…"), the palette and Settings › About Manta.
pub const SOURCE_URL: &str = "https://github.com/web-workers-com/manta";

/// [`SOURCE_URL`] as shown on its link.
pub const SOURCE_LABEL: &str = "GitHub";

/// The licence text (MIT) in the repository.
pub const LICENSE_URL: &str = "https://github.com/web-workers-com/manta/blob/main/LICENSE";

/// Inner size when first opened.
pub const SIZE: [f32; 2] = [980.0, 680.0];
const TOC_W: f32 = 250.0;
const BAR_H: f32 = 46.0;

/// A page of the help window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// A chapter by file name (`05-dateien.md`).
    Chapter(String),
    /// An embedded developer doc (`plugins.md`).
    Dev(String),
}

/// A place in the history: page, section and how far it was scrolled.
#[derive(Debug, Clone, PartialEq)]
pub struct Loc {
    pub page: Page,
    pub anchor: Option<String>,
    pub scroll: f32,
}

/// What the window asks the app to do.
#[derive(Default)]
pub struct HelpOutcome {
    pub open_url: Option<String>,
}

pub struct Help {
    pub open: bool,
    pub window: AuxWindow,
    pub query: String,
    pub loc: Loc,
    pub back: Vec<Loc>,
    pub forward: Vec<Loc>,
    /// Chapters whose sections are shown in the TOC.
    pub expanded: HashSet<String>,
    /// Search result under the keyboard cursor.
    pub selected_hit: usize,
    focus_search: bool,
    /// Jump to this heading once drawn.
    anchor: Option<String>,
    /// Set the content's scroll offset next frame.
    scroll_to: Option<f32>,
    scroll_y: f32,
    anchor_y: HashMap<String, f32>,
    heights: Heights,
    images: HashMap<ImageSource, ImageState>,
    /// (page, language, dark) the image table was made for.
    images_key: Option<(Page, &'static str, bool)>,
    /// Search results for (query, language).
    hits: Option<(String, &'static str, Arc<Vec<Hit>>)>,
}

impl Default for Help {
    fn default() -> Self {
        Help {
            open: false,
            window: AuxWindow::default(),
            query: String::new(),
            loc: Loc {
                page: Page::Chapter(String::new()),
                anchor: None,
                scroll: 0.0,
            },
            back: Vec::new(),
            forward: Vec::new(),
            expanded: HashSet::new(),
            selected_hit: 0,
            focus_search: false,
            anchor: None,
            scroll_to: None,
            scroll_y: 0.0,
            anchor_y: HashMap::new(),
            heights: Heights::default(),
            images: HashMap::new(),
            images_key: None,
            hits: None,
        }
    }
}

fn request_id() -> egui::Id {
    egui::Id::unique("manta-help-request")
}

/// Ask for the help window at `target` (`"05-dateien.md#pausieren"`, or
/// `""` for wherever it was). Picked up by the app with [`take_request`].
pub fn request(ctx: &egui::Context, target: &str) {
    ctx.data_mut(|d| d.insert_temp(request_id(), Some(target.to_string())));
    ctx.request_repaint();
}

pub fn take_request(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Option<String>>(request_id())
            .take()
    })
}

/// Split `"file.md#anchor"`.
fn parse_target(target: &str) -> (Option<String>, Option<String>) {
    let (f, a) = match target.split_once('#') {
        Some((f, a)) => (f, Some(a.to_string()).filter(|a| !a.is_empty())),
        None => (target, None),
    };
    (Some(f.to_string()).filter(|f| !f.is_empty()), a)
}

/// The UI language's manual (with fallback).
pub fn current_manual() -> Arc<Manual> {
    manual::get(crate::i18n::locale().code())
}

impl Help {
    /// Open (or bring forward) at `target` (see [`request`]).
    pub fn show(&mut self, target: &str) {
        let m = current_manual();
        let (file, anchor) = parse_target(target);
        let fresh = matches!(&self.loc.page, Page::Chapter(f) if f.is_empty());
        match file {
            Some(f) if m.chapter(&f).is_some() => self.go(Page::Chapter(f), anchor),
            Some(f) if manual::dev_doc(&f).is_some() => self.go(Page::Dev(f), anchor),
            Some(f) => log::warn!("help: no chapter {f}"),
            None if fresh => {
                let first = m
                    .chapters
                    .first()
                    .map(|c| c.file.clone())
                    .unwrap_or_default();
                self.loc = Loc {
                    page: Page::Chapter(first),
                    anchor: None,
                    scroll: 0.0,
                };
                self.scroll_to = Some(0.0);
            }
            None => {}
        }
        if let Page::Chapter(f) = &self.loc.page {
            self.expanded.insert(f.clone());
        }
        if self.open {
            self.window.request_focus();
        }
        self.open = true;
    }

    /// Go to a page (and section), remembering where we were.
    pub fn go(&mut self, page: Page, anchor: Option<String>) {
        let empty = matches!(&self.loc.page, Page::Chapter(f) if f.is_empty());
        if !empty {
            if self.loc.page == page && self.loc.anchor == anchor && anchor.is_none() {
                self.scroll_to = Some(0.0);
                return;
            }
            let mut cur = self.loc.clone();
            cur.scroll = self.scroll_y;
            self.back.push(cur);
            self.forward.clear();
        }
        // The TOC follows: only the chapter shown is expanded.
        if let Page::Chapter(f) = &page {
            self.expanded.clear();
            self.expanded.insert(f.clone());
        }
        self.loc = Loc {
            page,
            anchor: anchor.clone(),
            scroll: 0.0,
        };
        self.arrive(anchor, 0.0);
    }

    fn arrive(&mut self, anchor: Option<String>, scroll: f32) {
        if anchor.is_some() {
            self.anchor = anchor;
            self.scroll_to = None;
        } else {
            self.anchor = None;
            self.scroll_to = Some(scroll);
        }
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.back.pop() {
            let mut cur = self.loc.clone();
            cur.scroll = self.scroll_y;
            self.forward.push(cur);
            self.loc = prev.clone();
            self.arrive(None, prev.scroll);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            let mut cur = self.loc.clone();
            cur.scroll = self.scroll_y;
            self.back.push(cur);
            self.loc = next.clone();
            self.arrive(None, next.scroll);
        }
    }

    /// Open search result `hit`.
    pub fn open_hit(&mut self, m: &Manual, hit: &Hit) {
        let file = m.chapters[hit.chapter].file.clone();
        self.go(Page::Chapter(file), hit.anchor.clone());
    }

    /// Search results for the current query (cached).
    pub fn hits(&mut self, m: &Manual) -> Arc<Vec<Hit>> {
        match &self.hits {
            Some((q, l, h)) if *q == self.query && *l == m.lang => h.clone(),
            _ => {
                let h = Arc::new(manual::search(m, &self.query));
                self.hits = Some((self.query.clone(), m.lang, h.clone()));
                h
            }
        }
    }

    /// Draw the window while it is open (call every frame).
    pub fn ui(&mut self, ctx: &egui::Context) -> HelpOutcome {
        let mut out = HelpOutcome::default();
        if !self.open {
            self.window.hidden(ctx, AuxKind::Help);
            return out;
        }
        let spec = Spec {
            kind: AuxKind::Help,
            title: t!("help.window_title"),
            size: vec2(SIZE[0], SIZE[1]),
            min_size: vec2(640.0, 420.0),
            margin: egui::Margin::ZERO,
            activate: true,
            zoomable: true,
        };
        let mut window = std::mem::take(&mut self.window);
        let (_, closed) = window.show(ctx, &spec, |ui| self.contents(ui, &mut out));
        self.window = window;
        if closed.0 {
            self.open = false;
            self.window.hidden(ctx, AuxKind::Help);
        }
        out
    }

    /// Does this window get the keys (its own native window, or the key
    /// window in embedded mode)?
    fn has_keys(ui: &egui::Ui) -> bool {
        !ui.ctx().embed_viewports()
            || crate::aux_window::embedded_key(ui.ctx()) == Some(AuxKind::Help)
    }

    /// The window content (also drawn on its own by the tests).
    pub fn contents(&mut self, ui: &mut egui::Ui, out: &mut HelpOutcome) {
        let m = current_manual();
        if matches!(&self.loc.page, Page::Chapter(f) if f.is_empty() || m.chapter(f).is_none()) {
            self.loc.page = Page::Chapter(
                m.chapters
                    .first()
                    .map(|c| c.file.clone())
                    .unwrap_or_default(),
            );
        }
        if Self::has_keys(ui) {
            let cmd = egui::Modifiers::COMMAND;
            if ui.input_mut(|i| i.consume_key(cmd, egui::Key::OpenBracket)) {
                self.go_back();
            }
            if ui.input_mut(|i| i.consume_key(cmd, egui::Key::CloseBracket)) {
                self.go_forward();
            }
            if ui.input_mut(|i| {
                i.consume_key(cmd, egui::Key::F)
                    || i.consume_key(cmd, egui::Key::Questionmark)
                    || i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::Slash)
            }) {
                self.focus_search = true;
            }
        }
        let p = pal(ui);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // ------------------------------------------------ toolbar
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR_H), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, p.chrome);
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            Stroke::new(1.0, p.separator),
        );
        let search_w = (TOC_W - 2.0 * space::LG).max(120.0);
        let search_rect = Rect::from_min_size(
            pos2(
                bar.left() + space::LG,
                bar.center().y - theme::size::FIELD / 2.0,
            ),
            vec2(search_w, theme::size::FIELD),
        );
        let search_id = egui::Id::unique("manta-help-search");
        let mut search_resp = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(search_rect), |ui| {
            let r =
                widgets::search_field(ui, &mut self.query, t!("help.search"), search_id, search_w);
            search_resp = Some(r);
        });
        let search_resp = search_resp.expect("drawn");
        if std::mem::take(&mut self.focus_search) {
            ui.memory_mut(|mem| mem.request_focus(search_id));
        }
        if search_resp.changed() {
            self.selected_hit = 0;
        }
        let mut x = bar.left() + TOC_W + space::MD;
        for (glyph, label, shortcut, enabled, back) in [
            (
                icons::CARET_LEFT,
                t!("help.back"),
                "⌘[",
                !self.back.is_empty(),
                true,
            ),
            (
                icons::CARET_RIGHT,
                t!("help.forward"),
                "⌘]",
                !self.forward.is_empty(),
                false,
            ),
        ] {
            let r = Rect::from_min_size(
                pos2(x, bar.center().y - theme::size::TOOLBAR_BUTTON / 2.0),
                vec2(30.0, theme::size::TOOLBAR_BUTTON),
            );
            let resp = ui
                .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                    widgets::icon_button(ui, glyph, enabled, false)
                })
                .inner;
            let resp = resp.on_hover_text(format!("{label}  {shortcut}"));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
            if resp.clicked() && enabled {
                if back {
                    self.go_back();
                } else {
                    self.go_forward();
                }
            }
            x = r.right() + space::XXS;
        }
        // Title of the page.
        let title = match &self.loc.page {
            Page::Chapter(f) => m.chapter(f).map(|c| c.title.clone()).unwrap_or_default(),
            Page::Dev(n) => manual::dev_title(n),
        };
        let g = ui
            .painter()
            .layout_no_wrap(title, theme::semibold(text::BODY), p.text);
        ui.painter().galley(
            pos2(x + space::MD, bar.center().y - g.size().y / 2.0),
            g,
            p.text,
        );

        // ------------------------------------------------ search keys
        let searching = !self.query.trim().is_empty();
        let hits = if searching {
            self.hits(&m)
        } else {
            Arc::new(Vec::new())
        };
        if searching && search_resp.has_focus() || search_resp.lost_focus() {
            let (down, up, enter) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::Enter),
                )
            });
            if !hits.is_empty() {
                if down {
                    self.selected_hit = (self.selected_hit + 1).min(hits.len() - 1);
                }
                if up {
                    self.selected_hit = self.selected_hit.saturating_sub(1);
                }
                if enter && search_resp.lost_focus() {
                    let h = hits[self.selected_hit.min(hits.len() - 1)].clone();
                    self.open_hit(&m, &h);
                }
            }
        }

        // ------------------------------------------------ left: TOC / results
        let body = Rect::from_min_max(pos2(bar.left(), bar.bottom()), ui.max_rect().max);
        let side = Rect::from_min_max(body.min, pos2(body.left() + TOC_W, body.bottom()));
        ui.painter()
            .rect_filled(side, CornerRadius::ZERO, p.sidebar);
        ui.painter().vline(
            side.right() - 0.5,
            side.y_range(),
            Stroke::new(1.0, p.separator),
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(side.shrink2(vec2(0.0, 0.0))),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("manta-help-side")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(space::MD);
                        if searching {
                            self.results_ui(ui, &m, &hits);
                        } else {
                            self.toc_ui(ui, &m);
                        }
                        ui.add_space(space::MD);
                    });
            },
        );

        // ------------------------------------------------ right: content
        let content = Rect::from_min_max(pos2(side.right(), body.top()), body.max);
        ui.painter()
            .rect_filled(content, CornerRadius::ZERO, p.content);
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            self.page_ui(ui, &m, out)
        });
    }

    fn toc_ui(&mut self, ui: &mut egui::Ui, m: &Manual) {
        let p = pal(ui);
        let current_file = match &self.loc.page {
            Page::Chapter(f) => Some(f.clone()),
            Page::Dev(_) => None,
        };
        // The section in view: the last heading above the top edge.
        let current_section = self
            .anchor_y
            .iter()
            .filter(|(_, y)| **y <= self.scroll_y + 24.0)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(k, _)| k.clone());
        let w = ui.available_width();
        for ch in &m.chapters {
            let is_current = current_file.as_deref() == Some(ch.file.as_str());
            let subs: Vec<_> = ch
                .sections
                .iter()
                .filter(|s| s.level == 2 && s.anchor.is_some())
                .collect();
            let open = self.expanded.contains(&ch.file);
            let (r, resp) =
                ui.allocate_exact_size(vec2(w, theme::size::SIDEBAR_ROW), Sense::click());
            let row = r.shrink2(vec2(space::MD, 1.0));
            if is_current {
                ui.painter().rect_filled(
                    row,
                    CornerRadius::same(radius::ROW),
                    p.selection_inactive,
                );
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(row, CornerRadius::same(radius::ROW), p.hover);
            }
            let caret =
                Rect::from_center_size(pos2(row.left() + 10.0, row.center().y), vec2(16.0, 16.0));
            let caret_resp = ui.interact(
                caret,
                ui.scope_id().with(("toc-caret", &ch.file)),
                Sense::click(),
            );
            if !subs.is_empty() {
                widgets::paint_icon(
                    ui,
                    caret,
                    if open {
                        icons::CARET_DOWN
                    } else {
                        icons::CARET_RIGHT
                    },
                    10.0,
                    p.text_tertiary,
                    false,
                );
            }
            let num = ui.painter().layout_no_wrap(
                format!("{}", ch.number),
                theme::medium(text::SMALL),
                p.text_tertiary,
            );
            let num_x = row.left() + 22.0;
            ui.painter().galley(
                pos2(
                    num_x + 14.0 - num.size().x,
                    row.center().y - num.size().y / 2.0,
                ),
                num,
                p.text_tertiary,
            );
            let label = egui::WidgetText::from(ch.title.as_str()).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                row.right() - num_x - 22.0 - space::XS,
                egui::FontSelection::FontId(if is_current {
                    theme::medium(text::BODY)
                } else {
                    theme::regular(text::BODY)
                }),
            );
            ui.painter().galley(
                pos2(num_x + 22.0, row.center().y - label.size().y / 2.0),
                label,
                p.text,
            );
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::TreeItem, true, is_current, &ch.title)
            });
            if caret_resp.clicked() && !subs.is_empty() {
                if open {
                    self.expanded.remove(&ch.file);
                } else {
                    self.expanded.insert(ch.file.clone());
                }
            } else if resp.clicked() {
                self.go(Page::Chapter(ch.file.clone()), None);
            }
            if open {
                for s in subs {
                    let anchor = s.anchor.clone().unwrap_or_default();
                    let on = is_current && current_section.as_deref() == Some(anchor.as_str());
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                    let row = r.shrink2(vec2(space::MD, 1.0));
                    if resp.hovered() {
                        ui.painter()
                            .rect_filled(row, CornerRadius::same(radius::ROW), p.hover);
                    }
                    let x0 = num_x + 22.0;
                    if on {
                        ui.painter().vline(
                            x0 - 7.0,
                            row.shrink2(vec2(0.0, 3.0)).y_range(),
                            Stroke::new(2.0, p.accent),
                        );
                    }
                    let g = egui::WidgetText::from(s.title.as_str()).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        row.right() - x0 - space::XS,
                        egui::FontSelection::FontId(theme::regular(text::SMALL)),
                    );
                    ui.painter().galley(
                        pos2(x0, row.center().y - g.size().y / 2.0),
                        g,
                        if on { p.text } else { p.text_secondary },
                    );
                    let label = s.title.clone();
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::TreeItem, true, &label)
                    });
                    if resp.clicked() {
                        self.go(Page::Chapter(ch.file.clone()), Some(anchor));
                    }
                }
            }
        }
    }

    fn results_ui(&mut self, ui: &mut egui::Ui, m: &Manual, hits: &[Hit]) {
        let p = pal(ui);
        let w = ui.available_width();
        if hits.is_empty() {
            ui.add_space(space::XL);
            ui.horizontal(|ui| {
                ui.add_space(space::LG + space::XS);
                ui.label(
                    egui::RichText::new(t!("help.no_results"))
                        .color(p.text_secondary)
                        .size(text::BODY),
                );
            });
            return;
        }
        let head = ui.painter().layout_no_wrap(
            t!("help.results", count = hits.len()),
            theme::semibold(text::CAPTION),
            p.text_secondary,
        );
        let (r, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
        ui.painter().galley(
            pos2(
                r.left() + space::LG + space::XS,
                r.center().y - head.size().y / 2.0,
            ),
            head,
            p.text_secondary,
        );
        for (i, h) in hits.iter().enumerate() {
            let ch = &m.chapters[h.chapter];
            let inner_w = w - 2.0 * (space::MD + space::SM);
            let title = egui::WidgetText::from(h.section.as_str()).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                inner_w,
                egui::FontSelection::FontId(theme::medium(text::BODY)),
            );
            let crumb = egui::WidgetText::from(format!(
                "{} {} · {}",
                t!("help.chapter"),
                ch.number,
                ch.title
            ))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                inner_w,
                egui::FontSelection::FontId(theme::regular(text::CAPTION)),
            );
            let snippet = {
                let mut job = LayoutJob::default();
                let base = TextFormat {
                    font_id: theme::regular(text::SMALL),
                    color: p.text_secondary,
                    ..Default::default()
                };
                let s = &h.snippet;
                match &h.mark {
                    Some(r) if s.is_char_boundary(r.start) && s.is_char_boundary(r.end) => {
                        job.append(&s[..r.start], 0.0, base.clone());
                        job.append(
                            &s[r.clone()],
                            0.0,
                            TextFormat {
                                color: p.text,
                                background: p.accent_soft,
                                font_id: theme::semibold(text::SMALL),
                                ..base.clone()
                            },
                        );
                        job.append(&s[r.end..], 0.0, base);
                    }
                    _ => job.append(s, 0.0, base),
                }
                job.wrap.max_width = inner_w;
                job.wrap.max_rows = 3;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let hgt = title.size().y
                + crumb.size().y
                + snippet.size().y
                + 2.0 * space::SM
                + 2.0 * space::XXS;
            let (r, resp) = ui.allocate_exact_size(vec2(w, hgt + 2.0), Sense::click());
            let card = r.shrink2(vec2(space::MD, 1.0));
            let sel = i == self.selected_hit;
            if sel {
                ui.painter().rect_filled(
                    card,
                    CornerRadius::same(radius::ROW),
                    p.selection_inactive,
                );
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(card, CornerRadius::same(radius::ROW), p.hover);
            }
            let mut y = card.top() + space::SM;
            let x = card.left() + space::SM;
            let (th, ch_h) = (title.size().y, crumb.size().y);
            ui.painter().galley(pos2(x, y), title, p.text);
            y += th + space::XXS;
            ui.painter().galley(pos2(x, y), crumb, p.text_tertiary);
            y += ch_h + space::XXS;
            ui.painter().galley(pos2(x, y), snippet, p.text_secondary);
            let label = format!("{} — {}", h.section, ch.title);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            if resp.clicked() {
                self.selected_hit = i;
                self.open_hit(m, h);
            }
            if sel
                && ui.input(|i| {
                    i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
                })
            {
                resp.scroll_to_me(None);
            }
        }
    }

    /// The page: the chapter (or dev doc) rendered with the Markdown
    /// Preview's renderer.
    fn page_ui(&mut self, ui: &mut egui::Ui, m: &Manual, out: &mut HelpOutcome) {
        let (doc, dir) = match &self.loc.page {
            Page::Chapter(f) => match m.chapter(f) {
                Some(c) => (c.doc.clone(), manual::chapter_dir(m.lang)),
                None => return,
            },
            Page::Dev(n) => match manual::dev_doc(n) {
                Some(d) => (d, manta_core::VfsPath::new(manual::DEV_DIR)),
                None => return,
            },
        };
        let dark = ui.visuals().dark_mode;
        let key = (self.loc.page.clone(), m.lang, dark);
        if self.images_key.as_ref() != Some(&key) {
            self.images = image_table(&doc, &dir, m.lang, dark);
            self.images_key = Some(key);
            self.heights = Heights::default();
        }
        let mut env = Env::new(ui, &doc, dir, &self.images, false);
        env.anchor = self.anchor.clone();
        let mut area = egui::ScrollArea::vertical()
            .id_salt("manta-help-page")
            .auto_shrink([false, false]);
        if let Some(y) = self.scroll_to.take() {
            area = area.vertical_scroll_offset(y);
        }
        let heights = &mut self.heights;
        let res = area.show_viewport(ui, |ui, vp| {
            let avail = ui.available_width();
            let side = space::XL * 2.0;
            let w = (avail - 2.0 * side)
                .clamp(80.0, manta_plugin_markdown::style::size::MEASURE + 40.0);
            let pad = ((avail - w) / 2.0).max(side);
            ui.add_space(space::XL + space::SM);
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    env.show(ui, vp, heights);
                });
            });
            vp.top()
        });
        self.scroll_y = res.inner;
        self.anchor = None;
        self.anchor_y = env.anchor_y.clone();
        let actions = std::mem::take(&mut env.actions);
        drop(env);
        for a in actions {
            self.follow(a, out);
        }
    }

    /// What a click in the page does: links between chapters (and to the
    /// embedded developer docs) navigate here, web links go to the browser.
    pub fn follow(&mut self, a: MdAction, out: &mut HelpOutcome) {
        match a {
            MdAction::OpenUrl(u) => out.open_url = Some(u),
            MdAction::Anchor(a) => {
                let page = self.loc.page.clone();
                self.go(page, Some(a));
            }
            MdAction::Markdown { path, anchor } => match manual::place_of(&path, anchor) {
                Place::Chapter { file, anchor } => self.go(Page::Chapter(file), anchor),
                Place::Dev { name, anchor } => self.go(Page::Dev(name), anchor),
                Place::Unknown(p) => log::info!("help: link outside the manual: {p}"),
            },
            MdAction::OpenFile(p) => log::info!("help: file link not shown: {p}"),
            MdAction::LoadImage(src) => {
                self.images.insert(src, ImageState::Missing);
            }
        }
    }
}

/// The images `doc` shows: embedded screenshots, else a quiet placeholder.
fn image_table(
    doc: &manta_plugin_markdown::doc::Doc,
    dir: &manta_core::VfsPath,
    lang: &str,
    dark: bool,
) -> HashMap<ImageSource, ImageState> {
    let mut map = HashMap::new();
    for (src, _) in manual::image_refs(doc) {
        let source = links::image_source(&src, dir);
        let state = match &source {
            ImageSource::File(p) if p.parent().is_some_and(|d| d.file_name() == "images") => {
                match manual::image(p.file_name(), lang, dark) {
                    Some((key, bytes)) => ImageState::Ready {
                        uri: format!("bytes://manta-help/{key}"),
                        bytes: Arc::from(bytes),
                    },
                    None => ImageState::Missing,
                }
            }
            _ => ImageState::Missing,
        };
        map.insert(source, state);
    }
    map
}

/// A small round "?" button (like macOS's help buttons) that opens the
/// manual at `target` (`"16-einstellungen.md#plugins"`).
pub fn help_button(ui: &mut egui::Ui, target: &str) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    help_button_at(ui, rect, target)
}

/// [`help_button`] in `rect`, without taking layout space (for rows whose
/// height must not change, such as section titles).
pub fn help_button_at(ui: &mut egui::Ui, rect: Rect, target: &str) -> egui::Response {
    let p = pal(ui);
    let resp = ui.interact(
        rect,
        ui.scope_id().with(("manta-help-button", target)),
        Sense::click(),
    );
    let c = rect.center();
    let fill = if resp.is_pointer_button_down_on() {
        p.pressed
    } else if resp.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .circle(c, 8.5, fill, Stroke::new(1.0, p.border));
    ui.painter().text(
        c + vec2(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        "?",
        theme::semibold(text::SMALL),
        p.text_secondary,
    );
    let resp = widgets::tooltip(resp, t!("help.button.tip"), "");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t!("help.button")));
    if resp.clicked() {
        request(ui.ctx(), target);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_parse() {
        assert_eq!(
            parse_target("05-dateien.md#pausieren"),
            (Some("05-dateien.md".into()), Some("pausieren".into()))
        );
        assert_eq!(parse_target(""), (None, None));
        assert_eq!(
            parse_target("08-verbindungen.md"),
            (Some("08-verbindungen.md".into()), None)
        );
    }

    #[test]
    fn history_goes_back_and_forward() {
        let mut h = Help::default();
        h.show("");
        assert_eq!(h.loc.page, Page::Chapter("01-erste-schritte.md".into()));
        h.go(
            Page::Chapter("05-dateien.md".into()),
            Some("loeschen".into()),
        );
        h.go(Page::Chapter("08-verbindungen.md".into()), None);
        assert_eq!(h.back.len(), 2);
        h.go_back();
        assert_eq!(h.loc.page, Page::Chapter("05-dateien.md".into()));
        assert_eq!(h.loc.anchor.as_deref(), Some("loeschen"));
        h.go_forward();
        assert_eq!(h.loc.page, Page::Chapter("08-verbindungen.md".into()));
        assert!(h.forward.is_empty());
        // A new page drops the forward history.
        h.go_back();
        h.go(
            Page::Chapter("13-vergleichen-synchronisieren.md".into()),
            None,
        );
        assert!(h.forward.is_empty());
    }

    #[test]
    fn missing_screenshots_become_placeholders() {
        let m = manual::get("de");
        let ch = m.chapter("05-dateien.md").unwrap();
        let t = image_table(&ch.doc, &manual::chapter_dir("de"), "de", false);
        assert!(!t.is_empty());
        for (src, st) in &t {
            let ImageSource::File(p) = src else {
                panic!("{src:?}")
            };
            assert!(p.as_str().starts_with("/docs/manual/de/images/"), "{p}");
            let embedded = manual::image(p.file_name(), "de", false).is_some();
            assert_eq!(matches!(st, ImageState::Ready { .. }), embedded, "{p}");
            assert!(matches!(st, ImageState::Ready { .. } | ImageState::Missing));
        }
    }
}
