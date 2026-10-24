//! File operations across VFS backends: transfer queue, compare & sync.
//!
//! PUBLIC API CONTRACT (used by `manta-app`; do not change these signatures,
//! adding new items is fine):

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use manta_core::{SharedVfs, VfsPath, VfsResult};
use serde::{Deserialize, Serialize};

mod compare;
mod compress;
pub mod journal;
pub mod network;
pub mod rename;
pub mod resume;
mod sync;
mod transfer;

#[cfg(test)]
mod conflict_tests;
#[cfg(any(test, feature = "testing"))]
pub mod memfs;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod tests;

pub use compare::compare_files;
pub use compress::{is_junk, CompressFormat, CompressJob};
pub use journal::{JournalJob, JournalState};
pub use network::{NetworkMonitor, SimulatedNetwork, SystemNetwork};
pub use resume::{Backoff, Endpoint};
pub use transfer::{same_fs, unique_name, COPY_BUFFER_SIZE};

pub type JobId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Copy,
    Move,
    Delete,
    /// Copy out of a read-only archive (same semantics as `Copy`; only the
    /// title and the transfers panel differ).
    Extract,
    /// Create an archive ([`TransferManager::enqueue_compress`]).
    Compress,
}

/// What to do when a destination file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Stop at each conflict and ask ([`JobState::WaitingForDecision`],
    /// answered with [`TransferManager::resolve_conflict`]).
    Ask,
    Overwrite,
    Skip,
    /// Overwrite only if source is newer or sizes differ.
    OverwriteIfNewer,
    /// Keep both: write as "name 2.ext".
    Rename,
}

/// One side of a conflict ([`ConflictQuestion`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictItem {
    pub path: VfsPath,
    pub is_dir: bool,
    /// File size (`None` for folders).
    pub size: Option<u64>,
    pub modified: Option<DateTime<Utc>>,
}

/// An answer to a [`ConflictQuestion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    /// Overwrite the existing file.
    Replace,
    /// Overwrite only if the source is newer (or, without dates, differs in size).
    ReplaceIfNewer,
    /// Write the item as "name 2".
    KeepBoth,
    /// Leave the existing item; the source is not transferred.
    Skip,
    /// Folder onto folder: copy the contents into the existing folder
    /// (files inside follow the job's conflict policy).
    Merge,
    /// Stop the job; items already transferred stay.
    Cancel,
}

impl ConflictChoice {
    /// The policy "apply to all" sets for further file conflicts
    /// (`None`: files keep being asked about).
    pub fn policy(self) -> Option<ConflictPolicy> {
        match self {
            ConflictChoice::Replace => Some(ConflictPolicy::Overwrite),
            ConflictChoice::ReplaceIfNewer => Some(ConflictPolicy::OverwriteIfNewer),
            ConflictChoice::KeepBoth => Some(ConflictPolicy::Rename),
            ConflictChoice::Skip => Some(ConflictPolicy::Skip),
            ConflictChoice::Merge | ConflictChoice::Cancel => None,
        }
    }
}

/// What a job waiting in [`JobState::WaitingForDecision`] asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictQuestion {
    /// The item being transferred.
    pub src: ConflictItem,
    /// The item already at the destination (its path is where the source
    /// would go).
    pub dst: ConflictItem,
    /// The answers that make sense here, in display order. `Cancel` is
    /// always possible and not listed.
    pub choices: Vec<ConflictChoice>,
}

impl ConflictQuestion {
    /// `Some(true)`: the source is newer, `Some(false)`: the existing item
    /// is newer, `None`: same time (within a second) or unknown.
    pub fn source_newer(&self) -> Option<bool> {
        let (s, d) = (self.src.modified?, self.dst.modified?);
        let diff = (s - d).num_milliseconds();
        if diff.abs() < 1000 {
            None
        } else {
            Some(diff > 0)
        }
    }
}

/// A request to the transfer manager. For `Delete`, `dst_fs`/`dst_dir` are ignored.
/// `Extract` behaves like `Copy`. `Compress` jobs are enqueued as a
/// [`CompressJob`] instead (a `Job` with kind `Compress` fails).
#[derive(Clone)]
pub struct Job {
    pub kind: JobKind,
    pub src_fs: SharedVfs,
    /// Files and/or directories (recursive).
    pub sources: Vec<VfsPath>,
    pub dst_fs: Option<SharedVfs>,
    pub dst_dir: Option<VfsPath>,
    pub conflict: ConflictPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed(String),
    Cancelled,
    /// Paused by the user ([`TransferManager::pause`]); the partial file
    /// and position are kept.
    Paused,
    /// The network dropped (or the server went away): retried with
    /// exponential backoff, and at once when the network comes back.
    WaitingForNetwork {
        /// Retries so far (1 after the first failure).
        attempt: u32,
        /// When the next retry is due.
        next_retry: Option<Instant>,
        /// The error that interrupted the job.
        error: String,
    },
    /// Restored from the journal, but a connection it needs isn't open
    /// (label of that connection). [`TransferManager::provide_connection`]
    /// continues it.
    NeedsConnection(String),
    /// [`ConflictPolicy::Ask`]: stopped at an existing item until
    /// [`TransferManager::resolve_conflict`] answers (or the job is
    /// cancelled / paused). Other jobs keep running. A job restored after a
    /// restart asks again when it gets there.
    WaitingForDecision(Box<ConflictQuestion>),
}

/// Resume details of a job ([`JobStatus::resume`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeInfo {
    /// Pause / resume make sense for this job (not for archive creation).
    pub can_pause: bool,
    /// The current file was continued at this byte offset.
    pub resumed_from: Option<u64>,
    /// Notes about resuming ("source changed, started over", ...).
    pub log: Vec<String>,
    /// The job was restored from the journal after a restart.
    pub restored: bool,
    /// Connections the job waits for ([`JobState::NeedsConnection`]).
    pub needs: Vec<Endpoint>,
}

/// Snapshot of a job's progress, cheap to poll every frame.
#[derive(Debug, Clone)]
pub struct JobStatus {
    pub id: JobId,
    pub kind: JobKind,
    /// Short human description, e.g. "Copy 3 items to /tmp".
    pub title: String,
    pub state: JobState,
    pub current_file: Option<String>,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
    /// Bytes per second (smoothed).
    pub speed: f64,
    /// Non-fatal per-file errors (e.g. skipped files).
    pub errors: Vec<String>,
    /// Pause / network-resume details.
    pub resume: ResumeInfo,
}

/// Settings for [`TransferManager::with_options`].
pub struct TransferOptions {
    /// Number of jobs that may run concurrently (min 1).
    pub max_parallel: usize,
    /// Persist unfinished jobs here (see [`journal`]). `None`: no journal.
    pub journal: Option<PathBuf>,
    /// Network reachability; `None`: always online.
    pub network: Option<Arc<dyn NetworkMonitor>>,
    /// Retry delays after a network error.
    pub backoff: Backoff,
    /// Minimum time between two journal writes for progress only (state
    /// changes are written at once).
    pub journal_interval: Duration,
    /// How long dropping the manager waits for running jobs to stop at
    /// their next chunk (so nothing is written to a partial file after the
    /// journal's last write). Jobs still busy after it are left to stop in
    /// the background; their partial files stay claimed until they do.
    pub shutdown_timeout: Duration,
}

impl Default for TransferOptions {
    fn default() -> Self {
        TransferOptions {
            max_parallel: 3,
            journal: None,
            network: None,
            backoff: Backoff::default(),
            journal_interval: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

/// Background transfer queue. Runs jobs on worker threads.
///
/// Semantics:
/// - Per-item problems (unreadable file, refused conflict, unlistable
///   sub-directory, ...) go to `JobStatus::errors`; the job continues. Skipped
///   or failed files still count towards `files_done`/`bytes_done`.
/// - `Failed(msg)` only if the job could do nothing: no destination given,
///   destination dir can't be created / is a file, or every top-level source
///   failed (missing, unlistable, folder-into-itself, ...).
/// - Copy/Move: `files_total` counts files (not dirs). Same-backend moves that
///   succeed via `rename` count as one file with 0 bytes each.
/// - Delete: `files_total`/`files_done` count every entry incl. directories;
///   `bytes_total` stays 0.
/// - Missing destination directories are created (incl. parents).
/// - `ConflictPolicy::Rename` also renames an existing top-level *folder*
///   ("photos 2"); nested folders always merge. Copying an item onto itself
///   (same backend, same path) only works with `Rename` (duplicate), otherwise
///   it's reported as an error.
/// - `OverwriteIfNewer`: source mtime must be >= 1 s newer; if either mtime is
///   unknown, overwrite when sizes differ.
/// - Move removes a source file/folder only after everything in it was
///   transferred (skipped conflicts keep the source).
/// - `Ask`: each conflicting file, and each existing top-level folder,
///   pauses the job in `WaitingForDecision`; nested folders merge as with
///   the other policies. Folder answers: Merge, Keep both, Skip.
pub struct TransferManager {
    shared: Arc<transfer::Shared>,
}

impl TransferManager {
    /// `max_parallel`: number of jobs that may run concurrently (min 1).
    /// No journal, always online.
    pub fn new(max_parallel: usize) -> Self {
        Self::with_options(TransferOptions {
            max_parallel,
            ..Default::default()
        })
    }
    /// A manager with a journal and/or network monitor. Jobs found in an
    /// existing journal are not started: see [`Self::interrupted`].
    pub fn with_options(opts: TransferOptions) -> Self {
        TransferManager {
            shared: transfer::Shared::start(opts),
        }
    }
    /// Queue a job. Its endpoints are guessed (local or not restorable);
    /// use [`Self::enqueue_with_endpoints`] so a restart can reconnect.
    pub fn enqueue(&self, job: Job) -> JobId {
        self.shared.enqueue(job, None)
    }
    /// Queue a job and record which connections it uses, so the journal can
    /// reopen them after a restart.
    pub fn enqueue_with_endpoints(&self, job: Job, src: Endpoint, dst: Option<Endpoint>) -> JobId {
        self.shared.enqueue(job, Some((src, dst)))
    }
    /// Pause a queued, running or waiting copy / move / delete job. A
    /// running job stops at the next buffer chunk and keeps its partial
    /// file. Archive creation can't be paused (no-op).
    pub fn pause(&self, id: JobId) {
        self.shared.pause(id)
    }
    /// Continue a paused or waiting job (a waiting job retries at once).
    pub fn resume(&self, id: JobId) {
        self.shared.resume(id)
    }
    /// Pause every pausable unfinished job.
    pub fn pause_all(&self) {
        self.shared.pause_all()
    }
    /// Continue every paused or waiting job.
    pub fn resume_all(&self) {
        self.shared.resume_all()
    }
    /// The network monitor's view (always true without a monitor).
    pub fn is_online(&self) -> bool {
        self.shared.is_online()
    }
    /// Jobs found in the journal at start that wait for the user's
    /// decision ([`Self::restore_interrupted`] / [`Self::discard_interrupted`]).
    pub fn interrupted(&self) -> Vec<JournalJob> {
        self.shared.interrupted()
    }
    /// Continue the interrupted jobs. `resolve` opens the backend for an
    /// endpoint (local file system, saved favourite); jobs whose endpoint it
    /// can't open wait in [`JobState::NeedsConnection`].
    pub fn restore_interrupted(
        &self,
        resolve: &dyn Fn(&Endpoint) -> Option<SharedVfs>,
    ) -> Vec<JobId> {
        self.shared.restore_interrupted(resolve)
    }
    /// Drop the interrupted jobs and remove their partial files where
    /// `resolve` can open the backend (in the background).
    pub fn discard_interrupted(&self, resolve: &dyn Fn(&Endpoint) -> Option<SharedVfs>) {
        self.shared.discard_interrupted(resolve)
    }
    /// A connection became available: jobs in `NeedsConnection` that wait
    /// for it continue (once all their connections are there).
    pub fn provide_connection(&self, endpoint: &Endpoint, fs: SharedVfs) {
        self.shared.provide_connection(endpoint, fs)
    }
    /// Queue the creation of an archive (`JobKind::Compress`). Progress
    /// counts the bytes read from the sources, then (for a remote
    /// destination) the bytes uploaded.
    pub fn enqueue_compress(&self, job: CompressJob) -> JobId {
        self.shared.enqueue_work(transfer::Work::Compress(job))
    }
    /// Answer the question of a job in [`JobState::WaitingForDecision`].
    /// `apply_to_all`: use the answer for the job's further conflicts too
    /// (a folder answer also covers files where it maps: Skip, Keep both).
    /// `ConflictChoice::Cancel` cancels the job. No-op for other jobs.
    pub fn resolve_conflict(&self, id: JobId, choice: ConflictChoice, apply_to_all: bool) {
        self.shared.resolve_conflict(id, choice, apply_to_all)
    }
    /// Queued jobs are cancelled immediately; running jobs stop at the next
    /// buffer chunk / file boundary. No-op for unknown or finished jobs.
    pub fn cancel(&self, id: JobId) {
        self.shared.cancel(id)
    }
    /// Cheap: never touches any backend, only clones in-memory status.
    pub fn snapshot(&self) -> Vec<JobStatus> {
        self.shared.snapshot()
    }
    /// Remove Done/Failed/Cancelled jobs from the list.
    pub fn clear_finished(&self) {
        self.shared.clear_finished()
    }
    /// True if any job is queued, running, waiting for the network or for
    /// a conflict decision (paused jobs and jobs that need a connection
    /// don't count).
    pub fn is_busy(&self) -> bool {
        self.shared.is_busy()
    }
}

#[cfg(test)]
impl TransferManager {
    /// The engine, to inspect it after the manager is dropped.
    pub(crate) fn shared_for_tests(&self) -> Arc<transfer::Shared> {
        self.shared.clone()
    }
}

impl Drop for TransferManager {
    /// Stops all jobs (journaled ones like a pause, the rest cancelled),
    /// waits up to [`TransferOptions::shutdown_timeout`] for the worker
    /// threads to stop writing, then writes the journal one last time.
    fn drop(&mut self) {
        self.shared.shutdown();
    }
}

impl JobState {
    /// Done, Failed or Cancelled.
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            JobState::Done | JobState::Failed(_) | JobState::Cancelled
        )
    }

    /// Paused, waiting for the network, a connection or a decision: not
    /// finished, but not moving either.
    pub fn is_suspended(&self) -> bool {
        matches!(
            self,
            JobState::Paused
                | JobState::WaitingForNetwork { .. }
                | JobState::NeedsConnection(_)
                | JobState::WaitingForDecision(_)
        )
    }

    /// The question of a job waiting for a conflict decision.
    pub fn question(&self) -> Option<&ConflictQuestion> {
        match self {
            JobState::WaitingForDecision(q) => Some(q),
            _ => None,
        }
    }

    /// Seconds until the next retry of a waiting job (rounded up).
    pub fn retry_in_secs(&self) -> Option<u64> {
        match self {
            JobState::WaitingForNetwork {
                next_retry: Some(t),
                ..
            } => {
                let left = t.saturating_duration_since(Instant::now());
                Some(left.as_millis().div_ceil(1000) as u64)
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- compare/sync

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareStatus {
    OnlyLeft,
    OnlyRight,
    Identical,
    LeftNewer,
    RightNewer,
    /// Same mtime (or unknown) but different size.
    Different,
}

#[derive(Debug, Clone)]
pub struct CompareItem {
    /// Path relative to the compared roots, `/`-separated, no leading slash.
    pub rel_path: String,
    pub is_dir: bool,
    pub status: CompareStatus,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct CompareOptions {
    pub recursive: bool,
    pub include_hidden: bool,
    /// Tolerance when comparing mtimes (FTP has coarse timestamps).
    pub mtime_tolerance_secs: i64,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            recursive: true,
            include_hidden: false,
            mtime_tolerance_secs: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    LeftToRight,
    RightToLeft,
    /// Newer file wins in both directions.
    Bidirectional,
}

/// Recursively compare two directories (possibly on different backends).
pub fn compare_dirs(
    left_fs: &SharedVfs,
    left: &VfsPath,
    right_fs: &SharedVfs,
    right: &VfsPath,
    opts: &CompareOptions,
) -> VfsResult<Vec<CompareItem>> {
    compare::compare_dirs(left_fs, left, right_fs, right, opts)
}

/// Turn a comparison into transfer jobs (enqueue them on a `TransferManager`).
/// `delete_extra`: in one-way sync, delete files that exist only on the target.
pub fn plan_sync(
    items: &[CompareItem],
    left_fs: &SharedVfs,
    left: &VfsPath,
    right_fs: &SharedVfs,
    right: &VfsPath,
    direction: SyncDirection,
    delete_extra: bool,
) -> Vec<Job> {
    sync::plan_sync(
        items,
        left_fs,
        left,
        right_fs,
        right,
        direction,
        delete_extra,
    )
}

#[allow(dead_code)]
fn _assert_send(_: Arc<TransferManager>) {}
