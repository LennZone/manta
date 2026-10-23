//! The scanner, keep rules, settings and the trash command (no UI here, so
//! it is easy to test).
//!
//! Scanning narrows the candidates in three steps, so big folders stay fast:
//!
//! 1. group regular files by size (sizes that occur once are done);
//! 2. hash the first and last 64 KB of the rest (files up to 128 KB are read
//!    completely, so their hash is already final);
//! 3. hash the remaining candidates completely with SHA-256.
//!
//! Steps 2 and 3 run on a few worker threads (see [`workers`]); every read
//! checks the cancel flag. Hard links to the same inode count once,
//! symlinks are never followed and never reported, empty files are left
//! out, and so are other volumes mounted inside the folder.
//!
//! The hash is SHA-256 (`sha2`), as before: a strong hash, so two files are
//! only reported identical when their bytes are.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::i18n::tr;

/// Bytes hashed at each end in step 2.
pub const QUICK: u64 = 64 * 1024;
/// Read size for full hashes.
const CHUNK: usize = 1024 * 1024;

pub const KEEP_RULES: [&str; 4] = ["oldest", "newest", "shortest", "folder"];
/// The values "Min Size" cycles through.
pub const MIN_SIZES: [u64; 5] = [1, 1024, 100 * 1024, 1024 * 1024, 10 * 1024 * 1024];

/// Folders that are packages in the Finder (shown as one file).
pub const PACKAGE_EXTS: &[&str] = &[
    ".app",
    ".bundle",
    ".framework",
    ".plugin",
    ".kext",
    ".pkg",
    ".mpkg",
    ".photoslibrary",
    ".musiclibrary",
    ".tvlibrary",
    ".imovielibrary",
    ".fcpbundle",
    ".logicx",
    ".band",
    ".xcodeproj",
    ".xcworkspace",
    ".xcarchive",
    ".playground",
    ".rtfd",
    ".pages",
    ".numbers",
    ".key",
    ".sparsebundle",
    ".appex",
    ".prefPane",
    ".qlgenerator",
    ".mdimporter",
    ".saver",
    ".wdgt",
    ".docarchive",
    ".scptd",
    ".lproj",
];

/// `settings.json` in the plugin's data folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub min_size: u64,
    pub include_hidden: bool,
    pub exclude: Vec<String>,
    pub skip_packages: bool,
    pub keep_rule: String,
    pub keep_folder: String,
    pub trash_command: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            min_size: 1024,
            include_hidden: false,
            exclude: ["node_modules", "vendor", ".git", "*.photoslibrary"]
                .map(String::from)
                .to_vec(),
            skip_packages: true,
            keep_rule: "oldest".into(),
            keep_folder: String::new(),
            trash_command: Vec::new(),
        }
    }
}

/// The scan was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// One regular file the scan saw.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub path: PathBuf,
    /// Path relative to the scanned folder, "/"-separated.
    pub rel: String,
    pub size: u64,
    /// Modification time, nanoseconds since the epoch.
    pub mtime_ns: i128,
    pub dev: u64,
    pub ino: u64,
}

impl FileInfo {
    pub fn new(path: impl Into<PathBuf>, rel: &str, size: u64, mtime_secs: i64) -> FileInfo {
        FileInfo {
            path: path.into(),
            rel: rel.into(),
            size,
            mtime_ns: mtime_secs as i128 * 1_000_000_000,
            dev: 0,
            ino: 0,
        }
    }

    /// Seconds since the epoch.
    pub fn mtime(&self) -> i64 {
        self.mtime_ns.div_euclid(1_000_000_000) as i64
    }
}

/// Identical files (2 or more), all `size` bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The hash (hex), unique per group.
    pub key: String,
    pub size: u64,
    pub files: Vec<FileInfo>,
}

impl Group {
    pub fn reclaimable(&self) -> u64 {
        self.size * (self.files.len() as u64).saturating_sub(1)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Regular files considered (size ≥ minimum).
    pub files: u64,
    pub bytes: u64,
    /// Relative paths of excluded folders.
    pub skipped_dirs: Vec<String>,
    pub unreadable: u64,
    pub hard_links: u64,
}

/// Where a scan is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Listing,
    Hashing,
    Verifying,
}

/// Progress callback: (phase, done, total). Called often; keep it cheap.
pub type Progress<'a> = &'a (dyn Fn(Phase, usize, usize) + Sync);

// ------------------------------------------------------------------ settings

fn default_json(key: &str) -> String {
    let v = serde_json::to_value(Settings::default()).unwrap_or_default();
    serde_json::to_string(&v[key]).unwrap_or_default()
}

/// (settings, errors): the defaults overlaid with the file's valid values.
/// A missing file is fine; a broken one gives an error.
pub fn load_settings(path: &Path) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (s, Vec::new()),
        Err(e) => return (s, vec![tr!("settings.broken", error = e)]),
    };
    let data: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return (s, vec![tr!("settings.broken", error = e)]),
    };
    let Some(obj) = data.as_object() else {
        return (s, vec![tr!("settings.not_object").to_string()]);
    };
    let mut errors = Vec::new();
    let strings = |v: &serde_json::Value| -> Option<Vec<String>> {
        v.as_array()?
            .iter()
            .map(|x| x.as_str().map(String::from))
            .collect()
    };
    for (key, value) in obj {
        if key.starts_with('_') {
            continue;
        }
        let ok = match key.as_str() {
            "min_size" => value.as_u64().map(|v| s.min_size = v).is_some(),
            "include_hidden" => value.as_bool().map(|v| s.include_hidden = v).is_some(),
            "skip_packages" => value.as_bool().map(|v| s.skip_packages = v).is_some(),
            "exclude" => strings(value).map(|v| s.exclude = v).is_some(),
            "keep_rule" => value
                .as_str()
                .filter(|r| KEEP_RULES.contains(r))
                .map(|r| s.keep_rule = r.to_string())
                .is_some(),
            "keep_folder" => value
                .as_str()
                .map(|v| s.keep_folder = v.to_string())
                .is_some(),
            "trash_command" => strings(value).map(|v| s.trash_command = v).is_some(),
            _ => continue,
        };
        if !ok {
            errors.push(tr!(
                "settings.bad_value",
                key = key,
                default = default_json(key)
            ));
        }
    }
    (s, errors)
}

/// Write `settings.json` atomically (pretty, with a trailing newline).
pub fn save_settings(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut text = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// ------------------------------------------------------------------ scanning

pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Case-insensitive glob match (`*`, `?`, `[…]`), like Python's
/// `fnmatch.fnmatchcase` on lower-cased strings.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    fn rec(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| rec(rest, &n[i..])),
            Some(('?', rest)) => !n.is_empty() && rec(rest, &n[1..]),
            Some(('[', rest)) => {
                let Some(close) = rest.iter().skip(1).position(|&c| c == ']').map(|i| i + 1) else {
                    return n.first() == Some(&'[') && rec(rest, &n[1..]);
                };
                let Some(&c) = n.first() else { return false };
                let (neg, set) = match rest[..close].split_first() {
                    Some(('!', s)) => (true, s),
                    _ => (false, &rest[..close]),
                };
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    if i + 2 < set.len() && set[i + 1] == '-' {
                        hit |= set[i] <= c && c <= set[i + 2];
                        i += 3;
                    } else {
                        hit |= set[i] == c;
                        i += 1;
                    }
                }
                hit != neg && rec(&rest[close + 1..], &n[1..])
            }
            Some((&c, rest)) => n.first() == Some(&c) && rec(rest, &n[1..]),
        }
    }
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    rec(&p, &n)
}

/// Why the folder `name` is skipped (a short label), or `None`.
pub fn skip_dir(name: &str, s: &Settings) -> Option<String> {
    if !s.include_hidden && is_hidden(name) {
        return Some("hidden".into());
    }
    if let Some(p) = s.exclude.iter().find(|p| glob_match(p, name)) {
        return Some(p.clone());
    }
    if s.skip_packages {
        let lower = name.to_lowercase();
        if PACKAGE_EXTS
            .iter()
            .any(|ext| lower.ends_with(&ext.to_lowercase()) && name.len() > ext.len())
        {
            return Some("package".into());
        }
    }
    None
}

fn mtime_ns(m: &std::fs::Metadata) -> i128 {
    m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128
}

/// Regular files under `root` (recursively), without following symlinks or
/// entering other volumes.
pub fn walk(
    root: &Path,
    s: &Settings,
    cancel: &AtomicBool,
    progress: Progress<'_>,
    stats: &mut Stats,
) -> Result<Vec<FileInfo>, Cancelled> {
    let min_size = s.min_size.max(1);
    let root_dev = std::fs::symlink_metadata(root).ok().map(|m| m.dev());
    let mut out = Vec::new();
    let mut stack = vec![(String::new(), root.to_path_buf())];
    while let Some((rel_dir, path)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }
        let Ok(rd) = std::fs::read_dir(&path) else {
            stats.unreadable += 1;
            continue;
        };
        for e in rd {
            let Ok(e) = e else {
                stats.unreadable += 1;
                continue;
            };
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            // DirEntry::metadata doesn't follow symlinks.
            let Ok(m) = e.metadata() else {
                stats.unreadable += 1;
                continue;
            };
            let ft = m.file_type();
            if ft.is_dir() {
                if skip_dir(&name, s).is_some() || Some(m.dev()) != root_dev {
                    stats.skipped_dirs.push(rel);
                    continue;
                }
                stack.push((rel, e.path()));
            } else if ft.is_file() {
                if !s.include_hidden && is_hidden(&name) {
                    continue;
                }
                if m.len() < min_size {
                    continue;
                }
                stats.files += 1;
                stats.bytes += m.len();
                out.push(FileInfo {
                    path: e.path(),
                    rel,
                    size: m.len(),
                    mtime_ns: mtime_ns(&m),
                    dev: m.dev(),
                    ino: m.ino(),
                });
            } else if ft.is_symlink()
                || ft.is_socket()
                || ft.is_fifo()
                || ft.is_block_device()
                || ft.is_char_device()
            {
                // never followed, never reported
            }
        }
        progress(Phase::Listing, out.len(), 0);
    }
    Ok(out)
}

/// (digest, complete): SHA-256 of the whole file if it is small, else of
/// its size, first and last [`QUICK`] bytes.
fn read_quick(path: &Path, size: u64, cancel: &AtomicBool) -> Result<([u8; 32], bool), ReadErr> {
    if size <= 2 * QUICK {
        return read_full(path, cancel).map(|d| (d, true));
    }
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    h.update(size.to_string().as_bytes());
    let mut buf = vec![0u8; QUICK as usize];
    f.read_exact(&mut buf)?;
    h.update(&buf);
    f.seek(SeekFrom::End(-(QUICK as i64)))?;
    f.read_exact(&mut buf)?;
    h.update(&buf);
    Ok((h.finalize().into(), false))
}

fn read_full(path: &Path, cancel: &AtomicBool) -> Result<[u8; 32], ReadErr> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ReadErr::Cancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

enum ReadErr {
    Io,
    Cancelled,
}

impl From<std::io::Error> for ReadErr {
    fn from(_: std::io::Error) -> Self {
        ReadErr::Io
    }
}

fn hex(d: &[u8; 32]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Worker threads for hashing: a few, since the disk is usually the limit
/// (SSDs gain from parallel reads, spinning disks don't lose much at 4).
pub fn workers() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 6)
}

/// Run `f` on every item on [`workers`] threads; `None` results (unreadable
/// files) are dropped. Results keep the items' order.
fn parallel<R: Send>(
    files: &[FileInfo],
    cancel: &AtomicBool,
    phase: Phase,
    progress: Progress<'_>,
    f: impl Fn(&FileInfo) -> Result<R, ReadErr> + Sync,
) -> Result<Vec<(usize, Option<R>)>, Cancelled> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let total = files.len();
    let out = Mutex::new(Vec::with_capacity(total));
    let n = workers().min(total.max(1));
    std::thread::scope(|scope| {
        for _ in 0..n {
            scope.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(file) = files.get(i) else { return };
                let r = match f(file) {
                    Ok(r) => Some(r),
                    Err(ReadErr::Io) => None,
                    Err(ReadErr::Cancelled) => return,
                };
                out.lock().unwrap_or_else(|e| e.into_inner()).push((i, r));
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(phase, d, total);
            });
        }
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled);
    }
    let mut out = out.into_inner().unwrap_or_else(|e| e.into_inner());
    out.sort_by_key(|(i, _)| *i);
    Ok(out)
}

/// One FileInfo per inode (the one with the shortest path).
fn drop_hard_links(mut files: Vec<FileInfo>, stats: &mut Stats) -> Vec<FileInfo> {
    files.sort_by(|a, b| (a.rel.len(), &a.rel).cmp(&(b.rel.len(), &b.rel)));
    let mut seen = std::collections::HashSet::new();
    files
        .into_iter()
        .filter(|f| {
            if f.ino != 0 && !seen.insert((f.dev, f.ino)) {
                stats.hard_links += 1;
                return false;
            }
            true
        })
        .collect()
}

pub fn sort_groups(groups: &mut [Group]) {
    for g in groups.iter_mut() {
        g.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    }
    groups.sort_by(|a, b| {
        b.reclaimable()
            .cmp(&a.reclaimable())
            .then_with(|| a.files[0].rel.cmp(&b.files[0].rel))
    });
}

/// (groups sorted by reclaimable bytes, stats). `progress` is called with
/// [`Phase::Listing`], [`Phase::Hashing`] and [`Phase::Verifying`].
pub fn find_duplicates(
    root: &Path,
    s: &Settings,
    cancel: &AtomicBool,
    progress: Progress<'_>,
) -> Result<(Vec<Group>, Stats), Cancelled> {
    let mut stats = Stats::default();
    let files = walk(root, s, cancel, progress, &mut stats)?;

    let mut by_size: HashMap<u64, Vec<FileInfo>> = HashMap::new();
    for f in files {
        by_size.entry(f.size).or_default().push(f);
    }
    let mut sizes: Vec<u64> = by_size.keys().copied().collect();
    sizes.sort_unstable();
    let mut candidates = Vec::new();
    for size in sizes {
        let fs = by_size.remove(&size).unwrap_or_default();
        if fs.len() > 1 {
            let fs = drop_hard_links(fs, &mut stats);
            if fs.len() > 1 {
                candidates.extend(fs);
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled);
    }

    // Step 2: quick hash.
    progress(Phase::Hashing, 0, candidates.len());
    let quick = parallel(&candidates, cancel, Phase::Hashing, progress, |f| {
        read_quick(&f.path, f.size, cancel)
    })?;
    let mut by_quick: HashMap<(u64, [u8; 32], bool), Vec<FileInfo>> = HashMap::new();
    let mut order = Vec::new();
    for (i, r) in quick {
        let Some((digest, complete)) = r else {
            stats.unreadable += 1;
            continue;
        };
        let f = candidates[i].clone();
        let key = (f.size, digest, complete);
        if !by_quick.contains_key(&key) {
            order.push(key);
        }
        by_quick.entry(key).or_default().push(f);
    }

    // Step 3: full hash where the quick one isn't final.
    let mut groups = Vec::new();
    let mut todo: Vec<FileInfo> = Vec::new();
    for key in order {
        let fs = by_quick.remove(&key).unwrap_or_default();
        if fs.len() < 2 {
            continue;
        }
        let (size, digest, complete) = key;
        if complete {
            groups.push(Group {
                key: hex(&digest),
                size,
                files: fs,
            });
        } else {
            todo.extend(fs);
        }
    }
    if !todo.is_empty() {
        progress(Phase::Verifying, 0, todo.len());
    }
    let full = parallel(&todo, cancel, Phase::Verifying, progress, |f| {
        read_full(&f.path, cancel)
    })?;
    let mut by_full: HashMap<(u64, [u8; 32]), Vec<FileInfo>> = HashMap::new();
    let mut order = Vec::new();
    for (i, r) in full {
        let Some(digest) = r else {
            stats.unreadable += 1;
            continue;
        };
        let f = todo[i].clone();
        let key = (f.size, digest);
        if !by_full.contains_key(&key) {
            order.push(key);
        }
        by_full.entry(key).or_default().push(f);
    }
    for key in order {
        let fs = by_full.remove(&key).unwrap_or_default();
        if fs.len() > 1 {
            groups.push(Group {
                key: hex(&key.1),
                size: key.0,
                files: fs,
            });
        }
    }
    sort_groups(&mut groups);
    Ok((groups, stats))
}

// ------------------------------------------------------------------ keep rules

pub fn in_folder(path: &Path, folder: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    !folder.is_empty() && path.to_string_lossy().starts_with(&format!("{folder}/"))
}

/// Index of the file to keep in `files` under `rule`.
pub fn keep_index(files: &[FileInfo], rule: &str, folder: &str) -> usize {
    let oldest = |i: &usize| {
        (
            files[*i].mtime_ns,
            files[*i].rel.len(),
            files[*i].rel.clone(),
        )
    };
    let idx = 0..files.len();
    match rule {
        "newest" => {
            idx.min_by_key(|&i| (-files[i].mtime_ns, files[i].rel.len(), files[i].rel.clone()))
        }
        "shortest" => {
            idx.min_by_key(|&i| (files[i].rel.len(), files[i].mtime_ns, files[i].rel.clone()))
        }
        "folder" => {
            let inside: Vec<usize> = idx
                .clone()
                .filter(|&i| in_folder(&files[i].path, folder))
                .collect();
            if inside.is_empty() {
                idx.min_by_key(oldest)
            } else {
                inside.into_iter().min_by_key(oldest)
            }
        }
        _ => idx.min_by_key(oldest),
    }
    .unwrap_or(0)
}

pub fn folder_name(path: &str) -> String {
    let t = path.trim_end_matches('/');
    match t.rsplit_once('/') {
        Some((_, n)) if !n.is_empty() => n.to_string(),
        _ if !t.is_empty() => t.to_string(),
        _ => path.to_string(),
    }
}

pub fn rule_label(rule: &str, folder: &str) -> String {
    match rule {
        "folder" if folder.is_empty() => tr!("rule.unset").into(),
        "folder" => {
            let name = folder_name(folder);
            let name = if name.is_empty() {
                tr!("rule.a_folder").to_string()
            } else {
                name
            };
            tr!("rule.in", name = name)
        }
        "oldest" => tr!("rule.oldest").into(),
        "newest" => tr!("rule.newest").into(),
        "shortest" => tr!("rule.shortest").into(),
        other => other.into(),
    }
}

// ------------------------------------------------------------------ trashing

/// True if `f` is still the regular file the scan saw (same size and
/// modification time).
pub fn unchanged(f: &FileInfo) -> bool {
    match std::fs::symlink_metadata(&f.path) {
        Ok(m) => m.file_type().is_file() && m.len() == f.size && mtime_ns(&m) == f.mtime_ns,
        Err(_) => false,
    }
}

/// The Finder moves the files given as arguments to the Trash (the paths
/// never go into the script text).
pub const OSASCRIPT: &[&str] = &[
    "osascript",
    "-e",
    "on run argv",
    "-e",
    "set l to {}",
    "-e",
    "repeat with p in argv",
    "-e",
    "set end of l to (POSIX file (contents of p))",
    "-e",
    "end repeat",
    "-e",
    "tell application \"Finder\" to delete l",
    "-e",
    "end run",
];

/// `name` on `$PATH` or in the usual tool folders (apps started from the
/// Finder have a short `$PATH`).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for extra in ["/usr/bin", "/bin", "/opt/homebrew/bin", "/usr/local/bin"] {
        if !dirs.iter().any(|d| d == Path::new(extra)) {
            dirs.push(extra.into());
        }
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| {
        std::fs::metadata(p).is_ok_and(|m| {
            m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0
        })
    })
}

/// The command that moves `paths` to the Trash: the settings'
/// `trash_command` (paths appended), else the `trash` tool, else the Finder
/// via osascript. Never a permanent delete.
pub fn trash_argv(
    s: &Settings,
    paths: &[PathBuf],
    which: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<String> {
    let paths = paths.iter().map(|p| p.to_string_lossy().into_owned());
    if !s.trash_command.is_empty() {
        return s.trash_command.iter().cloned().chain(paths).collect();
    }
    if let Some(tool) = which("trash") {
        return std::iter::once(tool.to_string_lossy().into_owned())
            .chain(paths)
            .collect();
    }
    OSASCRIPT
        .iter()
        .map(|s| s.to_string())
        .chain(paths)
        .collect()
}

// ------------------------------------------------------------------ formatting

/// 1000-based like the Finder: "1.5 KB", "12 MB", "1 byte".
pub fn human_size(n: u64) -> String {
    if n < 1000 {
        return tr!("bytes", count = n);
    }
    let mut v = n as f64;
    for i in 0..4 {
        let unit = manta_plugin::i18n::size_unit(i);
        v /= 1000.0;
        if v < 1000.0 || i == 3 {
            let s = if v >= 100.0 {
                format!("{v:.0} {unit}")
            } else {
                format!("{v:.1} {unit}")
            };
            return manta_plugin::i18n::localize_decimal(&s);
        }
    }
    n.to_string()
}

pub fn thousands(n: u64) -> String {
    manta_plugin::i18n::format_count(n)
}

/// "28 Sep, 14:05" this year, "28 Sep 2024" before.
pub fn short_date(secs: i64, now: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let (Some(t), Some(n)) = (
        Local.timestamp_opt(secs, 0).single(),
        Local.timestamp_opt(now, 0).single(),
    ) else {
        return String::new();
    };
    let month = crate::i18n::month(t.month());
    if t.year() == n.year() {
        tr!(
            "date.this_year",
            day = t.day(),
            month = month,
            time = t.format("%H:%M")
        )
    } else {
        tr!(
            "date.other_year",
            day = t.day(),
            month = month,
            year = t.year()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_plugin::i18n::{with_lang, Lang};
    use std::time::{Duration, UNIX_EPOCH};

    fn settings() -> Settings {
        Settings {
            min_size: 1,
            ..Settings::default()
        }
    }

    struct Tree {
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    impl Tree {
        fn new() -> Tree {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            Tree { _tmp: tmp, root }
        }

        fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, data).unwrap();
            p
        }

        fn write_at(&self, rel: &str, data: &[u8], mtime: u64) -> PathBuf {
            let p = self.write(rel, data);
            let t = UNIX_EPOCH + Duration::from_secs(mtime);
            File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
            p
        }

        fn scan(&self, s: &Settings) -> (Vec<Group>, Stats) {
            find_duplicates(&self.root, s, &AtomicBool::new(false), &|_, _, _| {}).unwrap()
        }
    }

    fn rels(groups: &[Group]) -> Vec<Vec<String>> {
        let mut v: Vec<Vec<String>> = groups
            .iter()
            .map(|g| {
                let mut r: Vec<String> = g.files.iter().map(|f| f.rel.clone()).collect();
                r.sort();
                r
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn identical_files_are_grouped_and_same_size_different_content_not() {
        let t = Tree::new();
        t.write("a.txt", b"hello world");
        t.write("sub/b.txt", b"hello world");
        t.write("sub/deeper/c.txt", b"hello world");
        t.write("d.txt", b"hello WORLD");
        t.write("unique.txt", b"something else entirely");
        let (groups, stats) = t.scan(&settings());
        assert_eq!(
            rels(&groups),
            vec![vec!["a.txt", "sub/b.txt", "sub/deeper/c.txt"]]
        );
        assert_eq!(groups[0].size, 11);
        assert_eq!(groups[0].reclaimable(), 22);
        assert_eq!(stats.files, 5);
    }

    #[test]
    fn big_files_with_same_ends_but_other_middle_are_not_duplicates() {
        let t = Tree::new();
        let q = QUICK as usize;
        let make = |mid: u8| [vec![b'h'; q], vec![mid; 5000], vec![b't'; q]].concat();
        t.write("x.bin", &make(b'A'));
        t.write("y.bin", &make(b'B'));
        t.write("z.bin", &make(b'A'));
        assert_eq!(rels(&t.scan(&settings()).0), vec![vec!["x.bin", "z.bin"]]);
    }

    #[test]
    fn many_files_hash_in_parallel_and_group_correctly() {
        let t = Tree::new();
        let q = QUICK as usize;
        for i in 0..60 {
            // 3 contents over 128 KB (quick hash, then full), 2 small ones.
            let big = [vec![(i % 3) as u8; 2 * q], vec![7; 10]].concat();
            t.write(&format!("big/{i:02}.bin"), &big);
            t.write(
                &format!("small/{i:02}.txt"),
                format!("small {}", i % 2).as_bytes(),
            );
        }
        let (groups, _) = t.scan(&settings());
        assert_eq!(groups.len(), 5);
        assert_eq!(groups.iter().map(|g| g.files.len()).sum::<usize>(), 120);
        assert!(groups[0].size > 2 * QUICK, "the big ones first");
    }

    #[test]
    fn empty_files_and_files_below_the_minimum_are_left_out() {
        let t = Tree::new();
        t.write("e1", b"");
        t.write("e2", b"");
        t.write("s1", b"tiny");
        t.write("s2", b"tiny");
        assert_eq!(
            rels(
                &t.scan(&Settings {
                    min_size: 0,
                    ..settings()
                })
                .0
            ),
            vec![vec!["s1", "s2"]]
        );
        assert!(t
            .scan(&Settings {
                min_size: 1024,
                ..settings()
            })
            .0
            .is_empty());
    }

    #[test]
    fn hard_links_count_once() {
        let t = Tree::new();
        let a = t.write("a.dat", b"same bytes here");
        std::fs::hard_link(&a, t.root.join("a-link.dat")).unwrap();
        let (groups, stats) = t.scan(&settings());
        assert!(groups.is_empty());
        assert_eq!(stats.hard_links, 1);
        t.write("copy.dat", b"same bytes here");
        let (groups, _) = t.scan(&settings());
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].files.len(),
            2,
            "the link and its original are one file"
        );
        assert!(groups[0].files.iter().any(|f| f.rel == "copy.dat"));
    }

    #[test]
    fn symlinks_are_skipped() {
        let t = Tree::new();
        let a = t.write("a.txt", b"linked content");
        std::os::unix::fs::symlink(&a, t.root.join("link.txt")).unwrap();
        t.write("real/b.txt", b"other content!");
        std::os::unix::fs::symlink(t.root.join("real"), t.root.join("dirlink")).unwrap();
        let (groups, stats) = t.scan(&settings());
        assert!(groups.is_empty());
        assert_eq!(
            stats.files, 2,
            "neither the file link nor the folder link is followed"
        );
    }

    #[test]
    fn groups_are_sorted_by_reclaimable_bytes() {
        let t = Tree::new();
        for n in 0..3 {
            t.write(&format!("small{n}"), &[b'x'; 10]);
        }
        for n in 0..2 {
            t.write(&format!("big{n}"), &[b'y'; 100]);
        }
        let (groups, _) = t.scan(&settings());
        assert_eq!(
            groups.iter().map(Group::reclaimable).collect::<Vec<_>>(),
            vec![100, 20]
        );
    }

    #[test]
    fn hidden_files_only_when_asked() {
        let t = Tree::new();
        t.write(".a", b"hidden twin");
        t.write(".cache/b", b"hidden twin");
        t.write("c", b"hidden twin");
        assert!(t.scan(&settings()).0.is_empty());
        assert_eq!(
            rels(
                &t.scan(&Settings {
                    include_hidden: true,
                    ..settings()
                })
                .0
            ),
            vec![vec![".a", ".cache/b", "c"]]
        );
    }

    #[test]
    fn default_exclusions_and_packages() {
        let t = Tree::new();
        let data = b"duplicate payload";
        t.write("keep/a.txt", data);
        for rel in [
            "node_modules/x/a.txt",
            "vendor/a.txt",
            "Photos Library.photoslibrary/a.txt",
            "Tool.app/Contents/a.txt",
            "Sub/NODE_MODULES/a.txt",
        ] {
            t.write(rel, data);
        }
        let (groups, stats) = t.scan(&settings());
        assert!(groups.is_empty());
        let mut skipped = stats.skipped_dirs.clone();
        skipped.sort();
        assert_eq!(
            skipped,
            vec![
                "Photos Library.photoslibrary",
                "Sub/NODE_MODULES",
                "Tool.app",
                "node_modules",
                "vendor"
            ]
        );
        t.write("other/b.txt", data);
        assert_eq!(
            rels(&t.scan(&settings()).0),
            vec![vec!["keep/a.txt", "other/b.txt"]]
        );
    }

    #[test]
    fn exclusions_are_configurable() {
        let t = Tree::new();
        t.write("node_modules/a.txt", b"dup dup dup");
        t.write("build/a.txt", b"dup dup dup");
        t.write("Tool.app/a.txt", b"dup dup dup");
        let s = Settings {
            exclude: vec!["build".into()],
            skip_packages: false,
            ..settings()
        };
        assert_eq!(
            rels(&t.scan(&s).0),
            vec![vec!["Tool.app/a.txt", "node_modules/a.txt"]]
        );
    }

    #[test]
    fn skip_dir_reasons() {
        let s = settings();
        assert_eq!(skip_dir(".git", &s).as_deref(), Some("hidden"));
        assert_eq!(
            skip_dir(
                ".git",
                &Settings {
                    include_hidden: true,
                    ..s.clone()
                }
            )
            .as_deref(),
            Some(".git")
        );
        assert_eq!(skip_dir("My.app", &s).as_deref(), Some("package"));
        assert_eq!(
            skip_dir(
                ".app",
                &Settings {
                    include_hidden: true,
                    ..s.clone()
                }
            ),
            None,
            "a bare extension isn't a package"
        );
        assert_eq!(skip_dir("src", &s), None);
        assert!(glob_match(
            "*.photoslibrary",
            "Photos Library.PHOTOSLIBRARY"
        ));
        assert!(glob_match("b?ild", "build"));
        assert!(glob_match("[a-c]x", "bx"));
        assert!(!glob_match("[!a-c]x", "bx"));
        assert!(!glob_match("build", "builder"));
    }

    #[test]
    fn cancel_during_hashing_and_before_start() {
        let t = Tree::new();
        for i in 0..40 {
            t.write(&format!("f{i:02}"), format!("same {}", i % 2).as_bytes());
        }
        let cancel = AtomicBool::new(false);
        let seen = Mutex::new(Vec::new());
        let progress = |phase: Phase, _: usize, _: usize| {
            seen.lock().unwrap().push(phase);
            if phase == Phase::Hashing {
                cancel.store(true, Ordering::Relaxed);
            }
        };
        assert_eq!(
            find_duplicates(&t.root, &settings(), &cancel, &progress),
            Err(Cancelled)
        );
        assert!(seen.lock().unwrap().contains(&Phase::Hashing));
        let cancel = AtomicBool::new(true);
        assert_eq!(
            find_duplicates(&t.root, &settings(), &cancel, &|_, _, _| {}),
            Err(Cancelled)
        );
    }

    fn keep_files() -> Vec<FileInfo> {
        vec![
            FileInfo::new("/r/b/long/name.txt", "b/long/name.txt", 5, 300),
            FileInfo::new("/r/a.txt", "a.txt", 5, 200),
            FileInfo::new("/r/photos/a.txt", "photos/a.txt", 5, 100),
            FileInfo::new("/r/photos/x/a.txt", "photos/x/a.txt", 5, 100),
        ]
    }

    #[test]
    fn keep_rules() {
        let fs = keep_files();
        let rel = |i: usize| fs[i].rel.as_str();
        assert_eq!(
            rel(keep_index(&fs, "oldest", "")),
            "photos/a.txt",
            "ties: shorter path"
        );
        assert_eq!(rel(keep_index(&fs, "newest", "")), "b/long/name.txt");
        assert_eq!(rel(keep_index(&fs, "shortest", "")), "a.txt");
        assert_eq!(rel(keep_index(&fs, "folder", "/r/b")), "b/long/name.txt");
        assert_eq!(
            rel(keep_index(&fs, "folder", "/r/photos/x/")),
            "photos/x/a.txt"
        );
        assert_eq!(
            rel(keep_index(&fs, "folder", "/r/pho")),
            "photos/a.txt",
            "prefix isn't a folder: oldest"
        );
        assert_eq!(rel(keep_index(&fs, "folder", "")), "photos/a.txt");
    }

    #[test]
    fn rule_labels() {
        with_lang(Lang::En, || {
            assert_eq!(rule_label("folder", "/Users/anna/Photos/"), "In “Photos”");
            assert_eq!(rule_label("folder", ""), "In Folder (not set)");
            assert_eq!(rule_label("shortest", ""), "Shortest Path");
        });
        with_lang(Lang::De, || {
            assert_eq!(rule_label("folder", "/x/Fotos"), "In „Fotos“")
        });
    }

    #[test]
    fn settings_overlay_and_errors() {
        with_lang(Lang::En, || {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("settings.json");
            assert_eq!(load_settings(&p), (Settings::default(), vec![]));
            std::fs::write(&p, r#"{"min_size": 0, "keep_rule": "biggest", "include_hidden": true, "x": 1, "_note": "hi"}"#)
                .unwrap();
            let (s, errors) = load_settings(&p);
            assert_eq!(s.min_size, 0);
            assert!(s.include_hidden);
            assert_eq!(s.keep_rule, "oldest");
            assert_eq!(
                errors,
                vec!["settings.json: bad value for keep_rule (default \"oldest\" used)"]
            );
            save_settings(&p, &s).unwrap();
            assert_eq!(load_settings(&p), (s, vec![]));
            std::fs::write(&p, "[1]").unwrap();
            assert_eq!(
                load_settings(&p).1,
                vec!["settings.json must hold an object"]
            );
            std::fs::write(&p, "{broken").unwrap();
            assert!(load_settings(&p).1[0].starts_with("settings.json: "));
        });
    }

    #[test]
    fn settings_file_format_matches_the_python_plugin() {
        // Old settings.json files (written by the Python plugin) load as is.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(
            &p,
            "{\n  \"min_size\": 100,\n  \"include_hidden\": false,\n  \"exclude\": [\"build\"],\n  \"skip_packages\": true,\n  \"keep_rule\": \"folder\",\n  \"keep_folder\": \"/Users/anna/Fotos\",\n  \"trash_command\": []\n}\n",
        )
        .unwrap();
        let (s, errors) = load_settings(&p);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            (s.min_size, s.keep_rule.as_str(), s.keep_folder.as_str()),
            (100, "folder", "/Users/anna/Fotos")
        );
        assert_eq!(s.exclude, vec!["build"]);
    }

    #[test]
    fn trash_command_never_deletes() {
        let paths = vec![PathBuf::from("/a b/c.txt"), PathBuf::from("/d.txt")];
        let custom = Settings {
            trash_command: vec!["mytrash".into(), "-v".into()],
            ..Settings::default()
        };
        assert_eq!(
            trash_argv(&custom, &paths, |_| None),
            vec!["mytrash", "-v", "/a b/c.txt", "/d.txt"]
        );
        assert_eq!(
            trash_argv(&Settings::default(), &paths, |_| Some(
                "/usr/bin/trash".into()
            )),
            vec!["/usr/bin/trash", "/a b/c.txt", "/d.txt"]
        );
        let argv = trash_argv(&Settings::default(), &paths, |_| None);
        assert_eq!(argv[0], "osascript");
        assert_eq!(
            &argv[argv.len() - 2..],
            ["/a b/c.txt", "/d.txt"],
            "paths go in as arguments, never into the script"
        );
        assert!(argv
            .iter()
            .any(|a| a == "tell application \"Finder\" to delete l"));
        assert!(!argv.iter().any(|a| a == "rm" || a == "-rf"));
    }

    #[test]
    fn unchanged_checks_size_and_date() {
        let t = Tree::new();
        let p = t.write_at("a", b"abc", 1000);
        let m = std::fs::symlink_metadata(&p).unwrap();
        let f = FileInfo {
            path: p.clone(),
            rel: "a".into(),
            size: m.len(),
            mtime_ns: mtime_ns(&m),
            dev: 0,
            ino: 0,
        };
        assert!(unchanged(&f));
        t.write_at("a", b"abcd", 1000);
        assert!(!unchanged(&f));
        std::fs::remove_file(&p).unwrap();
        assert!(!unchanged(&f));
    }

    #[test]
    fn formatting() {
        with_lang(Lang::En, || {
            assert_eq!(human_size(1), "1 byte");
            assert_eq!(human_size(999), "999 bytes");
            assert_eq!(human_size(1500), "1.5 KB");
            assert_eq!(human_size(605_000), "605 KB");
            assert_eq!(human_size(1_234_000_000), "1.2 GB");
            assert_eq!(thousands(1240), "1,240");
            assert_eq!(tr!("files", count = 1240), "1,240 files");
        });
        with_lang(Lang::De, || {
            assert_eq!(human_size(1500), "1,5 KB");
            assert_eq!(tr!("files", count = 1240), "1.240 Dateien");
            assert_eq!(human_size(3), "3 Byte");
        });
        assert_eq!(folder_name("/Users/anna/Photos/"), "Photos");
        assert_eq!(folder_name("/"), "/");
    }

    #[test]
    fn dates() {
        use chrono::{Local, TimeZone};
        let now = Local
            .with_ymd_and_hms(2026, 10, 1, 12, 0, 0)
            .unwrap()
            .timestamp();
        let this_year = Local
            .with_ymd_and_hms(2026, 9, 28, 14, 5, 0)
            .unwrap()
            .timestamp();
        let before = Local
            .with_ymd_and_hms(2024, 3, 2, 9, 0, 0)
            .unwrap()
            .timestamp();
        with_lang(Lang::En, || {
            assert_eq!(short_date(this_year, now), "28 Sep, 14:05");
            assert_eq!(short_date(before, now), "2 Mar 2024");
        });
        with_lang(Lang::De, || {
            assert_eq!(short_date(this_year, now), "28. Sept., 14:05");
            assert_eq!(short_date(before, now), "2. März 2024");
        });
    }
}
