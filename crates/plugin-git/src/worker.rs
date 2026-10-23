//! One background thread per repository.
//!
//! The worker owns the repo's file watcher, runs `git status` (never more
//! than one at a time, since the thread is serial), and pushes decorations
//! and chips for every pane that shows a folder of this repo.
//!
//! Live updates: watcher events are filtered (noise inside `.git` and
//! git-ignored paths are dropped), then debounced: a status runs 300 ms after
//! the last relevant event, or at most 1 s after the first one of a burst.
//! Events that arrive while a status runs queue up in the channel and lead to
//! exactly one more run afterwards.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{Chip, Decoration, HostRequest, PaneId, PluginHandle, ToastLevel};
use notify::{RecursiveMode, Watcher};

use crate::git::{self, DiffRequest, RepoLocation};
use crate::i18n::tr;
use crate::model::{build_chip, view_decorations, ChipInput};
use crate::shared::{lock, DiffSlot, RepoSnapshot, SharedRef};
use crate::status::GitStatus;

/// Quiet time after the last event before a status runs.
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// Upper bound for the delay of a continuous event burst.
pub const MAX_DELAY: Duration = Duration::from_millis(1000);
/// Re-list requests within this time after a status run are debounced.
const RELIST_THROTTLE: Duration = Duration::from_millis(250);
/// Number of commits shown in the panel.
pub const LOG_LEN: usize = 30;
/// Above this many changed paths in one burst we skip the ignore check and
/// just run status.
const MAX_IGNORE_CHECK: usize = 2000;

/// A pane showing a folder of this repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub pane: PaneId,
    pub dir: VfsPath,
    /// Folder as the pane gave it.
    pub local: PathBuf,
    /// Folder relative to the work-tree root (`""` for the root).
    pub rel: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RepoAction {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageAll,
    Commit(String),
}

#[derive(Debug)]
pub enum WorkerMsg {
    Views(Vec<View>),
    /// Run status now. `relist`: the host re-listed a folder, so cached
    /// ignore results are stale.
    Refresh {
        relist: bool,
    },
    /// Paths reported by the watcher. Empty = "something changed, rescan".
    Fs(Vec<PathBuf>),
    Action(RepoAction),
    /// The UI language changed: send decorations and chips again.
    Relocalize,
    /// Load (or clear) the diff preview.
    Diff(Option<DiffRequest>),
    Shutdown,
}

/// Owner side of a worker thread.
pub struct WorkerHandle {
    pub tx: Sender<WorkerMsg>,
    pub stop: Arc<AtomicBool>,
    pub join: JoinHandle<()>,
}

impl WorkerHandle {
    pub fn send(&self, m: WorkerMsg) {
        let _ = self.tx.send(m);
    }

    /// Ask the thread to stop (non-blocking).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(WorkerMsg::Shutdown);
    }
}

pub fn spawn(loc: RepoLocation, handle: PluginHandle, shared: SharedRef) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let w_tx = tx.clone();
    let w_stop = stop.clone();
    let join = std::thread::Builder::new()
        .name(format!(
            "git-{}",
            loc.root
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ))
        .spawn(move || Worker::new(loc, handle, shared, w_tx, w_stop).run(rx))
        .expect("spawn git worker");
    WorkerHandle { tx, stop, join }
}

/// What a burst of watcher events means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relevance {
    Ignore,
    /// HEAD, index, refs, merge/rebase state.
    GitMeta,
    Worktree,
}

/// Classify a watcher path. `root` and `git_dir` must be canonical.
fn relevance(path: &Path, root: &Path, git_dir: &Path) -> Relevance {
    if let Ok(rel) = path.strip_prefix(git_dir) {
        let rel = rel.to_string_lossy();
        if rel.ends_with(".lock") {
            return Relevance::Ignore;
        }
        let first = rel.split('/').next().unwrap_or("");
        return match first {
            "" => Relevance::GitMeta, // the git dir itself
            "index" | "HEAD" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD"
            | "packed-refs" | "refs" | "rebase-merge" | "rebase-apply" => Relevance::GitMeta,
            "info" if rel == "info/exclude" => Relevance::GitMeta,
            _ => Relevance::Ignore,
        };
    }
    match path.strip_prefix(root) {
        // Inner `.git` folders (nested repos, submodules) are noise.
        Ok(rel) if rel.components().any(|c| c.as_os_str() == ".git") => Relevance::Ignore,
        Ok(_) => Relevance::Worktree,
        Err(_) => Relevance::Ignore,
    }
}

fn is_ignore_rules_file(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == ".gitignore") || path.ends_with("info/exclude")
}

struct Sent {
    dir: VfsPath,
    decorations: Vec<(String, Decoration)>,
    chips: Vec<Chip>,
}

struct Worker {
    loc: RepoLocation,
    /// Canonical root / git dir, for matching watcher paths.
    root_c: PathBuf,
    git_dir_c: PathBuf,
    handle: PluginHandle,
    shared: SharedRef,
    tx: Sender<WorkerMsg>,
    stop: Arc<AtomicBool>,
    views: Vec<View>,

    status: Option<GitStatus>,
    snapshot: RepoSnapshot,
    commits_for: Option<Option<String>>,
    /// Ignored entry names per shown folder.
    ignored_cache: HashMap<PathBuf, Arc<HashSet<String>>>,
    /// Known ignored folders (canonical), so build output can be dropped
    /// without asking git again.
    ignored_dirs: HashSet<PathBuf>,
    last_sent: HashMap<PaneId, Sent>,
    current_diff: Option<DiffRequest>,

    // Debounce state.
    pending_fs: Vec<PathBuf>,
    pending_rescan: bool,
    first_event: Option<Instant>,
    last_event: Option<Instant>,
    refresh_now: bool,
    last_run: Option<Instant>,
}

impl Worker {
    fn new(
        loc: RepoLocation,
        handle: PluginHandle,
        shared: SharedRef,
        tx: Sender<WorkerMsg>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        let root_c = std::fs::canonicalize(&loc.root).unwrap_or_else(|_| loc.root.clone());
        let git_dir_c = std::fs::canonicalize(&loc.git_dir).unwrap_or_else(|_| loc.git_dir.clone());
        Worker {
            snapshot: RepoSnapshot::new(loc.clone()),
            loc,
            root_c,
            git_dir_c,
            handle,
            shared,
            tx,
            stop,
            views: Vec::new(),
            status: None,
            commits_for: None,
            ignored_cache: HashMap::new(),
            ignored_dirs: HashSet::new(),
            last_sent: HashMap::new(),
            current_diff: None,
            pending_fs: Vec::new(),
            pending_rescan: false,
            first_event: None,
            last_event: None,
            refresh_now: true,
            last_run: None,
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn start_watcher(&self) -> Option<notify::RecommendedWatcher> {
        let tx = self.tx.clone();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let msg = match res {
                Ok(ev) if ev.need_rescan() => WorkerMsg::Fs(Vec::new()),
                Ok(ev) if ev.paths.is_empty() => return,
                Ok(ev) => WorkerMsg::Fs(ev.paths),
                Err(_) => WorkerMsg::Fs(Vec::new()),
            };
            let _ = tx.send(msg);
        })
        .map_err(|e| log::warn!("git: cannot create file watcher: {e}"))
        .ok()?;
        if let Err(e) = w.watch(&self.loc.root, RecursiveMode::Recursive) {
            log::warn!("git: cannot watch {}: {e}", self.loc.root.display());
            return None;
        }
        if !self.git_dir_c.starts_with(&self.root_c) {
            // Linked worktree / separate git dir.
            let _ = w.watch(&self.loc.git_dir, RecursiveMode::Recursive);
        }
        Some(w)
    }

    fn run(mut self, rx: Receiver<WorkerMsg>) {
        let _watcher = self.start_watcher();
        loop {
            let msg = match self.deadline() {
                _ if self.refresh_now => rx.try_recv().map_err(|e| match e {
                    mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                    mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                }),
                Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match msg {
                Ok(m) => {
                    if !self.handle_msg(m) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // Coalesce everything that is already queued.
            let mut quit = false;
            while let Ok(m) = rx.try_recv() {
                if !self.handle_msg(m) {
                    quit = true;
                    break;
                }
            }
            if quit || self.stopped() {
                break;
            }
            if self.deadline().is_some_and(|d| Instant::now() >= d) {
                self.flush_fs_events();
            }
            if self.refresh_now {
                self.refresh_now = false;
                self.refresh();
            }
        }
        log::debug!("git worker for {} stopped", self.loc.root.display());
    }

    fn deadline(&self) -> Option<Instant> {
        let (first, last) = (self.first_event?, self.last_event?);
        Some((last + DEBOUNCE).min(first + MAX_DELAY))
    }

    /// Returns false to stop.
    fn handle_msg(&mut self, m: WorkerMsg) -> bool {
        match m {
            WorkerMsg::Shutdown => return false,
            WorkerMsg::Views(views) => self.set_views(views),
            WorkerMsg::Refresh { relist } => {
                if relist {
                    self.ignored_cache.clear();
                }
                // A status just finished: fold rapid re-lists (the host may
                // re-list after every file operation) into the debounce.
                if self.last_run.is_some_and(|t| t.elapsed() < RELIST_THROTTLE) {
                    self.pending_rescan = true;
                    let now = Instant::now();
                    self.first_event.get_or_insert(now);
                    self.last_event = Some(now);
                } else {
                    self.refresh_now = true;
                }
            }
            WorkerMsg::Fs(paths) => self.on_fs(paths),
            WorkerMsg::Action(a) => self.run_action(a),
            WorkerMsg::Relocalize => self.publish(),
            WorkerMsg::Diff(req) => {
                self.current_diff = req;
                self.load_diff(true);
            }
        }
        true
    }

    fn set_views(&mut self, views: Vec<View>) {
        // Forget what we sent to panes whose folder changed (the host drops
        // decorations on navigation, so they must be re-sent).
        self.last_sent
            .retain(|pane, sent| views.iter().any(|v| v.pane == *pane && v.dir == sent.dir));
        self.ignored_cache
            .retain(|dir, _| views.iter().any(|v| &v.local == dir));
        self.views = views;
        if self.status.is_some() {
            // Status is kept live by the watcher; just publish for new views.
            self.publish();
        } else {
            self.refresh_now = true;
        }
    }

    fn on_fs(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            self.pending_rescan = true;
        } else {
            let before = self.pending_fs.len();
            for p in paths {
                if relevance(&p, &self.root_c, &self.git_dir_c) != Relevance::Ignore {
                    self.pending_fs.push(p);
                }
            }
            if self.pending_fs.len() == before {
                return;
            }
        }
        let now = Instant::now();
        self.first_event.get_or_insert(now);
        self.last_event = Some(now);
    }

    /// The debounce timer fired: decide whether the burst matters.
    fn flush_fs_events(&mut self) {
        self.first_event = None;
        self.last_event = None;
        let paths = std::mem::take(&mut self.pending_fs);
        if std::mem::take(&mut self.pending_rescan) {
            self.refresh_now = true;
            return;
        }
        let mut worktree: Vec<PathBuf> = Vec::new();
        for p in paths {
            match relevance(&p, &self.root_c, &self.git_dir_c) {
                Relevance::GitMeta => {
                    if p.ends_with("info/exclude") {
                        self.forget_ignores();
                    }
                    self.refresh_now = true;
                }
                Relevance::Worktree => {
                    if is_ignore_rules_file(&p) {
                        self.forget_ignores();
                        self.refresh_now = true;
                    }
                    worktree.push(p);
                }
                Relevance::Ignore => {}
            }
        }
        if self.refresh_now || worktree.is_empty() {
            return;
        }
        worktree.sort();
        worktree.dedup();
        if !self.all_ignored(&worktree) {
            self.refresh_now = true;
        }
    }

    fn forget_ignores(&mut self) {
        self.ignored_cache.clear();
        self.ignored_dirs.clear();
    }

    /// True if every path is git-ignored (e.g. a build writing to `target/`).
    fn all_ignored(&mut self, paths: &[PathBuf]) -> bool {
        if paths.len() > MAX_IGNORE_CHECK {
            return false;
        }
        let unknown: Vec<&PathBuf> = paths
            .iter()
            .filter(|p| !p.ancestors().any(|a| self.ignored_dirs.contains(a)))
            .collect();
        if unknown.is_empty() {
            return true;
        }
        // Ask for each path and its ancestors, so ignored folders are learned
        // and later bursts are answered from the cache.
        let mut query: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for p in &unknown {
            for a in p.ancestors() {
                if a == self.root_c {
                    break;
                }
                let Ok(rel) = a.strip_prefix(&self.root_c) else {
                    break;
                };
                let rel = rel.to_string_lossy().into_owned();
                if seen.insert(rel.clone()) {
                    query.push(rel);
                }
            }
        }
        let Ok(ignored) = git::check_ignored(&self.root_c, &query) else {
            return false;
        };
        for rel in &ignored {
            let abs = self.root_c.join(rel);
            if abs.is_dir() {
                self.ignored_dirs.insert(abs);
            }
        }
        unknown.iter().all(|p| {
            p.ancestors()
                .take_while(|a| *a != self.root_c)
                .filter_map(|a| a.strip_prefix(&self.root_c).ok())
                .any(|rel| ignored.contains(rel.to_string_lossy().as_ref()))
        })
    }

    fn refresh(&mut self) {
        if self.stopped() {
            return;
        }
        self.last_run = Some(Instant::now());
        let status = match git::status(&self.loc.root) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("git status failed in {}: {e}", self.loc.root.display());
                self.snapshot.error = Some(e);
                self.snapshot.generation += 1;
                self.store_snapshot();
                return;
            }
        };
        self.snapshot.error = None;
        self.snapshot.in_progress = git::in_progress(&self.loc.git_dir);
        // `git log` only when HEAD moved.
        if self.commits_for.as_ref() != Some(&status.branch.oid) {
            self.snapshot.commits = Arc::new(if status.branch.oid.is_some() {
                git::log(&self.loc.root, LOG_LEN)
            } else {
                Vec::new()
            });
            self.commits_for = Some(status.branch.oid.clone());
        }
        self.snapshot.status = Some(status.clone());
        self.snapshot.generation += 1;
        self.status = Some(status);
        self.store_snapshot();
        self.publish();
        self.load_diff(false);
    }

    fn store_snapshot(&self) {
        let snap = Arc::new(self.snapshot.clone());
        lock(&self.shared).repos.insert(self.loc.root.clone(), snap);
        self.handle.request_repaint();
    }

    fn ignored_for(&mut self, dir: &Path) -> Arc<HashSet<String>> {
        if let Some(c) = self.ignored_cache.get(dir) {
            return c.clone();
        }
        let names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n != ".git")
                    .collect()
            })
            .unwrap_or_default();
        let set = Arc::new(git::check_ignored(dir, &names).unwrap_or_default());
        self.ignored_cache.insert(dir.to_path_buf(), set.clone());
        set
    }

    /// Send decorations and chips for every view (only what changed).
    fn publish(&mut self) {
        let Some(status) = self.status.clone() else {
            return;
        };
        let last_subject = self.snapshot.commits.first().map(|c| c.subject.clone());
        let chip = build_chip(&ChipInput {
            status: &status,
            in_progress: self.snapshot.in_progress.as_ref(),
            last_subject: last_subject.as_deref(),
        });
        for view in self.views.clone() {
            if self.stopped() {
                return;
            }
            let ignored = self.ignored_for(&view.local);
            let decorations = view_decorations(&status, &view.rel, &ignored);
            let chips = vec![chip.clone()];
            let prev = self.last_sent.get(&view.pane);
            let same_dir = prev.is_some_and(|p| p.dir == view.dir);
            if !same_dir || prev.is_some_and(|p| p.decorations != decorations) {
                self.handle.send(HostRequest::SetDecorations {
                    pane: view.pane,
                    dir: view.dir.clone(),
                    entries: decorations.clone(),
                });
            }
            if !same_dir || prev.is_some_and(|p| p.chips != chips) {
                self.handle.send(HostRequest::SetChips {
                    pane: view.pane,
                    dir: view.dir.clone(),
                    chips: chips.clone(),
                });
            }
            self.last_sent.insert(
                view.pane,
                Sent {
                    dir: view.dir,
                    decorations,
                    chips,
                },
            );
        }
    }

    /// (Re)load the selected diff. `fresh`: a new request (show loading).
    fn load_diff(&mut self, fresh: bool) {
        let Some(req) = self.current_diff.clone() else {
            return;
        };
        if fresh {
            let mut sh = lock(&self.shared);
            if let Some(slot) = &mut sh.diff {
                if slot.req == req && slot.root == self.loc.root && slot.result.is_some() {
                    // Keep showing the old result while reloading.
                } else {
                    slot.result = None;
                }
            }
        }
        let result = git::diff(&self.loc.root, &req).map(Arc::new);
        let mut sh = lock(&self.shared);
        match &mut sh.diff {
            Some(slot)
                if slot.req == req
                    && slot.root == self.loc.root
                    && slot.result.as_ref() != Some(&result) =>
            {
                slot.result = Some(result);
                self.handle.request_repaint();
            }
            // The panel moved on to another file or repo.
            _ => {}
        }
    }

    fn run_action(&mut self, a: RepoAction) {
        lock(&self.shared).busy.insert(self.loc.root.clone());
        self.handle.request_repaint();
        let root = &self.loc.root;
        let initial = self.status.as_ref().is_some_and(|s| s.branch.is_initial());
        let res = match &a {
            RepoAction::Stage(p) => git::stage(root, p).map(|_| None),
            RepoAction::Unstage(p) => git::unstage(root, p, initial).map(|_| None),
            RepoAction::StageAll => git::stage_all(root).map(|_| None),
            RepoAction::Commit(msg) => git::commit(root, msg).map(Some),
        };
        {
            let mut sh = lock(&self.shared);
            sh.busy.remove(root);
            if matches!(a, RepoAction::Commit(_)) && res.is_ok() {
                sh.commit_seq += 1;
            }
        }
        match res {
            Ok(Some(sha)) => {
                let subject = match &a {
                    RepoAction::Commit(m) => m.lines().next().unwrap_or("").to_string(),
                    _ => String::new(),
                };
                self.handle.send(HostRequest::Toast {
                    level: ToastLevel::Success,
                    text: tr!("git.toast.committed", sha = sha, subject = subject),
                });
            }
            Ok(None) => {}
            Err(e) => {
                let text = match &a {
                    RepoAction::Stage(_) | RepoAction::StageAll => {
                        tr!("git.toast.stage_failed", error = e)
                    }
                    RepoAction::Unstage(_) => tr!("git.toast.unstage_failed", error = e),
                    RepoAction::Commit(_) => tr!("git.toast.commit_failed", error = e),
                };
                self.handle.send(HostRequest::Toast {
                    level: ToastLevel::Error,
                    text,
                });
            }
        }
        self.refresh_now = true;
    }
}

/// Make a [`DiffSlot`] for a new selection.
pub fn diff_slot(root: PathBuf, req: DiffRequest) -> DiffSlot {
    DiffSlot {
        root,
        req,
        result: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevance_filters_git_noise() {
        let root = Path::new("/r");
        let gd = Path::new("/r/.git");
        let rel = |p: &str| relevance(Path::new(p), root, gd);
        assert_eq!(rel("/r/src/a.rs"), Relevance::Worktree);
        assert_eq!(rel("/r/.git/index"), Relevance::GitMeta);
        assert_eq!(rel("/r/.git/index.lock"), Relevance::Ignore);
        assert_eq!(rel("/r/.git/HEAD"), Relevance::GitMeta);
        assert_eq!(rel("/r/.git/refs/heads/main"), Relevance::GitMeta);
        assert_eq!(rel("/r/.git/refs/heads/main.lock"), Relevance::Ignore);
        assert_eq!(rel("/r/.git/MERGE_HEAD"), Relevance::GitMeta);
        assert_eq!(rel("/r/.git/rebase-merge/msgnum"), Relevance::GitMeta);
        assert_eq!(rel("/r/.git/objects/ab/cdef"), Relevance::Ignore);
        assert_eq!(rel("/r/.git/logs/HEAD"), Relevance::Ignore);
        assert_eq!(rel("/r/.git/FETCH_HEAD"), Relevance::Ignore);
        assert_eq!(rel("/r/vendor/x/.git/index"), Relevance::Ignore);
        assert_eq!(rel("/elsewhere/file"), Relevance::Ignore);
    }

    #[test]
    fn ignore_rules_files() {
        assert!(is_ignore_rules_file(Path::new("/r/sub/.gitignore")));
        assert!(is_ignore_rules_file(Path::new("/r/.git/info/exclude")));
        assert!(!is_ignore_rules_file(Path::new("/r/gitignore.txt")));
    }
}
