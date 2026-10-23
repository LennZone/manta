//! The parsed document: GitHub-flavoured Markdown (pulldown-cmark) turned
//! into a small block/inline tree that the renderer draws, plus the
//! heading list for the table of contents with GitHub-compatible anchors.

use std::collections::HashSet;

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::highlight::{self, Highlight};

/// Inline text style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub strong: bool,
    pub emph: bool,
    pub strike: bool,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// A run of text. `link` indexes [`Doc::links`].
    Text {
        text: String,
        style: Style,
        link: Option<usize>,
    },
    /// A hard line break.
    Break,
    Image {
        src: String,
        alt: String,
        title: String,
        link: Option<usize>,
    },
    /// `[^label]`: shown as a small number that jumps to the footnote.
    FootnoteRef { label: String, number: usize },
}

/// GitHub alert kinds (`> [!NOTE]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// `Some(checked)` for a task list item.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        slug: String,
    },
    Paragraph(Vec<Inline>),
    Code {
        lang: String,
        text: String,
        highlight: Option<Box<Highlight>>,
    },
    Quote {
        alert: Option<Alert>,
        blocks: Vec<Block>,
    },
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table {
        aligns: Vec<Align>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    /// Raw HTML: the `<img>`s in it are shown, the rest as plain text.
    Html {
        images: Vec<(String, String)>,
        text: String,
    },
    Footnote {
        label: String,
        number: usize,
        blocks: Vec<Block>,
    },
}

/// One entry of the table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub slug: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
    pub headings: Vec<Heading>,
    /// Link targets, as written (`Inline::Text::link` indexes this).
    pub links: Vec<String>,
}

impl Doc {
    /// Parse GitHub-flavoured Markdown. YAML front matter is skipped.
    /// Fenced code is syntax-highlighted when `highlight` is set.
    pub fn parse(src: &str, with_highlight: bool) -> Doc {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        let opts = Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_GFM;
        let mut b = Builder {
            events: Parser::new_ext(src, opts).peekable(),
            doc: Doc::default(),
            slugs: Slugger::default(),
            task: None,
            footnotes: Vec::new(),
            highlight: with_highlight,
        };
        let blocks = b.blocks(None);
        b.doc.blocks = blocks;
        b.doc
    }

    /// Number of blocks, including nested ones (for tests).
    pub fn count(&self, f: &dyn Fn(&Block) -> bool) -> usize {
        fn walk(v: &[Block], f: &dyn Fn(&Block) -> bool) -> usize {
            v.iter()
                .map(|b| {
                    let inner = match b {
                        Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
                            walk(blocks, f)
                        }
                        Block::List { items, .. } => items.iter().map(|i| walk(&i.blocks, f)).sum(),
                        _ => 0,
                    };
                    inner + usize::from(f(b))
                })
                .sum()
        }
        walk(&self.blocks, f)
    }
}

/// The text of inlines without formatting (headings → TOC, slugs).
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text { text, .. } => s.push_str(text),
            Inline::Break => s.push(' '),
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::FootnoteRef { .. } => {}
        }
    }
    s
}

// ------------------------------------------------------------------ slugs

/// GitHub's heading anchors (github-slugger): lower-case, keep letters,
/// digits, `-` and `_`, spaces become `-`, everything else is dropped.
/// Repeats get `-1`, `-2`, … appended.
#[derive(Default)]
pub struct Slugger {
    seen: HashSet<String>,
}

pub fn slug_base(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            s.push(c);
        } else if c == ' ' {
            s.push('-');
        }
    }
    s
}

impl Slugger {
    pub fn slug(&mut self, text: &str) -> String {
        let base = slug_base(text);
        let mut candidate = base.clone();
        let mut n = 0;
        while self.seen.contains(&candidate) {
            n += 1;
            candidate = format!("{base}-{n}");
        }
        self.seen.insert(candidate.clone());
        candidate
    }

    /// Reserve an explicit id (`# Title {#id}`).
    pub fn reserve(&mut self, id: &str) {
        self.seen.insert(id.to_string());
    }
}

// ------------------------------------------------------------------ builder

struct Builder<'a> {
    events: std::iter::Peekable<Parser<'a>>,
    doc: Doc,
    slugs: Slugger,
    /// Task marker seen in the current list item.
    task: Option<bool>,
    footnotes: Vec<String>,
    highlight: bool,
}

fn is_inline_tag(t: &Tag) -> bool {
    matches!(
        t,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. }
            | Tag::Superscript
            | Tag::Subscript
    )
}

fn is_inline_end(t: &TagEnd) -> bool {
    matches!(
        t,
        TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image
            | TagEnd::Superscript
            | TagEnd::Subscript
    )
}

/// Events that belong into a run of inline content.
fn is_inline_event(e: &Event) -> bool {
    match e {
        Event::Start(t) => is_inline_tag(t),
        Event::End(t) => is_inline_end(t),
        Event::Text(_)
        | Event::Code(_)
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::InlineHtml(_)
        | Event::FootnoteReference(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::TaskListMarker(_) => true,
        Event::Html(_) | Event::Rule => false,
    }
}

impl Builder<'_> {
    fn footnote_number(&mut self, label: &str) -> usize {
        match self.footnotes.iter().position(|l| l == label) {
            Some(i) => i + 1,
            None => {
                self.footnotes.push(label.to_string());
                self.footnotes.len()
            }
        }
    }

    /// Blocks until `end` (consumed) or the end of input.
    fn blocks(&mut self, end: Option<TagEnd>) -> Vec<Block> {
        let mut out = Vec::new();
        while let Some(peek) = self.events.peek() {
            if is_inline_event(peek) {
                // Tight list items hold inline content without a paragraph.
                let inl = self.inlines(None);
                if !inl.is_empty() {
                    out.push(Block::Paragraph(inl));
                }
                continue;
            }
            let Some(ev) = self.events.next() else { break };
            match ev {
                Event::End(t) => {
                    if Some(t) == end {
                        break;
                    }
                }
                Event::Rule => out.push(Block::Rule),
                Event::Html(s) => {
                    let mut text = s.to_string();
                    while let Some(Event::Html(more)) = self.events.peek() {
                        text.push_str(more);
                        self.events.next();
                    }
                    out.push(html_block(&text));
                }
                Event::Start(tag) => self.block(tag, &mut out),
                _ => {}
            }
        }
        out
    }

    fn block(&mut self, tag: Tag<'_>, out: &mut Vec<Block>) {
        match tag {
            Tag::Paragraph => {
                let inl = self.inlines(Some(TagEnd::Paragraph));
                if !inl.is_empty() {
                    out.push(Block::Paragraph(inl));
                }
            }
            Tag::Heading { level, id, .. } => {
                let inlines = self.inlines(Some(TagEnd::Heading(level)));
                let level = heading_level(level);
                let text = plain_text(&inlines).trim().to_string();
                let slug = match id {
                    Some(id) => {
                        self.slugs.reserve(&id);
                        id.to_string()
                    }
                    None => self.slugs.slug(&text),
                };
                self.doc.headings.push(Heading {
                    level,
                    text,
                    slug: slug.clone(),
                });
                out.push(Block::Heading {
                    level,
                    inlines,
                    slug,
                });
            }
            Tag::BlockQuote(kind) => {
                let blocks = self.blocks(Some(TagEnd::BlockQuote(kind)));
                let alert = kind.map(|k| match k {
                    BlockQuoteKind::Note => Alert::Note,
                    BlockQuoteKind::Tip => Alert::Tip,
                    BlockQuoteKind::Important => Alert::Important,
                    BlockQuoteKind::Warning => Alert::Warning,
                    BlockQuoteKind::Caution => Alert::Caution,
                });
                out.push(Block::Quote { alert, blocks });
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
                        .next()
                        .unwrap_or("")
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let mut text = String::new();
                for ev in self.events.by_ref() {
                    match ev {
                        Event::Text(t) => text.push_str(&t),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                if text.ends_with('\n') {
                    text.pop();
                }
                let highlight = if self.highlight {
                    highlight::highlight(&lang, &text).map(Box::new)
                } else {
                    None
                };
                out.push(Block::Code {
                    lang,
                    text,
                    highlight,
                });
            }
            Tag::List(start) => {
                let mut items = Vec::new();
                loop {
                    match self.events.next() {
                        Some(Event::Start(Tag::Item)) => {
                            let saved = self.task.take();
                            let blocks = self.blocks(Some(TagEnd::Item));
                            let task = self.task.take();
                            self.task = saved;
                            items.push(Item { task, blocks });
                        }
                        Some(Event::End(TagEnd::List(_))) | None => break,
                        _ => {}
                    }
                }
                out.push(Block::List { start, items });
            }
            Tag::Table(aligns) => {
                let aligns = aligns
                    .iter()
                    .map(|a| match a {
                        Alignment::None => Align::None,
                        Alignment::Left => Align::Left,
                        Alignment::Center => Align::Center,
                        Alignment::Right => Align::Right,
                    })
                    .collect();
                let mut head = Vec::new();
                let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
                let mut in_head = false;
                loop {
                    match self.events.next() {
                        Some(Event::Start(Tag::TableHead)) => in_head = true,
                        Some(Event::End(TagEnd::TableHead)) => in_head = false,
                        Some(Event::Start(Tag::TableRow)) => rows.push(Vec::new()),
                        Some(Event::Start(Tag::TableCell)) => {
                            let cell = self.inlines(Some(TagEnd::TableCell));
                            if in_head {
                                head.push(cell);
                            } else if let Some(r) = rows.last_mut() {
                                r.push(cell);
                            }
                        }
                        Some(Event::End(TagEnd::Table)) | None => break,
                        _ => {}
                    }
                }
                out.push(Block::Table { aligns, head, rows });
            }
            Tag::FootnoteDefinition(label) => {
                let number = self.footnote_number(&label);
                let blocks = self.blocks(Some(TagEnd::FootnoteDefinition));
                out.push(Block::Footnote {
                    label: label.to_string(),
                    number,
                    blocks,
                });
            }
            Tag::HtmlBlock => {
                let mut text = String::new();
                for ev in self.events.by_ref() {
                    match ev {
                        Event::Html(s) | Event::Text(s) => text.push_str(&s),
                        Event::End(TagEnd::HtmlBlock) => break,
                        _ => {}
                    }
                }
                let b = html_block(&text);
                if !matches!(&b, Block::Html { images, text } if images.is_empty() && text.trim().is_empty())
                {
                    out.push(b);
                }
            }
            Tag::MetadataBlock(kind) => {
                for ev in self.events.by_ref() {
                    if ev == Event::End(TagEnd::MetadataBlock(kind)) {
                        break;
                    }
                }
            }
            // Definition lists and anything new: their content, unwrapped.
            other => {
                let end = other.to_end();
                let inner = self.blocks(Some(end));
                out.extend(inner);
            }
        }
    }

    /// Inline content until `end` (consumed), or, with `None`, until the
    /// next block-level event (not consumed).
    fn inlines(&mut self, end: Option<TagEnd>) -> Vec<Inline> {
        let mut out: Vec<Inline> = Vec::new();
        let mut style = Style::default();
        let (mut strong, mut emph, mut strike) = (0u32, 0u32, 0u32);
        let mut links: Vec<usize> = Vec::new();
        loop {
            if end.is_none() {
                match self.events.peek() {
                    Some(e) if is_inline_event(e) => {}
                    _ => break,
                }
            }
            let Some(ev) = self.events.next() else { break };
            let link = links.last().copied();
            match ev {
                Event::End(t) if Some(t) == end => break,
                Event::Text(t) => push_text(&mut out, &t, style, link),
                Event::Code(t) => push_text(
                    &mut out,
                    &t,
                    Style {
                        code: true,
                        ..style
                    },
                    link,
                ),
                Event::InlineMath(t) | Event::DisplayMath(t) => push_text(
                    &mut out,
                    &t,
                    Style {
                        code: true,
                        ..style
                    },
                    link,
                ),
                Event::SoftBreak => push_text(&mut out, " ", style, link),
                Event::HardBreak => out.push(Inline::Break),
                Event::Html(h) | Event::InlineHtml(h) => {
                    let lower = h.to_ascii_lowercase();
                    if lower.starts_with("<br") {
                        out.push(Inline::Break);
                    } else if lower.starts_with("<img") {
                        let (src, alt) = img_attrs(&h);
                        if let Some(src) = src {
                            out.push(Inline::Image {
                                src,
                                alt,
                                title: String::new(),
                                link,
                            });
                        }
                    }
                }
                Event::FootnoteReference(label) => {
                    let number = self.footnote_number(&label);
                    out.push(Inline::FootnoteRef {
                        label: label.to_string(),
                        number,
                    });
                }
                Event::TaskListMarker(done) => self.task = Some(done),
                Event::Start(Tag::Strong) => strong += 1,
                Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
                Event::Start(Tag::Emphasis) => emph += 1,
                Event::End(TagEnd::Emphasis) => emph = emph.saturating_sub(1),
                Event::Start(Tag::Strikethrough) => strike += 1,
                Event::End(TagEnd::Strikethrough) => strike = strike.saturating_sub(1),
                Event::Start(Tag::Link { dest_url, .. }) => {
                    self.doc.links.push(dest_url.to_string());
                    links.push(self.doc.links.len() - 1);
                }
                Event::End(TagEnd::Link) => {
                    links.pop();
                }
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) => {
                    let mut alt = String::new();
                    let mut depth = 1;
                    for ev in self.events.by_ref() {
                        match ev {
                            Event::Start(Tag::Image { .. }) => depth += 1,
                            Event::End(TagEnd::Image) => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                            _ => {}
                        }
                    }
                    out.push(Inline::Image {
                        src: dest_url.to_string(),
                        alt,
                        title: title.to_string(),
                        link,
                    });
                }
                _ => {}
            }
            style = Style {
                strong: strong > 0,
                emph: emph > 0,
                strike: strike > 0,
                code: false,
            };
        }
        // Trim a trailing soft break / spaces.
        if let Some(Inline::Text { text, .. }) = out.last_mut() {
            let t = text.trim_end().len();
            text.truncate(t);
        }
        if matches!(out.last(), Some(Inline::Text { text, .. }) if text.is_empty()) {
            out.pop();
        }
        out
    }
}

fn push_text(out: &mut Vec<Inline>, t: &str, style: Style, link: Option<usize>) {
    if let Some(Inline::Text {
        text,
        style: s,
        link: l,
    }) = out.last_mut()
    {
        if *s == style && *l == link {
            text.push_str(t);
            return;
        }
    }
    out.push(Inline::Text {
        text: t.to_string(),
        style,
        link,
    });
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ------------------------------------------------------------------ HTML

/// The value of attribute `name` in one HTML tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = lower[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let off = tag.len() - rest.len() + 1;
        let v = tag[off..].trim_start();
        let (q, body) = match v.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &v[1..]),
            _ => (None, v),
        };
        let end = match q {
            Some(q) => body.find(q).unwrap_or(body.len()),
            None => body
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(body.len()),
        };
        return Some(body[..end].to_string());
    }
    None
}

fn img_attrs(tag: &str) -> (Option<String>, String) {
    (attr(tag, "src"), attr(tag, "alt").unwrap_or_default())
}

/// `<img>` tags of an HTML block, and its text with tags removed.
fn html_block(html: &str) -> Block {
    let mut images = Vec::new();
    let mut text = String::new();
    let mut rest = html;
    let mut in_comment = false;
    while !rest.is_empty() {
        if in_comment {
            match rest.find("-->") {
                Some(i) => {
                    rest = &rest[i + 3..];
                    in_comment = false;
                }
                None => break,
            }
            continue;
        }
        if rest.starts_with("<!--") {
            in_comment = true;
            rest = &rest[4..];
            continue;
        }
        if rest.starts_with('<') {
            let end = rest.find('>').map_or(rest.len(), |i| i + 1);
            let tag = &rest[..end];
            let lower = tag.to_ascii_lowercase();
            if lower.starts_with("<img") {
                let (src, alt) = img_attrs(tag);
                if let Some(src) = src {
                    images.push((src, alt));
                }
            } else if lower.starts_with("<br")
                || lower.starts_with("</p")
                || lower.starts_with("</div")
            {
                text.push('\n');
            }
            rest = &rest[end..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            text.push_str(&decode_entities(&rest[..end]));
            rest = &rest[end..];
        }
    }
    let text = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Block::Html { images, text }
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_slugs() {
        let mut s = Slugger::default();
        assert_eq!(s.slug("Hello World"), "hello-world");
        assert_eq!(s.slug("Hello World"), "hello-world-1");
        assert_eq!(s.slug("Hello World"), "hello-world-2");
        assert_eq!(s.slug("What's new in v2.0?"), "whats-new-in-v20");
        assert_eq!(s.slug("C++ & Rust: a love story"), "c--rust-a-love-story");
        assert_eq!(
            s.slug("snake_case and kebab-case"),
            "snake_case-and-kebab-case"
        );
        assert_eq!(s.slug("Über Größe"), "über-größe");
        assert_eq!(s.slug("  Trim me  "), "trim-me");
        assert_eq!(s.slug("🚀 Launch"), "-launch");
        assert_eq!(s.slug("`code` heading"), "code-heading");
        // A heading that collides with a generated suffix.
        let mut s = Slugger::default();
        assert_eq!(s.slug("a"), "a");
        assert_eq!(s.slug("a-1"), "a-1");
        assert_eq!(s.slug("a"), "a-2");
    }

    #[test]
    fn headings_and_explicit_ids() {
        let d = Doc::parse(
            "# Intro\n\n## Set *up*\n\n## Intro\n\n### Custom {#my-id}\n",
            false,
        );
        let slugs: Vec<_> = d
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.slug.as_str()))
            .collect();
        assert_eq!(
            slugs,
            vec![
                (1, "Intro", "intro"),
                (2, "Set up", "set-up"),
                (2, "Intro", "intro-1"),
                (3, "Custom", "my-id")
            ]
        );
    }

    #[test]
    fn task_lists_and_tight_items() {
        let d = Doc::parse("- [x] done\n- [ ] open\n- plain\n", false);
        let Block::List { items, .. } = &d.blocks[0] else {
            panic!("{:?}", d.blocks)
        };
        assert_eq!(
            items.iter().map(|i| i.task).collect::<Vec<_>>(),
            vec![Some(true), Some(false), None]
        );
        assert!(matches!(&items[0].blocks[0], Block::Paragraph(p) if plain_text(p) == "done"));
    }

    #[test]
    fn html_images_are_kept() {
        let d = Doc::parse(
            "<p align=\"center\">\n  <img src=\"logo.png\" alt='Logo' width=120>\n</p>\n\nHi\n",
            false,
        );
        assert_eq!(
            d.blocks[0],
            Block::Html {
                images: vec![("logo.png".into(), "Logo".into())],
                text: String::new()
            }
        );
    }

    #[test]
    fn front_matter_is_skipped() {
        let d = Doc::parse("---\ntitle: x\n---\n\n# Body\n", false);
        assert_eq!(d.headings.len(), 1);
        assert_eq!(d.blocks.len(), 1);
    }
}
