//! Finder tags: read, show and change the tags macOS keeps on local files.
//!
//! macOS stores an item's tags in the extended attribute
//! `com.apple.metadata:_kMDItemUserTags`: a binary property list with an
//! array of strings, each `"Name"` or `"Name\nN"` where `N` is the colour
//! (0 none, 1 grey, 2 green, 3 purple, 4 blue, 5 yellow, 6 red, 7 orange).
//! This is what `NSURLTagNamesKey` reads and what Spotlight indexes as
//! `kMDItemUserTags`, so tags set here show up in Finder and in searches,
//! and Finder's tags show up here. Cloud folders (iCloud Drive, File
//! Provider) are ordinary local folders and work the same way.
//!
//! - [`read_tags`] / [`write_tags`] / [`set_tag`]: one item (`getxattr` /
//!   `setxattr`, never following symlinks, like Finder).
//! - [`TagStore`]: the tags of the visible folders' items, read on a
//!   background thread (inline in tests), and the user's tag list for the
//!   sidebar: Finder's (`FavoriteTagNames` in `com.apple.finder`, colours
//!   from `FinderTagDict` in the synced preferences) or, if that can't be
//!   read, the tags seen on files.
//! - [`paint_dots`]: the coloured dots after a name (list, columns) or
//!   under it (icons, gallery).
//! - [`tags_menu`]: the "Tags" submenu of the context menu.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use eframe::egui::{self, Color32, Stroke};

use crate::theme::{self, pal, text};

/// The extended attribute that holds an item's tags.
pub const XATTR: &str = "com.apple.metadata:_kMDItemUserTags";

/// A tag's colour, numbered like Finder stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum TagColor {
    #[default]
    None,
    Gray,
    Green,
    Purple,
    Blue,
    Yellow,
    Red,
    Orange,
}

impl TagColor {
    /// The seven colours in Finder's order (context menu, sidebar).
    pub const STANDARD: [TagColor; 7] = [
        TagColor::Red,
        TagColor::Orange,
        TagColor::Yellow,
        TagColor::Green,
        TagColor::Blue,
        TagColor::Purple,
        TagColor::Gray,
    ];

    pub fn index(self) -> u8 {
        match self {
            TagColor::None => 0,
            TagColor::Gray => 1,
            TagColor::Green => 2,
            TagColor::Purple => 3,
            TagColor::Blue => 4,
            TagColor::Yellow => 5,
            TagColor::Red => 6,
            TagColor::Orange => 7,
        }
    }

    pub fn from_index(i: u8) -> TagColor {
        match i {
            1 => TagColor::Gray,
            2 => TagColor::Green,
            3 => TagColor::Purple,
            4 => TagColor::Blue,
            5 => TagColor::Yellow,
            6 => TagColor::Red,
            7 => TagColor::Orange,
            _ => TagColor::None,
        }
    }

    /// The dot colour (Apple's system colours).
    pub fn rgb(self, dark: bool) -> Color32 {
        match (self, dark) {
            (TagColor::Red, false) => Color32::from_rgb(255, 59, 48),
            (TagColor::Red, true) => Color32::from_rgb(255, 69, 58),
            (TagColor::Orange, false) => Color32::from_rgb(255, 149, 0),
            (TagColor::Orange, true) => Color32::from_rgb(255, 159, 10),
            (TagColor::Yellow, false) => Color32::from_rgb(255, 204, 0),
            (TagColor::Yellow, true) => Color32::from_rgb(255, 214, 10),
            (TagColor::Green, false) => Color32::from_rgb(40, 205, 65),
            (TagColor::Green, true) => Color32::from_rgb(50, 215, 75),
            (TagColor::Blue, false) => Color32::from_rgb(0, 122, 255),
            (TagColor::Blue, true) => Color32::from_rgb(10, 132, 255),
            (TagColor::Purple, false) => Color32::from_rgb(175, 82, 222),
            (TagColor::Purple, true) => Color32::from_rgb(191, 90, 242),
            (TagColor::Gray, _) => Color32::from_rgb(142, 142, 147),
            (TagColor::None, _) => Color32::TRANSPARENT,
        }
    }

    /// The colour's name (also the standard tag's name in the UI language).
    pub fn label(self) -> &'static str {
        match self {
            TagColor::Red => t!("tags.color.red"),
            TagColor::Orange => t!("tags.color.orange"),
            TagColor::Yellow => t!("tags.color.yellow"),
            TagColor::Green => t!("tags.color.green"),
            TagColor::Blue => t!("tags.color.blue"),
            TagColor::Purple => t!("tags.color.purple"),
            TagColor::Gray => t!("tags.color.gray"),
            TagColor::None => t!("tags.color.none"),
        }
    }

    /// The names Finder gives the standard tags (in the languages Manta
    /// speaks): "Rot" on a German Mac is the red tag, too.
    fn standard_names(self) -> &'static [&'static str] {
        match self {
            TagColor::Red => &["Red", "Rot", "红色", "Rojo", "Rouge", "Rosso", "Красный"],
            TagColor::Orange => &["Orange", "橙色", "Naranja", "Arancione", "Оранжевый"],
            TagColor::Yellow => &[
                "Yellow",
                "Gelb",
                "黄色",
                "Amarillo",
                "Jaune",
                "Giallo",
                "Желтый",
                "Жёлтый",
            ],
            TagColor::Green => &[
                "Green",
                "Grün",
                "绿色",
                "Verde",
                "Vert",
                "Зеленый",
                "Зелёный",
            ],
            TagColor::Blue => &["Blue", "Blau", "蓝色", "Azul", "Bleu", "Blu", "Синий"],
            TagColor::Purple => &[
                "Purple",
                "Lila",
                "紫色",
                "Morado",
                "Violet",
                "Viola",
                "Фиолетовый",
            ],
            TagColor::Gray => &["Gray", "Grey", "Grau", "灰色", "Gris", "Grigio", "Серый"],
            TagColor::None => &[],
        }
    }

    /// The colour of a standard tag by its name (any of Manta's languages).
    pub fn of_standard_name(name: &str) -> Option<TagColor> {
        TagColor::STANDARD.into_iter().find(|c| {
            c.standard_names()
                .iter()
                .any(|n| n.eq_ignore_ascii_case(name))
        })
    }
}

/// One tag: a name and an optional colour.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub name: String,
    pub color: TagColor,
}

impl Tag {
    pub fn new(name: impl Into<String>, color: TagColor) -> Tag {
        Tag {
            name: name.into(),
            color,
        }
    }

    /// `"Name\n6"` → red "Name"; `"Name"` → no colour.
    pub fn parse(s: &str) -> Tag {
        match s.rsplit_once('\n') {
            Some((name, n)) if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() => Tag {
                name: name.to_string(),
                color: TagColor::from_index(n.as_bytes()[0] - b'0'),
            },
            _ => Tag {
                name: s.to_string(),
                color: TagColor::None,
            },
        }
    }

    /// The form stored in the attribute.
    pub fn encode(&self) -> String {
        if self.color == TagColor::None {
            self.name.clone()
        } else {
            format!("{}\n{}", self.name, self.color.index())
        }
    }

    /// Same tag (names compare case-insensitively, like Finder).
    pub fn same(&self, other: &Tag) -> bool {
        same_name(&self.name, &other.name)
    }
}

pub fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The tags in an attribute value (empty for anything unreadable).
pub fn decode_tags(bytes: &[u8]) -> Vec<Tag> {
    match bplist::parse(bytes) {
        Some(bplist::Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v {
                bplist::Value::String(s) if !s.is_empty() => Some(Tag::parse(&s)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The attribute value for `tags`.
pub fn encode_tags(tags: &[Tag]) -> Vec<u8> {
    let strings: Vec<String> = tags.iter().map(Tag::encode).collect();
    bplist::write_strings(&strings)
}

// ------------------------------------------------------------------ xattr

#[cfg(target_os = "macos")]
mod xattr {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const XATTR_NOFOLLOW: libc::c_int = 0x0001;

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
    }

    /// The attribute's value, `None` if the item has none.
    pub fn get(path: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
        let p = c_path(path)?;
        let n = CString::new(name).expect("attribute name");
        // SAFETY: valid C strings; a null buffer asks for the size.
        let size = unsafe {
            libc::getxattr(
                p.as_ptr(),
                n.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                XATTR_NOFOLLOW,
            )
        };
        if size < 0 {
            let e = io::Error::last_os_error();
            return if e.raw_os_error() == Some(libc::ENOATTR) {
                Ok(None)
            } else {
                Err(e)
            };
        }
        let mut buf = vec![0u8; size as usize];
        // SAFETY: `buf` has room for `size` bytes.
        let got = unsafe {
            libc::getxattr(
                p.as_ptr(),
                n.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                XATTR_NOFOLLOW,
            )
        };
        if got < 0 {
            let e = io::Error::last_os_error();
            return if e.raw_os_error() == Some(libc::ENOATTR) {
                Ok(None)
            } else {
                Err(e)
            };
        }
        buf.truncate(got as usize);
        Ok(Some(buf))
    }

    pub fn set(path: &Path, name: &str, value: &[u8]) -> io::Result<()> {
        let p = c_path(path)?;
        let n = CString::new(name).expect("attribute name");
        // SAFETY: valid C strings and buffer.
        let r = unsafe {
            libc::setxattr(
                p.as_ptr(),
                n.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                XATTR_NOFOLLOW,
            )
        };
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn remove(path: &Path, name: &str) -> io::Result<()> {
        let p = c_path(path)?;
        let n = CString::new(name).expect("attribute name");
        // SAFETY: valid C strings.
        let r = unsafe { libc::removexattr(p.as_ptr(), n.as_ptr(), XATTR_NOFOLLOW) };
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ENOATTR) {
                return Ok(());
            }
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod xattr {
    use std::io;
    use std::path::Path;

    pub fn get(_path: &Path, _name: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
    pub fn set(_path: &Path, _name: &str, _value: &[u8]) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    pub fn remove(_path: &Path, _name: &str) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub use xattr::get as get_xattr;

/// The tags of the item at `path` (empty if it has none).
pub fn read_tags(path: &Path) -> std::io::Result<Vec<Tag>> {
    Ok(xattr::get(path, XATTR)?
        .map(|b| decode_tags(&b))
        .unwrap_or_default())
}

/// Replace the item's tags (no tags: the attribute is removed).
pub fn write_tags(path: &Path, tags: &[Tag]) -> std::io::Result<()> {
    if tags.is_empty() {
        xattr::remove(path, XATTR)
    } else {
        xattr::set(path, XATTR, &encode_tags(tags))
    }
}

/// Add (`on`) or remove a tag on one item. Returns the new tags.
pub fn set_tag(path: &Path, tag: &Tag, on: bool) -> std::io::Result<Vec<Tag>> {
    let mut tags = read_tags(path)?;
    let had = tags.iter().position(|t| t.same(tag));
    match (had, on) {
        (Some(i), true) => {
            if tags[i].color != tag.color && tag.color != TagColor::None {
                tags[i].color = tag.color;
            } else {
                return Ok(tags);
            }
        }
        (None, true) => tags.push(tag.clone()),
        (Some(i), false) => {
            tags.remove(i);
        }
        (None, false) => return Ok(tags),
    }
    write_tags(path, &tags)?;
    Ok(tags)
}

/// The tags of every item in `dir` that has any, by name.
pub fn read_folder(dir: &Path) -> FolderTags {
    let mut out = FolderTags::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        if let Ok(tags) = read_tags(&e.path()) {
            if !tags.is_empty() {
                out.insert(e.file_name().to_string_lossy().into_owned(), tags);
            }
        }
    }
    out
}

/// Tags of a folder's items, by entry name (items without tags are left out).
pub type FolderTags = HashMap<String, Vec<Tag>>;

/// Items at `loc` can have tags: on this Mac, outside archives.
pub fn taggable(loc: &crate::pane::Location) -> bool {
    loc.conn.is_none()
        && loc.archive.is_none()
        && loc.fs.protocol() == manta_core::Protocol::Local
        && loc.fs.local_path(&loc.path).is_some()
}

// ------------------------------------------------------------------ Finder's tag list

/// Finder's tag list: the names in the sidebar (`FavoriteTagNames` in
/// `~/Library/Preferences/com.apple.finder.plist`) with the colours from
/// `FinderTagDict` (`~/Library/SyncedPreferences/com.apple.finder.plist`)
/// or, for the standard names, their colour. Empty if neither is readable.
pub fn finder_tags() -> Vec<Tag> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    finder_tags_from(
        &home.join("Library/Preferences/com.apple.finder.plist"),
        &home.join("Library/SyncedPreferences/com.apple.finder.plist"),
    )
}

/// [`finder_tags`] from the given preference files (tests).
pub fn finder_tags_from(prefs: &Path, synced: &Path) -> Vec<Tag> {
    use bplist::Value;
    let read = |p: &Path| std::fs::read(p).ok().and_then(|b| bplist::parse(&b));
    // Name → colour from the tag dictionary (all tags Finder knows).
    let mut colors: Vec<Tag> = Vec::new();
    if let Some(v) = read(synced) {
        let dict = v
            .get("values")
            .and_then(|v| v.get("FinderTagDict"))
            .and_then(|v| v.get("value"))
            .or_else(|| v.get("FinderTagDict"));
        if let Some(Value::Array(tags)) = dict.and_then(|d| d.get("FinderTags")) {
            for t in tags {
                if let Some(Value::String(n)) = t.get("n") {
                    let color = match t.get("l") {
                        Some(Value::Int(i)) => TagColor::from_index(*i as u8),
                        _ => TagColor::of_standard_name(n).unwrap_or_default(),
                    };
                    colors.push(Tag::new(n.clone(), color));
                }
            }
        }
    }
    let color_of = |name: &str| {
        colors
            .iter()
            .find(|t| same_name(&t.name, name))
            .map(|t| t.color)
            .or_else(|| TagColor::of_standard_name(name))
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    if let Some(Value::Array(names)) = read(prefs).and_then(|v| v.get("FavoriteTagNames").cloned())
    {
        for n in names {
            if let Value::String(n) = n {
                if !n.is_empty() && !out.iter().any(|t: &Tag| same_name(&t.name, &n)) {
                    let c = color_of(&n);
                    out.push(Tag::new(n, c));
                }
            }
        }
    }
    if out.is_empty() {
        out = colors;
    }
    out
}

// ------------------------------------------------------------------ store

enum Msg {
    Folder(PathBuf, FolderTags),
}

/// The tags of the visible folders and the user's tag list.
pub struct TagStore {
    folders: HashMap<PathBuf, Arc<FolderTags>>,
    inflight: HashSet<PathBuf>,
    /// Folders whose tags changed while a read was running: read again.
    stale: HashSet<PathBuf>,
    /// Finder's tag list (read at start and on refresh).
    finder: Vec<Tag>,
    finder_source: Box<dyn Fn() -> Vec<Tag>>,
    /// Tags seen on files this session (name → colour).
    seen: BTreeMap<String, Tag>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Read right away on the UI thread (tests: deterministic).
    inline: bool,
    /// Bumped whenever the known tags may have changed.
    version: u64,
}

impl TagStore {
    /// Finder's tag list and background reads.
    #[cfg_attr(test, allow(dead_code))]
    pub fn system() -> TagStore {
        TagStore::new(Box::new(finder_tags), false)
    }

    pub fn new(finder_source: Box<dyn Fn() -> Vec<Tag>>, inline: bool) -> TagStore {
        let (tx, rx) = channel();
        let finder = finder_source();
        TagStore {
            folders: HashMap::new(),
            inflight: HashSet::new(),
            stale: HashSet::new(),
            finder,
            finder_source,
            seen: BTreeMap::new(),
            tx,
            rx,
            inline,
            version: 1,
        }
    }

    /// Read Finder's tag list again (sidebar refresh).
    pub fn reload_finder(&mut self) {
        self.finder = (self.finder_source)();
        self.version += 1;
    }

    /// Once per frame: take finished reads, read the `wanted` folders that
    /// are missing, forget the others.
    pub fn frame(&mut self, ctx: &egui::Context, wanted: &[PathBuf]) -> bool {
        let mut changed = false;
        while let Ok(Msg::Folder(dir, tags)) = self.rx.try_recv() {
            self.inflight.remove(&dir);
            if self.stale.remove(&dir) {
                continue;
            }
            self.note_folder(&tags);
            self.folders.insert(dir, Arc::new(tags));
            changed = true;
        }
        self.folders.retain(|d, _| wanted.contains(d));
        for dir in wanted {
            if self.folders.contains_key(dir) || self.inflight.contains(dir) {
                continue;
            }
            self.read(ctx, dir.clone());
            changed |= self.inline;
        }
        changed
    }

    fn note_folder(&mut self, tags: &FolderTags) {
        for t in tags.values().flatten() {
            self.note(t);
        }
    }

    /// Remember a tag seen on a file (for the menu and the sidebar).
    pub fn note(&mut self, t: &Tag) {
        let key = t.name.to_lowercase();
        match self.seen.get_mut(&key) {
            Some(old) if old.color == TagColor::None && t.color != TagColor::None => {
                old.color = t.color;
                self.version += 1;
            }
            Some(_) => {}
            None => {
                self.seen.insert(key, t.clone());
                self.version += 1;
            }
        }
    }

    fn read(&mut self, ctx: &egui::Context, dir: PathBuf) {
        if self.inline {
            let tags = read_folder(&dir);
            self.note_folder(&tags);
            self.folders.insert(dir, Arc::new(tags));
            return;
        }
        self.inflight.insert(dir.clone());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("manta-tags".into())
            .spawn(move || {
                let tags = read_folder(&dir);
                let _ = tx.send(Msg::Folder(dir, tags));
                ctx.request_repaint();
            });
        if let Err(e) = spawned {
            log::warn!("tags: can't start a reader thread: {e}");
        }
    }

    /// The tags of `dir`'s items, if read. Never blocks.
    pub fn get(&self, dir: &Path) -> Option<Arc<FolderTags>> {
        self.folders.get(dir).cloned()
    }

    /// `dir` changed (tags set, live refresh): read it again.
    pub fn invalidate(&mut self, dir: &Path) {
        self.folders.remove(dir);
        if self.inflight.contains(dir) {
            self.stale.insert(dir.to_path_buf());
        }
    }

    /// Finder's tag list.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The tags for the sidebar: Finder's list, else the tags seen on files.
    pub fn sidebar(&self) -> Vec<Tag> {
        if !self.finder.is_empty() {
            return self.finder.clone();
        }
        self.seen.values().cloned().collect()
    }

    /// Every tag known (Finder's first, then the ones seen), for the menu.
    pub fn known(&self) -> Vec<Tag> {
        let mut out = self.finder.clone();
        for t in self.seen.values() {
            if !out.iter().any(|o| o.same(t)) {
                out.push(t.clone());
            }
        }
        out
    }

    #[cfg(test)]
    /// The colour of the tag called `name` (Finder's list, seen tags, the
    /// standard names).
    pub fn color_of(&self, name: &str) -> TagColor {
        self.finder
            .iter()
            .chain(self.seen.values())
            .find(|t| same_name(&t.name, name) && t.color != TagColor::None)
            .map(|t| t.color)
            .or_else(|| TagColor::of_standard_name(name))
            .unwrap_or_default()
    }

    /// The standard tag of a colour: the name Finder uses for it here, else
    /// the colour's name in the UI language.
    pub fn standard(&self, color: TagColor) -> Tag {
        let name = self
            .finder
            .iter()
            .chain(self.seen.values())
            .find(|t| t.color == color && TagColor::of_standard_name(&t.name) == Some(color))
            .map(|t| t.name.clone())
            .unwrap_or_else(|| color.label().to_string());
        Tag::new(name, color)
    }
}

// ------------------------------------------------------------------ filter

/// A filter text split into `tag:` terms and the rest: `"tag:Rot report"`
/// → (["rot"], "report"). Tag names with spaces: `tag:"Sehr wichtig"`.
pub fn split_filter(filter: &str) -> (Vec<String>, String) {
    let mut tags = Vec::new();
    let mut rest = Vec::new();
    let mut s = filter.trim();
    while !s.is_empty() {
        let lower = s.to_lowercase();
        if lower.starts_with("tag:") {
            let after = &s[4..];
            let (name, tail) = if let Some(q) = after.strip_prefix('"') {
                match q.find('"') {
                    Some(end) => (&q[..end], &q[end + 1..]),
                    None => (q, ""),
                }
            } else {
                match after.find(char::is_whitespace) {
                    Some(end) => (&after[..end], &after[end..]),
                    None => (after, ""),
                }
            };
            if !name.trim().is_empty() {
                tags.push(name.trim().to_lowercase());
            }
            s = tail.trim_start();
        } else {
            let end = s.find(char::is_whitespace).unwrap_or(s.len());
            rest.push(&s[..end]);
            s = s[end..].trim_start();
        }
    }
    (tags, rest.join(" "))
}

/// Does an item with `tags` match every wanted tag (by name or by colour
/// name: `tag:red` finds the red tag even when it's called "Rot")?
pub fn matches_tags(tags: &[Tag], wanted: &[String]) -> bool {
    wanted.iter().all(|w| {
        tags.iter().any(|t| {
            t.name.to_lowercase() == *w
                || (t.color != TagColor::None
                    && (t.color.label().to_lowercase() == *w
                        || t.color
                            .standard_names()
                            .iter()
                            .any(|n| n.to_lowercase() == *w)))
        })
    })
}

// ------------------------------------------------------------------ painting

/// Width of the dots for `tags` at diameter `d` (0 without coloured tags).
pub fn dots_width(tags: &[Tag], d: f32) -> f32 {
    let n = tags
        .iter()
        .filter(|t| t.color != TagColor::None)
        .count()
        .min(3);
    if n == 0 {
        0.0
    } else {
        d + (n - 1) as f32 * d * 0.55
    }
}

/// Paint the coloured dots of `tags` (up to three, overlapping like
/// Finder's) starting at `left`, centred on `cy`; each has a ring in
/// `ring` (the background) so they stay apart. Returns the width.
pub fn paint_dots(
    painter: &egui::Painter,
    left: f32,
    cy: f32,
    tags: &[Tag],
    d: f32,
    ring: Color32,
    dark: bool,
) -> f32 {
    let colored: Vec<&Tag> = tags
        .iter()
        .filter(|t| t.color != TagColor::None)
        .take(3)
        .collect();
    if colored.is_empty() {
        return 0.0;
    }
    let step = d * 0.55;
    // The first tag on top (rightmost drawn last would hide it): draw from
    // the last to the first.
    for (i, t) in colored.iter().enumerate().rev() {
        let c = egui::pos2(left + d / 2.0 + i as f32 * step, cy);
        painter.circle_filled(c, d / 2.0 + 1.0, ring);
        painter.circle_filled(c, d / 2.0, t.color.rgb(dark));
    }
    dots_width(tags, d)
}

/// The spoken form of an item's tags ("Tags: Red, Work").
pub fn spoken(tags: &[Tag]) -> Option<String> {
    if tags.is_empty() {
        return None;
    }
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    Some(t!("tags.spoken", names = names.join(", ")))
}

// ------------------------------------------------------------------ menu

/// A change asked for in the Tags menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagEdit {
    pub tag: Tag,
    /// Add (`true`) or remove.
    pub on: bool,
}

/// What the Tags menu shows: the tags of the selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagMenu {
    /// How many items are selected.
    pub items: usize,
    /// Each tag on at least one selected item, with on how many.
    pub on: Vec<(Tag, usize)>,
    /// The standard tags (names as Finder uses them here).
    pub standard: Vec<Tag>,
    /// The other known tags (custom ones).
    pub custom: Vec<Tag>,
}

/// The tags the menus offer: the seven standard ones (named as Finder
/// names them here) and the other known tags. The app keeps a copy in the
/// egui context ([`set_known`]) for the context menus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownTags {
    pub standard: Vec<Tag>,
    pub custom: Vec<Tag>,
}

impl KnownTags {
    pub fn of(store: &TagStore) -> KnownTags {
        let standard: Vec<Tag> = TagColor::STANDARD
            .into_iter()
            .map(|c| store.standard(c))
            .collect();
        let custom = store
            .known()
            .into_iter()
            .filter(|t| !standard.iter().any(|s| s.same(t)))
            .collect();
        KnownTags { standard, custom }
    }
}

fn known_id() -> egui::Id {
    egui::Id::unique("manta-known-tags")
}

/// Hand the known tags to the menus (once per frame, cheap when unchanged).
pub fn set_known(ctx: &egui::Context, known: KnownTags) {
    let same = ctx.data(|d| {
        d.get_temp::<Arc<KnownTags>>(known_id())
            .is_some_and(|k| *k == known)
    });
    if !same {
        ctx.data_mut(|d| d.insert_temp(known_id(), Arc::new(known)));
    }
}

/// The known tags for a menu (the standard colours if the app set none).
pub fn known(ctx: &egui::Context) -> Arc<KnownTags> {
    ctx.data(|d| d.get_temp::<Arc<KnownTags>>(known_id()))
        .unwrap_or_else(|| {
            Arc::new(KnownTags {
                standard: TagColor::STANDARD
                    .into_iter()
                    .map(|c| Tag::new(c.label(), c))
                    .collect(),
                custom: Vec::new(),
            })
        })
}

impl TagMenu {
    /// The menu for items with these tags.
    pub fn new(known: &KnownTags, selection: &[&[Tag]]) -> TagMenu {
        let mut on: Vec<(Tag, usize)> = Vec::new();
        for tags in selection {
            for t in tags.iter() {
                match on.iter_mut().find(|(o, _)| o.same(t)) {
                    Some((_, n)) => *n += 1,
                    None => on.push((t.clone(), 1)),
                }
            }
        }
        let standard = known.standard.clone();
        let mut custom: Vec<Tag> = known
            .custom
            .iter()
            .cloned()
            .chain(on.iter().map(|(t, _)| t.clone()))
            .filter(|t| !standard.iter().any(|s| s.same(t)))
            .collect();
        let mut seen = HashSet::new();
        custom.retain(|t| seen.insert(t.name.to_lowercase()));
        TagMenu {
            items: selection.len(),
            on,
            standard,
            custom,
        }
    }

    /// On how many selected items `tag` is.
    pub fn count(&self, tag: &Tag) -> usize {
        self.on
            .iter()
            .find(|(t, _)| t.same(tag))
            .map_or(0, |(_, n)| *n)
    }
}

fn new_tag_id() -> egui::Id {
    egui::Id::unique("manta-tags-new-name")
}

/// The "Tags" submenu: the seven colours as round buttons (✓ when every
/// selected item has the tag, – when some have), the custom tags as
/// check items, and a field for a new tag. A click toggles the tag on the
/// whole selection.
pub fn tags_menu(ui: &mut egui::Ui, menu: &TagMenu) -> Option<TagEdit> {
    let p = pal(ui);
    let mut out = None;
    ui.set_min_width(230.0);
    // The colour row.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(4.0);
        for t in &menu.standard {
            let n = menu.count(t);
            let all = n > 0 && n == menu.items;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
            let c = rect.center();
            let fill = t.color.rgb(p.dark);
            ui.painter().circle_filled(c, 9.0, fill);
            if resp.hovered() {
                ui.painter()
                    .circle_stroke(c, 10.5, Stroke::new(1.5, p.text_secondary));
            }
            if all {
                crate::widgets::paint_icon(
                    ui,
                    rect,
                    crate::icons::CHECK,
                    11.0,
                    Color32::WHITE,
                    false,
                );
            } else if n > 0 {
                // Some of the selected items have it.
                ui.painter().hline(
                    egui::Rangef::new(c.x - 4.0, c.x + 4.0),
                    c.y,
                    Stroke::new(2.0, Color32::WHITE),
                );
            } else if resp.hovered() {
                crate::widgets::paint_icon(
                    ui,
                    rect,
                    crate::icons::PLUS,
                    11.0,
                    Color32::WHITE,
                    false,
                );
            }
            let label = if all || n > 0 {
                t!("tags.remove_named", name = t.name)
            } else {
                t!("tags.add_named", name = t.name)
            };
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::CheckBox, true, all, &t.name)
            });
            let resp = resp.on_hover_text(label);
            if resp.clicked() {
                out = Some(TagEdit {
                    tag: t.clone(),
                    on: !all,
                });
                ui.close();
            }
        }
    });
    if !menu.custom.is_empty() {
        ui.separator();
        for t in &menu.custom {
            let n = menu.count(t);
            let all = n > 0 && n == menu.items;
            let mark = if all {
                crate::icons::CHECK
            } else if n > 0 {
                "–"
            } else {
                " "
            };
            let mut job = egui::text::LayoutJob::default();
            let font = theme::regular(text::BODY);
            job.append(
                &format!("{mark}  "),
                0.0,
                egui::TextFormat::simple(theme::icon_font(12.0), p.text),
            );
            job.append(
                "●",
                0.0,
                egui::TextFormat::simple(
                    font.clone(),
                    if t.color == TagColor::None {
                        p.text_tertiary
                    } else {
                        t.color.rgb(p.dark)
                    },
                ),
            );
            job.append(&t.name, 6.0, egui::TextFormat::simple(font, p.text));
            let resp = ui.add(egui::Button::new(job));
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::CheckBox, true, all, &t.name)
            });
            if resp.clicked() {
                out = Some(TagEdit {
                    tag: t.clone(),
                    on: !all,
                });
                ui.close();
            }
        }
    }
    ui.separator();
    // A new tag: type its name, Return adds it.
    let id = new_tag_id();
    let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let resp = crate::widgets::show_input(
        ui,
        crate::widgets::input(&mut name)
            .desired_width(210.0)
            .id(id.with("field")),
        t!("tags.new_hint"),
    );
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let n = name.trim().to_string();
        if !n.is_empty() {
            let color = menu
                .standard
                .iter()
                .chain(&menu.custom)
                .find(|t| same_name(&t.name, &n))
                .map(|t| t.color)
                .or_else(|| TagColor::of_standard_name(&n))
                .unwrap_or_default();
            out = Some(TagEdit {
                tag: Tag::new(n, color),
                on: true,
            });
            name.clear();
            ui.close();
        }
    }
    ui.data_mut(|d| d.insert_temp(id, name));
    out
}

// ------------------------------------------------------------------ binary plist

/// A small binary property list (`bplist00`) reader and a writer for
/// arrays of strings — all the tags attribute and Finder's preferences
/// need.
pub mod bplist {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Int(i64),
        Real(f64),
        Date(f64),
        Data(Vec<u8>),
        String(String),
        Uid(u64),
        Array(Vec<Value>),
        Dict(Vec<(String, Value)>),
    }

    impl Value {
        /// A dictionary's value for `key`.
        pub fn get(&self, key: &str) -> Option<&Value> {
            match self {
                Value::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }
    }

    struct Reader<'a> {
        d: &'a [u8],
        offsets: Vec<usize>,
        ref_size: usize,
    }

    fn be(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64)
    }

    impl Reader<'_> {
        fn obj(&self, i: usize, depth: usize) -> Option<Value> {
            if depth > 32 {
                return None;
            }
            let off = *self.offsets.get(i)?;
            let marker = *self.d.get(off)?;
            let (hi, lo) = (marker >> 4, (marker & 0x0f) as usize);
            let body = off + 1;
            // Length of data / strings / arrays / dicts: in the low nibble,
            // or an int object after the marker when it is 0xF.
            let len = |at: usize| -> Option<(usize, usize)> {
                if lo != 0x0f {
                    return Some((lo, at));
                }
                let m = *self.d.get(at)?;
                if m >> 4 != 1 {
                    return None;
                }
                let n = 1usize << (m & 0x0f);
                let v = be(self.d.get(at + 1..at + 1 + n)?) as usize;
                Some((v, at + 1 + n))
            };
            Some(match hi {
                0x0 => match marker {
                    0x08 => Value::Bool(false),
                    0x09 => Value::Bool(true),
                    _ => Value::Null,
                },
                0x1 => {
                    let n = 1usize << lo;
                    let b = self.d.get(body..body + n)?;
                    Value::Int(if n == 8 {
                        i64::from_be_bytes(b.try_into().ok()?)
                    } else {
                        be(b) as i64
                    })
                }
                0x2 => {
                    let n = 1usize << lo;
                    let b = self.d.get(body..body + n)?;
                    Value::Real(match n {
                        4 => f32::from_be_bytes(b.try_into().ok()?) as f64,
                        8 => f64::from_be_bytes(b.try_into().ok()?),
                        _ => return None,
                    })
                }
                0x3 => Value::Date(f64::from_be_bytes(
                    self.d.get(body..body + 8)?.try_into().ok()?,
                )),
                0x4 => {
                    let (n, at) = len(body)?;
                    Value::Data(self.d.get(at..at + n)?.to_vec())
                }
                0x5 => {
                    let (n, at) = len(body)?;
                    Value::String(String::from_utf8_lossy(self.d.get(at..at + n)?).into_owned())
                }
                0x6 => {
                    let (n, at) = len(body)?;
                    let b = self.d.get(at..at + 2 * n)?;
                    let units: Vec<u16> = b
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_be_bytes(*c))
                        .collect();
                    Value::String(String::from_utf16_lossy(&units))
                }
                0x8 => Value::Uid(be(self.d.get(body..body + lo + 1)?)),
                0xA => {
                    let (n, at) = len(body)?;
                    let mut items = Vec::with_capacity(n.min(4096));
                    for k in 0..n {
                        let r = self.reference(at + k * self.ref_size)?;
                        items.push(self.obj(r, depth + 1)?);
                    }
                    Value::Array(items)
                }
                0xD => {
                    let (n, at) = len(body)?;
                    let mut items = Vec::with_capacity(n.min(4096));
                    for k in 0..n {
                        let kr = self.reference(at + k * self.ref_size)?;
                        let vr = self.reference(at + (n + k) * self.ref_size)?;
                        let key = match self.obj(kr, depth + 1)? {
                            Value::String(s) => s,
                            _ => return None,
                        };
                        items.push((key, self.obj(vr, depth + 1)?));
                    }
                    Value::Dict(items)
                }
                _ => return None,
            })
        }

        fn reference(&self, at: usize) -> Option<usize> {
            Some(be(self.d.get(at..at + self.ref_size)?) as usize)
        }
    }

    /// Parse a binary property list (`None` if it isn't one or is damaged).
    pub fn parse(d: &[u8]) -> Option<Value> {
        if d.len() < 40 || &d[..8] != b"bplist00" {
            return None;
        }
        let t = &d[d.len() - 32..];
        let off_size = t[6] as usize;
        let ref_size = t[7] as usize;
        let count = be(&t[8..16]) as usize;
        let top = be(&t[16..24]) as usize;
        let table = be(&t[24..32]) as usize;
        if !(1..=8).contains(&off_size) || !(1..=8).contains(&ref_size) || count > d.len() {
            return None;
        }
        let offsets = (0..count)
            .map(|i| {
                let at = table + i * off_size;
                d.get(at..at + off_size).map(|b| be(b) as usize)
            })
            .collect::<Option<Vec<_>>>()?;
        Reader {
            d,
            offsets,
            ref_size,
        }
        .obj(top, 0)
    }

    fn push_len(out: &mut Vec<u8>, kind: u8, n: usize) {
        if n < 15 {
            out.push(kind << 4 | n as u8);
        } else {
            out.push(kind << 4 | 0x0f);
            if n <= 0xff {
                out.push(0x10);
                out.push(n as u8);
            } else if n <= 0xffff {
                out.push(0x11);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            } else {
                out.push(0x12);
                out.extend_from_slice(&(n as u32).to_be_bytes());
            }
        }
    }

    fn int_size(max: usize) -> usize {
        if max <= 0xff {
            1
        } else if max <= 0xffff {
            2
        } else if max <= 0xffff_ffff {
            4
        } else {
            8
        }
    }

    /// A binary property list holding an array of strings (ASCII strings
    /// as such, the rest UTF-16, like Apple's writer).
    pub fn write_strings(strings: &[String]) -> Vec<u8> {
        let count = strings.len() + 1;
        let ref_size = int_size(count);
        let mut out = b"bplist00".to_vec();
        let mut offsets = Vec::with_capacity(count);
        offsets.push(out.len());
        push_len(&mut out, 0xA, strings.len());
        for i in 1..count {
            out.extend_from_slice(&(i as u64).to_be_bytes()[8 - ref_size..]);
        }
        for s in strings {
            offsets.push(out.len());
            if s.is_ascii() {
                push_len(&mut out, 0x5, s.len());
                out.extend_from_slice(s.as_bytes());
            } else {
                let units: Vec<u16> = s.encode_utf16().collect();
                push_len(&mut out, 0x6, units.len());
                for u in units {
                    out.extend_from_slice(&u.to_be_bytes());
                }
            }
        }
        let table = out.len();
        let off_size = int_size(table);
        for o in &offsets {
            out.extend_from_slice(&(*o as u64).to_be_bytes()[8 - off_size..]);
        }
        out.extend_from_slice(&[0; 6]);
        out.push(off_size as u8);
        out.push(ref_size as u8);
        out.extend_from_slice(&(count as u64).to_be_bytes());
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&(table as u64).to_be_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_strings() {
        assert_eq!(Tag::parse("Rot\n6"), Tag::new("Rot", TagColor::Red));
        assert_eq!(Tag::parse("Work"), Tag::new("Work", TagColor::None));
        assert_eq!(Tag::new("Blau", TagColor::Blue).encode(), "Blau\n4");
        assert_eq!(Tag::new("Work", TagColor::None).encode(), "Work");
        for i in 0..8 {
            assert_eq!(TagColor::from_index(i).index(), i);
        }
    }

    #[test]
    fn plist_round_trip() {
        let tags = vec![
            Tag::new("Red", TagColor::Red),
            Tag::new("Projekt Ü", TagColor::None),
            Tag::new(
                "a very long tag name with more than fifteen chars",
                TagColor::Green,
            ),
            Tag::new("项目", TagColor::Blue),
        ];
        let bytes = encode_tags(&tags);
        assert_eq!(&bytes[..8], b"bplist00");
        assert_eq!(decode_tags(&bytes), tags);
        // Many tags: two-byte lengths and references still parse.
        let many: Vec<Tag> = (0..300)
            .map(|i| Tag::new(format!("t{i}"), TagColor::None))
            .collect();
        assert_eq!(decode_tags(&encode_tags(&many)), many);
        assert!(decode_tags(b"garbage").is_empty());
        assert!(decode_tags(&encode_tags(&[])).is_empty());
    }

    /// The bytes Finder writes for the tags "Red" (red) and "Work".
    #[test]
    fn reads_finders_bytes() {
        let finder: &[u8] = &[
            0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xa2, 0x01, 0x02, 0x55, 0x52, 0x65,
            0x64, 0x0a, 0x36, 0x54, 0x57, 0x6f, 0x72, 0x6b, 0x08, 0x0b, 0x11, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x16,
        ];
        assert_eq!(
            decode_tags(finder),
            vec![
                Tag::new("Red", TagColor::Red),
                Tag::new("Work", TagColor::None)
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn xattr_on_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("report.pdf");
        std::fs::write(&f, b"x").unwrap();
        let d = tmp.path().join("folder");
        std::fs::create_dir(&d).unwrap();
        assert!(read_tags(&f).unwrap().is_empty());
        let red = Tag::new("Rot", TagColor::Red);
        let work = Tag::new("Arbeit", TagColor::None);
        set_tag(&f, &red, true).unwrap();
        set_tag(&f, &work, true).unwrap();
        set_tag(&f, &red, true).unwrap(); // twice: still once
        assert_eq!(read_tags(&f).unwrap(), vec![red.clone(), work.clone()]);
        set_tag(&d, &work, true).unwrap();
        // What `xattr` sees is a real binary plist.
        let raw = get_xattr(&f, XATTR).unwrap().unwrap();
        assert_eq!(&raw[..8], b"bplist00");
        let folder = read_folder(tmp.path());
        assert_eq!(folder.len(), 2);
        assert_eq!(folder["folder"], vec![work.clone()]);
        // Removing the last tag removes the attribute.
        set_tag(&f, &red, false).unwrap();
        set_tag(&f, &Tag::new("ARBEIT", TagColor::None), false).unwrap();
        assert!(get_xattr(&f, XATTR).unwrap().is_none());
        assert!(read_tags(&f).unwrap().is_empty());
        // `xattr -w` (what scripts and other apps do) is read the same way.
        let st = std::process::Command::new("/usr/bin/xattr")
            .args(["-wx", XATTR])
            .arg(hex(&encode_tags(&[Tag::new("Blau", TagColor::Blue)])))
            .arg(&f)
            .status();
        if st.is_ok_and(|s| s.success()) {
            assert_eq!(
                read_tags(&f).unwrap(),
                vec![Tag::new("Blau", TagColor::Blue)]
            );
        }
    }

    #[cfg(target_os = "macos")]
    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn filter_terms() {
        assert_eq!(
            split_filter("tag:Rot"),
            (vec!["rot".to_string()], String::new())
        );
        assert_eq!(
            split_filter("report TAG:blau x"),
            (vec!["blau".to_string()], "report x".to_string())
        );
        assert_eq!(
            split_filter("tag:\"Sehr wichtig\" pdf"),
            (vec!["sehr wichtig".to_string()], "pdf".to_string())
        );
        assert_eq!(split_filter("plain"), (vec![], "plain".to_string()));
        let tags = [
            Tag::new("Rot", TagColor::Red),
            Tag::new("Work", TagColor::None),
        ];
        assert!(matches_tags(&tags, &["rot".into()]));
        assert!(matches_tags(&tags, &["red".into()]));
        assert!(matches_tags(&tags, &["work".into(), "rot".into()]));
        assert!(!matches_tags(&tags, &["blau".into()]));
        assert!(!matches_tags(&[], &["rot".into()]));
    }

    #[test]
    fn finder_preferences() {
        let tmp = tempfile::tempdir().unwrap();
        // FavoriteTagNames: "", Red, Work (a custom tag).
        let prefs = tmp.path().join("com.apple.finder.plist");
        let names = vec!["".to_string(), "Red".into(), "Work".into(), "Lila".into()];
        let mut dict = b"bplist00".to_vec();
        // A dict { "FavoriteTagNames": [..] } built by hand: keys/values
        // reference the array written by write_strings is easier: wrap it.
        let arr = bplist::write_strings(&names);
        // Parse the array, then check the reader on a dict made below.
        assert_eq!(
            bplist::parse(&arr),
            Some(bplist::Value::Array(
                names.iter().cloned().map(bplist::Value::String).collect()
            ))
        );
        // Dict: obj0 = dict(1 key), obj1 = "FavoriteTagNames", obj2 = array
        // of obj3.. strings.
        let mut offsets = Vec::new();
        offsets.push(dict.len());
        dict.extend_from_slice(&[0xD1, 1, 2]);
        offsets.push(dict.len());
        dict.push(0x5F);
        dict.extend_from_slice(&[0x10, 16]);
        dict.extend_from_slice(b"FavoriteTagNames");
        offsets.push(dict.len());
        dict.push(0xA0 | names.len() as u8);
        for i in 0..names.len() {
            dict.push(3 + i as u8);
        }
        for n in &names {
            offsets.push(dict.len());
            dict.push(0x50 | n.len() as u8);
            dict.extend_from_slice(n.as_bytes());
        }
        let table = dict.len();
        for o in &offsets {
            dict.push(*o as u8);
        }
        dict.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
        dict.extend_from_slice(&(offsets.len() as u64).to_be_bytes());
        dict.extend_from_slice(&0u64.to_be_bytes());
        dict.extend_from_slice(&(table as u64).to_be_bytes());
        std::fs::write(&prefs, &dict).unwrap();
        let tags = finder_tags_from(&prefs, &tmp.path().join("missing.plist"));
        assert_eq!(
            tags,
            vec![
                Tag::new("Red", TagColor::Red),
                Tag::new("Work", TagColor::None),
                Tag::new("Lila", TagColor::Purple),
            ]
        );
        assert!(finder_tags_from(&tmp.path().join("x"), &tmp.path().join("y")).is_empty());
    }

    #[test]
    fn store_uses_finders_names_for_the_colours() {
        let store = TagStore::new(
            Box::new(|| {
                vec![
                    Tag::new("Red", TagColor::Red),
                    Tag::new("Work", TagColor::Blue),
                ]
            }),
            true,
        );
        assert_eq!(
            store.standard(TagColor::Red),
            Tag::new("Red", TagColor::Red)
        );
        // Not in Finder's list: the colour's name in the UI language.
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            assert_eq!(store.standard(TagColor::Green).name, "Grün");
        });
        assert_eq!(store.color_of("work"), TagColor::Blue);
        assert_eq!(store.color_of("Gelb"), TagColor::Yellow);
        let red = [Tag::new("Red", TagColor::Red)];
        let menu = TagMenu::new(&KnownTags::of(&store), &[&red, &[]]);
        assert_eq!(menu.items, 2);
        assert_eq!(menu.count(&Tag::new("red", TagColor::Red)), 1);
        assert_eq!(menu.standard.len(), 7);
        assert_eq!(menu.custom, vec![Tag::new("Work", TagColor::Blue)]);
    }
}
