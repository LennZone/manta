//! Creating archives (zip, tar.gz, tar.xz, tar.zst) from files on any
//! backend, as a background job of the [`crate::TransferManager`].
//!
//! The archive is always written to a local temporary file first: next to
//! the destination for a local folder (then renamed into place), in the
//! system temp folder for a remote one (then uploaded with progress). A
//! cancelled or failed job leaves nothing behind.
//!
//! Inside the archive every source keeps its name ("docs/…"); symlinks are
//! stored as symlinks (also links to folders, which are not followed);
//! permissions and mtimes are kept.

use std::fs::File;
use std::io::{self, Read, Write};

use manta_core::{Entry, EntryKind, SharedVfs, Vfs, VfsError, VfsPath};

use crate::transfer::{Cancelled, Fatal, Node, Runner};
use crate::unique_name;

/// Formats Manta can create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompressFormat {
    Zip,
    TarGz,
    TarXz,
    TarZst,
}

impl CompressFormat {
    pub const ALL: [CompressFormat; 4] = [
        CompressFormat::Zip,
        CompressFormat::TarGz,
        CompressFormat::TarXz,
        CompressFormat::TarZst,
    ];

    /// File extension including the dot, e.g. ".tar.gz".
    pub fn ext(self) -> &'static str {
        match self {
            CompressFormat::Zip => ".zip",
            CompressFormat::TarGz => ".tar.gz",
            CompressFormat::TarXz => ".tar.xz",
            CompressFormat::TarZst => ".tar.zst",
        }
    }

    /// Short label, e.g. "tar.gz".
    pub fn label(self) -> &'static str {
        match self {
            CompressFormat::Zip => "ZIP",
            CompressFormat::TarGz => "tar.gz",
            CompressFormat::TarXz => "tar.xz",
            CompressFormat::TarZst => "tar.zst",
        }
    }

    /// The level most tools use by default (on the 1–9 scale).
    pub fn default_level(self) -> u32 {
        match self {
            CompressFormat::Zip | CompressFormat::TarGz => 6,
            CompressFormat::TarXz => 6,
            CompressFormat::TarZst => 3,
        }
    }
}

/// A request to create an archive.
#[derive(Clone)]
pub struct CompressJob {
    pub src_fs: SharedVfs,
    /// Files and folders to put into the archive (each under its own name).
    pub sources: Vec<VfsPath>,
    pub dst_fs: SharedVfs,
    /// Full path of the archive. If it exists, a free "name 2.zip" is used.
    pub dst_path: VfsPath,
    pub format: CompressFormat,
    /// 1 (fastest) ..= 9 (smallest); clamped.
    pub level: u32,
    /// Leave out `.DS_Store` files and `__MACOSX` folders.
    pub exclude_junk: bool,
}

/// Files the "exclude" option leaves out of new archives.
pub fn is_junk(name: &str, is_dir: bool) -> bool {
    if is_dir {
        name == "__MACOSX"
    } else {
        name == ".DS_Store"
    }
}

pub(crate) fn title(job: &CompressJob, archive_name: &str) -> String {
    let what = match job.sources.as_slice() {
        [one] => format!("\"{}\"", if one.is_root() { "/" } else { one.file_name() }),
        many => format!("{} items", many.len()),
    };
    format!("Compressing {what} to \"{archive_name}\"")
}

/// zstd levels for the 1–9 scale.
const ZSTD_LEVELS: [i32; 9] = [1, 3, 5, 7, 9, 12, 15, 17, 19];

/// A compressing writer that must be finished explicitly.
enum Enc {
    Gz(flate2::write::GzEncoder<File>),
    Xz(Box<lzma_rust2::XzWriter<File>>),
    Zst(zstd::stream::write::Encoder<'static, File>),
}

impl Enc {
    fn new(format: CompressFormat, file: File, level: u32) -> io::Result<Enc> {
        Ok(match format {
            CompressFormat::TarXz => {
                let opts = lzma_rust2::XzOptions::with_preset(level);
                Enc::Xz(Box::new(lzma_rust2::XzWriter::new(file, opts)?))
            }
            CompressFormat::TarZst => {
                let l = ZSTD_LEVELS[(level.clamp(1, 9) - 1) as usize];
                Enc::Zst(zstd::stream::write::Encoder::new(file, l)?)
            }
            _ => Enc::Gz(flate2::write::GzEncoder::new(
                file,
                flate2::Compression::new(level),
            )),
        })
    }

    fn finish(self) -> io::Result<File> {
        match self {
            Enc::Gz(e) => e.finish(),
            Enc::Xz(e) => e.finish(),
            Enc::Zst(e) => e.finish(),
        }
    }
}

impl Write for Enc {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Enc::Gz(e) => e.write(buf),
            Enc::Xz(e) => e.write(buf),
            Enc::Zst(e) => e.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Enc::Gz(e) => e.flush(),
            Enc::Xz(e) => e.flush(),
            Enc::Zst(e) => e.flush(),
        }
    }
}

/// The archive being written.
enum Sink {
    Zip(Box<zip::ZipWriter<File>>),
    Tar(Box<tar::Builder<Enc>>),
}

/// Why writing one entry stopped.
enum EntryError {
    Cancelled,
    /// The source could not be read: skip the item, go on.
    Source(String),
    /// The archive could not be written: the job fails.
    Archive(String),
}

/// Source reader for tar entries: counts progress, honours cancel, and
/// delivers exactly `size` bytes (zero-padded if the file shrank).
struct Feed<'r, 'a> {
    inner: Box<dyn Read + Send>,
    runner: &'r mut Runner<'a>,
    left: u64,
    short: bool,
    read_error: Option<io::Error>,
}

impl Read for Feed<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        if self.runner.cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        let max = buf.len().min(self.left.min(usize::MAX as u64) as usize);
        let n = if self.short {
            0
        } else {
            match self.inner.read(&mut buf[..max]) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                Err(e) => {
                    self.read_error = Some(e);
                    self.short = true;
                    0
                }
            }
        };
        let n = if n == 0 {
            // Shorter than announced: pad so the tar stays valid.
            self.short = true;
            buf[..max].fill(0);
            max
        } else {
            n
        };
        self.left -= n as u64;
        self.runner.add_bytes(n as u64);
        Ok(n)
    }
}

impl<'a> Runner<'a> {
    pub(crate) fn run_compress(&mut self, job: &CompressJob) -> Result<(), Fatal> {
        if job.dst_fs.is_read_only() {
            return Err(Fatal::Failed(format!(
                "{}: the destination is read-only",
                job.dst_path
            )));
        }
        if job.sources.is_empty() {
            return Err(Fatal::Failed("nothing to compress".into()));
        }
        // Scan.
        let mut nodes = Vec::new();
        for src in &job.sources {
            self.check()?;
            if src.is_root() {
                self.error("cannot compress the root directory".into());
                continue;
            }
            match job.src_fs.stat(src) {
                Ok(mut e) => {
                    e.path = src.clone();
                    let n = self.scan_archive_tree(&*job.src_fs, e, job.exclude_junk)?;
                    let (f, b) = n.totals();
                    self.add_totals(f, b);
                    nodes.push(n);
                }
                Err(e) => self.error(format!("{src}: {e}")),
            }
        }
        if nodes.is_empty() {
            return Err(Fatal::Failed(self.first_error("nothing to compress")));
        }

        // Final name and the local temp file.
        let mut dst = job.dst_path.clone();
        if job.dst_fs.stat(&dst).is_ok() {
            dst = unique_name(&*job.dst_fs, &dst, false);
            self.set_title(title(job, dst.file_name()));
        }
        let local_dst = job.dst_fs.local_path(&dst);
        let builder = || {
            let mut b = tempfile::Builder::new();
            b.prefix(".manta-compress-").suffix(".partial");
            b
        };
        let temp = match local_dst.as_ref().and_then(|p| p.parent()) {
            Some(dir) => builder().tempfile_in(dir),
            None => builder().tempfile(),
        }
        .map_err(|e| Fatal::Failed(format!("can't create a temporary file: {e}")))?;
        let (file, temp_path) = temp.into_parts();

        // Write.
        let level = job.level.clamp(1, 9);
        let mut sink = match job.format {
            CompressFormat::Zip => Sink::Zip(Box::new(zip::ZipWriter::new(file))),
            f => {
                let enc = Enc::new(f, file, level).map_err(|e| Fatal::Failed(e.to_string()))?;
                let mut b = tar::Builder::new(enc);
                b.follow_symlinks(false);
                Sink::Tar(Box::new(b))
            }
        };
        self.buf = vec![0u8; crate::COPY_BUFFER_SIZE];
        for n in &nodes {
            let name = n.entry.path.file_name().to_string();
            self.add_node(&*job.src_fs, &mut sink, n, &name, level)?;
        }
        let file = match sink {
            Sink::Zip(z) => z
                .finish()
                .map_err(|e| Fatal::Failed(format!("writing the archive: {e}")))?,
            Sink::Tar(b) => (*b)
                .into_inner()
                .and_then(Enc::finish)
                .map_err(|e| Fatal::Failed(format!("writing the archive: {e}")))?,
        };
        file.sync_all().ok();
        drop(file);
        self.check()?;

        // Deliver.
        match local_dst {
            Some(os) => {
                temp_path
                    .persist(&os)
                    .map_err(|e| Fatal::Failed(format!("{dst}: {}", e.error)))?;
            }
            None => self.upload(&temp_path, &*job.dst_fs, &dst)?,
        }
        Ok(())
    }

    /// Like the transfer scan, but skips junk (if asked) and never follows
    /// symlinked folders.
    fn scan_archive_tree(
        &mut self,
        fs: &dyn Vfs,
        entry: Entry,
        exclude: bool,
    ) -> Result<Node, Cancelled> {
        self.check()?;
        let mut node = Node {
            entry,
            children: Vec::new(),
            list_failed: false,
        };
        if node.is_dir() && node.entry.link_target.is_none() {
            self.set_current(&node.entry.path);
            match fs.list(&node.entry.path) {
                Ok(mut entries) => {
                    entries.sort_by(|a, b| a.name.cmp(&b.name));
                    for e in entries {
                        if e.name == "."
                            || e.name == ".."
                            || (exclude && is_junk(&e.name, e.is_dir()))
                        {
                            continue;
                        }
                        let child = self.scan_archive_tree(fs, e, exclude)?;
                        node.children.push(child);
                    }
                }
                Err(e) => {
                    node.list_failed = true;
                    self.error(format!("{}: {e}", node.entry.path));
                }
            }
        }
        Ok(node)
    }

    fn add_node(
        &mut self,
        fs: &dyn Vfs,
        sink: &mut Sink,
        node: &Node,
        name: &str,
        level: u32,
    ) -> Result<(), Fatal> {
        self.check()?;
        let e = &node.entry;
        let symlink = e.link_target.is_some() && (e.kind == EntryKind::Symlink || e.is_dir());
        if symlink {
            let target = e.link_target.clone().unwrap_or_default();
            self.write_link(sink, e, name, &target, level)
                .map_err(Fatal::Failed)?;
            self.file_done();
            return Ok(());
        }
        if node.is_dir() {
            self.write_dir(sink, e, name, level)
                .map_err(Fatal::Failed)?;
            for c in &node.children {
                let child = format!("{name}/{}", c.entry.path.file_name());
                self.add_node(fs, sink, c, &child, level)?;
            }
            return Ok(());
        }
        if e.kind == EntryKind::Other {
            self.error(format!("{}: skipping special file", e.path));
            self.skip_node(node);
            return Ok(());
        }
        self.set_current(&e.path);
        let reader = match fs.open_read(&e.path) {
            Ok(r) => r,
            Err(err) => {
                self.error(format!("{}: {err}", e.path));
                self.skip_node(node);
                return Ok(());
            }
        };
        match self.write_file(sink, e, name, level, reader) {
            Ok(()) => {}
            Err(EntryError::Cancelled) => return Err(Fatal::Cancelled),
            Err(EntryError::Source(msg)) => self.error(format!("{}: {msg}", e.path)),
            Err(EntryError::Archive(msg)) => {
                return Err(Fatal::Failed(format!("writing the archive: {msg}")))
            }
        }
        self.file_done();
        Ok(())
    }

    fn write_dir(
        &mut self,
        sink: &mut Sink,
        e: &Entry,
        name: &str,
        level: u32,
    ) -> Result<(), String> {
        match sink {
            Sink::Zip(z) => z
                .add_directory(format!("{name}/"), zip_options(e, level, 0o755))
                .map_err(|err| err.to_string()),
            Sink::Tar(b) => {
                let mut h = tar_header(e, tar::EntryType::Directory, 0o755);
                b.append_data(&mut h, format!("{name}/"), io::empty())
                    .map_err(|err| err.to_string())
            }
        }
    }

    fn write_link(
        &mut self,
        sink: &mut Sink,
        e: &Entry,
        name: &str,
        target: &str,
        level: u32,
    ) -> Result<(), String> {
        match sink {
            Sink::Zip(z) => z
                .add_symlink(name, target, zip_options(e, level, 0o777))
                .map_err(|err| err.to_string()),
            Sink::Tar(b) => {
                let mut h = tar_header(e, tar::EntryType::Symlink, 0o777);
                b.append_link(&mut h, name, target)
                    .map_err(|err| err.to_string())
            }
        }
    }

    fn write_file(
        &mut self,
        sink: &mut Sink,
        e: &Entry,
        name: &str,
        level: u32,
        mut reader: Box<dyn Read + Send>,
    ) -> Result<(), EntryError> {
        match sink {
            Sink::Zip(z) => {
                let opts = zip_options(e, level, 0o644).large_file(e.size >= u32::MAX as u64);
                z.start_file(name, opts)
                    .map_err(|err| EntryError::Archive(err.to_string()))?;
                let mut buf = std::mem::take(&mut self.buf);
                let result = loop {
                    if self.cancelled() {
                        break Err(EntryError::Cancelled);
                    }
                    let n = match reader.read(&mut buf) {
                        Ok(0) => break Ok(()),
                        Ok(n) => n,
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        // The entry stays in the archive with what was read.
                        Err(err) => break Err(EntryError::Source(err.to_string())),
                    };
                    if let Err(err) = z.write_all(&buf[..n]) {
                        break Err(EntryError::Archive(err.to_string()));
                    }
                    self.add_bytes(n as u64);
                };
                self.buf = buf;
                result
            }
            Sink::Tar(b) => {
                let mut h = tar_header(e, tar::EntryType::Regular, 0o644);
                h.set_size(e.size);
                let mut feed = Feed {
                    inner: reader,
                    runner: self,
                    left: e.size,
                    short: false,
                    read_error: None,
                };
                let r = b.append_data(&mut h, name, &mut feed);
                let (short, read_error) = (feed.short, feed.read_error.take());
                if self.cancelled() {
                    return Err(EntryError::Cancelled);
                }
                r.map_err(|err| EntryError::Archive(err.to_string()))?;
                match (read_error, short) {
                    (Some(err), _) => Err(EntryError::Source(err.to_string())),
                    (None, true) => Err(EntryError::Source(
                        "file changed while it was compressed".into(),
                    )),
                    _ => Ok(()),
                }
            }
        }
    }

    /// Upload the finished archive to a remote destination.
    fn upload(
        &mut self,
        local: &std::path::Path,
        dst_fs: &dyn Vfs,
        dst: &VfsPath,
    ) -> Result<(), Fatal> {
        let size = std::fs::metadata(local).map(|m| m.len()).unwrap_or(0);
        self.add_totals(0, size);
        self.set_current(dst);
        let mut src = File::open(local).map_err(|e| Fatal::Failed(e.to_string()))?;
        let mut out = dst_fs
            .open_write(dst)
            .map_err(|e| Fatal::Failed(format!("{dst}: {e}")))?;
        let mut buf = std::mem::take(&mut self.buf);
        let result = loop {
            if self.cancelled() {
                break Err(Fatal::Cancelled);
            }
            let n = match src.read(&mut buf) {
                Ok(0) => {
                    break out
                        .flush()
                        .map_err(|e| Fatal::Failed(format!("{dst}: {e}")))
                }
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => break Err(Fatal::Failed(e.to_string())),
            };
            if let Err(e) = out.write_all(&buf[..n]) {
                break Err(Fatal::Failed(format!("{dst}: {e}")));
            }
            self.add_bytes(n as u64);
        };
        self.buf = buf;
        drop(out);
        if result.is_err() {
            match dst_fs.remove_file(dst) {
                Ok(()) | Err(VfsError::NotFound(_)) => {}
                Err(e) => log::debug!("removing partial {dst}: {e}"),
            }
        }
        result
    }
}

fn zip_options(e: &Entry, level: u32, default_mode: u32) -> zip::write::SimpleFileOptions {
    let mut o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(level as i64))
        .unix_permissions(e.permissions.unwrap_or(default_mode) & 0o7777);
    if let Some(t) = e.modified.and_then(zip_time) {
        o = o.last_modified_time(t);
    }
    o
}

fn zip_time(t: chrono::DateTime<chrono::Utc>) -> Option<zip::DateTime> {
    use chrono::{Datelike, Timelike};
    let l = t.with_timezone(&chrono::Local);
    zip::DateTime::from_date_and_time(
        l.year().clamp(1980, 2107) as u16,
        l.month() as u8,
        l.day() as u8,
        l.hour() as u8,
        l.minute() as u8,
        l.second() as u8,
    )
    .ok()
}

fn tar_header(e: &Entry, kind: tar::EntryType, default_mode: u32) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(kind);
    h.set_mode(e.permissions.unwrap_or(default_mode) & 0o7777);
    h.set_mtime(e.modified.map(|t| t.timestamp().max(0) as u64).unwrap_or(0));
    h.set_size(0);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk() {
        assert!(is_junk(".DS_Store", false));
        assert!(is_junk("__MACOSX", true));
        assert!(!is_junk("__MACOSX", false));
        assert!(!is_junk("notes.txt", false));
    }

    #[test]
    fn extensions() {
        assert_eq!(CompressFormat::TarZst.ext(), ".tar.zst");
        assert_eq!(CompressFormat::ALL.len(), 4);
    }
}
