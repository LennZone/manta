//! The coordinator thread. The plugin (UI thread) only sends messages here;
//! this thread detects repositories (cached per folder), owns one
//! [`worker`](crate::worker) per repository that a pane shows, and drops a
//! worker together with its watcher when no pane shows that repo anymore.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use manta_core::VfsPath;
use manta_plugin::{HostRequest, PaneId, PaneState, PluginHandle};

use crate::git::{self, DiffRequest, RepoLocation};
use crate::shared::{lock, PaneRepo, SharedRef};
use crate::worker::{self, RepoAction, View, WorkerHandle, WorkerMsg};

#[derive(Debug)]
pub enum EngineMsg {
    /// A pane shows `state`. `refresh`: the listing was re-read.
    Pane {
        state: PaneState,
        refresh: bool,
    },
    /// Refresh every repo (the "Refresh Git Status" command).
    RefreshAll,
    /// The UI language changed: re-send translated decorations and chips.
    Relocalize,
    Action {
        root: PathBuf,
        action: RepoAction,
    },
    Diff {
        root: PathBuf,
        req: Option<DiffRequest>,
    },
    Shutdown,
}

pub struct Engine {
    pub tx: Sender<EngineMsg>,
    pub join: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn start(handle: PluginHandle, shared: SharedRef) -> Engine {
        let (tx, rx) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name("git-engine".into())
            .spawn(move || Coordinator::new(handle, shared).run(rx))
            .expect("spawn git engine");
        Engine {
            tx,
            join: Some(join),
        }
    }

    pub fn send(&self, m: EngineMsg) {
        let _ = self.tx.send(m);
    }
}

struct PaneEntry {
    state: PaneState,
    /// Repo root of the shown folder, with the folder's path relative to it.
    repo: Option<(PathBuf, String)>,
}

struct Coordinator {
    handle: PluginHandle,
    shared: SharedRef,
    panes: HashMap<PaneId, PaneEntry>,
    /// Detection cache: folder → repository (or none).
    detected: HashMap<PathBuf, Option<RepoLocation>>,
    workers: HashMap<PathBuf, WorkerHandle>,
    /// Stopped workers that may still be finishing a git command.
    retired: Vec<JoinHandle<()>>,
}

impl Coordinator {
    fn new(handle: PluginHandle, shared: SharedRef) -> Self {
        Coordinator {
            handle,
            shared,
            panes: HashMap::new(),
            detected: HashMap::new(),
            workers: HashMap::new(),
            retired: Vec::new(),
        }
    }

    fn run(mut self, rx: Receiver<EngineMsg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                EngineMsg::Shutdown => break,
                EngineMsg::Pane { state, refresh } => self.on_pane(state, refresh),
                EngineMsg::RefreshAll => {
                    self.detected.clear();
                    let states: Vec<PaneState> =
                        self.panes.values().map(|p| p.state.clone()).collect();
                    for s in states {
                        self.on_pane(s, true);
                    }
                }
                EngineMsg::Relocalize => {
                    for w in self.workers.values() {
                        w.send(WorkerMsg::Relocalize);
                    }
                }
                EngineMsg::Action { root, action } => {
                    if let Some(w) = self.workers.get(&root) {
                        w.send(WorkerMsg::Action(action));
                    }
                }
                EngineMsg::Diff { root, req } => {
                    if let Some(w) = self.workers.get(&root) {
                        w.send(WorkerMsg::Diff(req));
                    }
                }
            }
            self.retired.retain(|j| !j.is_finished());
        }
        // Shutdown: stop all workers, then wait for them (they exit after
        // their current git command at the latest).
        for (_, w) in self.workers.drain() {
            w.stop();
            self.retired.push(w.join);
        }
        for j in self.retired.drain(..) {
            let _ = j.join();
        }
        let mut sh = lock(&self.shared);
        sh.repos.clear();
        sh.panes.clear();
        sh.diff = None;
    }

    fn set_pane_repo(&self, pane: PaneId, r: PaneRepo) {
        lock(&self.shared).panes.insert(pane, r);
        self.handle.request_repaint();
    }

    fn clear_pane(&self, pane: PaneId, dir: &VfsPath) {
        self.handle.send(HostRequest::SetDecorations {
            pane,
            dir: dir.clone(),
            entries: Vec::new(),
        });
        self.handle.send(HostRequest::SetChips {
            pane,
            dir: dir.clone(),
            chips: Vec::new(),
        });
    }

    fn detect(&mut self, dir: &Path, refresh: bool) -> Option<RepoLocation> {
        match self.detected.get(dir) {
            // A refresh re-checks negatives (`git init` may have happened).
            Some(None) if refresh => {}
            Some(hit) => return hit.clone(),
            None => {}
        }
        let res = git::detect(dir).unwrap_or_else(|e| {
            log::warn!("git: {e}");
            None
        });
        self.detected.insert(dir.to_path_buf(), res.clone());
        res
    }

    fn on_pane(&mut self, state: PaneState, refresh: bool) {
        let pane = state.pane;
        let old_root = self
            .panes
            .get(&pane)
            .and_then(|p| p.repo.as_ref().map(|r| r.0.clone()));
        let Some(local) = state.local_path.clone() else {
            self.set_pane_repo(pane, PaneRepo::NotLocal);
            self.clear_pane(pane, &state.path);
            self.panes.insert(pane, PaneEntry { state, repo: None });
            self.update_workers(old_root.as_deref(), None);
            return;
        };
        if !self.detected.contains_key(&local) {
            self.set_pane_repo(pane, PaneRepo::Detecting);
        }
        let loc = self.detect(&local, refresh);
        let Some(loc) = loc else {
            self.set_pane_repo(pane, PaneRepo::NotRepo);
            self.clear_pane(pane, &state.path);
            self.panes.insert(pane, PaneEntry { state, repo: None });
            self.update_workers(old_root.as_deref(), None);
            return;
        };
        let rel = relative_to(&local, &loc.root);
        let root = loc.root.clone();
        if !self.workers.contains_key(&root) {
            let w = worker::spawn(loc, self.handle.clone(), self.shared.clone());
            self.workers.insert(root.clone(), w);
        }
        self.set_pane_repo(
            pane,
            PaneRepo::Repo {
                root: root.clone(),
                rel: rel.clone(),
            },
        );
        self.panes.insert(
            pane,
            PaneEntry {
                state,
                repo: Some((root.clone(), rel)),
            },
        );
        self.update_workers(old_root.as_deref(), Some(&root));
        if refresh {
            if let Some(w) = self.workers.get(&root) {
                w.send(WorkerMsg::Refresh { relist: true });
            }
        }
    }

    /// Send fresh view lists to the affected workers; drop unused ones.
    fn update_workers(&mut self, old: Option<&Path>, new: Option<&Path>) {
        let mut roots: Vec<PathBuf> = Vec::new();
        roots.extend(old.map(Path::to_path_buf));
        if let Some(n) = new {
            if Some(n) != old {
                roots.push(n.to_path_buf());
            }
        }
        // The same root also gets its view list (the folder may differ).
        for root in roots {
            let views: Vec<View> = self
                .panes
                .values()
                .filter_map(|p| {
                    let (r, rel) = p.repo.as_ref()?;
                    (r == &root).then(|| View {
                        pane: p.state.pane,
                        dir: p.state.path.clone(),
                        local: p.state.local_path.clone().unwrap_or_default(),
                        rel: rel.clone(),
                    })
                })
                .collect();
            if views.is_empty() {
                if let Some(w) = self.workers.remove(&root) {
                    w.stop();
                    self.retired.push(w.join);
                }
                let mut sh = lock(&self.shared);
                sh.repos.remove(&root);
                if sh.diff.as_ref().is_some_and(|d| d.root == root) {
                    sh.diff = None;
                }
            } else if let Some(w) = self.workers.get(&root) {
                w.send(WorkerMsg::Views(views));
            }
        }
    }
}

/// `dir` relative to the work-tree `root`, `/`-separated (`""` for the root).
/// Handles symlinked paths (e.g. macOS `/var` → `/private/var`).
pub fn relative_to(dir: &Path, root: &Path) -> String {
    let try_rel = |d: &Path, r: &Path| {
        d.strip_prefix(r)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    };
    if let Some(r) = try_rel(dir, root) {
        return r;
    }
    let dc = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let rc = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    try_rel(&dc, &rc).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative_to(Path::new("/a/b"), Path::new("/a/b")), "");
        assert_eq!(
            relative_to(Path::new("/a/b/c d/e"), Path::new("/a/b")),
            "c d/e"
        );
    }
}
