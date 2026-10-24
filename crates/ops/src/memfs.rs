//! In-memory [`Vfs`] used by the tests (and by other crates with the
//! `testing` feature). Cloning a `MemFs` shares the same tree.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use manta_core::{Entry, EntryKind, Protocol, Vfs, VfsError, VfsPath, VfsResult};

#[derive(Debug, Clone)]
enum Content {
    Dir,
    File(Vec<u8>),
}

#[derive(Debug, Clone)]
struct Node {
    content: Content,
    modified: DateTime<Utc>,
    permissions: u32,
}

#[derive(Debug, Default)]
struct State {
    nodes: BTreeMap<VfsPath, Node>,
    /// Artificial delay per read call (to test cancellation / speed).
    read_delay: Option<Duration>,
    /// Max bytes returned per read call (0 = unlimited).
    read_chunk: usize,
    /// Paths whose `open_read` fails.
    unreadable: Vec<VfsPath>,
    /// When false, `rename` returns `Unsupported`.
    no_rename: bool,
    /// When true, `open_append` returns `Unsupported` (no resume).
    no_append: bool,
    /// While set and closed, writes block (see [`WriteGate`]).
    write_gate: Option<Arc<WriteGate>>,
    /// Write calls that report success without storing anything (a
    /// backend that silently loses data).
    lost_writes: usize,
}

/// Holds writers of a [`MemFs`] at a known point: while closed, every
/// `write` call blocks before it touches the tree. Tests use it to freeze a
/// transfer in the middle of a chunk (no sleeps).
#[derive(Debug, Default)]
pub struct WriteGate {
    /// (closed, writers waiting)
    state: Mutex<(bool, usize)>,
    cv: std::sync::Condvar,
}

impl WriteGate {
    /// A closed gate.
    pub fn closed() -> Arc<WriteGate> {
        Arc::new(WriteGate {
            state: Mutex::new((true, 0)),
            cv: std::sync::Condvar::new(),
        })
    }

    /// Let every writer through (now and later).
    pub fn open(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0 = false;
        self.cv.notify_all();
    }

    /// Writers blocked right now.
    pub fn waiting(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).1
    }

    /// Block until `n` writers wait at the gate (or `timeout` passes;
    /// returns whether they do).
    pub fn wait_for_writers(&self, n: usize, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while st.1 < n {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            st = self
                .cv
                .wait_timeout(st, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }

    fn pass(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !st.0 {
            return;
        }
        st.1 += 1;
        self.cv.notify_all();
        while st.0 {
            st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        st.1 -= 1;
        self.cv.notify_all();
    }
}

#[derive(Clone, Debug)]
pub struct MemFs {
    state: Arc<Mutex<State>>,
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFs {
    pub fn new() -> Self {
        let mut st = State::default();
        st.nodes.insert(
            VfsPath::root(),
            Node {
                content: Content::Dir,
                modified: Utc::now(),
                permissions: 0o755,
            },
        );
        MemFs {
            state: Arc::new(Mutex::new(st)),
        }
    }

    fn st(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Create a directory and all parents.
    pub fn mkdir_p(&self, path: &str) {
        let p = VfsPath::new(path);
        let mut cur = VfsPath::root();
        let mut st = self.st();
        for c in p.components() {
            cur = cur.join(c);
            st.nodes.entry(cur.clone()).or_insert(Node {
                content: Content::Dir,
                modified: Utc::now(),
                permissions: 0o755,
            });
        }
    }

    /// Create a file (and parent dirs) with the given content and mtime.
    pub fn put(&self, path: &str, data: &[u8], modified: DateTime<Utc>) {
        let p = VfsPath::new(path);
        if let Some(parent) = p.parent() {
            self.mkdir_p(parent.as_str());
        }
        self.st().nodes.insert(
            p,
            Node {
                content: Content::File(data.to_vec()),
                modified,
                permissions: 0o644,
            },
        );
    }

    pub fn read_all(&self, path: &str) -> Option<Vec<u8>> {
        match &self.st().nodes.get(&VfsPath::new(path))?.content {
            Content::File(d) => Some(d.clone()),
            Content::Dir => None,
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.st().nodes.contains_key(&VfsPath::new(path))
    }

    pub fn is_dir(&self, path: &str) -> bool {
        matches!(
            self.st().nodes.get(&VfsPath::new(path)),
            Some(Node {
                content: Content::Dir,
                ..
            })
        )
    }

    pub fn permissions_of(&self, path: &str) -> Option<u32> {
        self.st()
            .nodes
            .get(&VfsPath::new(path))
            .map(|n| n.permissions)
    }

    /// All paths (except the root), sorted.
    pub fn paths(&self) -> Vec<String> {
        self.st()
            .nodes
            .keys()
            .filter(|p| !p.is_root())
            .map(|p| p.to_string())
            .collect()
    }

    pub fn set_read_throttle(&self, chunk: usize, delay: Option<Duration>) {
        let mut st = self.st();
        st.read_chunk = chunk;
        st.read_delay = delay;
    }

    pub fn set_unreadable(&self, path: &str) {
        self.st().unreadable.push(VfsPath::new(path));
    }

    /// Make `open_append` fail (a backend that can't continue partial files).
    pub fn disable_append(&self) {
        self.st().no_append = true;
    }

    /// Replace a file's content and mtime (simulates a changed source).
    pub fn modify(&self, path: &str, data: &[u8], modified: DateTime<Utc>) {
        self.put(path, data, modified);
    }

    /// Route every write through `gate` (`None`: no gate).
    pub fn set_write_gate(&self, gate: Option<Arc<WriteGate>>) {
        self.st().write_gate = gate;
    }

    /// The next `n` write calls claim success but store nothing.
    pub fn lose_writes(&self, n: usize) {
        self.st().lost_writes = n;
    }

    pub fn disable_rename(&self) {
        self.st().no_rename = true;
    }

    fn entry(path: &VfsPath, n: &Node) -> Entry {
        let name = path.file_name().to_string();
        Entry {
            hidden: name.starts_with('.'),
            name,
            path: path.clone(),
            kind: match n.content {
                Content::Dir => EntryKind::Directory,
                Content::File(_) => EntryKind::File,
            },
            size: match &n.content {
                Content::Dir => 0,
                Content::File(d) => d.len() as u64,
            },
            modified: Some(n.modified),
            permissions: Some(n.permissions),
            link_target: None,
        }
    }
}

fn require_dir(st: &State, p: &VfsPath) -> VfsResult<()> {
    match st.nodes.get(p) {
        Some(Node {
            content: Content::Dir,
            ..
        }) => Ok(()),
        Some(_) => Err(VfsError::NotADirectory(p.to_string())),
        None => Err(VfsError::NotFound(p.to_string())),
    }
}

struct MemReader {
    data: Vec<u8>,
    pos: usize,
    chunk: usize,
    delay: Option<Duration>,
}

impl Read for MemReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(d) = self.delay {
            std::thread::sleep(d);
        }
        let mut n = buf.len().min(self.data.len() - self.pos);
        if self.chunk > 0 {
            n = n.min(self.chunk);
        }
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Writes go straight into the tree, so partial files are observable.
struct MemWriter {
    fs: MemFs,
    path: VfsPath,
}

impl Write for MemWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let gate = self.fs.st().write_gate.clone();
        if let Some(g) = gate {
            g.pass();
        }
        let mut st = self.fs.st();
        if st.lost_writes > 0 {
            st.lost_writes -= 1;
            return Ok(buf.len());
        }
        match st.nodes.get_mut(&self.path) {
            Some(Node {
                content: Content::File(d),
                modified,
                ..
            }) => {
                d.extend_from_slice(buf);
                *modified = Utc::now();
                Ok(buf.len())
            }
            _ => Err(io::Error::new(io::ErrorKind::NotFound, "file vanished")),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Vfs for MemFs {
    fn label(&self) -> String {
        "Memory".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Local
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let st = self.st();
        require_dir(&st, dir)?;
        Ok(st
            .nodes
            .iter()
            .filter(|(p, _)| !p.is_root() && p.parent().as_ref() == Some(dir))
            .map(|(p, n)| MemFs::entry(p, n))
            .collect())
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let st = self.st();
        st.nodes
            .get(path)
            .map(|n| MemFs::entry(path, n))
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let mut st = self.st();
        if st.nodes.contains_key(path) {
            return Err(VfsError::AlreadyExists(path.to_string()));
        }
        require_dir(&st, &path.parent().unwrap_or_else(VfsPath::root))?;
        st.nodes.insert(
            path.clone(),
            Node {
                content: Content::Dir,
                modified: Utc::now(),
                permissions: 0o755,
            },
        );
        Ok(())
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        let mut st = self.st();
        match st.nodes.get(path) {
            Some(Node {
                content: Content::File(_),
                ..
            }) => {
                st.nodes.remove(path);
                Ok(())
            }
            Some(_) => Err(VfsError::Other(format!("is a directory: {path}"))),
            None => Err(VfsError::NotFound(path.to_string())),
        }
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let mut st = self.st();
        require_dir(&st, path)?;
        if path.is_root() || st.nodes.keys().any(|p| p.parent().as_ref() == Some(path)) {
            return Err(VfsError::DirectoryNotEmpty(path.to_string()));
        }
        st.nodes.remove(path);
        Ok(())
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        let mut st = self.st();
        if st.no_rename {
            return Err(VfsError::Unsupported("rename".into()));
        }
        if !st.nodes.contains_key(from) {
            return Err(VfsError::NotFound(from.to_string()));
        }
        if to.starts_with(from) {
            return Err(VfsError::Other("cannot move into itself".into()));
        }
        require_dir(&st, &to.parent().unwrap_or_else(VfsPath::root))?;
        if st.nodes.contains_key(to) {
            return Err(VfsError::AlreadyExists(to.to_string()));
        }
        let moved: Vec<VfsPath> = st
            .nodes
            .keys()
            .filter(|p| p.starts_with(from))
            .cloned()
            .collect();
        for p in moved {
            let node = st.nodes.remove(&p).expect("present");
            let rest = p.strip_prefix(from).unwrap_or_default();
            let np = if rest.is_empty() {
                to.clone()
            } else {
                to.join(&rest)
            };
            st.nodes.insert(np, node);
        }
        Ok(())
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let st = self.st();
        if st.unreadable.contains(path) {
            return Err(VfsError::PermissionDenied(path.to_string()));
        }
        match st.nodes.get(path) {
            Some(Node {
                content: Content::File(d),
                ..
            }) => Ok(Box::new(MemReader {
                data: d.clone(),
                pos: 0,
                chunk: st.read_chunk,
                delay: st.read_delay,
            })),
            Some(_) => Err(VfsError::Other(format!("is a directory: {path}"))),
            None => Err(VfsError::NotFound(path.to_string())),
        }
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let mut st = self.st();
        require_dir(&st, &path.parent().unwrap_or_else(VfsPath::root))?;
        if let Some(Node {
            content: Content::Dir,
            ..
        }) = st.nodes.get(path)
        {
            return Err(VfsError::Other(format!("is a directory: {path}")));
        }
        let perms = st.nodes.get(path).map_or(0o644, |n| n.permissions);
        st.nodes.insert(
            path.clone(),
            Node {
                content: Content::File(Vec::new()),
                modified: Utc::now(),
                permissions: perms,
            },
        );
        Ok(Box::new(MemWriter {
            fs: self.clone(),
            path: path.clone(),
        }))
    }

    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        let st = self.st();
        if st.unreadable.contains(path) {
            return Err(VfsError::PermissionDenied(path.to_string()));
        }
        match st.nodes.get(path) {
            Some(Node {
                content: Content::File(d),
                ..
            }) if offset as usize <= d.len() => Ok(Box::new(MemReader {
                data: d.clone(),
                pos: offset as usize,
                chunk: st.read_chunk,
                delay: st.read_delay,
            })),
            Some(Node {
                content: Content::File(_),
                ..
            }) => Err(VfsError::Other(format!(
                "{path}: offset {offset} beyond end of file"
            ))),
            Some(_) => Err(VfsError::Other(format!("is a directory: {path}"))),
            None => Err(VfsError::NotFound(path.to_string())),
        }
    }

    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let mut st = self.st();
        if st.no_append {
            return Err(VfsError::Unsupported("append".into()));
        }
        require_dir(&st, &path.parent().unwrap_or_else(VfsPath::root))?;
        match st.nodes.get(path) {
            Some(Node {
                content: Content::Dir,
                ..
            }) => return Err(VfsError::Other(format!("is a directory: {path}"))),
            Some(_) => {}
            None => {
                st.nodes.insert(
                    path.clone(),
                    Node {
                        content: Content::File(Vec::new()),
                        modified: Utc::now(),
                        permissions: 0o644,
                    },
                );
            }
        }
        Ok(Box::new(MemWriter {
            fs: self.clone(),
            path: path.clone(),
        }))
    }

    fn supports_resume(&self) -> manta_core::ResumeCaps {
        let append = !self.st().no_append;
        manta_core::ResumeCaps {
            read_at: true,
            append,
            upload_session: false,
        }
    }

    fn set_permissions(&self, path: &VfsPath, mode: u32) -> VfsResult<()> {
        let mut st = self.st();
        let n = st
            .nodes
            .get_mut(path)
            .ok_or_else(|| VfsError::NotFound(path.to_string()))?;
        n.permissions = mode;
        Ok(())
    }

    fn set_modified(&self, path: &VfsPath, mtime: DateTime<Utc>) -> VfsResult<()> {
        let mut st = self.st();
        let n = st
            .nodes
            .get_mut(path)
            .ok_or_else(|| VfsError::NotFound(path.to_string()))?;
        n.modified = mtime;
        Ok(())
    }
}
