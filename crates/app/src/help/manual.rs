//! The user manual embedded in the binary (`docs/manual`, see `build.rs`):
//! chapters per language, the table of contents from `index.json`, full-text
//! search and the mapping of links and images to embedded files.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use manta_core::VfsPath;
use manta_plugin_markdown::doc::{plain_text, Block, Doc, Inline};
use serde::Deserialize;

mod files {
    include!(concat!(env!("OUT_DIR"), "/manual_files.rs"));
}

/// Where the chapters of `lang` live for the renderer (links and images
/// resolve against it, so `../../plugins.md` lands in [`DEV_DIR`]).
pub fn chapter_dir(lang: &str) -> VfsPath {
    VfsPath::new(&format!("/docs/manual/{lang}"))
}

/// Folder of the developer docs the chapters link to.
pub const DEV_DIR: &str = "/docs";

#[derive(Deserialize)]
struct IndexJson {
    #[serde(default)]
    languages: Vec<String>,
    fallback_language: String,
    chapters: Vec<ChapterMeta>,
}

#[derive(Deserialize, Clone)]
struct ChapterMeta {
    // Checked against the chapter heading by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    id: String,
    number: u32,
    file: String,
    title: HashMap<String, String>,
    #[serde(default)]
    keywords: HashMap<String, Vec<String>>,
}

fn index() -> &'static IndexJson {
    static INDEX: OnceLock<IndexJson> = OnceLock::new();
    INDEX.get_or_init(|| {
        serde_json::from_str(files::INDEX_JSON).expect("docs/manual/index.json is valid")
    })
}

/// The manual's languages (`index.json`).
#[cfg(test)]
pub fn languages() -> Vec<String> {
    index().languages.clone()
}

fn source(lang: &str, file: &str) -> Option<&'static str> {
    files::CHAPTER_FILES
        .iter()
        .find(|(l, f, _)| *l == lang && *f == file)
        .map(|e| e.2)
}

/// The manual language for UI language `ui` ("de", "zh-Hans"): a listed
/// language with every chapter whose code is `ui` or matches its language
/// part ("zh" for "zh-Hans", "zh-Hans" for "zh"), else the fallback
/// language (English).
pub fn language_for(ui: &str) -> &'static str {
    let idx = index();
    let complete = |l: &str| idx.chapters.iter().all(|c| source(l, &c.file).is_some());
    let primary = |code: &str| {
        code.split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let exact = idx
        .languages
        .iter()
        .find(|l| l.eq_ignore_ascii_case(ui) && complete(l));
    let same_language = || {
        idx.languages
            .iter()
            .find(|l| primary(l) == primary(ui) && complete(l))
    };
    exact
        .or_else(same_language)
        .map(String::as_str)
        .unwrap_or(&idx.fallback_language)
}

/// A heading of a chapter with the text under it (up to the next heading).
#[derive(Debug, Clone)]
pub struct Section {
    pub level: u8,
    pub title: String,
    /// `None` for the chapter's introduction (above the first `##`).
    pub anchor: Option<String>,
    /// Plain text of the section (Markdown and comments removed).
    pub text: String,
}

pub struct Chapter {
    pub number: u32,
    pub file: String,
    pub title: String,
    pub keywords: Vec<String>,
    pub doc: Arc<Doc>,
    pub sections: Vec<Section>,
}

pub struct Manual {
    /// The language actually shown (after fallback).
    pub lang: &'static str,
    pub chapters: Vec<Chapter>,
}

/// The manual for UI language `ui` (parsed once per language).
pub fn get(ui: &str) -> Arc<Manual> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Manual>>>> = OnceLock::new();
    let lang = language_for(ui);
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache
        .entry(lang)
        .or_insert_with(|| Arc::new(Manual::load(lang)))
        .clone()
}

impl Manual {
    fn load(lang: &'static str) -> Manual {
        let idx = index();
        let fb = idx.fallback_language.as_str();
        let chapters = idx
            .chapters
            .iter()
            .map(|c| {
                let src = source(lang, &c.file)
                    .or_else(|| source(fb, &c.file))
                    .unwrap_or("");
                let doc = Doc::parse(src, true);
                let sections = sections(&doc);
                let pick = |m: &HashMap<String, String>| {
                    m.get(lang)
                        .or_else(|| m.get(fb))
                        .cloned()
                        .unwrap_or_default()
                };
                Chapter {
                    number: c.number,
                    file: c.file.clone(),
                    title: pick(&c.title),
                    keywords: c
                        .keywords
                        .get(lang)
                        .or_else(|| c.keywords.get(fb))
                        .cloned()
                        .unwrap_or_default(),
                    doc: Arc::new(doc),
                    sections,
                }
            })
            .collect();
        Manual { lang, chapters }
    }

    pub fn chapter(&self, file: &str) -> Option<&Chapter> {
        self.chapters.iter().find(|c| c.file == file)
    }
}

/// An embedded developer doc (`plugins.md`), parsed once.
pub fn dev_doc(name: &str) -> Option<Arc<Doc>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Doc>>>> = OnceLock::new();
    let src = files::DEV_DOCS.iter().find(|(n, _)| *n == name)?.1;
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    Some(
        cache
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Doc::parse(src, true)))
            .clone(),
    )
}

/// A developer doc's title: its first heading, else the file name.
pub fn dev_title(name: &str) -> String {
    dev_doc(name)
        .and_then(|d| {
            d.blocks.iter().find_map(|b| match b {
                Block::Heading {
                    level: 1, inlines, ..
                } => Some(plain_text(inlines)),
                _ => None,
            })
        })
        .unwrap_or_else(|| name.to_string())
}

// ------------------------------------------------------------------ text

fn sections(doc: &Doc) -> Vec<Section> {
    let mut out = vec![Section {
        level: 1,
        title: String::new(),
        anchor: None,
        text: String::new(),
    }];
    for b in &doc.blocks {
        match b {
            Block::Heading {
                level: 1, inlines, ..
            } if out.len() == 1 && out[0].title.is_empty() => {
                out[0].title = plain_text(inlines);
            }
            Block::Heading {
                level,
                inlines,
                slug,
            } => out.push(Section {
                level: *level,
                title: plain_text(inlines),
                anchor: Some(slug.clone()),
                text: String::new(),
            }),
            other => {
                let s = out.last_mut().expect("never empty");
                block_text(other, &mut s.text);
            }
        }
    }
    out
}

fn push(out: &mut String, s: &str) {
    let s = s.trim();
    if s.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(s);
}

fn inline_text(inl: &[Inline]) -> String {
    plain_text(inl)
}

fn block_text(b: &Block, out: &mut String) {
    match b {
        Block::Heading { inlines, .. } | Block::Paragraph(inlines) => {
            push(out, &inline_text(inlines))
        }
        Block::Code { text, .. } => push(out, text),
        Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
            blocks.iter().for_each(|b| block_text(b, out))
        }
        Block::List { items, .. } => items
            .iter()
            .flat_map(|i| &i.blocks)
            .for_each(|b| block_text(b, out)),
        Block::Table { head, rows, .. } => {
            for c in head.iter().chain(rows.iter().flatten()) {
                push(out, &inline_text(c));
            }
        }
        Block::Html { text, .. } => push(out, text),
        Block::Rule => {}
    }
}

/// Every image a document references (`images/NAME.png`), in order.
pub fn image_refs(doc: &Doc) -> Vec<(String, String)> {
    fn inl(v: &[Inline], out: &mut Vec<(String, String)>) {
        for i in v {
            if let Inline::Image { src, alt, .. } = i {
                out.push((src.clone(), alt.clone()));
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<(String, String)>) {
        for b in blocks {
            match b {
                Block::Heading { inlines, .. } | Block::Paragraph(inlines) => inl(inlines, out),
                Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => walk(blocks, out),
                Block::List { items, .. } => items.iter().for_each(|i| walk(&i.blocks, out)),
                Block::Table { head, rows, .. } => head
                    .iter()
                    .chain(rows.iter().flatten())
                    .for_each(|c| inl(c, out)),
                Block::Html { images, .. } => out.extend(images.iter().cloned()),
                Block::Code { .. } | Block::Rule => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out);
    out
}

/// The image loader's URI for embedded screenshot `key` (`en/sidebar.png`):
/// the build embeds most of them as WebP (see `build.rs`), and the URI's
/// extension says which.
pub fn image_uri(key: &str, bytes: &[u8]) -> String {
    let webp = bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP";
    match key.strip_suffix(".png") {
        Some(stem) if webp => format!("bytes://manta-help/{stem}.webp"),
        _ => format!("bytes://manta-help/{key}"),
    }
}

/// The embedded screenshot (PNG or WebP bytes) for image `name`
/// (`overview.png`) in `lang`: the dark
/// variant when `dark` and present, else the light one, else the fallback
/// language's (English), else any other language's (screenshots without
/// text exist once).
pub fn image(name: &str, lang: &str, dark: bool) -> Option<(String, &'static [u8])> {
    let stem = name.strip_suffix(".png").unwrap_or(name);
    let find = |l: &str, n: &str| {
        let key = format!("{l}/{n}");
        files::IMAGE_FILES
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(k, b)| (k.to_string(), *b))
    };
    let dark_name = format!("{stem}-dark.png");
    let idx = index();
    let fb = idx.fallback_language.as_str();
    let mut langs = vec![lang];
    if fb != lang {
        langs.push(fb);
    }
    langs.extend(
        idx.languages
            .iter()
            .map(String::as_str)
            .filter(|l| *l != lang && *l != fb),
    );
    for l in &langs {
        if dark {
            if let Some(hit) = find(l, &dark_name) {
                return Some(hit);
            }
        }
        if let Some(hit) = find(l, name) {
            return Some(hit);
        }
    }
    None
}

// ------------------------------------------------------------------ links

/// Where a link in the help window goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Chapter {
        file: String,
        anchor: Option<String>,
    },
    Dev {
        name: String,
        anchor: Option<String>,
    },
    /// Outside the manual and not embedded: inactive.
    Unknown(VfsPath),
}

/// Map a resolved Markdown link target to a help page.
pub fn place_of(path: &VfsPath, anchor: Option<String>) -> Place {
    let comps = path.components();
    match comps.as_slice() {
        ["docs", "manual", _lang, file] if index().chapters.iter().any(|c| c.file == *file) => {
            Place::Chapter {
                file: file.to_string(),
                anchor,
            }
        }
        ["docs", name] if files::DEV_DOCS.iter().any(|(n, _)| n == name) => Place::Dev {
            name: name.to_string(),
            anchor,
        },
        _ => Place::Unknown(path.clone()),
    }
}

// ------------------------------------------------------------------ search

/// Lower-case, accents removed (`Ä` → `a`, `ß` → `ss`), for matching.
fn fold_char(c: char, out: &mut String) {
    for l in c.to_lowercase() {
        match l {
            'ä' | 'á' | 'à' | 'â' | 'å' | 'ã' => out.push('a'),
            'ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ø' => out.push('o'),
            'ü' | 'ú' | 'ù' | 'û' => out.push('u'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' => out.push('i'),
            'ç' => out.push('c'),
            'ñ' => out.push('n'),
            'ß' => out.push_str("ss"),
            l => out.push(l),
        }
    }
}

pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    s.chars().for_each(|c| fold_char(c, &mut out));
    out
}

/// Folded text with, per folded byte, the byte offset of its source char.
fn fold_mapped(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        let before = out.len();
        fold_char(c, &mut out);
        map.extend(std::iter::repeat_n(i, out.len() - before));
    }
    map.push(s.len());
    (out, map)
}

/// Byte range of the first occurrence of folded `needle` in `hay`.
pub fn find_folded(hay: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let (f, map) = fold_mapped(hay);
    let i = f.find(needle)?;
    let start = map[i];
    // End: the source char after the last matched folded byte.
    let last = map[i + needle.len() - 1];
    let end = hay[last..]
        .chars()
        .next()
        .map_or(hay.len(), |c| last + c.len_utf8());
    Some(start..end)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub chapter: usize,
    pub anchor: Option<String>,
    /// Section heading (or the chapter title for the introduction).
    pub section: String,
    pub snippet: String,
    /// Highlighted range in `snippet` (bytes).
    pub mark: Option<std::ops::Range<usize>>,
    pub score: i32,
}

const MAX_HITS: usize = 60;

/// Search titles, keywords, headings and text. Every word of `query` must
/// occur in the section (or its chapter's title and keywords).
pub fn search(manual: &Manual, query: &str) -> Vec<Hit> {
    let terms: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (ci, ch) in manual.chapters.iter().enumerate() {
        let title = fold(&ch.title);
        let kw: Vec<String> = ch.keywords.iter().map(|k| fold(k)).collect();
        for (si, s) in ch.sections.iter().enumerate() {
            let head = fold(&s.title);
            let body = fold(&s.text);
            let mut score = 0;
            let mut all = true;
            // A section must match on its own (heading or text); the
            // chapter's title and keywords only help multi-word queries.
            let mut own = si == 0;
            for t in &terms {
                let mut sc = 0;
                if head.contains(t.as_str()) {
                    sc = sc.max(
                        if head
                            .split(|c: char| !c.is_alphanumeric())
                            .any(|w| w.starts_with(t.as_str()))
                        {
                            120
                        } else {
                            90
                        },
                    );
                }
                // Chapter title and keywords count for the introduction most.
                let intro = si == 0;
                if title.contains(t.as_str()) {
                    sc = sc.max(if intro { 110 } else { 20 });
                }
                if kw.iter().any(|k| k.contains(t.as_str())) {
                    sc = sc.max(if intro { 70 } else { 15 });
                }
                if body.contains(t.as_str()) {
                    sc = sc.max(10 + body.matches(t.as_str()).count().min(5) as i32);
                }
                own |= head.contains(t.as_str()) || body.contains(t.as_str());
                if sc == 0 {
                    all = false;
                    break;
                }
                score += sc;
            }
            if !all || !own {
                continue;
            }
            let section = if s.title.is_empty() {
                ch.title.clone()
            } else {
                s.title.clone()
            };
            let (snippet, mark) = snippet(&s.text, &terms);
            hits.push(Hit {
                chapter: ci,
                anchor: s.anchor.clone(),
                section,
                snippet,
                mark,
                score,
            });
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.chapter.cmp(&b.chapter)));
    hits.truncate(MAX_HITS);
    hits
}

/// About 140 characters of `text` around the first matching term.
fn snippet(text: &str, terms: &[String]) -> (String, Option<std::ops::Range<usize>>) {
    const BEFORE: usize = 45;
    const LEN: usize = 150;
    let found = terms
        .iter()
        .filter_map(|t| find_folded(text, t))
        .min_by_key(|r| r.start);
    let Some(r) = found else {
        let s: String = text.chars().take(LEN).collect();
        let more = s.len() < text.len();
        return (
            if more {
                format!("{}…", s.trim_end())
            } else {
                s
            },
            None,
        );
    };
    // Start a few words before the match, at a char boundary.
    let mut start = r.start.saturating_sub(BEFORE * 2);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let pre_chars = text[start..r.start].chars().count();
    if pre_chars > BEFORE {
        start = text[start..]
            .char_indices()
            .nth(pre_chars - BEFORE)
            .map_or(start, |(i, _)| start + i);
    }
    if start > 0 {
        if let Some(sp) = text[start..r.start].find(' ') {
            start += sp + 1;
        }
    }
    let end = text[start..]
        .char_indices()
        .nth(LEN)
        .map_or(text.len(), |(i, _)| start + i)
        .max(r.end);
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    let offset = s.len();
    s.push_str(&text[start..end]);
    if end < text.len() {
        s.push('…');
    }
    let mark = (r.start - start + offset)..(r.end - start + offset);
    (s, Some(mark))
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_plugin_markdown::links::{self, Target};

    #[test]
    fn every_chapter_exists_in_every_language() {
        let idx = index();
        assert!(idx.chapters.len() >= 20);
        for l in &idx.languages {
            for c in &idx.chapters {
                let src = source(l, &c.file).unwrap_or_else(|| panic!("{l}/{} missing", c.file));
                assert!(c.title.contains_key(l), "{} has no {l} title", c.file);
                let doc = Doc::parse(src, false);
                // `id` equals the chapter's top-level heading anchor.
                assert_eq!(
                    doc.headings.first().map(|h| h.slug.as_str()),
                    Some(c.id.as_str()),
                    "{l}/{}",
                    c.file
                );
            }
        }
    }

    /// Every internal link and anchor, in every language, points at an
    /// existing chapter / heading (dev doc links at an embedded doc).
    #[test]
    fn every_internal_link_resolves() {
        let mut bad = Vec::new();
        for l in languages() {
            let m = get(&l);
            assert_eq!(m.lang, l);
            for ch in &m.chapters {
                for href in &ch.doc.links {
                    match links::classify(href, &chapter_dir(&l)) {
                        Target::External(_) => {}
                        Target::Anchor(a) => {
                            if !ch.doc.headings.iter().any(|h| h.slug == a) {
                                bad.push(format!("{l}/{}: #{a}", ch.file));
                            }
                        }
                        Target::Markdown { path, anchor } => match place_of(&path, anchor) {
                            Place::Chapter { file, anchor } => {
                                let target = m.chapter(&file).expect("place_of checked it");
                                if let Some(a) = anchor {
                                    if !target.doc.headings.iter().any(|h| h.slug == a) {
                                        bad.push(format!("{l}/{}: {file}#{a}", ch.file));
                                    }
                                }
                            }
                            Place::Dev { name, anchor } => {
                                let d = dev_doc(&name).unwrap();
                                if let Some(a) = anchor {
                                    if !d.headings.iter().any(|h| h.slug == a) {
                                        bad.push(format!("{l}/{}: {name}#{a}", ch.file));
                                    }
                                }
                            }
                            Place::Unknown(p) => bad.push(format!("{l}/{}: {href} → {p}", ch.file)),
                        },
                        Target::File(p) => bad.push(format!("{l}/{}: {href} → file {p}", ch.file)),
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{} broken links:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// Explicit heading ids (`{#id}`) are the same in both languages, so
    /// links and context help work in either.
    #[test]
    fn same_anchors_in_both_languages() {
        let ids = |src: &str| -> std::collections::BTreeSet<String> {
            src.lines()
                .filter(|l| l.starts_with('#'))
                .filter_map(|l| {
                    l.rsplit_once("{#")
                        .map(|(_, r)| r.trim_end().trim_end_matches('}').to_string())
                })
                .collect()
        };
        for c in &index().chapters {
            let de = ids(source("de", &c.file).unwrap());
            let en = ids(source("en", &c.file).unwrap());
            assert!(!de.is_empty());
            assert_eq!(de, en, "{}", c.file);
        }
    }

    #[test]
    fn fallback_language() {
        assert_eq!(language_for("de"), "de");
        assert_eq!(language_for("en"), "en");
        assert_eq!(language_for("fr"), "fr");
        assert_eq!(language_for("zh"), "zh-Hans");
        assert_eq!(language_for("pt"), "en");
        assert_eq!(language_for("DE"), "de");
        assert_eq!(get("ja").lang, "en");
    }

    /// Each language shows its own screenshots; a missing one comes from
    /// English (the fallback language), not from the first language listed.
    #[test]
    fn images_per_language_fall_back_to_english() {
        let has = |key: &str| files::IMAGE_FILES.iter().any(|(k, _)| *k == key);
        for lang in &index().languages {
            let (key, _) = image("sidebar.png", lang, false).expect("sidebar.png");
            if has(&format!("{lang}/sidebar.png")) {
                assert_eq!(key, format!("{lang}/sidebar.png"));
            } else {
                assert_eq!(key, "en/sidebar.png");
            }
        }
        assert_eq!(
            image("sidebar.png", "pt", false).unwrap().0,
            "en/sidebar.png"
        );
        assert!(image("no-such-image.png", "de", false).is_none());
    }

    /// Every embedded screenshot (WebP made by the build, or the PNG)
    /// decodes, at the size of its PNG in the repository, and its URI names
    /// the format the image loader has to use.
    #[test]
    fn embedded_images_decode_like_the_pngs() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/manual/images");
        assert!(!files::IMAGE_FILES.is_empty());
        for (key, bytes) in files::IMAGE_FILES {
            let img = image::load_from_memory(bytes).unwrap_or_else(|e| panic!("{key}: {e}"));
            let png = image::open(dir.join(key)).unwrap_or_else(|e| panic!("{key}: {e}"));
            assert_eq!(
                (img.width(), img.height()),
                (png.width(), png.height()),
                "{key}"
            );
            let uri = image_uri(key, bytes);
            match image::guess_format(bytes).unwrap() {
                image::ImageFormat::WebP => {
                    assert!(uri.ends_with(".webp"), "{uri}");
                    // Lossless: the same pixels as the PNG.
                    assert!(img.to_rgba8() == png.to_rgba8(), "{key} differs");
                }
                image::ImageFormat::Png => assert!(uri.ends_with(".png"), "{uri}"),
                f => panic!("{key}: unexpected {f:?}"),
            }
        }
        // The help window's loaders (egui_extras, as installed by the app)
        // turn an embedded screenshot into an image by its URI.
        use eframe::egui;
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let (key, bytes) = image("sidebar.png", "de", false).unwrap();
        let uri = image_uri(&key, bytes);
        ctx.include_bytes(uri.clone(), bytes);
        let start = std::time::Instant::now();
        let size = loop {
            match ctx.try_load_image(&uri, egui::SizeHint::default()) {
                Ok(egui::load::ImagePoll::Ready { image }) => break image.size,
                Ok(egui::load::ImagePoll::Pending { .. }) => {
                    assert!(start.elapsed().as_secs() < 30, "{uri} never loaded");
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => panic!("{uri}: {e}"),
            }
        };
        let png = image::open(dir.join(&key)).unwrap();
        assert_eq!(size, [png.width() as usize, png.height() as usize]);
    }

    #[test]
    fn search_is_accent_and_case_insensitive() {
        let de = get("de");
        let hits = search(&de, "LOSCHEN");
        assert!(!hits.is_empty());
        let first = &hits[0];
        assert!(fold(&first.section).contains("losch"), "{first:?}");
        // Keywords only in index.json still find the chapter.
        let hits = search(&de, "gatekeeper");
        assert!(hits
            .iter()
            .any(|h| de.chapters[h.chapter].file == "01-erste-schritte.md"));
        assert!(search(&de, "  ").is_empty());
        assert!(search(&de, "xyzzy-nothing").is_empty());
    }

    #[test]
    fn snippet_marks_the_match() {
        let text = "Ein langer Text über Dateien. Hier steht etwas zum Löschen von Ordnern und mehr Text danach.";
        let (s, m) = snippet(text, &["loschen".into()]);
        assert_eq!(&s[m.unwrap()], "Löschen");
        let long = format!(
            "{} Übertragungen {}",
            "wort ".repeat(40),
            "ende ".repeat(60)
        );
        let (s, m) = snippet(&long, &["ubertragungen".into()]);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert_eq!(&s[m.unwrap()], "Übertragungen");
    }

    #[test]
    fn dev_doc_links_resolve() {
        let p = links::classify("../../plugins.md", &chapter_dir("de"));
        let Target::Markdown { path, .. } = p else {
            panic!("{p:?}")
        };
        assert_eq!(
            place_of(&path, None),
            Place::Dev {
                name: "plugins.md".into(),
                anchor: None
            }
        );
        assert!(dev_doc("plugins.md").is_some());
    }
}
