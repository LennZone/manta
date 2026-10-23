//! State shared between the background threads and the panel (UI thread).
//! Locks are held only for short copies; the big pieces are `Arc`s.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use manta_plugin::PaneId;

use crate::git::{Commit, Diff, DiffRequest, RepoLocation};
use crate::model::InProgress;
use crate::status::GitStatus;

/// What the plugin knows about the folder a pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneRepo {
    /// Remote (SFTP/FTP) pane.
    NotLocal,
    /// Repository detection is running.
    Detecting,
    /// Local folder outside any work tree.
    NotRepo,
    /// Inside the work tree with this root; the shown folder relative to
    /// it (`""` for the root).
    Repo { root: PathBuf, rel: String },
}

/// The latest known state of one repository.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub loc: RepoLocation,
    /// `None` until the first status has loaded.
    pub status: Option<GitStatus>,
    pub error: Option<String>,
    pub in_progress: Option<InProgress>,
    pub commits: Arc<Vec<Commit>>,
    /// Bumped on every status run.
    pub generation: u64,
}

impl RepoSnapshot {
    pub fn new(loc: RepoLocation) -> Self {
        RepoSnapshot {
            loc,
            status: None,
            error: None,
            in_progress: None,
            commits: Arc::new(Vec::new()),
            generation: 0,
        }
    }

    pub fn name(&self) -> String {
        self.loc
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.loc.root.to_string_lossy().into_owned())
    }
}

/// The diff currently shown in the panel.
#[derive(Debug, Clone)]
pub struct DiffSlot {
    pub root: PathBuf,
    pub req: DiffRequest,
    /// `None` while loading the first time.
    pub result: Option<Result<Arc<Diff>, String>>,
}

#[derive(Debug, Default)]
pub struct Shared {
    pub panes: HashMap<PaneId, PaneRepo>,
    pub repos: HashMap<PathBuf, Arc<RepoSnapshot>>,
    pub diff: Option<DiffSlot>,
    /// Incremented after each successful commit (the panel clears its
    /// message box when it sees a new value).
    pub commit_seq: u64,
    /// Repos that are running a stage/unstage/commit.
    pub busy: HashSet<PathBuf>,
}

pub type SharedRef = Arc<Mutex<Shared>>;

pub fn lock(s: &SharedRef) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}
