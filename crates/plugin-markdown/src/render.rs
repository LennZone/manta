//! Draws a [`Doc`] with egui: a calm reader with a readable measure,
//! generous line height, headings on the app's type scale, highlighted
//! code, tables, task lists, quotes / GitHub alerts, images and footnotes.
//!
//! The renderer never does IO. It reports what the user did (links,
//! copies) and which images it needs as [`Action`]s.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{pos2, vec2, Align, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Ui};
use manta_core::VfsPath;

use crate::doc::{plain_text, Alert, Align as ColAlign, Block, Doc, Inline, Item};
use crate::i18n::tr;
use crate::links::{self, ImageSource, Target};
use crate::style::{self, icon, mix, size, space, Colors};

/// What the user asked for while the document was drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Open in the browser / default app.
    OpenUrl(String),
    /// Show another Markdown file in the preview.
    Markdown {
        path: VfsPath,
        anchor: Option<String>,
    },
    /// Scroll to a heading (or footnote) of this document.
    Anchor(String),
    /// A link to a non-Markdown file or folder.
    OpenFile(VfsPath),
    /// Load this image (not yet in the image table).
    LoadImage(ImageSource),
}

/// An image the plugin loaded (or tried to).
#[derive(Debug, Clone)]
pub enum ImageState {
    Loading,
    Ready {
        uri: String,
        bytes: Arc<[u8]>,
    },
    Failed(String),
    /// Known not to exist (yet), e.g. a screenshot the manual will get
    /// later: a quiet box with the alt text instead of an error.
    Missing,
}

/// Heights of the top-level blocks, from earlier frames: blocks far
/// outside the visible area are skipped (their space is just reserved),
/// so long documents stay fast.
#[derive(Default, Debug, Clone)]
pub struct Heights {
    width: f32,
    pub tops: Vec<f32>,
    pub heights: Vec<f32>,
}

/// The renderer's inputs and outputs for one frame.
pub struct Env<'a> {
    pub doc: &'a Doc,
    /// Folder of the document (links and images are relative to it).
    pub dir: VfsPath,
    pub images: &'a HashMap<ImageSource, ImageState>,
    /// Remote (http) images are allowed.
    pub web_images: bool,
    /// Scroll to this heading slug / footnote (`fn-label`) once it is drawn.
    pub anchor: Option<String>,
    pub actions: Vec<Action>,
    /// Where each anchor was drawn this frame (content coordinates).
    pub anchor_y: HashMap<String, f32>,
    col: Colors,
    ids: usize,
    /// Nesting of bullet lists (bullet shape).
    depth: usize,
    origin_y: f32,
}

/// Text settings handed down the tree.
#[derive(Clone, Copy)]
struct Tx {
    size: f32,
    line: f32,
    color: Color32,
    strong: bool,
}

enum Hit {
    Link(usize),
    Footnote(String),
}

const PARA_GAP: f32 = 12.0;

impl<'a> Env<'a> {
    pub fn new(
        ui: &Ui,
        doc: &'a Doc,
        dir: VfsPath,
        images: &'a HashMap<ImageSource, ImageState>,
        web_images: bool,
    ) -> Self {
        Env {
            doc,
            dir,
            images,
            web_images,
            anchor: None,
            actions: Vec::new(),
            anchor_y: HashMap::new(),
            col: Colors::of(ui),
            ids: 0,
            depth: 0,
            origin_y: 0.0,
        }
    }

    fn body(&self) -> Tx {
        Tx {
            size: size::BODY,
            line: size::BODY_LINE,
            color: self.col.text,
            strong: false,
        }
    }

    fn next_id(&mut self, ui: &Ui) -> egui::Id {
        self.ids += 1;
        ui.scope_id().with(("md-block", self.ids))
    }

    /// Draw the whole document into `ui` (inside a vertical scroll area;
    /// `viewport` is its visible rect in content coordinates).
    pub fn show(&mut self, ui: &mut Ui, viewport: Rect, heights: &mut Heights) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let width = ui.available_width();
        if (heights.width - width).abs() > 0.5 || heights.heights.len() != self.doc.blocks.len() {
            *heights = Heights {
                width,
                tops: vec![f32::NAN; self.doc.blocks.len()],
                heights: vec![f32::NAN; self.doc.blocks.len()],
            };
        }
        let top = ui.min_rect().top();
        self.origin_y = top;
        // Draw everything while jumping to an anchor (its block must exist).
        let cull = self.anchor.is_none();
        let margin = 400.0;
        let tx = self.body();
        let blocks = self.doc.blocks.clone();
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                ui.add_space(gap_before(b));
            }
            let y = ui.cursor().top() - top;
            let h = heights.heights[i];
            let visible =
                y + h.max(0.0) >= viewport.top() - margin && y <= viewport.bottom() + margin;
            if cull && h.is_finite() && !visible {
                ui.allocate_space(vec2(width, h));
                if let Block::Heading { slug, .. } = b {
                    self.anchor_y.insert(slug.clone(), y);
                }
                heights.tops[i] = y;
                continue;
            }
            let start = ui.cursor().top();
            self.block(ui, b, tx);
            heights.tops[i] = y;
            heights.heights[i] = ui.min_rect().bottom() - start;
        }
        ui.add_space(space::XL * 2.0);
    }

    // -------------------------------------------------------------- blocks

    fn blocks(&mut self, ui: &mut Ui, blocks: &[Block], tx: Tx, gap: f32) {
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                ui.add_space(if matches!(b, Block::Heading { .. }) {
                    gap_before(b)
                } else {
                    gap
                });
            }
            self.block(ui, b, tx);
        }
    }

    fn block(&mut self, ui: &mut Ui, b: &Block, tx: Tx) {
        match b {
            Block::Heading {
                level,
                inlines,
                slug,
            } => self.heading(ui, *level, inlines, slug),
            Block::Paragraph(inl) => self.paragraph(ui, inl, tx),
            Block::Code {
                lang,
                text,
                highlight,
            } => self.code(ui, lang, text, highlight.as_deref()),
            Block::Quote { alert, blocks } => self.quote(ui, *alert, blocks, tx),
            Block::List { start, items } => self.list(ui, *start, items, tx),
            Block::Table { aligns, head, rows } => self.table(ui, aligns, head, rows, tx),
            Block::Rule => {
                let (r, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 17.0), Sense::hover());
                ui.painter().hline(
                    r.x_range(),
                    r.center().y,
                    Stroke::new(1.0, self.col.separator),
                );
            }
            Block::Html { images, text } => {
                if !images.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(space::XS, space::XS);
                        for (src, alt) in images {
                            self.image(ui, src, alt, "", None);
                        }
                    });
                }
                if !text.is_empty() {
                    if !images.is_empty() {
                        ui.add_space(space::SM);
                    }
                    let inl = vec![Inline::Text {
                        text: text.clone(),
                        style: Default::default(),
                        link: None,
                    }];
                    self.paragraph(
                        ui,
                        &inl,
                        Tx {
                            color: self.col.text2,
                            ..tx
                        },
                    );
                }
            }
            Block::Footnote {
                label,
                number,
                blocks,
            } => {
                let y = ui.cursor().top() - self.origin_y;
                let key = format!("fn-{label}");
                self.anchor_y.insert(key.clone(), y);
                let small = Tx {
                    size: size::SMALL,
                    line: 18.0,
                    color: self.col.text2,
                    ..tx
                };
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, small.line), Sense::hover());
                    let g = ui.painter().layout_no_wrap(
                        format!("{number}."),
                        style::regular(size::SMALL),
                        self.col.text3,
                    );
                    ui.painter().galley(
                        pos2(
                            r.right() - g.size().x - space::SM,
                            r.center().y - g.size().y / 2.0,
                        ),
                        g,
                        self.col.text3,
                    );
                    ui.vertical(|ui| self.blocks(ui, blocks, small, space::XS));
                });
                self.jump_if_target(ui, &key, ui.min_rect());
            }
        }
    }

    fn jump_if_target(&mut self, ui: &Ui, key: &str, rect: Rect) {
        if self.anchor.as_deref() == Some(key) {
            ui.scroll_to_rect(rect.expand2(vec2(0.0, space::LG)), Some(Align::TOP));
            self.anchor = None;
        }
    }

    fn heading(&mut self, ui: &mut Ui, level: u8, inlines: &[Inline], slug: &str) {
        let (sz, color) = match level {
            1 => (size::H1, self.col.text),
            2 => (size::H2, self.col.text),
            3 => (size::H3, self.col.text),
            4 => (size::H4, self.col.text),
            5 => (size::SMALL, self.col.text),
            _ => (size::SMALL, self.col.text2),
        };
        let line = (sz * 1.3).round();
        let tx = Tx {
            size: sz,
            line,
            color,
            strong: true,
        };
        let top = ui.cursor().top();
        self.anchor_y.insert(slug.to_string(), top - self.origin_y);
        self.paragraph(ui, inlines, tx);
        if level <= 2 {
            ui.add_space(space::SM);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().hline(
                r.x_range(),
                r.center().y,
                Stroke::new(1.0, self.col.separator),
            );
        }
        let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top());
        self.jump_if_target(ui, slug, rect);
    }

    // -------------------------------------------------------------- inline

    fn font(&self, ui: &Ui, tx: Tx, strong: bool) -> FontId {
        if tx.strong || strong {
            style::semibold(ui, tx.size)
        } else {
            style::regular(tx.size)
        }
    }

    /// A paragraph: one wrapping text block, or a wrapped flow of text and
    /// images when it contains images.
    fn paragraph(&mut self, ui: &mut Ui, inlines: &[Inline], tx: Tx) {
        if !inlines.iter().any(|i| matches!(i, Inline::Image { .. })) {
            self.text_run(ui, inlines, tx);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(space::XS, space::XS);
            let mut run: Vec<Inline> = Vec::new();
            for i in inlines {
                match i {
                    Inline::Image {
                        src,
                        alt,
                        title,
                        link,
                    } => {
                        if run.iter().any(
                            |r| !matches!(r, Inline::Text { text, .. } if text.trim().is_empty()),
                        ) {
                            self.text_run(ui, &run, tx);
                        }
                        run.clear();
                        self.image(ui, src, alt, title, *link);
                    }
                    Inline::Break => {
                        self.text_run(ui, &run, tx);
                        run.clear();
                        ui.end_row();
                    }
                    other => run.push(other.clone()),
                }
            }
            if !run.is_empty() {
                self.text_run(ui, &run, tx);
            }
        });
    }

    fn job(&self, ui: &Ui, inlines: &[Inline], tx: Tx) -> (LayoutJob, Vec<(Range<usize>, Hit)>) {
        let mut job = LayoutJob::default();
        let mut hits = Vec::new();
        let mut chars = 0usize;
        for i in inlines {
            match i {
                Inline::Text {
                    text,
                    style: st,
                    link,
                } => {
                    let mut f = TextFormat {
                        font_id: self.font(ui, tx, st.strong),
                        color: if link.is_some() {
                            self.col.link
                        } else {
                            tx.color
                        },
                        line_height: Some(tx.line),
                        italics: st.emph,
                        valign: Align::Center,
                        ..Default::default()
                    };
                    if st.strike {
                        f.strikethrough = Stroke::new(1.0, mix(tx.color, self.col.bg, 0.25));
                        f.color = mix(f.color, self.col.bg, 0.25);
                    }
                    let text = if st.code {
                        f.font_id = style::mono((tx.size - 1.0).max(10.0));
                        f.background = self.col.inline_code_bg;
                        f.expand_bg = 1.0;
                        // The pill hugs the glyphs, not the whole line.
                        f.line_height = None;
                        if link.is_none() {
                            f.color = tx.color;
                        }
                        // Hair spaces give the code pill some air.
                        format!("\u{200A}{text}\u{200A}")
                    } else {
                        text.clone()
                    };
                    let n = text.chars().count();
                    if let Some(l) = link {
                        hits.push((chars..chars + n, Hit::Link(*l)));
                    }
                    chars += n;
                    job.append(&text, 0.0, f);
                }
                Inline::Break => {
                    job.append(
                        "\n",
                        0.0,
                        TextFormat {
                            font_id: self.font(ui, tx, false),
                            line_height: Some(tx.line),
                            ..Default::default()
                        },
                    );
                    chars += 1;
                }
                Inline::FootnoteRef { label, number } => {
                    let text = format!("\u{2009}{number}");
                    let n = text.chars().count();
                    hits.push((chars..chars + n, Hit::Footnote(label.clone())));
                    chars += n;
                    job.append(
                        &text,
                        0.0,
                        TextFormat {
                            font_id: style::medium(ui, (tx.size * 0.72).round()),
                            color: self.col.link,
                            line_height: Some(tx.line),
                            valign: Align::TOP,
                            ..Default::default()
                        },
                    );
                }
                Inline::Image { alt, .. } => {
                    // Only in table cells / headings: the alt text.
                    let n = alt.chars().count();
                    chars += n;
                    job.append(
                        alt,
                        0.0,
                        TextFormat {
                            font_id: self.font(ui, tx, false),
                            color: self.col.text2,
                            line_height: Some(tx.line),
                            ..Default::default()
                        },
                    );
                }
            }
        }
        (job, hits)
    }

    /// Text that wraps in `ui` (continuing a wrapped row if there is one).
    fn text_run(&mut self, ui: &mut Ui, inlines: &[Inline], tx: Tx) {
        let (mut job, hits) = self.job(ui, inlines, tx);
        if job.text.is_empty() {
            return;
        }
        job.wrap.max_width = ui.available_width();
        // Clicks follow links; text is no Tab stop. VoiceOver reads it as
        // static text.
        let (pos, galley, resp) = egui::Label::new(job)
            .sense(Sense::CLICK)
            .selectable(false)
            .layout_in_ui(ui);
        let text = galley.text().to_string();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        let hover = self.hit(&resp, pos, &galley, &hits);
        ui.painter().galley(pos, galley, tx.color);
        self.follow(ui, &resp, hover);
    }

    /// The link under the pointer, if any.
    fn hit<'h>(
        &self,
        resp: &egui::Response,
        pos: egui::Pos2,
        galley: &egui::Galley,
        hits: &'h [(Range<usize>, Hit)],
    ) -> Option<&'h Hit> {
        let p = resp.hover_pos()?;
        if hits.is_empty() {
            return None;
        }
        let local = p - pos;
        let c = galley.cursor_from_pos(local).index.0;
        let at = |i: usize| galley.pos_from_cursor(egui::text::CCursor::new(i));
        let under = [c.saturating_sub(1), c].into_iter().find(|&i| {
            let (a, b) = (at(i), at(i + 1));
            b.min.x > a.min.x
                && local.x >= a.min.x
                && local.x <= b.min.x
                && local.y >= a.min.y
                && local.y <= a.max.y
        })?;
        hits.iter()
            .find(|(r, _)| r.contains(&under))
            .map(|(_, h)| h)
    }

    fn follow(&mut self, ui: &Ui, resp: &egui::Response, hover: Option<&Hit>) {
        let Some(h) = hover else { return };
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let href = match h {
            Hit::Link(l) => self.doc.links.get(*l).cloned().unwrap_or_default(),
            Hit::Footnote(label) => format!("#fn-{label}"),
        };
        if matches!(h, Hit::Link(_)) && !href.starts_with('#') {
            resp.clone().on_hover_text_at_pointer(&href);
        }
        if resp.clicked() {
            self.open_href(&href);
        }
    }

    fn open_href(&mut self, href: &str) {
        let a = match links::classify(href, &self.dir) {
            Target::External(u) => Action::OpenUrl(u),
            Target::Anchor(a) => Action::Anchor(a),
            Target::Markdown { path, anchor } => Action::Markdown { path, anchor },
            Target::File(p) => Action::OpenFile(p),
        };
        self.actions.push(a);
    }

    // -------------------------------------------------------------- images

    fn image(&mut self, ui: &mut Ui, src: &str, alt: &str, title: &str, link: Option<usize>) {
        let source = links::image_source(src, &self.dir);
        let name = || {
            if alt.is_empty() {
                src.rsplit('/').next().unwrap_or(src).to_string()
            } else {
                alt.to_string()
            }
        };
        let is_svg = src
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".svg");
        let resp = match (&source, self.images.get(&source)) {
            (ImageSource::Unsupported(_), _) => {
                self.placeholder(ui, icon::FILE_IMAGE, &name(), tr!("image.unsupported"))
            }
            (_, _) if is_svg => self.placeholder(ui, icon::FILE_IMAGE, &name(), tr!("image.svg")),
            (ImageSource::Web(_), None) if !self.web_images => {
                self.placeholder(ui, icon::GLOBE, &name(), tr!("image.web_off"))
            }
            (_, None) => {
                self.actions.push(Action::LoadImage(source.clone()));
                self.placeholder(ui, icon::FILE_IMAGE, &name(), tr!("loading"))
            }
            (_, Some(ImageState::Loading)) => {
                self.placeholder(ui, icon::FILE_IMAGE, &name(), tr!("loading"))
            }
            (_, Some(ImageState::Missing)) => self.missing_box(ui, &name()),
            (_, Some(ImageState::Failed(e))) => self.placeholder(
                ui,
                icon::WARNING,
                &name(),
                &tr!("image.failed", error = crate::i18n::show_msg(e)),
            ),
            (_, Some(ImageState::Ready { uri, bytes })) => {
                let max_w = ui.available_width().max(16.0);
                let img =
                    egui::Image::from_bytes(uri.clone(), egui::load::Bytes::Shared(bytes.clone()))
                        .fit_to_original_size(1.0)
                        .max_width(max_w)
                        .corner_radius(CornerRadius::same(4))
                        .show_loading_spinner(false)
                        .sense(Sense::click());
                let r = ui.add(img);
                let tip = if !title.is_empty() { title } else { alt };
                if tip.is_empty() {
                    r
                } else {
                    r.on_hover_text(tip)
                }
            }
        };
        if let Some(l) = link {
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                let href = self.doc.links.get(l).cloned().unwrap_or_default();
                self.open_href(&href);
            }
        }
    }

    /// A calm, full-width box with the alt text where an image will go.
    fn missing_box(&mut self, ui: &mut Ui, alt: &str) -> egui::Response {
        let w = ui.available_width().clamp(40.0, size::MEASURE);
        let font = style::regular(size::SMALL);
        let g = ui.painter().layout_job({
            let mut j = LayoutJob::simple(
                alt.to_string(),
                font,
                self.col.text3,
                (w - 2.0 * space::LG).max(20.0),
            );
            j.halign = Align::Center;
            j
        });
        let h = (g.size().y + 44.0).max(72.0);
        let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let p = ui.painter();
        p.rect(
            r,
            CornerRadius::same(6),
            mix(self.col.neutral_soft, self.col.bg, 0.55),
            Stroke::new(1.0, self.col.separator),
            StrokeKind::Inside,
        );
        let icon_r = Rect::from_center_size(pos2(r.center().x, r.top() + 20.0), vec2(18.0, 18.0));
        style::paint_icon(ui, icon_r, icon::FILE_IMAGE, 16.0, self.col.text3);
        p.galley(pos2(r.center().x, r.top() + 34.0), g, self.col.text3);
        resp
    }

    /// A small chip standing in for an image.
    fn placeholder(&mut self, ui: &mut Ui, glyph: &str, label: &str, tip: &str) -> egui::Response {
        let font = style::regular(size::SMALL);
        let g = ui.painter().layout_job({
            let mut j = LayoutJob::simple_singleline(label.to_string(), font, self.col.text2);
            j.wrap.max_width = (ui.available_width() - 30.0).clamp(30.0, 260.0);
            j.wrap.max_rows = 1;
            j
        });
        let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 22.0), Sense::click());
        ui.painter()
            .rect_filled(r, CornerRadius::same(5), self.col.neutral_soft);
        style::paint_icon(
            ui,
            Rect::from_min_size(r.min + vec2(4.0, 0.0), vec2(16.0, r.height())),
            glyph,
            12.0,
            self.col.text3,
        );
        ui.painter().galley(
            pos2(r.left() + 22.0, r.center().y - g.size().y / 2.0),
            g,
            self.col.text2,
        );
        resp.on_hover_text(tip)
    }

    // -------------------------------------------------------------- code

    fn code(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        text: &str,
        hl: Option<&crate::highlight::Highlight>,
    ) {
        let id = self.next_id(ui);
        let col = self.col;
        let font = style::mono(size::CODE);
        let mut job = LayoutJob::default();
        let fmt = |color: Color32, italic: bool| TextFormat {
            font_id: font.clone(),
            color,
            italics: italic,
            line_height: Some(size::CODE_LINE),
            ..Default::default()
        };
        match hl.map(|h| if col.dark { &h.dark } else { &h.light }) {
            Some(spans) if !spans.is_empty() => {
                for s in spans {
                    if let Some(t) = text.get(s.start..s.end) {
                        let mut c = s.color;
                        // The themes' plain text colour → ours.
                        if !col.dark && c == Color32::from_rgb(50, 50, 50)
                            || col.dark && c == Color32::from_rgb(192, 197, 206)
                        {
                            c = col.text;
                        }
                        job.append(t, 0.0, fmt(c, s.italic));
                    }
                }
            }
            _ => job.append(text, 0.0, fmt(col.text, false)),
        }
        job.wrap.max_width = f32::INFINITY;
        let galley = ui.painter().layout_job(job);
        let frame = egui::Frame::new()
            .fill(col.code_bg)
            .stroke(Stroke::new(1.0, col.code_border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 12,
                bottom: 12,
            });
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::horizontal()
                    .id_salt(id)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let (r, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                        ui.painter().galley(r.min, galley.clone(), col.text);
                    });
            })
            .response;
        // Language and a copy button in the top right corner.
        let hovered = ui.rect_contains_pointer(resp.rect);
        let corner = resp.rect.right_top() + vec2(-6.0, 6.0);
        let mut right = corner.x;
        if hovered {
            let b = Rect::from_min_size(pos2(right - 24.0, corner.y), vec2(24.0, 22.0));
            let r = ui.interact(b, id.with("copy"), Sense::click());
            let copied_id = id.with("copied");
            let copied = ui
                .ctx()
                .data(|d| d.get_temp::<f64>(copied_id))
                .is_some_and(|t| ui.input(|i| i.time) - t < 1.5);
            ui.painter().rect_filled(
                b,
                CornerRadius::same(5),
                if r.hovered() { col.hover } else { col.code_bg },
            );
            style::paint_icon(
                ui,
                b,
                if copied { icon::CHECK } else { icon::COPY },
                13.0,
                if copied { col.success } else { col.text2 },
            );
            if r.clicked() {
                ui.ctx().copy_text(text.to_string());
                let now = ui.input(|i| i.time);
                ui.ctx().data_mut(|d| d.insert_temp(copied_id, now));
            }
            r.on_hover_text(if copied { tr!("copied") } else { tr!("copy") });
            right = b.left() - space::XS;
        }
        if !lang.is_empty() && !hovered {
            let g = ui.painter().layout_no_wrap(
                lang.to_string(),
                style::regular(size::CAPTION),
                col.text3,
            );
            ui.painter()
                .galley(pos2(right - g.size().x - 2.0, corner.y + 4.0), g, col.text3);
        }
    }

    // -------------------------------------------------------------- quote

    fn quote(&mut self, ui: &mut Ui, alert: Option<Alert>, blocks: &[Block], tx: Tx) {
        let col = self.col;
        let (bar, title) = match alert {
            None => (col.quote_bar, None),
            Some(Alert::Note) => (col.link, Some((icon::INFO, tr!("alert.note")))),
            Some(Alert::Tip) => (col.success, Some((icon::LIGHTBULB, tr!("alert.tip")))),
            Some(Alert::Important) => (
                col.purple,
                Some((icon::CHAT_CIRCLE, tr!("alert.important"))),
            ),
            Some(Alert::Warning) => (col.warning, Some((icon::WARNING, tr!("alert.warning")))),
            Some(Alert::Caution) => (
                col.danger,
                Some((icon::WARNING_CIRCLE, tr!("alert.caution"))),
            ),
        };
        let inner = Tx {
            color: if alert.is_some() { col.text } else { col.text2 },
            ..tx
        };
        let resp = egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 16,
                right: 0,
                top: 2,
                bottom: 2,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some((glyph, label)) = title {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
                        style::paint_icon(ui, r, glyph, 14.0, bar);
                        ui.add_space(space::XS);
                        ui.label(
                            egui::RichText::new(label)
                                .font(style::semibold(ui, size::BODY))
                                .color(bar),
                        );
                    });
                    ui.add_space(space::XS);
                }
                self.blocks(ui, blocks, inner, PARA_GAP * 0.75);
            })
            .response;
        let r = Rect::from_min_size(resp.rect.min, vec2(3.0, resp.rect.height()));
        ui.painter().rect_filled(r, CornerRadius::same(2), bar);
    }

    // -------------------------------------------------------------- lists

    fn list(&mut self, ui: &mut Ui, start: Option<u64>, items: &[Item], tx: Tx) {
        let col = self.col;
        let marker_w = match start {
            Some(s) => {
                let last = s + items.len().saturating_sub(1) as u64;
                ui.painter()
                    .layout_no_wrap(format!("{last}."), style::regular(tx.size), col.text)
                    .size()
                    .x
                    + 8.0
            }
            None => 18.0,
        }
        .max(18.0);
        let loose = items.iter().any(|i| i.blocks.len() > 1);
        for (n, item) in items.iter().enumerate() {
            if n > 0 {
                ui.add_space(if loose { space::MD } else { space::XS });
            }
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let w = if item.task.is_some() { 24.0 } else { marker_w };
                let (r, _) = ui.allocate_exact_size(vec2(w, tx.line), Sense::hover());
                let cy = r.center().y;
                match (item.task, start) {
                    (Some(done), _) => {
                        let b = Rect::from_center_size(pos2(r.left() + 7.5, cy), vec2(14.0, 14.0));
                        if done {
                            ui.painter()
                                .rect_filled(b, CornerRadius::same(4), col.accent);
                            style::paint_icon(ui, b, icon::CHECK, 10.0, Color32::WHITE);
                            if style::icon_font(ui, 10.0).is_none() {
                                let p = ui.painter();
                                let s = Stroke::new(1.6, Color32::WHITE);
                                p.line_segment(
                                    [
                                        b.lerp_inside(vec2(0.25, 0.52)),
                                        b.lerp_inside(vec2(0.43, 0.7)),
                                    ],
                                    s,
                                );
                                p.line_segment(
                                    [
                                        b.lerp_inside(vec2(0.43, 0.7)),
                                        b.lerp_inside(vec2(0.76, 0.32)),
                                    ],
                                    s,
                                );
                            }
                        } else {
                            ui.painter().rect_stroke(
                                b,
                                CornerRadius::same(4),
                                Stroke::new(1.2, mix(col.text3, col.text2, 0.3)),
                                StrokeKind::Inside,
                            );
                        }
                    }
                    (None, Some(s)) => {
                        let g = ui.painter().layout_no_wrap(
                            format!("{}.", s + n as u64),
                            style::regular(tx.size),
                            col.text2,
                        );
                        ui.painter().galley(
                            pos2(r.right() - 6.0 - g.size().x, cy - g.size().y / 2.0),
                            g,
                            col.text2,
                        );
                    }
                    (None, None) => {
                        let depth = self.depth;
                        let c = pos2(r.left() + 6.0, cy);
                        match depth % 3 {
                            0 => ui.painter().circle_filled(c, 2.6, col.text2),
                            1 => ui
                                .painter()
                                .circle_stroke(c, 2.4, Stroke::new(1.1, col.text2)),
                            _ => ui.painter().rect_filled(
                                Rect::from_center_size(c, vec2(4.4, 4.4)),
                                CornerRadius::ZERO,
                                col.text2,
                            ),
                        };
                    }
                }
                ui.vertical(|ui| {
                    self.depth += 1;
                    let itx = if item.task == Some(true) {
                        Tx {
                            color: col.text2,
                            ..tx
                        }
                    } else {
                        tx
                    };
                    self.blocks(
                        ui,
                        &item.blocks,
                        itx,
                        if loose { PARA_GAP * 0.75 } else { space::XS },
                    );
                    self.depth -= 1;
                });
            });
        }
    }

    // -------------------------------------------------------------- tables

    fn table(
        &mut self,
        ui: &mut Ui,
        aligns: &[ColAlign],
        head: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        tx: Tx,
    ) {
        let col = self.col;
        let ncols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
        if ncols == 0 {
            return;
        }
        let pad = vec2(10.0, 6.0);
        let cell_tx = Tx {
            size: size::BODY,
            line: 19.0,
            ..tx
        };
        let head_tx = Tx {
            strong: true,
            ..cell_tx
        };
        let natural = |env: &Env, ui: &Ui, c: usize| -> f32 {
            let mut w: f32 = 0.0;
            for (k, row) in std::iter::once(head)
                .chain(rows.iter().map(Vec::as_slice))
                .enumerate()
            {
                if let Some(cell) = row.get(c) {
                    let (job, _) = env.job(ui, cell, if k == 0 { head_tx } else { cell_tx });
                    let g = ui.painter().layout_job(job);
                    w = w.max(g.size().x);
                }
            }
            (w + 2.0 * pad.x + 1.0).min(360.0)
        };
        let nat: Vec<f32> = (0..ncols).map(|c| natural(self, ui, c)).collect();
        let mins: Vec<f32> = nat.iter().map(|w| w.min(90.0)).collect();
        let avail = ui.available_width();
        let widths = fit_columns(&nat, &mins, avail);
        let total: f32 = widths.iter().sum();
        let id = self.next_id(ui);
        egui::ScrollArea::horizontal()
            .id_salt(id)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let top = ui.cursor().top();
                let left = ui.cursor().left();
                let mut y = top;
                let mut row_rects = Vec::new();
                for (k, row) in std::iter::once(head)
                    .chain(rows.iter().map(Vec::as_slice))
                    .enumerate()
                {
                    let rtx = if k == 0 { head_tx } else { cell_tx };
                    let mut cells = Vec::new();
                    let mut h: f32 = rtx.line;
                    for (c, w) in widths.iter().enumerate() {
                        let empty = Vec::new();
                        let cell = row.get(c).unwrap_or(&empty);
                        let (mut job, hits) = self.job(ui, cell, rtx);
                        job.wrap.max_width = w - 2.0 * pad.x;
                        job.halign = match aligns.get(c) {
                            Some(ColAlign::Center) => Align::Center,
                            Some(ColAlign::Right) => Align::RIGHT,
                            _ => Align::LEFT,
                        };
                        let g = ui.painter().layout_job(job);
                        h = h.max(g.size().y);
                        cells.push((g, hits));
                    }
                    let row_h = h + 2.0 * pad.y;
                    let row_rect = Rect::from_min_size(pos2(left, y), vec2(total, row_h));
                    if k == 0 {
                        ui.painter().rect_filled(
                            row_rect,
                            CornerRadius {
                                nw: 6,
                                ne: 6,
                                sw: 0,
                                se: 0,
                            },
                            col.table_head,
                        );
                    } else if k % 2 == 0 {
                        ui.painter()
                            .rect_filled(row_rect, CornerRadius::ZERO, col.table_stripe);
                    }
                    let mut x = left;
                    for (c, (g, hits)) in cells.into_iter().enumerate() {
                        let w = widths[c];
                        let anchor_x = match aligns.get(c) {
                            Some(ColAlign::Center) => x + w / 2.0,
                            Some(ColAlign::Right) => x + w - pad.x,
                            _ => x + pad.x,
                        };
                        // Cells are centred vertically (a lone `code` cell is
                        // shorter than a text line).
                        let pos = pos2(anchor_x, y + (row_h - g.size().y) / 2.0);
                        let cell_rect = Rect::from_min_size(pos2(x, y), vec2(w, row_h));
                        if !hits.is_empty() {
                            let resp = ui.interact(cell_rect, id.with((k, c)), Sense::click());
                            let gpos = pos2(pos.x + g.rect.min.x, pos.y);
                            let hover = self.hit(&resp, gpos, &g, &hits);
                            let hover = hover.map(|h| match h {
                                Hit::Link(l) => Hit::Link(*l),
                                Hit::Footnote(s) => Hit::Footnote(s.clone()),
                            });
                            self.follow(ui, &resp, hover.as_ref());
                        }
                        ui.painter().galley(pos, g, rtx.color);
                        x += w;
                    }
                    row_rects.push(row_rect);
                    y += row_h;
                }
                let outer = Rect::from_min_max(pos2(left, top), pos2(left + total, y));
                let stroke = Stroke::new(1.0, col.code_border);
                for r in row_rects.iter().skip(1) {
                    ui.painter().hline(outer.x_range(), r.top(), stroke);
                }
                let mut x = left;
                for w in &widths[..widths.len() - 1] {
                    x += w;
                    ui.painter().vline(x, outer.y_range(), stroke);
                }
                ui.painter()
                    .rect_stroke(outer, CornerRadius::same(6), stroke, StrokeKind::Inside);
                ui.allocate_rect(outer, Sense::hover());
            });
    }
}

/// Column widths: natural widths if they fit, else the widest columns
/// give way first (never below their minimum; the table then scrolls).
pub fn fit_columns(natural: &[f32], mins: &[f32], avail: f32) -> Vec<f32> {
    if natural.iter().sum::<f32>() <= avail {
        return natural.to_vec();
    }
    if mins.iter().sum::<f32>() >= avail {
        return mins.to_vec();
    }
    let at = |cap: f32| -> f32 {
        natural
            .iter()
            .zip(mins)
            .map(|(n, m)| n.min(cap.max(*m)))
            .sum()
    };
    let (mut lo, mut hi) = (0.0_f32, natural.iter().cloned().fold(0.0, f32::max));
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if at(mid) <= avail {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    natural
        .iter()
        .zip(mins)
        .map(|(n, m)| n.min(lo.max(*m)).floor())
        .collect()
}

/// Space above a top-level block.
fn gap_before(b: &Block) -> f32 {
    match b {
        Block::Heading { level: 1, .. } => 28.0,
        Block::Heading { level: 2, .. } => 26.0,
        Block::Heading { .. } => 20.0,
        _ => PARA_GAP,
    }
}

/// Plain text of a heading (for tests).
pub fn heading_text(b: &Block) -> Option<String> {
    match b {
        Block::Heading { inlines, .. } => Some(plain_text(inlines)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::fit_columns;

    #[test]
    fn columns_fit() {
        assert_eq!(
            fit_columns(&[100.0, 200.0], &[80.0, 90.0], 400.0),
            vec![100.0, 200.0]
        );
        let w = fit_columns(&[100.0, 300.0], &[80.0, 90.0], 300.0);
        assert_eq!(w[0], 100.0);
        assert!((w[1] - 200.0).abs() <= 1.0, "{w:?}");
        assert_eq!(
            fit_columns(&[300.0, 300.0], &[90.0, 90.0], 100.0),
            vec![90.0, 90.0]
        );
    }
}
