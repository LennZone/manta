//! Archives as read-only folders: zip (and zip-based formats such as jar,
//! apk, docx), tar, compressed tar (gz, bz2, xz, zst) and single compressed
//! files (`.gz`, `.bz2`, `.xz`, `.zst`).
//!
//! [`ArchiveFs::open`] indexes the archive once and builds a directory tree
//! from the entry list:
//! - missing parent folders are synthesised;
//! - `\` separators and `./` prefixes are normalised;
//! - entries with `..` components, absolute paths or drive letters are
//!   refused (listed in [`ArchiveFs::rejected`]), so nothing can escape the
//!   archive root ("zip-slip");
//! - symlinks stay symlinks (`EntryKind::Symlink` with `link_target`); they
//!   are followed only *inside* the archive (an absolute target or one that
//!   climbs above the root can't be read);
//! - mtimes, permissions and sizes come from the archive.
//!
//! Reading:
//! - zip: random access through the central directory. Small entries are
//!   decompressed in place, large ones are streamed by a helper thread.
//!   Encrypted entries are listed (see [`ArchiveFs::is_encrypted`]) but
//!   reading them fails with `PermissionDenied`.
//! - tar: indexed once on open (data offsets). A compressed tar is
//!   decompressed while indexing into a temporary file (up to
//!   [`OpenOptions::spool_limit`] bytes), so later reads are plain seeks. Above
//!   the limit every read decompresses the stream from the start.
//!
//! All write operations return `VfsError::Unsupported("archive is read-only")`.
//! Temporary files (the spooled tar, a downloaded archive handed over with
//! [`ArchiveFs::adopt_temp`]) are deleted when the `ArchiveFs` is dropped.

use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use manta_core::{Entry, EntryKind, Protocol, Vfs, VfsError, VfsPath, VfsResult};

const READ_ONLY: &str = "archive is read-only";
/// Zip entries up to this size are decompressed in place on `open_read`.
const SMALL_ENTRY: u64 = 4 * 1024 * 1024;
/// Chunk size of the zip streaming thread.
const CHUNK: usize = 256 * 1024;
/// Default for [`OpenOptions::spool_limit`].
pub const DEFAULT_SPOOL_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
/// Prefix of every temporary file the archive code creates.
pub const TEMP_PREFIX: &str = "manta-archive-";

// ------------------------------------------------------------------ formats

/// The archive formats Manta can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    /// A single gzip-compressed file (shown as the one decompressed file).
    Gz,
    Bz2,
    Xz,
    Zst,
}

/// Zip-based container formats. They are archives, but people usually open
/// them with their app, so the GUI browses them only on request.
const ZIP_CONTAINERS: &[&str] = &[
    ".jar", ".war", ".ear", ".apk", ".aar", ".ipa", ".docx", ".xlsx", ".pptx", ".odt", ".ods",
    ".odp", ".epub", ".whl", ".nupkg", ".vsix", ".xpi", ".crx",
];

impl ArchiveFormat {
    /// The format of a file, from its name (case-insensitive).
    pub fn detect(name: &str) -> Option<ArchiveFormat> {
        let n = name.to_lowercase();
        let ends = |sfx: &str| n.len() > sfx.len() && n.ends_with(sfx);
        Some(if ends(".tar.gz") || ends(".tgz") {
            ArchiveFormat::TarGz
        } else if ends(".tar.bz2") || ends(".tbz2") || ends(".tbz") {
            ArchiveFormat::TarBz2
        } else if ends(".tar.xz") || ends(".txz") {
            ArchiveFormat::TarXz
        } else if ends(".tar.zst") || ends(".tzst") || ends(".tar.zstd") {
            ArchiveFormat::TarZst
        } else if ends(".tar") {
            ArchiveFormat::Tar
        } else if ends(".zip") || ZIP_CONTAINERS.iter().any(|s| ends(s)) {
            ArchiveFormat::Zip
        } else if ends(".gz") {
            ArchiveFormat::Gz
        } else if ends(".bz2") {
            ArchiveFormat::Bz2
        } else if ends(".xz") {
            ArchiveFormat::Xz
        } else if ends(".zst") || ends(".zstd") {
            ArchiveFormat::Zst
        } else {
            return None;
        })
    }

    /// True for plain archives that open as folders on Enter (zip, tar,
    /// compressed tar, single compressed files) — not for zip-based
    /// document/app formats such as `.docx` or `.jar`.
    pub fn opens_as_folder(name: &str) -> bool {
        let n = name.to_lowercase();
        Self::detect(name).is_some() && !ZIP_CONTAINERS.iter().any(|s| n.ends_with(s))
    }

    /// The name without its archive extension: "backup.tar.gz" → "backup",
    /// "notes.txt.gz" → "notes.txt".
    pub fn stem(name: &str) -> &str {
        let lower = name.to_lowercase();
        const EXTS: &[&str] = &[
            ".tar.gz",
            ".tar.bz2",
            ".tar.xz",
            ".tar.zst",
            ".tar.zstd",
            ".tgz",
            ".tbz2",
            ".tbz",
            ".txz",
            ".tzst",
            ".tar",
            ".zip",
            ".gz",
            ".bz2",
            ".xz",
            ".zst",
            ".zstd",
        ];
        for ext in EXTS.iter().chain(ZIP_CONTAINERS) {
            if lower.ends_with(ext)
                && name.len() > ext.len()
                && name.is_char_boundary(name.len() - ext.len())
            {
                return &name[..name.len() - ext.len()];
            }
        }
        name
    }

    /// Short name, e.g. "ZIP", "tar.gz".
    pub fn label(self) -> &'static str {
        match self {
            ArchiveFormat::Zip => "ZIP",
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::TarBz2 => "tar.bz2",
            ArchiveFormat::TarXz => "tar.xz",
            ArchiveFormat::TarZst => "tar.zst",
            ArchiveFormat::Gz => "gzip",
            ArchiveFormat::Bz2 => "bzip2",
            ArchiveFormat::Xz => "xz",
            ArchiveFormat::Zst => "zstd",
        }
    }

    fn codec(self) -> Codec {
        match self {
            ArchiveFormat::Zip | ArchiveFormat::Tar => Codec::None,
            ArchiveFormat::TarGz | ArchiveFormat::Gz => Codec::Gz,
            ArchiveFormat::TarBz2 | ArchiveFormat::Bz2 => Codec::Bz2,
            ArchiveFormat::TarXz | ArchiveFormat::Xz => Codec::Xz,
            ArchiveFormat::TarZst | ArchiveFormat::Zst => Codec::Zst,
        }
    }

    fn is_tar(self) -> bool {
        matches!(
            self,
            ArchiveFormat::Tar
                | ArchiveFormat::TarGz
                | ArchiveFormat::TarBz2
                | ArchiveFormat::TarXz
                | ArchiveFormat::TarZst
        )
    }

    /// The extension a single compressed file carries (".gz" …).
    fn single_ext(self) -> &'static [&'static str] {
        match self {
            ArchiveFormat::Gz => &[".gz"],
            ArchiveFormat::Bz2 => &[".bz2"],
            ArchiveFormat::Xz => &[".xz"],
            ArchiveFormat::Zst => &[".zst", ".zstd"],
            _ => &[],
        }
    }
}

/// A stream compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    None,
    Gz,
    Bz2,
    Xz,
    Zst,
}

impl Codec {
    fn decoder<'a>(self, r: impl Read + Send + 'a) -> io::Result<Box<dyn Read + Send + 'a>> {
        Ok(match self {
            Codec::None => Box::new(r),
            Codec::Gz => Box::new(flate2::read::MultiGzDecoder::new(r)),
            Codec::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(r)),
            Codec::Xz => Box::new(lzma_rust2::XzReader::new(r, true)),
            Codec::Zst => Box::new(zstd::stream::read::Decoder::new(r)?),
        })
    }
}

// ------------------------------------------------------------------ paths

/// Why an entry name was refused.
fn sanitize(raw: &str) -> Result<Vec<String>, &'static str> {
    let name = raw.replace('\\', "/");
    if name.starts_with('/') {
        return Err("absolute path");
    }
    let b = name.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return Err("absolute path (drive letter)");
    }
    let mut out = Vec::new();
    for c in name.split('/') {
        match c {
            "" | "." => {}
            ".." => return Err("path leaves the archive (..)"),
            c if c.contains('\0') => return Err("invalid name"),
            c => out.push(c.to_string()),
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ index

#[derive(Debug, Clone, Copy)]
enum Data {
    None,
    /// Index in the zip central directory.
    Zip(usize),
    /// Bytes `offset..offset + len` of the (decompressed) stream.
    Range {
        offset: u64,
        len: u64,
    },
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    kind: EntryKind,
    size: u64,
    mtime: Option<DateTime<Utc>>,
    mode: Option<u32>,
    link: Option<String>,
    encrypted: bool,
    data: Data,
    parent: usize,
    children: BTreeMap<String, usize>,
    /// Listed in the archive (not synthesised as a missing parent).
    explicit: bool,
}

impl Node {
    fn dir(name: String, parent: usize) -> Node {
        Node {
            name,
            kind: EntryKind::Directory,
            size: 0,
            mtime: None,
            mode: None,
            link: None,
            encrypted: false,
            data: Data::None,
            parent,
            children: BTreeMap::new(),
            explicit: false,
        }
    }
}

/// What the index builder gets per archive entry.
struct RawEntry {
    name: String,
    kind: EntryKind,
    size: u64,
    mtime: Option<DateTime<Utc>>,
    mode: Option<u32>,
    link: Option<String>,
    encrypted: bool,
    data: Data,
    /// tar hard link: the target's name (resolved after indexing).
    hardlink: Option<String>,
}

#[derive(Default)]
struct Tree {
    nodes: Vec<Node>,
    rejected: Vec<String>,
    hardlinks: Vec<(usize, Vec<String>)>,
}

impl Tree {
    fn new() -> Tree {
        Tree {
            nodes: vec![Node::dir(String::new(), 0)],
            ..Default::default()
        }
    }

    fn insert(&mut self, e: RawEntry) {
        let comps = match sanitize(&e.name) {
            Ok(c) => c,
            Err(why) => {
                log::warn!("archive: refusing entry {:?}: {why}", e.name);
                self.rejected.push(format!("{} ({why})", e.name));
                return;
            }
        };
        let Some((last, parents)) = comps.split_last() else {
            // "./" or "/" itself: the root. Take its attributes.
            if e.kind == EntryKind::Directory {
                self.nodes[0].mtime = e.mtime.or(self.nodes[0].mtime);
            }
            return;
        };
        let mut cur = 0usize;
        for c in parents {
            cur = match self.nodes[cur].children.get(c).copied() {
                Some(i) if self.nodes[i].kind == EntryKind::Directory => i,
                Some(_) => {
                    self.rejected
                        .push(format!("{} (a parent is not a folder)", e.name));
                    return;
                }
                None => {
                    let i = self.nodes.len();
                    self.nodes.push(Node::dir(c.clone(), cur));
                    self.nodes[cur].children.insert(c.clone(), i);
                    i
                }
            };
        }
        let node = Node {
            name: last.clone(),
            kind: e.kind,
            size: if e.kind == EntryKind::Directory {
                0
            } else {
                e.size
            },
            mtime: e.mtime,
            mode: e.mode,
            link: e.link,
            encrypted: e.encrypted,
            data: e.data,
            parent: cur,
            children: BTreeMap::new(),
            explicit: true,
        };
        let idx = match self.nodes[cur].children.get(last).copied() {
            Some(i) => {
                let existing = &mut self.nodes[i];
                if existing.kind == EntryKind::Directory && node.kind == EntryKind::Directory {
                    // A folder listed after (or again): keep its children.
                    existing.mtime = node.mtime.or(existing.mtime);
                    existing.mode = node.mode.or(existing.mode);
                    existing.explicit = true;
                    return;
                }
                if existing.kind == EntryKind::Directory && !existing.children.is_empty() {
                    self.rejected
                        .push(format!("{} (clashes with a folder)", e.name));
                    return;
                }
                // Later entries win (tar semantics for appended files).
                let children = std::mem::take(&mut existing.children);
                *existing = node;
                existing.children = children;
                i
            }
            None => {
                let i = self.nodes.len();
                self.nodes.push(node);
                self.nodes[cur].children.insert(last.clone(), i);
                i
            }
        };
        if let Some(target) = e.hardlink {
            match sanitize(&target) {
                Ok(t) => self.hardlinks.push((idx, t)),
                Err(_) => self.nodes[idx].kind = EntryKind::Other,
            }
        }
    }

    /// Point hard links at their target's data.
    fn resolve_hardlinks(&mut self) {
        for (idx, target) in std::mem::take(&mut self.hardlinks) {
            let mut cur = 0usize;
            let mut found = true;
            for c in &target {
                match self.nodes[cur].children.get(c) {
                    Some(&i) => cur = i,
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            if found && self.nodes[cur].kind == EntryKind::File && cur != idx {
                let (data, size) = (self.nodes[cur].data, self.nodes[cur].size);
                let n = &mut self.nodes[idx];
                n.kind = EntryKind::File;
                n.data = data;
                n.size = size;
            } else {
                self.nodes[idx].kind = EntryKind::Other;
            }
        }
    }
}

// ------------------------------------------------------------------ readers

/// Positional reader over a shared file (cheap to clone, no shared cursor).
#[derive(Clone)]
struct PReader {
    file: Arc<File>,
    pos: u64,
    len: u64,
}

impl Read for PReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let max = buf
            .len()
            .min((self.len - self.pos).min(usize::MAX as u64) as usize);
        let n = self.file.read_at(&mut buf[..max], self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for PReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let new = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos =
            new.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        Ok(self.pos)
    }
}

/// Bytes `pos..end` of a shared file.
struct RangeReader {
    file: Arc<File>,
    pos: u64,
    end: u64,
}

impl Read for RangeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.end {
            return Ok(0);
        }
        let max = buf
            .len()
            .min((self.end - self.pos).min(usize::MAX as u64) as usize);
        let n = self.file.read_at(&mut buf[..max], self.pos)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "archive data ends early",
            ));
        }
        self.pos += n as u64;
        Ok(n)
    }
}

/// Receives chunks from a streaming thread. Dropping it stops the thread.
struct ChannelReader {
    rx: Receiver<io::Result<Vec<u8>>>,
    cur: Vec<u8>,
    pos: usize,
    done: bool,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.cur.len() {
            if self.done {
                return Ok(0);
            }
            match self.rx.recv() {
                Ok(Ok(chunk)) if chunk.is_empty() => self.done = true,
                Ok(Ok(chunk)) => {
                    self.cur = chunk;
                    self.pos = 0;
                }
                Ok(Err(e)) => {
                    self.done = true;
                    return Err(e);
                }
                Err(_) => {
                    self.done = true;
                    return Err(io::Error::other("archive reader stopped unexpectedly"));
                }
            }
        }
        let n = buf.len().min(self.cur.len() - self.pos);
        buf[..n].copy_from_slice(&self.cur[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Counts the bytes read (and seeks) for progress; stops on cancel.
struct Progress<R> {
    inner: R,
    pos: u64,
    counter: Option<Arc<AtomicU64>>,
    cancel: Option<Arc<AtomicBool>>,
}

impl<R> Progress<R> {
    fn report(&self) -> io::Result<()> {
        if let Some(c) = &self.counter {
            c.store(self.pos, Ordering::Relaxed);
        }
        if self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            // Not `Interrupted`: readers retry those.
            return Err(io::Error::other(CANCELLED));
        }
        Ok(())
    }
}

const CANCELLED: &str = "manta: cancelled";

impl<R: Read> Read for Progress<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.report()?;
        let n = self.inner.read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for Progress<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = self.inner.seek(to)?;
        self.report()?;
        Ok(self.pos)
    }
}

/// Copies everything read into a spool file until it would exceed `limit`;
/// then the spool is abandoned.
struct Tee<R> {
    inner: R,
    spool: Option<File>,
    written: u64,
    limit: u64,
    total: u64,
}

impl<R: Read> Read for Tee<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.total += n as u64;
        if let Some(f) = &mut self.spool {
            if self.written + n as u64 > self.limit {
                self.spool = None;
            } else if f.write_all(&buf[..n]).is_err() {
                // Disk full or similar: fall back to streaming.
                self.spool = None;
            } else {
                self.written += n as u64;
            }
        }
        Ok(n)
    }
}

fn map_io(e: io::Error) -> VfsError {
    if e.to_string() == CANCELLED {
        VfsError::Cancelled
    } else {
        VfsError::Io(e)
    }
}

fn corrupt(e: impl std::fmt::Display) -> VfsError {
    VfsError::Other(format!("damaged or unsupported archive: {e}"))
}

// ------------------------------------------------------------------ backend

/// Where file data comes from.
enum Backing {
    Zip(zip::ZipArchive<PReader>),
    /// Uncompressed tar, or the decompressed stream spooled to a temp file:
    /// `Data::Range` offsets index into this file.
    File(Arc<File>),
    /// Compressed stream that was too big to spool: decompress `path` from
    /// the start for every read.
    Stream {
        path: PathBuf,
        codec: Codec,
    },
}

/// Options for [`ArchiveFs::open`].
#[derive(Clone)]
pub struct OpenOptions {
    /// Receives the number of archive bytes read so far while indexing.
    pub progress: Option<Arc<AtomicU64>>,
    /// Set to stop indexing (`open` then fails with `VfsError::Cancelled`).
    pub cancel: Option<Arc<AtomicBool>>,
    /// Compressed tar / single files: decompress into a temp file while
    /// indexing as long as the result stays below this many bytes (0 = never).
    pub spool_limit: u64,
    /// Format to assume instead of detecting it from the file name.
    pub format: Option<ArchiveFormat>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            progress: None,
            cancel: None,
            spool_limit: DEFAULT_SPOOL_LIMIT,
            format: None,
        }
    }
}

/// A read-only [`Vfs`] over an archive file on the local disk.
pub struct ArchiveFs {
    name: String,
    source: PathBuf,
    format: ArchiveFormat,
    nodes: Vec<Node>,
    backing: Backing,
    rejected: Vec<String>,
    /// Temp files deleted on drop (spool, downloaded archive).
    temps: Vec<tempfile::TempPath>,
}

impl ArchiveFs {
    /// Index the archive at `path` (blocking; call off the UI thread). The
    /// format comes from the file name unless `opts.format` is set.
    pub fn open(path: &Path, opts: &OpenOptions) -> VfsResult<ArchiveFs> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::open_named(path, &name, opts)
    }

    /// Like [`open`](Self::open), but `name` is the archive's display name
    /// (and is used for format detection), e.g. when `path` is a temp copy.
    pub fn open_named(path: &Path, name: &str, opts: &OpenOptions) -> VfsResult<ArchiveFs> {
        let format = opts
            .format
            .or_else(|| ArchiveFormat::detect(name))
            .ok_or_else(|| VfsError::Unsupported(format!("“{name}” is not a supported archive")))?;
        let file = File::open(path)
            .map_err(|e| crate::local::map_io(e, &VfsPath::new(&path.to_string_lossy())))?;
        let len = file.metadata()?.len();
        let file = Arc::new(file);
        let mut fs = ArchiveFs {
            name: name.to_string(),
            source: path.to_path_buf(),
            format,
            nodes: Vec::new(),
            backing: Backing::File(file.clone()),
            rejected: Vec::new(),
            temps: Vec::new(),
        };
        let tree = if format == ArchiveFormat::Zip {
            fs.index_zip(file, len, opts)?
        } else if format == ArchiveFormat::Tar {
            fs.index_plain_tar(file, len, opts)?
        } else {
            fs.index_compressed(len, opts)?
        };
        let mut tree = tree;
        tree.resolve_hardlinks();
        fs.nodes = tree.nodes;
        fs.rejected = tree.rejected;
        Ok(fs)
    }

    /// Delete `temp` together with this archive (e.g. the downloaded copy of
    /// a remote archive).
    pub fn adopt_temp(&mut self, temp: tempfile::TempPath) {
        self.temps.push(temp);
    }

    /// Temporary files owned by this archive (deleted on drop).
    pub fn temp_files(&self) -> Vec<PathBuf> {
        self.temps.iter().map(|t| t.to_path_buf()).collect()
    }

    /// The archive's file name, e.g. "backup.zip".
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn format(&self) -> ArchiveFormat {
        self.format
    }

    /// The local file the archive is read from (may be a temp copy).
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Entries that were refused while indexing (unsafe paths, clashes),
    /// with the reason.
    pub fn rejected(&self) -> &[String] {
        &self.rejected
    }

    /// True if `path` is an encrypted (password-protected) zip entry.
    pub fn is_encrypted(&self, path: &VfsPath) -> bool {
        self.lookup(path, false)
            .is_ok_and(|i| self.nodes[i].encrypted)
    }

    /// Number of files and folders in the archive (excluding the root).
    pub fn entry_count(&self) -> usize {
        self.nodes.len().saturating_sub(1)
    }

    /// Sum of the uncompressed file sizes.
    pub fn total_size(&self) -> u64 {
        self.nodes
            .iter()
            .filter(|n| n.kind == EntryKind::File)
            .map(|n| n.size)
            .sum()
    }

    /// True when reads of a compressed tar have to decompress from the
    /// start (the archive was too big to spool).
    pub fn is_streaming(&self) -> bool {
        matches!(self.backing, Backing::Stream { .. })
    }

    // -------------------------------------------------------------- index

    fn index_zip(&mut self, file: Arc<File>, len: u64, opts: &OpenOptions) -> VfsResult<Tree> {
        let reader = PReader { file, pos: 0, len };
        let mut archive = zip::ZipArchive::new(reader).map_err(corrupt)?;
        let meta = archive.metadata();
        let mut tree = Tree::new();
        let mut symlinks = Vec::new();
        for i in 0..meta.len() {
            if opts
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
            {
                return Err(VfsError::Cancelled);
            }
            let e = meta.entry(i).map_err(corrupt)?;
            let name = match e.name() {
                Ok(n) => n.into_owned(),
                Err(_) => String::from_utf8_lossy(e.name_raw()).into_owned(),
            };
            let mode = e.unix_mode();
            let is_dir = e.is_dir() || mode.is_some_and(|m| m & 0o170000 == 0o040000);
            let kind = if is_dir {
                EntryKind::Directory
            } else if e.is_symlink() {
                EntryKind::Symlink
            } else {
                EntryKind::File
            };
            if kind == EntryKind::Symlink {
                symlinks.push((i, name.clone()));
            }
            tree.insert(RawEntry {
                name,
                kind,
                size: e.size(),
                mtime: e.last_modified().and_then(zip_time),
                mode: mode.map(|m| m & 0o7777),
                link: None,
                encrypted: e.encrypted(),
                data: Data::Zip(i),
                hardlink: None,
            });
            if let Some(p) = &opts.progress {
                p.store(e.central_header_start().min(len), Ordering::Relaxed);
            }
        }
        // Symlink targets are the entries' contents.
        for (i, name) in symlinks {
            let target = archive.by_index(i).ok().and_then(|mut f| {
                let mut s = String::new();
                (&mut f).take(4096).read_to_string(&mut s).ok().map(|_| s)
            });
            if let Ok(comps) = sanitize(&name) {
                if let Some(idx) = find(&tree.nodes, &comps) {
                    match target {
                        Some(t) => tree.nodes[idx].link = Some(t),
                        None => tree.nodes[idx].kind = EntryKind::File,
                    }
                }
            }
        }
        if let Some(p) = &opts.progress {
            p.store(len, Ordering::Relaxed);
        }
        self.backing = Backing::Zip(archive);
        Ok(tree)
    }

    fn index_plain_tar(
        &mut self,
        file: Arc<File>,
        len: u64,
        opts: &OpenOptions,
    ) -> VfsResult<Tree> {
        let reader = Progress {
            inner: PReader {
                file: file.clone(),
                pos: 0,
                len,
            },
            pos: 0,
            counter: opts.progress.clone(),
            cancel: opts.cancel.clone(),
        };
        let mut ar = tar::Archive::new(reader);
        let entries = ar.entries_with_seek().map_err(map_io)?;
        let tree = index_tar_entries(entries)?;
        if let Some(p) = &opts.progress {
            p.store(len, Ordering::Relaxed);
        }
        self.backing = Backing::File(file);
        Ok(tree)
    }

    /// Compressed tar or single compressed file: one pass through the
    /// decompressor, spooling the output to a temp file.
    fn index_compressed(&mut self, len: u64, opts: &OpenOptions) -> VfsResult<Tree> {
        let codec = self.format.codec();
        let src = File::open(&self.source)?;
        let counted = Progress {
            inner: io::BufReader::new(src),
            pos: 0,
            counter: opts.progress.clone(),
            cancel: opts.cancel.clone(),
        };
        let decoder = codec.decoder(counted).map_err(map_io)?;
        let spool = if opts.spool_limit > 0 {
            tempfile::Builder::new()
                .prefix(TEMP_PREFIX)
                .suffix(".spool")
                .tempfile()
                .ok()
        } else {
            None
        };
        let (spool_file, spool_path) = match spool {
            Some(t) => {
                let (f, p) = t.into_parts();
                (Some(f), Some(p))
            }
            None => (None, None),
        };
        let mut tee = Tee {
            inner: decoder,
            spool: spool_file,
            written: 0,
            limit: opts.spool_limit,
            total: 0,
        };

        // Peek at the first block: a ".gz" may well be a tar.
        let mut head = Vec::with_capacity(512);
        (&mut tee)
            .take(512)
            .read_to_end(&mut head)
            .map_err(map_io)?;
        let is_tar = self.format.is_tar() || looks_like_tar(&head);
        let mut tree = Tree::new();
        if is_tar {
            let chained = io::Cursor::new(head).chain(&mut tee);
            let mut ar = tar::Archive::new(chained);
            let entries = ar.entries().map_err(map_io)?;
            tree = index_tar_entries(entries)?;
            // Drain the rest (end-of-archive blocks, padding) for the spool.
            drop(ar);
            io::copy(&mut tee, &mut io::sink()).map_err(map_io)?;
            if !self.format.is_tar() {
                self.format = match codec {
                    Codec::Gz => ArchiveFormat::TarGz,
                    Codec::Bz2 => ArchiveFormat::TarBz2,
                    Codec::Xz => ArchiveFormat::TarXz,
                    _ => ArchiveFormat::TarZst,
                };
            }
        } else {
            io::copy(&mut tee, &mut io::sink()).map_err(map_io)?;
            let total = tee.total;
            let inner_name = single_name(&self.name, self.format);
            let mtime = std::fs::metadata(&self.source)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(DateTime::<Utc>::from);
            tree.insert(RawEntry {
                name: inner_name,
                kind: EntryKind::File,
                size: total,
                mtime,
                mode: Some(0o644),
                link: None,
                encrypted: false,
                data: Data::Range {
                    offset: 0,
                    len: total,
                },
                hardlink: None,
            });
        }
        if let Some(p) = &opts.progress {
            p.store(len, Ordering::Relaxed);
        }
        match (tee.spool.take(), spool_path) {
            (Some(mut f), Some(path)) => {
                f.flush()?;
                self.backing = Backing::File(Arc::new(File::open(&path)?));
                self.temps.push(path);
            }
            _ => {
                self.backing = Backing::Stream {
                    path: self.source.clone(),
                    codec,
                };
            }
        }
        Ok(tree)
    }

    // -------------------------------------------------------------- lookup

    /// Resolve `path` to a node. Symlinks inside the archive are followed
    /// for every component, and for the last one if `follow_last`.
    fn lookup(&self, path: &VfsPath, follow_last: bool) -> VfsResult<usize> {
        let mut comps: VecDeque<String> = path.components().into_iter().map(String::from).collect();
        let mut cur = 0usize;
        let mut hops = 0;
        while let Some(c) = comps.pop_front() {
            if c == "." || c.is_empty() {
                continue;
            }
            if c == ".." {
                if cur == 0 {
                    return Err(VfsError::NotFound(format!(
                        "{path}: link points outside the archive"
                    )));
                }
                cur = self.nodes[cur].parent;
                continue;
            }
            let node = &self.nodes[cur];
            if node.kind != EntryKind::Directory {
                return Err(VfsError::NotADirectory(path.to_string()));
            }
            let Some(&child) = node.children.get(&c) else {
                return Err(VfsError::NotFound(path.to_string()));
            };
            let last = comps.is_empty();
            let n = &self.nodes[child];
            if n.kind == EntryKind::Symlink && (!last || follow_last) {
                hops += 1;
                if hops > 40 {
                    return Err(VfsError::Other(format!(
                        "{path}: too many levels of symbolic links"
                    )));
                }
                let target = n.link.clone().unwrap_or_default();
                if target.starts_with('/') || target.is_empty() {
                    return Err(VfsError::NotFound(format!(
                        "{path}: link points outside the archive"
                    )));
                }
                let mut next: VecDeque<String> = target.split('/').map(String::from).collect();
                next.extend(comps);
                comps = next;
                continue;
            }
            cur = child;
        }
        Ok(cur)
    }

    fn entry_for(&self, idx: usize, path: VfsPath) -> Entry {
        let n = &self.nodes[idx];
        let name = if idx == 0 {
            "/".to_string()
        } else {
            n.name.clone()
        };
        Entry {
            hidden: name.starts_with('.') && idx != 0,
            name,
            path,
            kind: n.kind,
            size: n.size,
            modified: n.mtime,
            permissions: n.mode,
            link_target: n.link.clone(),
        }
    }

    fn read_node(&self, idx: usize, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let n = &self.nodes[idx];
        match n.kind {
            EntryKind::Directory => return Err(VfsError::Other(format!("is a directory: {path}"))),
            EntryKind::File => {}
            EntryKind::Symlink => {
                return Err(VfsError::Unsupported(format!(
                    "{path}: link to a folder can't be read as a file"
                )))
            }
            EntryKind::Other => return Err(VfsError::Unsupported(format!("{path}: special file"))),
        }
        if n.encrypted {
            return Err(VfsError::PermissionDenied(format!(
                "{path}: encrypted entry (password-protected archives aren't supported)"
            )));
        }
        match (n.data, &self.backing) {
            (Data::None, _) => Ok(Box::new(io::empty())),
            (Data::Zip(i), Backing::Zip(archive)) => {
                let mut archive = archive.clone();
                if n.size <= SMALL_ENTRY {
                    let mut f = archive
                        .by_index(i)
                        .map_err(|e| VfsError::Other(format!("{path}: {e}")))?;
                    let mut buf = Vec::with_capacity(n.size as usize);
                    f.read_to_end(&mut buf).map_err(VfsError::Io)?;
                    return Ok(Box::new(io::Cursor::new(buf)));
                }
                let (tx, rx) = sync_channel::<io::Result<Vec<u8>>>(4);
                std::thread::Builder::new()
                    .name("manta-unzip".into())
                    .spawn(move || {
                        let mut f = match archive.by_index(i) {
                            Ok(f) => f,
                            Err(e) => {
                                let _ = tx.send(Err(io::Error::other(e.to_string())));
                                return;
                            }
                        };
                        loop {
                            let mut buf = vec![0u8; CHUNK];
                            match f.read(&mut buf) {
                                Ok(0) => {
                                    let _ = tx.send(Ok(Vec::new()));
                                    return;
                                }
                                Ok(k) => {
                                    buf.truncate(k);
                                    if tx.send(Ok(buf)).is_err() {
                                        return; // reader dropped
                                    }
                                }
                                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                                Err(e) => {
                                    let _ = tx.send(Err(e));
                                    return;
                                }
                            }
                        }
                    })
                    .map_err(VfsError::Io)?;
                Ok(Box::new(ChannelReader {
                    rx,
                    cur: Vec::new(),
                    pos: 0,
                    done: false,
                }))
            }
            (Data::Range { offset, len }, Backing::File(file)) => Ok(Box::new(RangeReader {
                file: file.clone(),
                pos: offset,
                end: offset + len,
            })),
            (Data::Range { offset, len }, Backing::Stream { path: src, codec }) => {
                let f = io::BufReader::new(File::open(src)?);
                let mut dec = codec.decoder(f)?;
                let skipped = io::copy(&mut (&mut dec).take(offset), &mut io::sink())?;
                if skipped < offset {
                    return Err(VfsError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "archive data ends early",
                    )));
                }
                Ok(Box::new(dec.take(len)))
            }
            _ => Err(VfsError::Other(format!(
                "{path}: inconsistent archive index"
            ))),
        }
    }
}

fn find(nodes: &[Node], comps: &[String]) -> Option<usize> {
    let mut cur = 0;
    for c in comps {
        cur = *nodes[cur].children.get(c)?;
    }
    Some(cur)
}

/// The decompressed name of a single compressed file: "notes.txt.gz" → "notes.txt".
fn single_name(archive: &str, format: ArchiveFormat) -> String {
    let lower = archive.to_lowercase();
    for ext in format.single_ext() {
        if lower.ends_with(ext) && archive.len() > ext.len() {
            return archive[..archive.len() - ext.len()].to_string();
        }
    }
    format!("{archive}.out")
}

fn looks_like_tar(head: &[u8]) -> bool {
    head.len() >= 262 && &head[257..262] == b"ustar"
}

fn zip_time(t: zip::DateTime) -> Option<DateTime<Utc>> {
    let naive = chrono::NaiveDate::from_ymd_opt(t.year() as i32, t.month() as u32, t.day() as u32)?
        .and_hms_opt(t.hour() as u32, t.minute() as u32, t.second() as u32)?;
    // DOS times are local times without a zone.
    chrono::Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
}

fn index_tar_entries<R: Read>(entries: tar::Entries<'_, R>) -> VfsResult<Tree> {
    let mut tree = Tree::new();
    for item in entries {
        let entry = item.map_err(|e| {
            let v = map_io(e);
            match v {
                VfsError::Cancelled => v,
                other => corrupt(other),
            }
        })?;
        let header = entry.header();
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let link = entry
            .link_name_bytes()
            .map(|l| String::from_utf8_lossy(&l).into_owned());
        let et = header.entry_type();
        let mode = header.mode().ok().map(|m| m & 0o7777);
        let mtime = header
            .mtime()
            .ok()
            .and_then(|s| DateTime::from_timestamp(s as i64, 0));
        let (kind, hardlink) = if et.is_dir() {
            (EntryKind::Directory, None)
        } else if et.is_symlink() {
            (EntryKind::Symlink, None)
        } else if et.is_hard_link() {
            (EntryKind::File, link.clone())
        } else if et.is_file() || et == tar::EntryType::Continuous {
            (EntryKind::File, None)
        } else if et.is_pax_global_extensions()
            || et.is_pax_local_extensions()
            || et.is_gnu_longname()
            || et.is_gnu_longlink()
        {
            continue;
        } else {
            (EntryKind::Other, None)
        };
        let size = entry.size();
        let offset = entry.raw_file_position();
        tree.insert(RawEntry {
            name,
            kind,
            size: if kind == EntryKind::File && hardlink.is_none() {
                size
            } else {
                0
            },
            mtime,
            mode,
            link: if kind == EntryKind::Symlink {
                link
            } else {
                None
            },
            encrypted: false,
            data: if kind == EntryKind::File && hardlink.is_none() {
                Data::Range { offset, len: size }
            } else {
                Data::None
            },
            hardlink,
        });
    }
    Ok(tree)
}

impl Vfs for ArchiveFs {
    fn label(&self) -> String {
        self.name.clone()
    }

    fn protocol(&self) -> Protocol {
        Protocol::Local
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let idx = self.lookup(dir, true)?;
        let n = &self.nodes[idx];
        if n.kind != EntryKind::Directory {
            return Err(VfsError::NotADirectory(dir.to_string()));
        }
        Ok(n.children
            .iter()
            .map(|(name, &c)| self.entry_for(c, dir.join(name)))
            .collect())
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let idx = self.lookup(path, false)?;
        Ok(self.entry_for(idx, path.clone()))
    }

    fn create_dir(&self, _path: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn remove_file(&self, _path: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn remove_dir(&self, _path: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn rename(&self, _from: &VfsPath, _to: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let idx = self.lookup(path, true)?;
        self.read_node(idx, path)
    }

    fn open_write(&self, _path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn set_permissions(&self, _path: &VfsPath, _mode: u32) -> VfsResult<()> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn set_modified(&self, _path: &VfsPath, _mtime: DateTime<Utc>) -> VfsResult<()> {
        Err(VfsError::Unsupported(READ_ONLY.into()))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// The [`ArchiveFs`] behind a shared backend, if it is one.
pub fn as_archive(fs: &dyn Vfs) -> Option<&ArchiveFs> {
    fs.as_any().and_then(|a| a.downcast_ref::<ArchiveFs>())
}

/// Remove leftover temp files of earlier runs (older than a day).
pub fn clean_stale_temps() {
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let day = std::time::Duration::from_secs(24 * 3600);
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
            continue;
        }
        let old = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_formats() {
        assert_eq!(ArchiveFormat::detect("a.ZIP"), Some(ArchiveFormat::Zip));
        assert_eq!(
            ArchiveFormat::detect("a.tar.gz"),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(ArchiveFormat::detect("a.tgz"), Some(ArchiveFormat::TarGz));
        assert_eq!(
            ArchiveFormat::detect("a.tar.bz2"),
            Some(ArchiveFormat::TarBz2)
        );
        assert_eq!(ArchiveFormat::detect("a.txz"), Some(ArchiveFormat::TarXz));
        assert_eq!(
            ArchiveFormat::detect("a.tar.zst"),
            Some(ArchiveFormat::TarZst)
        );
        assert_eq!(ArchiveFormat::detect("a.tar"), Some(ArchiveFormat::Tar));
        assert_eq!(
            ArchiveFormat::detect("notes.txt.gz"),
            Some(ArchiveFormat::Gz)
        );
        assert_eq!(ArchiveFormat::detect("a.bz2"), Some(ArchiveFormat::Bz2));
        assert_eq!(
            ArchiveFormat::detect("report.docx"),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(ArchiveFormat::detect("a.txt"), None);
        assert_eq!(ArchiveFormat::detect(".zip"), None);
        assert!(ArchiveFormat::opens_as_folder("x.zip"));
        assert!(!ArchiveFormat::opens_as_folder("x.docx"));
        assert!(!ArchiveFormat::opens_as_folder("x.txt"));
        assert_eq!(ArchiveFormat::stem("backup.tar.gz"), "backup");
        assert_eq!(ArchiveFormat::stem("notes.txt.gz"), "notes.txt");
        assert_eq!(ArchiveFormat::stem("A.ZIP"), "A");
        assert_eq!(ArchiveFormat::stem("plain"), "plain");
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize("./a/b.txt").unwrap(), vec!["a", "b.txt"]);
        assert_eq!(sanitize("a\\b\\c").unwrap(), vec!["a", "b", "c"]);
        assert_eq!(sanitize("a//b/").unwrap(), vec!["a", "b"]);
        assert!(sanitize("../evil").is_err());
        assert!(sanitize("a/../../evil").is_err());
        assert!(sanitize("/abs").is_err());
        assert!(sanitize("C:\\win").is_err());
        assert!(sanitize("..\\evil").is_err());
    }

    #[test]
    fn single_names() {
        assert_eq!(single_name("notes.txt.gz", ArchiveFormat::Gz), "notes.txt");
        assert_eq!(single_name("X.ZST", ArchiveFormat::Zst), "X");
    }
}
