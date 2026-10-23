//! The background side of the preview: reads local files, finds a
//! folder's README, parses and highlights documents (local and remote),
//! watches the shown local file for changes (mtime polling, debounced) and
//! loads images. Nothing here runs on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use manta_core::VfsPath;

use crate::doc::Doc;
use crate::i18n::msg_key;
use crate::links::ImageSource;

/// Larger documents are not rendered unless asked ("Show first 2 MB").
pub const MAX_DOC_BYTES: u64 = 2_000_000;
/// Largest image read.
pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
/// How often the shown local file is checked for changes.
pub const POLL: Duration = Duration::from_millis(200);

/// What the preview shows for one request.
#[derive(Debug, Clone)]
pub enum Content {
    Doc {
        doc: Arc<Doc>,
        /// Only the first part is shown: (bytes shown, file size).
        partial: Option<(u64, u64)>,
    },
    /// The folder has no README (nothing selected).
    NoReadme,
    /// Over [`MAX_DOC_BYTES`]; `size` is the file size.
    TooLarge {
        size: u64,
    },
    Unreadable(String),
}

/// A loaded document (or why there is none).
#[derive(Debug, Clone)]
pub struct Loaded {
    pub gen: u64,
    /// The file shown (for `NoReadme`: the folder).
    pub path: VfsPath,
    /// Shown because it is the folder's README.
    pub readme: bool,
    pub content: Content,
    /// Increases with every reload of the same file.
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalTarget {
    File(PathBuf),
    /// The README of this folder, if it has one.
    ReadmeIn(PathBuf),
}

pub enum Cmd {
    /// Show and watch a local file / folder README.
    Local {
        gen: u64,
        target: LocalTarget,
        allow_partial: bool,
    },
    /// Parse a remote file's bytes (from `HostRequest::ReadFile`).
    Parse {
        gen: u64,
        path: VfsPath,
        readme: bool,
        bytes: Vec<u8>,
        size: u64,
        allow_partial: bool,
    },
    /// Stop watching (panel hidden or remote document).
    Idle,
    /// Read a local image file.
    Image(ImageSource, PathBuf),
    /// Download an http(s) image (only sent when the setting allows it).
    WebImage(ImageSource, String),
    Shutdown,
}

pub enum Out {
    Loaded(Loaded),
    Image(ImageSource, Result<Arc<[u8]>, String>),
}

pub struct Loader {
    tx: Sender<Cmd>,
    pub join: Option<JoinHandle<()>>,
}

impl Loader {
    /// Start the thread; `wake` is called after every result.
    pub fn start(out: Sender<Out>, wake: Arc<dyn Fn() + Send + Sync>) -> Loader {
        let (tx, rx) = channel();
        let join = std::thread::Builder::new()
            .name("manta-markdown".into())
            .spawn(move || run(rx, out, wake))
            .ok();
        Loader { tx, join }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
}

/// Identity of a file version: (mtime, length).
type Sig = Option<(SystemTime, u64)>;

fn sig(p: &Path) -> Sig {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

struct Watch {
    gen: u64,
    target: LocalTarget,
    allow_partial: bool,
    /// The README file the target resolved to last time.
    file: Option<PathBuf>,
    loaded: Sig,
    /// A change seen on the previous tick (reloaded once it holds still).
    candidate: Option<Sig>,
    version: u64,
}

fn run(rx: Receiver<Cmd>, out: Sender<Out>, wake: Arc<dyn Fn() + Send + Sync>) {
    let send = |o: Out| {
        let _ = out.send(o);
        wake();
    };
    let mut watch: Option<Watch> = None;
    // Warm up the highlighter so the first code block isn't slow.
    crate::highlight::warm_up();
    loop {
        let cmd = match rx.recv_timeout(POLL) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match cmd {
            Some(Cmd::Shutdown) => return,
            Some(Cmd::Idle) => watch = None,
            Some(Cmd::Local {
                gen,
                target,
                allow_partial,
            }) => {
                let mut w = Watch {
                    gen,
                    target,
                    allow_partial,
                    file: None,
                    loaded: None,
                    candidate: None,
                    version: 0,
                };
                send(Out::Loaded(load_local(&mut w)));
                watch = Some(w);
            }
            Some(Cmd::Parse {
                gen,
                path,
                readme,
                bytes,
                size,
                allow_partial,
            }) => {
                let content = content_from(&bytes, size, allow_partial);
                send(Out::Loaded(Loaded {
                    gen,
                    path,
                    readme,
                    content,
                    version: 0,
                }));
            }
            Some(Cmd::Image(key, path)) => {
                let out = out.clone();
                let wake = wake.clone();
                std::thread::spawn(move || {
                    let r = read_capped(&path, MAX_IMAGE_BYTES).map(Arc::from);
                    let _ = out.send(Out::Image(key, r));
                    wake();
                });
            }
            Some(Cmd::WebImage(key, url)) => {
                let out = out.clone();
                let wake = wake.clone();
                std::thread::spawn(move || {
                    let r = fetch(&url).map(Arc::from);
                    let _ = out.send(Out::Image(key, r));
                    wake();
                });
            }
            None => {
                if let Some(w) = &mut watch {
                    if let Some(l) = poll(w) {
                        send(Out::Loaded(l));
                    }
                }
            }
        }
    }
}

/// One tick of the watch: reload when the file changed and then held still
/// for a tick (debounces editors that write in several steps).
fn poll(w: &mut Watch) -> Option<Loaded> {
    let file = match &w.target {
        LocalTarget::File(p) => Some(p.clone()),
        LocalTarget::ReadmeIn(d) => find_readme(d),
    };
    let now = file.as_deref().and_then(sig);
    if file == w.file && now == w.loaded {
        w.candidate = None;
        return None;
    }
    if w.candidate != Some(now) {
        w.candidate = Some(now);
        return None;
    }
    w.candidate = None;
    Some(load_local(w))
}

fn load_local(w: &mut Watch) -> Loaded {
    let (file, readme) = match &w.target {
        LocalTarget::File(p) => (Some(p.clone()), false),
        LocalTarget::ReadmeIn(d) => (find_readme(d), true),
    };
    w.file = file.clone();
    w.version += 1;
    let Some(file) = file else {
        w.loaded = None;
        let LocalTarget::ReadmeIn(d) = &w.target else {
            unreachable!()
        };
        return Loaded {
            gen: w.gen,
            path: vpath(d),
            readme: true,
            content: Content::NoReadme,
            version: w.version,
        };
    };
    let before = sig(&file);
    let content = match read_capped(&file, MAX_DOC_BYTES + 1) {
        Ok(bytes) => {
            let size = before.map_or(bytes.len() as u64, |s| s.1);
            content_from(&bytes, size, w.allow_partial)
        }
        Err(e) => Content::Unreadable(e),
    };
    w.loaded = before;
    Loaded {
        gen: w.gen,
        path: vpath(&file),
        readme,
        content,
        version: w.version,
    }
}

pub fn vpath(p: &Path) -> VfsPath {
    VfsPath::new(&p.to_string_lossy())
}

/// Turn file bytes into what the preview shows.
pub fn content_from(bytes: &[u8], size: u64, allow_partial: bool) -> Content {
    let size = size.max(bytes.len() as u64);
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Content::Unreadable(msg_key("error.not_text"));
    }
    let mut partial = None;
    let mut text_bytes = bytes;
    if size > MAX_DOC_BYTES || bytes.len() as u64 > MAX_DOC_BYTES {
        if !allow_partial {
            return Content::TooLarge { size };
        }
        // Cut at the last line break before the limit.
        let cut = &bytes[..(MAX_DOC_BYTES as usize).min(bytes.len())];
        let end = cut
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(cut.len(), |i| i + 1);
        text_bytes = &cut[..end];
        partial = Some((end as u64, size));
    }
    let text = String::from_utf8_lossy(text_bytes);
    Content::Doc {
        doc: Arc::new(Doc::parse(&text, true)),
        partial,
    }
}

fn read_capped(p: &Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let f = std::fs::File::open(p).map_err(|e| io_message(&e))?;
    if f.metadata().is_ok_and(|m| m.is_dir()) {
        return Err(msg_key("error.folder"));
    }
    let mut buf = Vec::new();
    f.take(max)
        .read_to_end(&mut buf)
        .map_err(|e| io_message(&e))?;
    Ok(buf)
}

fn io_message(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => msg_key("error.not_found"),
        std::io::ErrorKind::PermissionDenied => msg_key("error.permission"),
        _ => e.to_string(),
    }
}

/// README names, best first (compared case-insensitively).
const README_NAMES: &[&str] = &["readme.md", "readme.markdown", "readme.mdx"];

/// Pick the README from a folder's file names.
pub fn pick_readme<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let names: Vec<&str> = names.into_iter().collect();
    for want in README_NAMES {
        // Exact upper-case "README.md" wins over "readme.md".
        let mut hits: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| n.eq_ignore_ascii_case(want))
            .collect();
        hits.sort_by_key(|n| !n.starts_with("README"));
        if let Some(h) = hits.first() {
            return Some(h);
        }
    }
    None
}

pub fn find_readme(dir: &Path) -> Option<PathBuf> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    pick_readme(names.iter().map(String::as_str)).map(|n| dir.join(n))
}

/// Download an image with the system `curl` (time and size limited).
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args([
            "-sSfL",
            "--proto",
            "=http,https",
            "--max-time",
            "15",
            "--max-filesize",
        ])
        .arg(MAX_IMAGE_BYTES.to_string())
        .arg("--")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if msg.is_empty() {
            msg_key("error.download")
        } else {
            msg
        });
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_names() {
        assert_eq!(
            pick_readme(["a.md", "readme.md", "README.md"]),
            Some("README.md")
        );
        assert_eq!(
            pick_readme(["Readme.markdown", "notes.md"]),
            Some("Readme.markdown")
        );
        assert_eq!(pick_readme(["README.txt", "README"]), None);
        assert_eq!(
            pick_readme(["readme.mdx", "README.markdown"]),
            Some("README.markdown")
        );
    }

    #[test]
    fn size_limits() {
        assert!(matches!(
            content_from(b"# Hi\n", 5, false),
            Content::Doc { partial: None, .. }
        ));
        assert!(matches!(
            content_from(b"a\0b", 3, false),
            Content::Unreadable(_)
        ));
        let big = "line of text\n".repeat((MAX_DOC_BYTES / 13 + 10) as usize);
        let size = big.len() as u64;
        assert!(
            matches!(content_from(big.as_bytes(), size, false), Content::TooLarge { size: s } if s == size)
        );
        let Content::Doc {
            partial: Some((shown, total)),
            ..
        } = content_from(&big.as_bytes()[..MAX_DOC_BYTES as usize + 1], size, true)
        else {
            panic!("partial");
        };
        assert!(shown <= MAX_DOC_BYTES && shown % 13 == 0, "{shown}");
        assert_eq!(total, size);
    }
}
