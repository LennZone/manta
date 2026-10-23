//! Measuring folders on the local file system.
//!
//! [`measure`] walks a folder without following symlinks, counts every hard
//! linked file once (by device and inode), stays on the folder's own volume
//! and can be cancelled at any time. It reports partial totals while it
//! runs.
//!
//! **What "size" means here:** the *logical* size, the sum of file lengths
//! (`st_size`), the same number the file list shows for files. The
//! allocated size on disk (`st_blocks × 512`, Finder's "on disk") is
//! counted too and shown in tooltips. Both differ for sparse files, APFS
//! clones and compressed files.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::i18n::tr;

/// File categories for colouring (treemap, legend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Images,
    Video,
    Audio,
    Documents,
    Code,
    Archives,
    Apps,
    Other,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Images,
        Category::Video,
        Category::Audio,
        Category::Documents,
        Category::Code,
        Category::Archives,
        Category::Apps,
        Category::Other,
    ];
    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Images => tr!("cat.images"),
            Category::Video => tr!("cat.video"),
            Category::Audio => tr!("cat.audio"),
            Category::Documents => tr!("cat.documents"),
            Category::Code => tr!("cat.code"),
            Category::Archives => tr!("cat.archives"),
            Category::Apps => tr!("cat.apps"),
            Category::Other => tr!("cat.misc"),
        }
    }

    /// Category of a file (or bundle folder) by its name.
    pub fn of_name(name: &str) -> Category {
        let ext = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
            _ => return Category::Other,
        };
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "heic" | "heif" | "webp" | "tif" | "tiff" | "bmp"
            | "svg" | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng" | "psd" | "ico" | "icns"
            | "avif" | "sketch" | "fig" => Category::Images,
            "mov" | "mp4" | "m4v" | "mkv" | "avi" | "webm" | "wmv" | "flv" | "mpg" | "mpeg"
            | "3gp" => Category::Video,
            "mp3" | "m4a" | "aac" | "wav" | "aiff" | "aif" | "flac" | "ogg" | "opus" | "caf"
            | "mid" | "midi" | "logicx" => Category::Audio,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers"
            | "key" | "txt" | "md" | "rtf" | "odt" | "ods" | "csv" | "epub" | "tex" => {
                Category::Documents
            }
            "rs" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "c" | "h" | "cc"
            | "cpp" | "hpp" | "m" | "mm" | "swift" | "go" | "java" | "kt" | "rb" | "php" | "cs"
            | "sh" | "zsh" | "lua" | "json" | "toml" | "yaml" | "yml" | "xml" | "html" | "css"
            | "scss" | "sql" | "map" | "lock" | "o" | "a" | "rlib" | "rmeta" | "d" | "class"
            | "jar" | "wasm" | "pyc" => Category::Code,
            "zip" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "tar" | "dmg" | "iso"
            | "pkg" | "sparseimage" | "sparsebundle" | "img" | "xip" => Category::Archives,
            "app" | "framework" | "dylib" | "so" | "exe" | "appex" | "bundle" | "kext"
            | "plugin" => Category::Apps,
            _ => Category::Other,
        }
    }
}

/// What [`measure`] counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    /// Sum of file lengths.
    pub logical: u64,
    /// Allocated blocks × 512 (files and folders).
    pub on_disk: u64,
    /// Files, symlinks and other non-folders (hard links once).
    pub files: u64,
    /// Sub-folders (not counting the measured folder itself).
    pub dirs: u64,
    /// Folders (or entries) that could not be read.
    pub unreadable: u64,
    /// Logical bytes per [`Category`].
    pub by_cat: [u64; Category::COUNT],
    /// The walk finished: not cancelled. Partial totals have `false`.
    pub done: bool,
    /// The walk was cancelled before it finished.
    pub cancelled: bool,
}

impl Totals {
    /// Finished and everything was readable.
    pub fn complete(&self) -> bool {
        self.done && !self.cancelled && self.unreadable == 0
    }

    /// The category with the most bytes (`Other` if empty).
    pub fn dominant(&self) -> Category {
        let mut best = Category::Other;
        let mut max = 0;
        for c in Category::ALL {
            if self.by_cat[c.index()] > max {
                max = self.by_cat[c.index()];
                best = c;
            }
        }
        best
    }
}

/// How often [`measure`] reports partial totals.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

/// Walk `root` and add up what is below it. `progress` gets partial totals
/// at most every [`PROGRESS_INTERVAL`]. Returns early (with `cancelled`)
/// once `cancel` is set.
///
/// - Symlinks are counted as small files and never followed.
/// - Hard-linked files (`nlink > 1`) are counted once.
/// - Folders on another volume (mount points: `st_dev` differs from the
///   root's) are skipped.
/// - Unreadable folders are counted in `unreadable`; the rest is measured.
pub fn measure(root: &Path, cancel: &AtomicBool, mut progress: impl FnMut(&Totals)) -> Totals {
    let mut t = Totals::default();
    let meta = match fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(_) => {
            t.unreadable = 1;
            t.done = true;
            return t;
        }
    };
    if !meta.is_dir() {
        t.files = 1;
        t.logical = meta.len();
        t.on_disk = meta.blocks() * 512;
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        t.by_cat[Category::of_name(&name).index()] = meta.len();
        t.done = true;
        return t;
    }
    let dev = meta.dev();
    t.on_disk = meta.blocks() * 512;
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    // Files inside app bundles (Foo.app/…) count as Apps.
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let root_cat = (Category::of_name(&root_name) == Category::Apps).then_some(Category::Apps);
    let mut stack: Vec<(PathBuf, Option<Category>)> = vec![(root.to_path_buf(), root_cat)];
    let mut last = Instant::now();
    let mut n: u32 = 0;
    'walk: while let Some((dir, bundle)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            t.cancelled = true;
            break;
        }
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => {
                t.unreadable += 1;
                continue;
            }
        };
        for ent in rd {
            n = n.wrapping_add(1);
            if n.is_multiple_of(256) {
                if cancel.load(Ordering::Relaxed) {
                    t.cancelled = true;
                    break 'walk;
                }
                if last.elapsed() >= PROGRESS_INTERVAL {
                    last = Instant::now();
                    progress(&t);
                }
            }
            let Ok(ent) = ent else {
                t.unreadable += 1;
                continue;
            };
            // `DirEntry::metadata` does not follow symlinks.
            let Ok(md) = ent.metadata() else { continue };
            if md.dev() != dev {
                continue; // another volume mounted here
            }
            if md.is_dir() {
                t.dirs += 1;
                t.on_disk += md.blocks() * 512;
                let inner = bundle.or_else(|| {
                    (Category::of_name(&ent.file_name().to_string_lossy()) == Category::Apps)
                        .then_some(Category::Apps)
                });
                stack.push((ent.path(), inner));
                continue;
            }
            if md.nlink() > 1 && !seen.insert((md.dev(), md.ino())) {
                continue; // another link to a file counted already
            }
            t.files += 1;
            t.logical += md.len();
            t.on_disk += md.blocks() * 512;
            let cat =
                bundle.unwrap_or_else(|| Category::of_name(&ent.file_name().to_string_lossy()));
            t.by_cat[cat.index()] += md.len();
        }
    }
    t.done = !t.cancelled;
    t
}

/// Cache key of a folder: its own mtime and ctime (nanoseconds).
pub type StampKey = (i128, i128);

pub fn stamp(md: &fs::Metadata) -> StampKey {
    (
        md.mtime() as i128 * 1_000_000_000 + md.mtime_nsec() as i128,
        md.ctime() as i128 * 1_000_000_000 + md.ctime_nsec() as i128,
    )
}

/// One direct child of a listed folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Child {
    pub name: String,
    pub path: PathBuf,
    /// A real folder (not a symlink to one).
    pub is_dir: bool,
    /// Bundle folders like `.app` are measured but shown as one item.
    pub category: Category,
    /// Files: logical size; folders: 0 (measured separately).
    pub logical: u64,
    pub on_disk: u64,
    pub stamp: StampKey,
}

/// List the direct children of `dir` (without following symlinks).
pub fn list_dir(dir: &Path) -> std::io::Result<Vec<Child>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(dir)? {
        let Ok(ent) = ent else { continue };
        let Ok(md) = ent.metadata() else { continue };
        let name = ent.file_name().to_string_lossy().into_owned();
        let is_dir = md.is_dir();
        out.push(Child {
            category: Category::of_name(&name),
            path: ent.path(),
            is_dir,
            logical: if is_dir { 0 } else { md.len() },
            on_disk: if is_dir { 0 } else { md.blocks() * 512 },
            stamp: stamp(&md),
            name,
        });
    }
    Ok(out)
}

/// The cache key of a folder now (`None` if it is gone).
pub fn stamp_of(path: &Path) -> Option<StampKey> {
    fs::symlink_metadata(path).ok().map(|m| stamp(&m))
}
