//! Icon set: a vendored copy of the Phosphor icon font (MIT, see
//! `assets/fonts/Phosphor-LICENSE-MIT.txt`), the codepoints we use, and the
//! mapping from directory entries to icon + tint.
//!
//! Draw icons with [`theme::icon_font`] (outline) or [`theme::icon_fill_font`]
//! (filled), or mix them into ordinary text (Phosphor is a fallback of the
//! proportional fonts).

#![allow(dead_code)]

use eframe::egui::Color32;
use manta_core::{Entry, EntryKind};

use crate::theme::Palette;

pub const ARCHIVE: &str = "\u{E00C}";
pub const ARROW_CLOCKWISE: &str = "\u{E036}";
pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{E038}";
pub const ARROW_LEFT: &str = "\u{E058}";
pub const ARROW_LINE_LEFT: &str = "\u{E062}";
pub const ARROW_LINE_RIGHT: &str = "\u{E064}";
pub const ARROW_RIGHT: &str = "\u{E06C}";
pub const ARROW_UP: &str = "\u{E08E}";
pub const ARROW_U_UP_LEFT: &str = "\u{E08A}";
/// The Return key (↩ is missing from SF Pro and falls back to an emoji).
pub const RETURN: &str = "\u{E07E}";
pub const ARROWS_CLOCKWISE: &str = "\u{E094}";
pub const ARROWS_DOWN_UP: &str = "\u{E098}";
pub const ARROWS_LEFT_RIGHT: &str = "\u{E0A0}";
pub const ARROW_SQUARE_OUT: &str = "\u{E5DE}";
pub const BROOM: &str = "\u{EC54}";
pub const CARET_DOWN: &str = "\u{E136}";
pub const CARET_LEFT: &str = "\u{E138}";
pub const CARET_RIGHT: &str = "\u{E13A}";
pub const CARET_UP: &str = "\u{E13C}";
pub const CHECK: &str = "\u{E182}";
pub const CHECK_CIRCLE: &str = "\u{E184}";
pub const CIRCLE: &str = "\u{E18A}";
pub const CLOCK: &str = "\u{E19A}";
pub const CLOUD: &str = "\u{E1AA}";
pub const CLOUD_ARROW_DOWN: &str = "\u{E1AC}";
pub const CLOUD_ARROW_UP: &str = "\u{E1AE}";
pub const CLOUD_CHECK: &str = "\u{E1B0}";
pub const CLOUD_SLASH: &str = "\u{E1B6}";
pub const CLOUD_WARNING: &str = "\u{EA98}";
pub const ARROW_CIRCLE_DOWN: &str = "\u{E028}";
pub const DROPBOX_LOGO: &str = "\u{E7D0}";
pub const GOOGLE_DRIVE_LOGO: &str = "\u{E8F6}";
pub const CODE: &str = "\u{E1BC}";
pub const COMMAND: &str = "\u{E1C4}";
pub const COPY: &str = "\u{E1CA}";
pub const COLUMNS_PLUS_RIGHT: &str = "\u{E542}";
pub const COLUMNS: &str = "\u{E546}";
pub const DESKTOP: &str = "\u{E560}";
pub const DOTS_THREE: &str = "\u{E1FE}";
pub const DOWNLOAD_SIMPLE: &str = "\u{E20C}";
pub const EJECT: &str = "\u{E212}";
pub const EQUALS: &str = "\u{E21C}";
pub const EYE: &str = "\u{E220}";
pub const EYE_SLASH: &str = "\u{E224}";
/// Hidden files (a dashed file): not an eye, which is Quick Look.
pub const FILE_DASHED: &str = "\u{E704}";
pub const FILE: &str = "\u{E230}";
pub const FILE_ARCHIVE: &str = "\u{EB2A}";
pub const FILE_AUDIO: &str = "\u{EA20}";
pub const FILE_CODE: &str = "\u{E914}";
pub const FILE_CSV: &str = "\u{EB1C}";
pub const FILE_DOC: &str = "\u{EB1E}";
pub const FILE_IMAGE: &str = "\u{EA24}";
pub const FILE_PDF: &str = "\u{E702}";
pub const FILE_TEXT: &str = "\u{E23A}";
pub const FILE_VIDEO: &str = "\u{EA22}";
pub const FILE_XLS: &str = "\u{EB22}";
pub const FILE_PPT: &str = "\u{EB20}";
pub const FOLDER: &str = "\u{E24A}";
pub const FOLDER_DASHED: &str = "\u{E8F8}";
pub const FOLDER_OPEN: &str = "\u{E256}";
pub const FOLDER_PLUS: &str = "\u{E258}";
pub const FOLDER_SIMPLE: &str = "\u{E25A}";
pub const FOLDER_SIMPLE_STAR: &str = "\u{EC2E}";
pub const FUNNEL_SIMPLE: &str = "\u{E268}";
pub const GEAR_SIX: &str = "\u{E272}";
pub const GIT_BRANCH: &str = "\u{E278}";
pub const GLOBE: &str = "\u{E288}";
pub const HARD_DRIVE: &str = "\u{E29E}";
pub const HARD_DRIVES: &str = "\u{E2A0}";
pub const HOUSE: &str = "\u{E2C2}";
pub const INFO: &str = "\u{E2CE}";
pub const QUESTION: &str = "\u{E3E8}";
pub const SCROLL: &str = "\u{EB7A}";
pub const BOOK_OPEN: &str = "\u{E0E6}";
pub const KEY: &str = "\u{E2D6}";
pub const KEYBOARD: &str = "\u{E2D8}";
pub const LINK: &str = "\u{E2E2}";
pub const LINK_SIMPLE: &str = "\u{E2E6}";
pub const LOCK_SIMPLE: &str = "\u{E308}";
pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
pub const NOT_EQUALS: &str = "\u{EDA6}";
pub const PACKAGE: &str = "\u{E390}";
pub const PENCIL_SIMPLE: &str = "\u{E3B4}";
pub const PLUG: &str = "\u{E946}";
pub const PLUGS: &str = "\u{EB56}";
pub const PLUGS_CONNECTED: &str = "\u{EB5A}";
pub const PLUS: &str = "\u{E3D4}";
pub const POWER: &str = "\u{E3DA}";
pub const PUZZLE_PIECE: &str = "\u{E596}";
pub const SCISSORS: &str = "\u{EAE0}";
/// Go to Path (a signpost).
pub const SHIELD_WARNING: &str = "\u{E412}";
pub const SIDEBAR_SIMPLE: &str = "\u{EC24}";
pub const SIGNPOST: &str = "\u{E89C}";
pub const SPINNER: &str = "\u{E66A}";
pub const STAR: &str = "\u{E46A}";
pub const STOP: &str = "\u{E46C}";
pub const TERMINAL: &str = "\u{E47E}";
pub const TERMINAL_WINDOW: &str = "\u{EAE8}";
pub const PLAY: &str = "\u{E3D0}";
pub const DATABASE: &str = "\u{E1DE}";
pub const CUBE: &str = "\u{E1DA}";
/// System Load plugin: processor, graphics card, memory module.
pub const CPU: &str = "\u{E610}";
pub const GRAPHICS_CARD: &str = "\u{E612}";
pub const MEMORY: &str = "\u{E9C4}";
/// Docker containers.
pub const SHIPPING_CONTAINER: &str = "\u{E78C}";
/// Kubernetes (its logo is a helm).
pub const STEERING_WHEEL: &str = "\u{E9AC}";
pub const STACK: &str = "\u{E466}";
pub const HAMMER: &str = "\u{E80E}";
pub const FLASK: &str = "\u{E79E}";
pub const ROCKET_LAUNCH: &str = "\u{E3FE}";
pub const LIGHTNING: &str = "\u{E2DE}";
pub const LIST_BULLETS: &str = "\u{E2F2}";
pub const LIST: &str = "\u{E2F0}";
pub const SQUARES_FOUR: &str = "\u{E464}";
/// The Gallery view (a slide between two edges).
pub const SLIDESHOW: &str = "\u{ED32}";
pub const SLIDERS_HORIZONTAL: &str = "\u{E434}";
pub const IMAGE: &str = "\u{E2CA}";
pub const WRENCH: &str = "\u{E5D4}";
pub const BUG: &str = "\u{E5F4}";
pub const TRASH: &str = "\u{E4A6}";
pub const SUN: &str = "\u{E472}";
pub const MOON: &str = "\u{E330}";
pub const CIRCLE_HALF: &str = "\u{E1D8}";
pub const USER: &str = "\u{E4C2}";
pub const WARNING: &str = "\u{E4E0}";
pub const WARNING_CIRCLE: &str = "\u{E4E2}";
pub const X: &str = "\u{E4F6}";
pub const X_CIRCLE: &str = "\u{E4F8}";

/// Broad file type, used for icon and tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Folder,
    Image,
    Video,
    Audio,
    Archive,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Code,
    Text,
    Script,
    Other,
}

pub fn extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn file_kind(name: &str, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Folder;
    }
    match extension(name).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "heic" | "tif" | "tiff" | "svg"
        | "ico" | "psd" => FileKind::Image,
        "mov" | "mp4" | "m4v" | "mkv" | "avi" | "webm" => FileKind::Video,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "aiff" => FileKind::Audio,
        "zip" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "tar" | "dmg" | "pkg" | "zst" => {
            FileKind::Archive
        }
        "pdf" => FileKind::Pdf,
        "doc" | "docx" | "pages" | "rtf" | "odt" => FileKind::Document,
        "xls" | "xlsx" | "numbers" | "csv" | "ods" => FileKind::Spreadsheet,
        "ppt" | "pptx" | "key" | "odp" => FileKind::Presentation,
        "rs" | "c" | "h" | "cpp" | "hpp" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "java"
        | "kt" | "swift" | "rb" | "php" | "html" | "css" | "scss" | "json" | "toml" | "yaml"
        | "yml" | "xml" | "sql" | "lock" => FileKind::Code,
        "sh" | "zsh" | "bash" | "fish" | "command" => FileKind::Script,
        "txt" | "md" | "markdown" | "log" | "env" | "ini" | "conf" | "cfg" => FileKind::Text,
        _ => FileKind::Other,
    }
}

impl FileKind {
    pub fn icon(self) -> &'static str {
        match self {
            FileKind::Folder => FOLDER,
            FileKind::Image => FILE_IMAGE,
            FileKind::Video => FILE_VIDEO,
            FileKind::Audio => FILE_AUDIO,
            FileKind::Archive => FILE_ARCHIVE,
            FileKind::Pdf => FILE_PDF,
            FileKind::Document => FILE_DOC,
            FileKind::Spreadsheet => FILE_XLS,
            FileKind::Presentation => FILE_PPT,
            FileKind::Code => FILE_CODE,
            FileKind::Text => FILE_TEXT,
            FileKind::Script => TERMINAL_WINDOW,
            FileKind::Other => FILE,
        }
    }

    pub fn tint(self, p: &Palette) -> Color32 {
        match self {
            FileKind::Folder => p.folder,
            FileKind::Image => p.image,
            FileKind::Video | FileKind::Audio => p.media,
            FileKind::Archive => p.archive,
            FileKind::Pdf | FileKind::Presentation => p.document,
            FileKind::Spreadsheet | FileKind::Code | FileKind::Script => p.code,
            FileKind::Document => p.folder,
            FileKind::Text | FileKind::Other => p.generic,
        }
    }

    /// Human-readable kind, e.g. for the preview header.
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Folder => t!("kind.folder"),
            FileKind::Image => t!("kind.image"),
            FileKind::Video => t!("kind.video"),
            FileKind::Audio => t!("kind.audio"),
            FileKind::Archive => t!("kind.archive"),
            FileKind::Pdf => t!("kind.pdf"),
            FileKind::Document => t!("kind.document"),
            FileKind::Spreadsheet => t!("kind.spreadsheet"),
            FileKind::Presentation => t!("kind.presentation"),
            FileKind::Code => t!("kind.code"),
            FileKind::Text => t!("kind.text"),
            FileKind::Script => t!("kind.script"),
            FileKind::Other => t!("kind.file"),
        }
    }
}

/// Icon, tint and whether to use the filled variant for an entry. Symlinks
/// use their target's look (folder if the target looks like a folder name)
/// and get a link badge from the caller.
pub fn entry_icon(e: &Entry, p: &Palette) -> (&'static str, Color32, bool) {
    let kind = match e.kind {
        EntryKind::Directory => FileKind::Folder,
        EntryKind::Symlink => {
            // We don't stat the target on the UI thread; guess from its name.
            let target = e.link_target.as_deref().unwrap_or(&e.name);
            let k = file_kind(target, false);
            if k == FileKind::Other && !target.contains('.') {
                FileKind::Folder
            } else {
                k
            }
        }
        EntryKind::File => file_kind(&e.name, false),
        EntryKind::Other => FileKind::Other,
    };
    (kind.icon(), kind.tint(p), kind == FileKind::Folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(file_kind("Photo.JPG", false), FileKind::Image);
        assert_eq!(file_kind("main.rs", false), FileKind::Code);
        assert_eq!(file_kind("backup.tar.gz", false), FileKind::Archive);
        assert_eq!(file_kind(".env", false), FileKind::Other);
        assert_eq!(file_kind("README", false), FileKind::Other);
        assert_eq!(file_kind("src", true), FileKind::Folder);
    }
}
