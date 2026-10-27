//! The coordinator thread and its worker pool.
//!
//! The coordinator owns the cache and decides what to measure. It is told
//! which folder each *consumer* (a pane, or the Disk Usage panel) shows,
//! lists that folder, answers from the cache where it can and queues a job
//! per sub-folder otherwise. A small pool of worker threads runs the jobs
//! ([`walker::measure`]) and streams partial totals back.
//!
//! - **Dedup:** one job per path, shared by every consumer that wants it.
//! - **Cancel:** when no consumer wants a job any more (the pane moved on),
//!   its cancel flag is set and the walk stops within a few hundred entries.
//! - **Cache:** per path, keyed by the folder's own mtime/ctime. File
//!   actions invalidate cached ancestors and descendants of what they
//!   touched. A refresh re-measures entries older than [`STALE_AFTER`] in
//!   the background while the cached value stays visible.
//! - **On disk:** the cache is saved ([`crate::store`]) a few seconds after
//!   it changed and on shutdown. Sizes from the last run show at once and
//!   are re-measured quietly the first time they are shown.
//! - **Watched:** the folders shown are watched recursively (FSEvents via
//!   `notify`). A change anywhere inside a sub-folder re-measures that
//!   sub-folder quietly once it has been quiet for [`CHANGE_QUIET`] (at
//!   the latest after [`CHANGE_MAX_DELAY`], and never while it is still
//!   being measured), so a busy folder is not measured over and over.
//! - **Rate limit:** sizes go to the host at most every [`FLUSH_INTERVAL`].

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{HostRequest, PaneId, PluginHandle, SizeInfo};

use crate::store::{self, Stored};
use crate::walker::{self, Category, StampKey, Totals};

/// At most this often partial sizes are sent to the host / the panel.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(200);
/// A refresh re-measures cached folders older than this.
pub const STALE_AFTER: Duration = Duration::from_secs(60);
/// Worker threads.
pub const WORKERS: usize = 3;
/// The cache is saved this long after its first unsaved change.
pub const SAVE_AFTER: Duration = Duration::from_secs(5);
/// A changed sub-folder is re-measured after this long without changes …
pub const CHANGE_QUIET: Duration = Duration::from_secs(1);
/// … or this long after its first change (steady streams).
pub const CHANGE_MAX_DELAY: Duration = Duration::from_secs(5);

/// Who wants sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Consumer {
    Pane(PaneId),
    Panel,
}

pub enum Msg {
    /// `consumer` now shows `dir` (`None`: nothing local, stop).
    /// `refresh`: the same folder was listed again.
    Show {
        consumer: Consumer,
        dir: Option<PathBuf>,
        vfs_dir: Option<VfsPath>,
        refresh: bool,
    },
    /// "Calculate Size": (re)measure these children of the pane's folder.
    Measure {
        pane: PaneId,
        paths: Vec<PathBuf>,
    },
    /// Something changed at these paths (file actions).
    Invalidate(Vec<PathBuf>),
    /// Measure every sub-folder automatically, or only on demand.
    SetAuto(bool),
    /// The watcher saw these paths change (anything inside a shown folder).
    Changed(Vec<PathBuf>),
    Shutdown,
    /// From a worker.
    Progress {
        job: u64,
        path: PathBuf,
        totals: Totals,
    },
}

/// One item of the folder the panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub category: Category,
    pub bytes: u64,
    pub on_disk: u64,
    pub files: u64,
    /// Measured completely (files always are).
    pub complete: bool,
    /// Not measured yet at all.
    pub unknown: bool,
    pub unreadable: u64,
    /// Category with the most bytes inside (folders).
    pub dominant: Category,
}

/// What the Disk Usage panel shows, shared with the UI thread.
#[derive(Debug, Default)]
pub struct PanelData {
    /// The folder the items belong to.
    pub dir: Option<PathBuf>,
    pub items: Vec<Item>,
    /// The folder has been listed.
    pub listed: bool,
    pub error: Option<String>,
    /// Bumped on every change.
    pub version: u64,
}

pub type PanelRef = Arc<Mutex<PanelData>>;

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Handle to the coordinator thread.
pub struct Engine {
    tx: Sender<Msg>,
    pub join: Option<JoinHandle<()>>,
}

impl Engine {
    /// `store`: where the cache is kept between runs (`None`: memory only).
    pub fn start(
        handle: PluginHandle,
        panel: PanelRef,
        auto: bool,
        store: Option<PathBuf>,
    ) -> Engine {
        let (tx, rx) = mpsc::channel();
        let wtx = tx.clone();
        let join = std::thread::Builder::new()
            .name("foldersize".into())
            .spawn(move || Core::new(handle, panel, auto, wtx, store).run(rx))
            .expect("spawn foldersize thread");
        Engine {
            tx,
            join: Some(join),
        }
    }

    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
    }
}

// ------------------------------------------------------------------ watcher

/// The watcher thread: watches the folders it is sent (the newest set wins)
/// and reports changes as [`Msg::Changed`]. Ends with the coordinator.
fn spawn_watcher(tx: Sender<Msg>) -> Sender<HashSet<PathBuf>> {
    let (watch_tx, watch_rx) = mpsc::channel::<HashSet<PathBuf>>();
    let spawned = std::thread::Builder::new()
        .name("foldersize-watch".into())
        .spawn(move || {
            use notify::Watcher;
            let mut w =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if let Ok(ev) = res {
                        if !matches!(ev.kind, notify::EventKind::Access(_)) && !ev.paths.is_empty()
                        {
                            let _ = tx.send(Msg::Changed(ev.paths));
                        }
                    }
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        log::warn!("folder sizes: no file watcher: {e}");
                        return;
                    }
                };
            let mut watched: HashSet<PathBuf> = HashSet::new();
            while let Ok(mut want) = watch_rx.recv() {
                while let Ok(newer) = watch_rx.try_recv() {
                    want = newer;
                }
                for gone in watched.difference(&want) {
                    let _ = w.unwatch(gone);
                }
                for new in want.difference(&watched) {
                    if let Err(e) = w.watch(new, notify::RecursiveMode::Recursive) {
                        log::warn!("folder sizes: can't watch {}: {e}", new.display());
                    }
                }
                watched = want;
            }
        });
    if let Err(e) = spawned {
        log::warn!("folder sizes: no watcher thread: {e}");
    }
    watch_tx
}

// ------------------------------------------------------------------ workers

struct Work {
    job: u64,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Queue {
    q: Mutex<(VecDeque<Work>, bool)>,
    cv: Condvar,
}

impl Queue {
    fn push(&self, w: Work, front: bool) {
        let mut g = lock(&self.q);
        if front {
            g.0.push_front(w);
        } else {
            g.0.push_back(w);
        }
        self.cv.notify_one();
    }

    fn pop(&self) -> Option<Work> {
        let mut g = lock(&self.q);
        loop {
            if g.1 {
                return None;
            }
            if let Some(w) = g.0.pop_front() {
                return Some(w);
            }
            g = self.cv.wait(g).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn shutdown(&self) {
        let mut g = lock(&self.q);
        g.1 = true;
        for w in g.0.drain(..) {
            w.cancel.store(true, Ordering::Relaxed);
        }
        self.cv.notify_all();
    }
}

fn worker(queue: Arc<Queue>, tx: Sender<Msg>) {
    while let Some(w) = queue.pop() {
        if w.cancel.load(Ordering::Relaxed) {
            let totals = Totals {
                cancelled: true,
                ..Default::default()
            };
            let _ = tx.send(Msg::Progress {
                job: w.job,
                path: w.path,
                totals,
            });
            continue;
        }
        let totals = walker::measure(&w.path, &w.cancel, |t| {
            let _ = tx.send(Msg::Progress {
                job: w.job,
                path: w.path.clone(),
                totals: *t,
            });
        });
        let _ = tx.send(Msg::Progress {
            job: w.job,
            path: w.path,
            totals,
        });
    }
}

// ------------------------------------------------------------------ coordinator

struct Job {
    id: u64,
    cancel: Arc<AtomicBool>,
    consumers: HashSet<Consumer>,
    stamp: StampKey,
}

struct Cached {
    stamp: StampKey,
    totals: Totals,
    at: Instant,
    /// Measured in this run (not just loaded from disk).
    verified: bool,
    /// When it was measured (seconds since the epoch, for the disk cache).
    measured: u64,
}

struct Target {
    dir: PathBuf,
    /// `dir` with symlinks resolved, as the watcher reports paths
    /// (`/private/var/…` for `/var/…`).
    real_dir: PathBuf,
    vfs_dir: Option<VfsPath>,
    /// Sub-folders shown: path → name.
    children: HashMap<PathBuf, String>,
    /// Asked for with "Calculate Size" (on-demand mode).
    requested: HashSet<PathBuf>,
    /// Sizes not sent yet.
    pending: BTreeMap<String, SizeInfo>,
}

struct Core {
    handle: PluginHandle,
    panel: PanelRef,
    auto: bool,
    queue: Arc<Queue>,
    workers: Vec<JoinHandle<()>>,
    cache: HashMap<PathBuf, Cached>,
    partial: HashMap<PathBuf, Totals>,
    jobs: HashMap<PathBuf, Job>,
    next_job: u64,
    targets: HashMap<Consumer, Target>,
    panel_dirty: bool,
    last_flush: Instant,
    /// The cache file.
    store: Option<PathBuf>,
    /// The cache changed since it was saved (since when).
    unsaved: Option<Instant>,
    /// To the watcher thread: the folders to watch (recursively). Starting
    /// a watch can take a while, so it never holds up the sizes.
    watch_tx: Sender<HashSet<PathBuf>>,
    watched: HashSet<PathBuf>,
    /// Changed sub-folders waiting to be re-measured: first and last change.
    changed: HashMap<PathBuf, (Instant, Instant)>,
}

fn size_info(t: &Totals) -> SizeInfo {
    SizeInfo {
        bytes: t.logical,
        complete: t.done,
        files: t.files,
        on_disk: Some(t.on_disk),
        unreadable: t.unreadable,
    }
}

fn related(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

impl Core {
    fn new(
        handle: PluginHandle,
        panel: PanelRef,
        auto: bool,
        tx: Sender<Msg>,
        store: Option<PathBuf>,
    ) -> Core {
        let queue = Arc::new(Queue::default());
        let watch_tx = spawn_watcher(tx.clone());
        let workers = (0..WORKERS)
            .map(|i| {
                let q = queue.clone();
                let tx = tx.clone();
                std::thread::Builder::new()
                    .name(format!("foldersize-{i}"))
                    .spawn(move || worker(q, tx))
                    .expect("spawn foldersize worker")
            })
            .collect();
        let now = Instant::now();
        let cache = store
            .as_deref()
            .map(store::load)
            .unwrap_or_default()
            .into_iter()
            .map(|(path, s)| {
                let c = Cached {
                    stamp: s.stamp,
                    totals: s.totals,
                    at: now,
                    verified: false,
                    measured: s.measured,
                };
                (path, c)
            })
            .collect();
        Core {
            handle,
            panel,
            auto,
            queue,
            workers,
            cache,
            partial: HashMap::new(),
            jobs: HashMap::new(),
            next_job: 1,
            targets: HashMap::new(),
            panel_dirty: false,
            last_flush: Instant::now(),
            store,
            unsaved: None,
            watch_tx,
            watched: HashSet::new(),
            changed: HashMap::new(),
        }
    }

    /// Watch exactly the folders the consumers show.
    fn sync_watches(&mut self) {
        let want: HashSet<PathBuf> = self.targets.values().map(|t| t.dir.clone()).collect();
        if want != self.watched {
            self.watched = want.clone();
            let _ = self.watch_tx.send(want);
        }
    }

    /// Note which shown sub-folders the changed paths are in.
    fn note_changes(&mut self, paths: Vec<PathBuf>) {
        let now = Instant::now();
        for p in paths {
            // Our own cache file (someone looking at ~/Library/Caches).
            if let Some(store) = &self.store {
                if p.parent() == store.parent()
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("size-cache"))
                {
                    continue;
                }
            }
            for t in self.targets.values() {
                let Ok(rest) = p
                    .strip_prefix(&t.real_dir)
                    .or_else(|_| p.strip_prefix(&t.dir))
                else {
                    continue;
                };
                let Some(first) = rest.components().next() else {
                    continue; // the shown folder itself
                };
                let child = t.dir.join(first);
                if t.children.contains_key(&child) {
                    let e = self.changed.entry(child).or_insert((now, now));
                    e.1 = now;
                }
            }
        }
    }

    /// When the next noted change is due (for the run loop's timeout).
    fn changes_wait(&self) -> Option<Duration> {
        self.changed
            .values()
            .map(|(first, last)| {
                CHANGE_QUIET
                    .saturating_sub(last.elapsed())
                    .min(CHANGE_MAX_DELAY.saturating_sub(first.elapsed()))
            })
            .min()
            // Still being measured: look again a little later.
            .map(|w| w.max(Duration::from_millis(100)))
    }

    /// Re-measure the changed sub-folders that are due, quietly (their
    /// sizes stay shown). One still being measured waits for its result.
    fn apply_changes(&mut self) {
        let shown: HashSet<&PathBuf> = self
            .targets
            .values()
            .flat_map(|t| t.children.keys())
            .collect();
        self.changed.retain(|p, _| shown.contains(p));
        let due: Vec<PathBuf> = self
            .changed
            .iter()
            .filter(|(p, (first, last))| {
                (last.elapsed() >= CHANGE_QUIET || first.elapsed() >= CHANGE_MAX_DELAY)
                    && !self.jobs.contains_key(*p)
            })
            .map(|(p, _)| p.clone())
            .collect();
        if due.is_empty() {
            return;
        }
        for p in &due {
            self.changed.remove(p);
            if let Some(c) = self.cache.get_mut(p) {
                c.verified = false;
            }
        }
        let all: Vec<Consumer> = self.targets.keys().copied().collect();
        for c in all {
            self.fill(c, false);
        }
    }

    fn mark_unsaved(&mut self) {
        if self.store.is_some() && self.unsaved.is_none() {
            self.unsaved = Some(Instant::now());
        }
    }

    /// Save the cache when it has been unsaved for [`SAVE_AFTER`] (or now).
    fn save(&mut self, now: bool) {
        let (Some(path), Some(since)) = (&self.store, self.unsaved) else {
            return;
        };
        if !now && since.elapsed() < SAVE_AFTER {
            return;
        }
        self.unsaved = None;
        let rows: Vec<(PathBuf, Stored)> = self
            .cache
            .iter()
            .map(|(p, c)| {
                let s = Stored {
                    stamp: c.stamp,
                    totals: c.totals,
                    measured: c.measured,
                };
                (p.clone(), s)
            })
            .collect();
        if let Err(e) = store::save(path, rows.iter().map(|(p, s)| (p, s))) {
            log::warn!("folder size cache {}: {e}", path.display());
        }
    }

    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let busy = self.panel_dirty || self.targets.values().any(|t| !t.pending.is_empty());
            let save_wait = self.unsaved.map(|t| SAVE_AFTER.saturating_sub(t.elapsed()));
            let change_wait = self.changes_wait();
            let msg = if busy || save_wait.is_some() || change_wait.is_some() {
                let mut wait = Duration::MAX;
                if busy {
                    wait = FLUSH_INTERVAL.saturating_sub(self.last_flush.elapsed());
                }
                for w in [save_wait, change_wait].into_iter().flatten() {
                    wait = wait.min(w);
                }
                match rx.recv_timeout(wait) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(m) => Some(m),
                    Err(_) => break,
                }
            };
            let mut urgent = false;
            let mut next = msg;
            // Handle everything that is queued, then flush once.
            while let Some(m) = next {
                match m {
                    Msg::Shutdown => {
                        self.shutdown();
                        return;
                    }
                    Msg::Progress { job, path, totals } => self.progress(job, path, totals),
                    Msg::Changed(paths) => self.note_changes(paths),
                    other => {
                        urgent = true;
                        self.handle_msg(other);
                    }
                }
                next = rx.try_recv().ok();
            }
            self.apply_changes();
            if urgent || self.last_flush.elapsed() >= FLUSH_INTERVAL {
                self.flush();
            }
            self.save(false);
        }
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.save(true);
        for j in self.jobs.values() {
            j.cancel.store(true, Ordering::Relaxed);
        }
        self.queue.shutdown();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }

    fn handle_msg(&mut self, m: Msg) {
        match m {
            Msg::Show {
                consumer,
                dir,
                vfs_dir,
                refresh,
            } => {
                let same = self
                    .targets
                    .get(&consumer)
                    .is_some_and(|t| Some(&t.dir) == dir.as_ref());
                if !same {
                    self.detach(consumer);
                    self.targets.remove(&consumer);
                    if consumer == Consumer::Panel {
                        let mut p = lock(&self.panel);
                        *p = PanelData {
                            dir: dir.clone(),
                            version: p.version + 1,
                            ..Default::default()
                        };
                        self.panel_dirty = true;
                    }
                }
                let Some(dir) = dir else {
                    self.sync_watches();
                    return;
                };
                let t = self.targets.entry(consumer).or_insert_with(|| Target {
                    real_dir: std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone()),
                    dir,
                    vfs_dir: None,
                    children: HashMap::new(),
                    requested: HashSet::new(),
                    pending: BTreeMap::new(),
                });
                t.vfs_dir = vfs_dir;
                self.fill(consumer, refresh);
                self.sync_watches();
            }
            Msg::Measure { pane, paths } => {
                let c = Consumer::Pane(pane);
                let Some(t) = self.targets.get_mut(&c) else {
                    return;
                };
                for p in &paths {
                    t.requested.insert(p.clone());
                }
                for p in &paths {
                    if self.cache.remove(p).is_some() {
                        self.mark_unsaved();
                    }
                    self.restart_job(p);
                }
                self.fill(c, false);
            }
            Msg::Invalidate(paths) => {
                let before = self.cache.len();
                self.cache
                    .retain(|k, _| !paths.iter().any(|p| related(k, p)));
                if self.cache.len() != before {
                    self.mark_unsaved();
                }
                let running: Vec<PathBuf> = self
                    .jobs
                    .keys()
                    .filter(|k| paths.iter().any(|p| related(k, p)))
                    .cloned()
                    .collect();
                for p in running {
                    self.restart_job(&p);
                }
                let all: Vec<Consumer> = self.targets.keys().copied().collect();
                for c in all {
                    self.fill(c, false);
                }
            }
            Msg::SetAuto(auto) => {
                self.auto = auto;
                let all: Vec<Consumer> = self.targets.keys().copied().collect();
                if auto {
                    for c in all {
                        self.fill(c, false);
                    }
                } else {
                    // Stop what the panes asked for automatically.
                    for c in all.into_iter().filter(|c| *c != Consumer::Panel) {
                        let keep = self.targets[&c].requested.clone();
                        let drop: Vec<PathBuf> = self
                            .jobs
                            .iter()
                            .filter(|(p, j)| j.consumers.contains(&c) && !keep.contains(*p))
                            .map(|(p, _)| p.clone())
                            .collect();
                        for p in drop {
                            self.unsubscribe(&p, c);
                        }
                    }
                }
            }
            Msg::Shutdown | Msg::Progress { .. } | Msg::Changed(_) => {}
        }
    }

    /// Cancel a running job for `path` and start it again for the same
    /// consumers (its result would be out of date).
    fn restart_job(&mut self, path: &Path) {
        let Some(j) = self.jobs.remove(path) else {
            return;
        };
        j.cancel.store(true, Ordering::Relaxed);
        self.partial.remove(path);
        let stamp = walker::stamp_of(path).unwrap_or(j.stamp);
        for (i, c) in j.consumers.into_iter().enumerate() {
            if i == 0 {
                self.start_job(path.to_path_buf(), c, stamp, true);
            } else if let Some(nj) = self.jobs.get_mut(path) {
                nj.consumers.insert(c);
            }
        }
    }

    /// Stop wanting everything for `consumer`; cancel orphaned jobs.
    fn detach(&mut self, consumer: Consumer) {
        let paths: Vec<PathBuf> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.consumers.contains(&consumer))
            .map(|(p, _)| p.clone())
            .collect();
        for p in paths {
            self.unsubscribe(&p, consumer);
        }
    }

    fn unsubscribe(&mut self, path: &Path, consumer: Consumer) {
        if let Some(j) = self.jobs.get_mut(path) {
            j.consumers.remove(&consumer);
            if j.consumers.is_empty() {
                j.cancel.store(true, Ordering::Relaxed);
                self.jobs.remove(path);
                self.partial.remove(path);
            }
        }
    }

    fn start_job(&mut self, path: PathBuf, consumer: Consumer, stamp: StampKey, front: bool) {
        if let Some(j) = self.jobs.get_mut(&path) {
            j.consumers.insert(consumer);
            return;
        }
        let id = self.next_job;
        self.next_job += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.insert(
            path.clone(),
            Job {
                id,
                cancel: cancel.clone(),
                consumers: HashSet::from([consumer]),
                stamp,
            },
        );
        self.queue.push(
            Work {
                job: id,
                path,
                cancel,
            },
            front,
        );
    }

    /// List the consumer's folder; answer from the cache, join running jobs
    /// and start the missing ones.
    fn fill(&mut self, consumer: Consumer, refresh: bool) {
        let Some(t) = self.targets.get(&consumer) else {
            return;
        };
        let dir = t.dir.clone();
        let children = match walker::list_dir(&dir) {
            Ok(c) => c,
            Err(e) => {
                if consumer == Consumer::Panel {
                    let mut p = lock(&self.panel);
                    p.listed = true;
                    p.error = Some(e.to_string());
                    p.items.clear();
                    p.version += 1;
                    self.panel_dirty = true;
                }
                return;
            }
        };
        let auto = self.auto || consumer == Consumer::Panel;
        let requested = self.targets[&consumer].requested.clone();
        let mut items = Vec::new();
        let mut shown = HashMap::new();
        for ch in children {
            if !ch.is_dir {
                items.push(Item {
                    name: ch.name,
                    path: ch.path,
                    is_dir: false,
                    category: ch.category,
                    bytes: ch.logical,
                    on_disk: ch.on_disk,
                    files: 1,
                    complete: true,
                    unknown: false,
                    unreadable: 0,
                    dominant: ch.category,
                });
                continue;
            }
            shown.insert(ch.path.clone(), ch.name.clone());
            let wants = auto || requested.contains(&ch.path);
            let mut known: Option<Totals> = None;
            match self.cache.get(&ch.path) {
                Some(c) if c.stamp == ch.stamp => {
                    known = Some(c.totals);
                    // From the last run (something deeper inside may have
                    // changed since), or old on a refresh: re-measure
                    // quietly; the cached value stays shown.
                    let stale = !c.verified || (refresh && c.at.elapsed() > STALE_AFTER);
                    if wants && stale {
                        self.start_job(ch.path.clone(), consumer, ch.stamp, false);
                    } else if let Some(j) = self.jobs.get_mut(&ch.path) {
                        j.consumers.insert(consumer);
                    }
                }
                Some(_) => {
                    // Changed since it was measured.
                    self.cache.remove(&ch.path);
                    self.mark_unsaved();
                }
                None => {}
            }
            if known.is_none() {
                if let Some(j) = self.jobs.get_mut(&ch.path) {
                    j.consumers.insert(consumer);
                    known = self.partial.get(&ch.path).copied();
                } else if wants {
                    self.start_job(ch.path.clone(), consumer, ch.stamp, false);
                }
            }
            items.push(Item {
                name: ch.name.clone(),
                path: ch.path.clone(),
                is_dir: true,
                category: ch.category,
                bytes: known.map_or(0, |k| k.logical),
                on_disk: known.map_or(0, |k| k.on_disk),
                files: known.map_or(0, |k| k.files),
                complete: known.is_some_and(|k| k.done),
                unknown: known.is_none(),
                unreadable: known.map_or(0, |k| k.unreadable),
                dominant: known.map_or(Category::Other, |k| k.dominant()),
            });
            if let (Some(k), Consumer::Pane(_)) = (known, consumer) {
                if let Some(t) = self.targets.get_mut(&consumer) {
                    t.pending.insert(ch.name, size_info(&k));
                }
            }
        }
        if let Some(t) = self.targets.get_mut(&consumer) {
            t.children = shown;
        }
        if consumer == Consumer::Panel {
            let mut p = lock(&self.panel);
            p.dir = Some(dir);
            p.items = items;
            p.listed = true;
            p.error = None;
            p.version += 1;
            self.panel_dirty = true;
        }
    }

    fn progress(&mut self, job: u64, path: PathBuf, totals: Totals) {
        let Some(j) = self.jobs.get(&path) else {
            return;
        };
        if j.id != job {
            return; // from a cancelled run
        }
        if totals.cancelled {
            self.jobs.remove(&path);
            self.partial.remove(&path);
            return;
        }
        let consumers: Vec<Consumer> = j.consumers.iter().copied().collect();
        if totals.done {
            let stamp = j.stamp;
            self.cache.insert(
                path.clone(),
                Cached {
                    stamp,
                    totals,
                    at: Instant::now(),
                    verified: true,
                    measured: store::now(),
                },
            );
            self.mark_unsaved();
            self.jobs.remove(&path);
            self.partial.remove(&path);
        } else {
            self.partial.insert(path.clone(), totals);
            if self.cache.contains_key(&path) {
                return; // re-measuring quietly
            }
        }
        for c in consumers {
            self.report(c, &path, &totals);
        }
    }

    fn report(&mut self, consumer: Consumer, path: &Path, totals: &Totals) {
        let Some(t) = self.targets.get_mut(&consumer) else {
            return;
        };
        if consumer == Consumer::Panel {
            let mut p = lock(&self.panel);
            if let Some(it) = p.items.iter_mut().find(|i| i.path == path) {
                it.bytes = totals.logical;
                it.on_disk = totals.on_disk;
                it.files = totals.files;
                it.complete = totals.done;
                it.unknown = false;
                it.unreadable = totals.unreadable;
                it.dominant = totals.dominant();
                p.version += 1;
                self.panel_dirty = true;
            }
            return;
        }
        if let Some(name) = t.children.get(path) {
            t.pending.insert(name.clone(), size_info(totals));
        }
    }

    fn flush(&mut self) {
        self.last_flush = Instant::now();
        for (c, t) in self.targets.iter_mut() {
            let Consumer::Pane(pane) = *c else { continue };
            if t.pending.is_empty() {
                continue;
            }
            let sizes: Vec<(String, SizeInfo)> =
                std::mem::take(&mut t.pending).into_iter().collect();
            if let Some(dir) = &t.vfs_dir {
                self.handle.send(HostRequest::SetSizes {
                    pane,
                    dir: dir.clone(),
                    sizes,
                });
            }
        }
        if std::mem::take(&mut self.panel_dirty) {
            self.handle.request_repaint();
        }
    }
}
