//! Transfer queue implementation (copy / move / delete across backends).
//!
//! Design:
//! - `max_parallel` std worker threads pull jobs from a shared FIFO queue
//!   (`Mutex` + `Condvar`). Each job has a `JobSlot` holding its status behind
//!   its own small mutex; workers update it per buffer chunk, `snapshot()` only
//!   clones these statuses, so it never blocks on IO.
//! - Copy streams `open_read_at` -> a hidden `.NAME.manta-part` file (or a
//!   backend upload session for S3 / Google Drive) with a 256 KiB buffer
//!   after a pre-scan that fills `bytes_total` / `files_total`. The part file
//!   is renamed to the final name when complete; the conflict policy is
//!   applied again at that point.
//! - Lock order: `Shared::inner` before any `JobSlot` mutex; the runner only
//!   ever takes slot mutexes.
//!
//! Resuming (see also `resume.rs` and `journal.rs`):
//! - Every job keeps a [`Progress`]: finished sources, chosen destinations
//!   and the file in flight. A job that stops (pause, network error, quit)
//!   is simply run again later: the re-run skips finished files and
//!   continues the file in flight at the part file's size (if the source is
//!   unchanged and the destination can append), otherwise starts it over.
//! - Errors are classified (`VfsError::class`). A transient one stops the
//!   whole job in `WaitingForNetwork`; a scheduler thread re-queues it after
//!   an exponential backoff or as soon as the network monitor reports
//!   "online". Before a retry the worker calls `Vfs::reconnect` on the
//!   job's backends.
//!
//! Semantics:
//! - Per-item problems (unreadable file, conflict refused, a sub-directory
//!   that can't be listed, ...) are appended to `errors` and the job goes on.
//!   Skipped/failed files still count towards `files_done`/`bytes_done`, so
//!   progress always reaches 100 %.
//! - A job ends `Failed(msg)` only when it could not do anything useful:
//!   missing destination in the `Job`, destination directory that can't be
//!   created / isn't a directory, or *every* top-level source failed at the
//!   top level (can't stat / list, copy into itself, ...). Otherwise it ends
//!   `Done`, possibly with a non-empty `errors` list.
//! - Delete progress is counted in entries (files *and* directories);
//!   `bytes_total` stays 0 for deletes.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use manta_core::{
    Entry, EntryKind, SharedVfs, UploadCheckpoint, UploadSession, Vfs, VfsError, VfsPath,
};

use crate::journal::{Journal, JournalJob, JournalState};
use crate::network::NetworkMonitor;
use crate::resume::{part_path, Backoff, Endpoint, Partial, Progress};
use crate::{
    CompressJob, ConflictChoice, ConflictItem, ConflictPolicy, ConflictQuestion, Job, JobId,
    JobKind, JobState, JobStatus, ResumeInfo, TransferOptions,
};

/// What a queued slot runs.
pub(crate) enum Work {
    Transfer(Job),
    Compress(CompressJob),
}

impl Work {
    fn kind(&self) -> JobKind {
        match self {
            Work::Transfer(j) => j.kind,
            Work::Compress(_) => JobKind::Compress,
        }
    }

    fn title(&self) -> String {
        match self {
            Work::Transfer(j) => job_title(j),
            Work::Compress(c) => crate::compress::title(c, c.dst_path.file_name()),
        }
    }

    /// Backends this work touches (for `reconnect`).
    fn backends(&self) -> Vec<SharedVfs> {
        match self {
            Work::Transfer(j) => {
                let mut v = vec![j.src_fs.clone()];
                if let Some(d) = &j.dst_fs {
                    if !same_fs(d, &j.src_fs) {
                        v.push(d.clone());
                    }
                }
                v
            }
            Work::Compress(c) => vec![c.src_fs.clone(), c.dst_fs.clone()],
        }
    }
}

/// Buffer size used when streaming file contents.
pub const COPY_BUFFER_SIZE: usize = 256 * 1024;
/// Cap on the number of collected per-file errors per job.
const MAX_ERRORS: usize = 1000;
/// Cap on resume notes per job.
const MAX_NOTES: usize = 100;
/// Tolerance for `OverwriteIfNewer` (coarse FTP / FAT timestamps).
const NEWER_TOLERANCE_SECS: i64 = 1;
const SPEED_WINDOW: Duration = Duration::from_millis(500);
/// Files at least this big get a content tag (ETag) recorded so a resume
/// can tell whether the source changed. Smaller ones restart cheaply.
const TAG_MIN_SIZE: u64 = 1024 * 1024;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// True if both handles point to the same backend instance.
pub fn same_fs(a: &SharedVfs, b: &SharedVfs) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

/// First free "name N.ext" (N = 2, 3, ...) next to `path` on `fs`.
/// Directories and dot-files without a further dot get "name N".
pub fn unique_name(fs: &dyn Vfs, path: &VfsPath, is_dir: bool) -> VfsPath {
    let parent = path.parent().unwrap_or_else(VfsPath::root);
    let name = path.file_name();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if !is_dir && i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2..100_000u32 {
        let candidate = parent.join(&format!("{stem} {n}{ext}"));
        if fs.stat(&candidate).is_err() {
            return candidate;
        }
    }
    parent.join(&format!("{stem} {}{ext}", std::process::id()))
}

// ------------------------------------------------------------------ manager

/// The serializable description of a copy / move / delete job.
#[derive(Clone)]
struct JobDesc {
    kind: JobKind,
    conflict: ConflictPolicy,
    sources: Vec<VfsPath>,
    dst_dir: Option<VfsPath>,
}

pub(crate) struct JobSlot {
    id: JobId,
    job: Mutex<Option<Work>>,
    status: Mutex<JobStatus>,
    cancel: AtomicBool,
    /// Stop at the next chunk and keep the partial file (pause, or quit).
    pause: AtomicBool,
    /// `None` for archive creation (not resumable, not journaled).
    desc: Option<JobDesc>,
    /// Source and destination connection (journal, reconnect after restart).
    endpoints: (Endpoint, Option<Endpoint>),
    /// Persisted with the job: what is done already.
    progress: Mutex<Progress>,
    /// Network retries so far (0 = none pending).
    attempt: AtomicU32,
    retry_at: Mutex<Option<Instant>>,
    signal: Arc<JournalSignal>,
    /// The answer to the conflict question the runner waits on (choice,
    /// apply to all).
    answer: Mutex<Option<(ConflictChoice, bool)>>,
    /// Wakes the runner waiting for `answer` (also on cancel / pause).
    answer_cv: Condvar,
}

impl JobSlot {
    /// Journal this job? (copy/move/delete whose endpoints can be reopened)
    fn journaled(&self) -> bool {
        self.desc.is_some()
            && !matches!(self.endpoints.0, Endpoint::Unknown { .. })
            && !matches!(self.endpoints.1, Some(Endpoint::Unknown { .. }))
    }

    fn journal_job(&self) -> Option<JournalJob> {
        let desc = self.desc.as_ref()?;
        let st = lock(&self.status);
        let state = match &st.state {
            JobState::Queued => JournalState::Queued,
            JobState::Running => JournalState::Running,
            JobState::Paused => JournalState::Paused,
            JobState::WaitingForNetwork { .. } => JournalState::Waiting,
            JobState::NeedsConnection(_) => JournalState::NeedsConnection,
            JobState::WaitingForDecision(_) => JournalState::Deciding,
            JobState::Done | JobState::Failed(_) | JobState::Cancelled => return None,
        };
        let progress = lock(&self.progress).clone();
        Some(JournalJob {
            id: self.id,
            kind: desc.kind,
            title: st.title.clone(),
            state,
            conflict: progress.conflict.unwrap_or(desc.conflict),
            src: self.endpoints.0.clone(),
            dst: self.endpoints.1.clone(),
            sources: desc.sources.iter().map(|p| p.to_string()).collect(),
            dst_dir: desc.dst_dir.as_ref().map(|p| p.to_string()),
            progress,
            bytes_done: st.bytes_done,
            bytes_total: st.bytes_total,
            files_done: st.files_done,
            files_total: st.files_total,
            errors: st.errors.iter().take(100).cloned().collect(),
            log: st.resume.log.clone(),
        })
    }
}

/// "The journal needs writing" flag shared by all slots.
#[derive(Default)]
pub(crate) struct JournalSignal {
    /// (dirty, urgent)
    flags: Mutex<(bool, bool)>,
    cv: Condvar,
}

impl JournalSignal {
    /// Progress changed: written within the journal interval.
    fn touch(&self) {
        lock(&self.flags).0 = true;
    }
    /// State changed: written at once.
    fn urgent(&self) {
        *lock(&self.flags) = (true, true);
        self.cv.notify_all();
    }
}

struct Inner {
    jobs: Vec<Arc<JobSlot>>,
    queue: VecDeque<Arc<JobSlot>>,
    next_id: JobId,
    shutdown: bool,
    /// Jobs from the journal of the last run, waiting for the user's
    /// decision (kept in the journal until then).
    interrupted: Vec<JournalJob>,
    /// Open backends for endpoints (restore / provide_connection).
    known: Vec<(Endpoint, SharedVfs)>,
    /// Worker threads alive.
    workers: usize,
    /// Jobs whose runner waits for a conflict decision. Each one gets an
    /// extra worker, so other jobs keep `max_parallel` workers.
    deciding: usize,
}

pub(crate) struct Shared {
    inner: Mutex<Inner>,
    /// Wakes workers (new queue entries).
    cv: Condvar,
    /// Wakes the retry scheduler (new waiting job, network back, shutdown).
    sched_cv: Condvar,
    signal: Arc<JournalSignal>,
    journal: Option<Journal>,
    network: Option<Arc<dyn NetworkMonitor>>,
    backoff: Backoff,
    journal_interval: Duration,
    max_parallel: usize,
}

fn new_status(id: JobId, kind: JobKind, title: String, can_pause: bool) -> JobStatus {
    JobStatus {
        id,
        kind,
        title,
        state: JobState::Queued,
        current_file: None,
        bytes_done: 0,
        bytes_total: 0,
        files_done: 0,
        files_total: 0,
        speed: 0.0,
        errors: Vec::new(),
        resume: ResumeInfo {
            can_pause,
            ..Default::default()
        },
    }
}

impl Shared {
    pub(crate) fn start(opts: TransferOptions) -> Arc<Shared> {
        let journal = opts.journal.map(Journal::new);
        let interrupted = journal.as_ref().map(Journal::load).unwrap_or_default();
        let next_id = interrupted.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                jobs: Vec::new(),
                queue: VecDeque::new(),
                next_id,
                shutdown: false,
                interrupted,
                known: Vec::new(),
                workers: opts.max_parallel.max(1),
                deciding: 0,
            }),
            cv: Condvar::new(),
            sched_cv: Condvar::new(),
            signal: Arc::new(JournalSignal::default()),
            journal,
            network: opts.network,
            backoff: opts.backoff,
            journal_interval: opts.journal_interval,
            max_parallel: opts.max_parallel.max(1),
        });
        for i in 0..shared.max_parallel {
            shared.spawn_worker(i);
        }
        let s = shared.clone();
        std::thread::Builder::new()
            .name("manta-transfer-retry".into())
            .spawn(move || s.scheduler())
            .expect("spawn transfer scheduler");
        if shared.journal.is_some() {
            let s = shared.clone();
            std::thread::Builder::new()
                .name("manta-transfer-journal".into())
                .spawn(move || s.journal_writer())
                .expect("spawn transfer journal");
        }
        if let Some(net) = &shared.network {
            let weak: Weak<Shared> = Arc::downgrade(&shared);
            net.subscribe(Box::new(move |online| {
                if let (true, Some(s)) = (online, weak.upgrade()) {
                    s.wake_waiting();
                }
            }));
        }
        shared
    }

    pub(crate) fn is_online(&self) -> bool {
        self.network.as_ref().is_none_or(|n| n.is_online())
    }

    pub(crate) fn enqueue(
        &self,
        job: Job,
        endpoints: Option<(Endpoint, Option<Endpoint>)>,
    ) -> JobId {
        let endpoints = endpoints.unwrap_or_else(|| {
            (
                Endpoint::guess(&job.src_fs),
                job.dst_fs.as_ref().map(Endpoint::guess),
            )
        });
        let desc = JobDesc {
            kind: job.kind,
            conflict: job.conflict,
            sources: job.sources.clone(),
            dst_dir: job.dst_dir.clone(),
        };
        self.push(
            Work::Transfer(job),
            Some(desc),
            endpoints,
            Progress::default(),
            None,
        )
    }

    pub(crate) fn enqueue_work(&self, job: Work) -> JobId {
        let ep = Endpoint::Unknown {
            label: String::new(),
        };
        let desc = match &job {
            Work::Transfer(j) => Some(JobDesc {
                kind: j.kind,
                conflict: j.conflict,
                sources: j.sources.clone(),
                dst_dir: j.dst_dir.clone(),
            }),
            Work::Compress(_) => None,
        };
        self.push(job, desc, (ep, None), Progress::default(), None)
    }

    /// Create a slot. `work: None` + `restored: Some` = waiting for a
    /// connection.
    fn push(
        &self,
        work: Work,
        desc: Option<JobDesc>,
        endpoints: (Endpoint, Option<Endpoint>),
        progress: Progress,
        restored: Option<&JournalJob>,
    ) -> JobId {
        let mut inner = lock(&self.inner);
        let id = inner.next_id;
        inner.next_id += 1;
        let mut status = new_status(id, work.kind(), work.title(), desc.is_some());
        if let Some(r) = restored {
            restore_counters(&mut status, r);
        }
        let slot = self.new_slot(id, Some(work), status, desc, endpoints, progress);
        inner.jobs.push(slot.clone());
        inner.queue.push_back(slot);
        drop(inner);
        self.cv.notify_one();
        self.signal.urgent();
        id
    }

    fn new_slot(
        &self,
        id: JobId,
        work: Option<Work>,
        status: JobStatus,
        desc: Option<JobDesc>,
        endpoints: (Endpoint, Option<Endpoint>),
        progress: Progress,
    ) -> Arc<JobSlot> {
        Arc::new(JobSlot {
            id,
            job: Mutex::new(work),
            status: Mutex::new(status),
            cancel: AtomicBool::new(false),
            pause: AtomicBool::new(false),
            desc,
            endpoints,
            progress: Mutex::new(progress),
            attempt: AtomicU32::new(0),
            retry_at: Mutex::new(None),
            signal: self.signal.clone(),
            answer: Mutex::new(None),
            answer_cv: Condvar::new(),
        })
    }

    fn find(inner: &Inner, id: JobId) -> Option<Arc<JobSlot>> {
        inner.jobs.iter().find(|s| s.id == id).cloned()
    }

    pub(crate) fn cancel(&self, id: JobId) {
        let mut inner = lock(&self.inner);
        let Some(slot) = Self::find(&inner, id) else {
            return;
        };
        slot.cancel.store(true, Ordering::SeqCst);
        slot.answer_cv.notify_all();
        let state = lock(&slot.status).state.clone();
        let idle = match state {
            // The runner waits for the answer: it stops by itself.
            JobState::WaitingForDecision(_) => false,
            JobState::Queued => {
                if let Some(pos) = inner.queue.iter().position(|s| s.id == id) {
                    inner.queue.remove(pos);
                }
                true
            }
            s => s.is_suspended(),
        };
        if idle {
            let work = lock(&slot.job).take();
            let partial = lock(&slot.progress).partial.take();
            lock(&slot.status).state = JobState::Cancelled;
            if let Some(p) = partial {
                let dst_fs = match &work {
                    Some(Work::Transfer(j)) => j.dst_fs.clone(),
                    _ => None,
                };
                let ep = slot.endpoints.1.clone();
                std::thread::spawn(move || {
                    discard_partial_files(dst_fs.as_deref(), ep.as_ref(), &p)
                });
            }
        }
        drop(inner);
        self.signal.urgent();
    }

    pub(crate) fn pause(&self, id: JobId) {
        let mut inner = lock(&self.inner);
        if let Some(slot) = Self::find(&inner, id) {
            Self::pause_slot(&mut inner, &slot);
        }
        drop(inner);
        self.signal.urgent();
    }

    fn pause_slot(inner: &mut Inner, slot: &Arc<JobSlot>) {
        if slot.desc.is_none() {
            return;
        }
        let mut st = lock(&slot.status);
        match st.state {
            JobState::Queued => {
                if let Some(pos) = inner.queue.iter().position(|s| s.id == slot.id) {
                    inner.queue.remove(pos);
                }
                st.state = JobState::Paused;
            }
            JobState::WaitingForNetwork { .. } => {
                *lock(&slot.retry_at) = None;
                st.state = JobState::Paused;
            }
            JobState::Running | JobState::WaitingForDecision(_) => {
                slot.pause.store(true, Ordering::SeqCst);
                slot.answer_cv.notify_all();
            }
            _ => {}
        }
    }

    pub(crate) fn resume(&self, id: JobId) {
        let mut inner = lock(&self.inner);
        if let Some(slot) = Self::find(&inner, id) {
            self.resume_slot(&mut inner, &slot);
        }
        drop(inner);
        self.signal.urgent();
    }

    fn resume_slot(&self, inner: &mut Inner, slot: &Arc<JobSlot>) {
        slot.pause.store(false, Ordering::SeqCst);
        let mut st = lock(&slot.status);
        if matches!(
            st.state,
            JobState::Paused | JobState::WaitingForNetwork { .. }
        ) && lock(&slot.job).is_some()
        {
            st.state = JobState::Queued;
            *lock(&slot.retry_at) = None;
            inner.queue.push_back(slot.clone());
            self.cv.notify_one();
        }
    }

    pub(crate) fn pause_all(&self) {
        let mut inner = lock(&self.inner);
        for slot in inner.jobs.clone() {
            Self::pause_slot(&mut inner, &slot);
        }
        drop(inner);
        self.signal.urgent();
    }

    pub(crate) fn resume_all(&self) {
        let mut inner = lock(&self.inner);
        for slot in inner.jobs.clone() {
            self.resume_slot(&mut inner, &slot);
        }
        drop(inner);
        self.signal.urgent();
    }

    pub(crate) fn snapshot(&self) -> Vec<JobStatus> {
        let inner = lock(&self.inner);
        inner.jobs.iter().map(|s| lock(&s.status).clone()).collect()
    }

    pub(crate) fn clear_finished(&self) {
        let mut inner = lock(&self.inner);
        inner.jobs.retain(|s| !lock(&s.status).state.is_finished());
    }

    pub(crate) fn is_busy(&self) -> bool {
        let inner = lock(&self.inner);
        inner.jobs.iter().any(|s| {
            matches!(
                lock(&s.status).state,
                JobState::Queued
                    | JobState::Running
                    | JobState::WaitingForNetwork { .. }
                    | JobState::WaitingForDecision(_)
            )
        })
    }

    /// Stop everything. Journaled jobs are interrupted like a pause (their
    /// partial files stay for the next start) and the journal is written one
    /// last time; everything else is cancelled.
    pub(crate) fn shutdown(&self) {
        let mut inner = lock(&self.inner);
        inner.shutdown = true;
        let keep = self.journal.is_some();
        for s in &inner.jobs {
            if keep && s.journaled() {
                s.pause.store(true, Ordering::SeqCst);
            } else {
                s.cancel.store(true, Ordering::SeqCst);
            }
            s.answer_cv.notify_all();
        }
        inner.queue.clear();
        drop(inner);
        self.write_journal();
        self.cv.notify_all();
        self.sched_cv.notify_all();
        self.signal.cv.notify_all();
    }

    // ------------------------------------------------------------ restore

    pub(crate) fn interrupted(&self) -> Vec<JournalJob> {
        lock(&self.inner).interrupted.clone()
    }

    pub(crate) fn restore_interrupted(
        &self,
        resolve: &dyn Fn(&Endpoint) -> Option<SharedVfs>,
    ) -> Vec<JobId> {
        let jobs = std::mem::take(&mut lock(&self.inner).interrupted);
        let mut ids = Vec::new();
        for jj in jobs {
            for ep in jj.endpoints() {
                if self.known_fs(ep).is_none() {
                    if let Some(fs) = resolve(ep) {
                        self.remember(ep, fs);
                    }
                }
            }
            ids.push(self.restore_one(jj));
        }
        self.signal.urgent();
        ids
    }

    fn known_fs(&self, ep: &Endpoint) -> Option<SharedVfs> {
        lock(&self.inner)
            .known
            .iter()
            .find(|(e, _)| e.same_connection(ep))
            .map(|(_, f)| f.clone())
    }

    fn remember(&self, ep: &Endpoint, fs: SharedVfs) {
        let mut inner = lock(&self.inner);
        inner.known.retain(|(e, _)| !e.same_connection(ep));
        inner.known.push((ep.clone(), fs));
    }

    fn restore_one(&self, jj: JournalJob) -> JobId {
        let desc = JobDesc {
            kind: jj.kind,
            conflict: jj.conflict,
            sources: jj.sources.iter().map(|s| VfsPath::new(s)).collect(),
            dst_dir: jj.dst_dir.as_deref().map(VfsPath::new),
        };
        let mut inner = lock(&self.inner);
        let id = inner.next_id;
        inner.next_id += 1;
        let mut status = new_status(id, jj.kind, jj.title.clone(), true);
        restore_counters(&mut status, &jj);
        let slot = self.new_slot(
            id,
            None,
            status,
            Some(desc),
            (jj.src.clone(), jj.dst.clone()),
            jj.progress.clone(),
        );
        inner.jobs.push(slot.clone());
        Self::try_attach(&mut inner, &slot);
        if lock(&slot.job).is_some() {
            inner.queue.push_back(slot);
            self.cv.notify_one();
        }
        id
    }

    /// Build the job of a restored slot once all its backends are known;
    /// otherwise mark it `NeedsConnection`.
    fn try_attach(inner: &mut Inner, slot: &Arc<JobSlot>) {
        let Some(desc) = &slot.desc else { return };
        let find = |ep: &Endpoint| {
            inner
                .known
                .iter()
                .find(|(e, _)| e.same_connection(ep))
                .map(|(_, f)| f.clone())
        };
        let src = find(&slot.endpoints.0);
        let dst = slot.endpoints.1.as_ref().map(find);
        let mut missing = Vec::new();
        if src.is_none() {
            missing.push(slot.endpoints.0.clone());
        }
        if let (Some(None), Some(ep)) = (&dst, &slot.endpoints.1) {
            if !missing.iter().any(|m: &Endpoint| m.same_connection(ep)) {
                missing.push(ep.clone());
            }
        }
        let mut st = lock(&slot.status);
        if let (Some(src_fs), true) = (src, missing.is_empty()) {
            let job = Job {
                kind: desc.kind,
                src_fs,
                sources: desc.sources.clone(),
                dst_fs: dst.flatten(),
                dst_dir: desc.dst_dir.clone(),
                conflict: desc.conflict,
            };
            *lock(&slot.job) = Some(Work::Transfer(job));
            st.state = JobState::Queued;
            st.resume.needs.clear();
        } else {
            st.state =
                JobState::NeedsConnection(missing.first().map(Endpoint::label).unwrap_or_default());
            st.resume.needs = missing;
        }
    }

    pub(crate) fn provide_connection(&self, endpoint: &Endpoint, fs: SharedVfs) {
        self.remember(endpoint, fs);
        let mut inner = lock(&self.inner);
        let waiting: Vec<Arc<JobSlot>> = inner
            .jobs
            .iter()
            .filter(|s| matches!(lock(&s.status).state, JobState::NeedsConnection(_)))
            .cloned()
            .collect();
        for slot in waiting {
            Self::try_attach(&mut inner, &slot);
            if lock(&slot.job).is_some() {
                inner.queue.push_back(slot);
                self.cv.notify_one();
            }
        }
        drop(inner);
        self.signal.urgent();
    }

    pub(crate) fn discard_interrupted(&self, resolve: &dyn Fn(&Endpoint) -> Option<SharedVfs>) {
        let jobs = std::mem::take(&mut lock(&self.inner).interrupted);
        let mut work = Vec::new();
        for jj in jobs {
            if let Some(p) = jj.progress.partial {
                let fs = jj
                    .dst
                    .as_ref()
                    .and_then(|ep| self.known_fs(ep).or_else(|| resolve(ep)));
                work.push((fs, jj.dst, p));
            }
        }
        self.signal.urgent();
        if !work.is_empty() {
            std::thread::spawn(move || {
                for (fs, ep, p) in work {
                    discard_partial_files(fs.as_deref(), ep.as_ref(), &p);
                }
            });
        }
    }

    // ----------------------------------------------------------- conflicts

    pub(crate) fn resolve_conflict(&self, id: JobId, choice: ConflictChoice, all: bool) {
        let Some(slot) = Self::find(&lock(&self.inner), id) else {
            return;
        };
        if !matches!(lock(&slot.status).state, JobState::WaitingForDecision(_)) {
            return;
        }
        *lock(&slot.answer) = Some((choice, all));
        slot.answer_cv.notify_all();
    }

    /// A runner starts waiting for a decision: make sure the other jobs
    /// still get `max_parallel` workers.
    fn decision_started(self: &Arc<Self>) {
        let mut inner = lock(&self.inner);
        inner.deciding += 1;
        if !inner.shutdown && inner.workers < self.max_parallel + inner.deciding {
            inner.workers += 1;
            let n = inner.workers;
            drop(inner);
            self.spawn_worker(n);
        }
    }

    /// The decision came: a surplus worker winds down when idle.
    fn decision_ended(&self) {
        let mut inner = lock(&self.inner);
        inner.deciding = inner.deciding.saturating_sub(1);
        drop(inner);
        self.cv.notify_all();
    }

    // ------------------------------------------------------------- threads

    fn spawn_worker(self: &Arc<Self>, n: usize) {
        let s = self.clone();
        std::thread::Builder::new()
            .name(format!("manta-transfer-{n}"))
            .spawn(move || s.worker())
            .expect("spawn transfer worker");
    }

    fn worker(self: &Arc<Self>) {
        loop {
            let slot = {
                let mut inner = lock(&self.inner);
                loop {
                    if inner.shutdown {
                        return;
                    }
                    if inner.workers > self.max_parallel + inner.deciding {
                        // An extra worker from a decision that is over.
                        inner.workers -= 1;
                        drop(inner);
                        self.cv.notify_one();
                        return;
                    }
                    if let Some(s) = inner.queue.pop_front() {
                        break s;
                    }
                    inner = self.cv.wait(inner).unwrap_or_else(|e| e.into_inner());
                }
            };
            let Some(job) = lock(&slot.job).take() else {
                continue;
            };
            let result = if slot.cancel.load(Ordering::SeqCst) {
                Err(Fatal::Cancelled)
            } else if slot.pause.load(Ordering::SeqCst) {
                Err(Fatal::Paused)
            } else {
                {
                    let mut st = lock(&slot.status);
                    st.state = JobState::Running;
                }
                self.signal.urgent();
                self.reconnect_before_retry(&slot, &job).and_then(|()| {
                    catch_unwind(AssertUnwindSafe(|| Runner::new(&slot, self).run(&job)))
                        .unwrap_or_else(|_| Err(Fatal::Failed("internal error (panic)".into())))
                })
            };
            self.finish_run(&slot, job, result);
        }
    }

    /// After a network error: let the backends log in again first.
    fn reconnect_before_retry(&self, slot: &JobSlot, job: &Work) -> Result<(), Fatal> {
        if slot.attempt.load(Ordering::SeqCst) == 0 {
            return Ok(());
        }
        for fs in job.backends() {
            if let Err(e) = fs.reconnect() {
                return Err(if e.is_transient() || !self.is_online() {
                    Fatal::Transient(format!("{}: {e}", fs.label()))
                } else {
                    Fatal::Failed(format!("{}: {e}", fs.label()))
                });
            }
        }
        Ok(())
    }

    fn finish_run(&self, slot: &Arc<JobSlot>, job: Work, result: Result<(), Fatal>) {
        let mut inner = lock(&self.inner);
        let mut st = lock(&slot.status);
        st.speed = 0.0;
        match result {
            Ok(()) => {
                st.current_file = None;
                st.state = JobState::Done;
                st.resume.resumed_from = None;
                slot.attempt.store(0, Ordering::SeqCst);
                *lock(&slot.progress) = Progress::default();
            }
            Err(Fatal::Cancelled) => {
                st.current_file = None;
                st.state = JobState::Cancelled;
                let partial = lock(&slot.progress).partial.take();
                if let (Some(p), Work::Transfer(j)) = (partial, &job) {
                    let dst_fs = j.dst_fs.clone();
                    let ep = slot.endpoints.1.clone();
                    std::thread::spawn(move || {
                        discard_partial_files(dst_fs.as_deref(), ep.as_ref(), &p)
                    });
                }
            }
            Err(Fatal::Failed(m)) => {
                st.current_file = None;
                st.state = JobState::Failed(m);
                // Keep nothing half-done for a job that won't come back.
                let partial = lock(&slot.progress).partial.take();
                if let (Some(p), Work::Transfer(j)) = (partial, &job) {
                    let dst_fs = j.dst_fs.clone();
                    let ep = slot.endpoints.1.clone();
                    std::thread::spawn(move || {
                        discard_partial_files(dst_fs.as_deref(), ep.as_ref(), &p)
                    });
                }
            }
            Err(Fatal::Paused) => {
                if slot.pause.load(Ordering::SeqCst) || inner.shutdown {
                    st.state = JobState::Paused;
                    *lock(&slot.job) = Some(job);
                } else {
                    // Resumed again before the pause took effect.
                    st.state = JobState::Queued;
                    *lock(&slot.job) = Some(job);
                    inner.queue.push_back(slot.clone());
                    self.cv.notify_one();
                }
            }
            Err(Fatal::Transient(error)) => {
                let attempt = slot.attempt.fetch_add(1, Ordering::SeqCst) + 1;
                let next = Instant::now() + self.backoff.delay(attempt);
                log::info!(
                    "transfer job {}: {error}; retry {attempt} in {:?}",
                    slot.id,
                    self.backoff.delay(attempt)
                );
                *lock(&slot.retry_at) = Some(next);
                *lock(&slot.job) = Some(job);
                st.state = JobState::WaitingForNetwork {
                    attempt,
                    next_retry: Some(next),
                    error,
                };
                self.sched_cv.notify_all();
            }
        }
        drop(st);
        drop(inner);
        self.signal.urgent();
    }

    /// Re-queues waiting jobs when their retry is due.
    fn scheduler(&self) {
        let mut inner = lock(&self.inner);
        loop {
            if inner.shutdown {
                return;
            }
            let now = Instant::now();
            let mut next: Option<Instant> = None;
            for slot in inner.jobs.clone() {
                let mut st = lock(&slot.status);
                if !matches!(st.state, JobState::WaitingForNetwork { .. }) {
                    continue;
                }
                let due = *lock(&slot.retry_at);
                match due {
                    Some(t) if t <= now => {
                        st.state = JobState::Queued;
                        *lock(&slot.retry_at) = None;
                        inner.queue.push_back(slot.clone());
                        self.cv.notify_one();
                    }
                    Some(t) => next = Some(next.map_or(t, |n: Instant| n.min(t))),
                    None => {}
                }
            }
            let wait = next.map_or(Duration::from_secs(3600), |t| {
                t.saturating_duration_since(now)
            });
            inner = self
                .sched_cv
                .wait_timeout(inner, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// The network is back: retry every waiting job now.
    pub(crate) fn wake_waiting(&self) {
        let inner = lock(&self.inner);
        let now = Instant::now();
        for slot in &inner.jobs {
            let mut st = lock(&slot.status);
            if let JobState::WaitingForNetwork { next_retry, .. } = &mut st.state {
                *next_retry = Some(now);
                *lock(&slot.retry_at) = Some(now);
            }
        }
        drop(inner);
        self.sched_cv.notify_all();
    }

    /// Writes the journal: at once on state changes, at most every
    /// `journal_interval` for progress.
    fn journal_writer(&self) {
        let mut last = Instant::now() - self.journal_interval;
        loop {
            let (dirty, urgent) = {
                let mut flags = lock(&self.signal.flags);
                if !flags.1 {
                    flags = self
                        .signal
                        .cv
                        .wait_timeout(flags, self.journal_interval)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                *flags
            };
            if lock(&self.inner).shutdown {
                return;
            }
            if urgent || (dirty && last.elapsed() >= self.journal_interval) {
                *lock(&self.signal.flags) = (false, false);
                self.write_journal();
                last = Instant::now();
            }
        }
    }

    fn write_journal(&self) {
        let Some(journal) = &self.journal else { return };
        let jobs = {
            let inner = lock(&self.inner);
            let mut jobs: Vec<JournalJob> = inner.interrupted.clone();
            jobs.extend(
                inner
                    .jobs
                    .iter()
                    .filter(|s| s.journaled())
                    .filter_map(|s| s.journal_job()),
            );
            jobs
        };
        if let Err(e) = journal.save(&jobs) {
            log::warn!(
                "writing the transfer journal {}: {e}",
                journal.path().display()
            );
        }
    }
}

/// Status counters of a job restored from the journal.
fn restore_counters(st: &mut JobStatus, jj: &JournalJob) {
    st.bytes_done = jj.bytes_done;
    st.bytes_total = jj.bytes_total;
    st.files_done = jj.files_done;
    st.files_total = jj.files_total;
    st.errors = jj.errors.clone();
    st.resume.log = jj.log.clone();
    st.resume.restored = true;
}

/// Remove a partial file / abort an upload session (best effort). Without
/// an open backend only local part files can be removed.
fn discard_partial_files(fs: Option<&dyn Vfs>, ep: Option<&Endpoint>, p: &Partial) {
    match (fs, &p.part, &p.session) {
        (Some(fs), Some(part), _) => {
            let _ = fs.remove_file(&VfsPath::new(part));
        }
        (Some(fs), None, Some(session)) => {
            if let Err(e) = fs.abort_upload(&VfsPath::new(&p.dst), session) {
                log::debug!("aborting upload of {}: {e}", p.dst);
            }
        }
        (None, Some(part), _) if matches!(ep, Some(Endpoint::Local)) => {
            let _ = std::fs::remove_file(part);
        }
        _ => {}
    }
}

fn job_title(job: &Job) -> String {
    let what = match job.sources.as_slice() {
        [one] => format!("\"{}\"", display_name(one)),
        many => format!("{} items", many.len()),
    };
    let dst = job
        .dst_dir
        .as_ref()
        .map(|d| d.to_string())
        .unwrap_or_default();
    match job.kind {
        JobKind::Copy => format!("Copying {what} to {dst}"),
        JobKind::Move => format!("Moving {what} to {dst}"),
        JobKind::Delete => format!("Deleting {what}"),
        JobKind::Extract => format!("Extracting {what} to {dst}"),
        JobKind::Compress => format!("Compressing {what}"),
    }
}

fn display_name(p: &VfsPath) -> String {
    if p.is_root() {
        "/".into()
    } else {
        p.file_name().to_string()
    }
}

// ------------------------------------------------------------------- runner

pub(crate) enum Fatal {
    Cancelled,
    Failed(String),
    /// Stopped by pause (or quit); the job keeps its progress.
    Paused,
    /// A network-type error: wait and retry.
    Transient(String),
}

pub(crate) struct Cancelled;

impl From<Cancelled> for Fatal {
    fn from(_: Cancelled) -> Self {
        Fatal::Cancelled
    }
}

/// Why a copy / move / delete stopped before the end.
pub(crate) enum Stop {
    Cancelled,
    Paused,
    Transient(String),
}

impl From<Cancelled> for Stop {
    fn from(_: Cancelled) -> Self {
        Stop::Cancelled
    }
}

impl From<Stop> for Fatal {
    fn from(s: Stop) -> Self {
        match s {
            Stop::Cancelled => Fatal::Cancelled,
            Stop::Paused => Fatal::Paused,
            Stop::Transient(m) => Fatal::Transient(m),
        }
    }
}

/// Pre-scanned source tree.
pub(crate) struct Node {
    pub(crate) entry: Entry,
    pub(crate) children: Vec<Node>,
    pub(crate) list_failed: bool,
}

impl Node {
    pub(crate) fn is_dir(&self) -> bool {
        self.entry.kind == EntryKind::Directory
    }

    /// (files, bytes) in this subtree; directories are not counted.
    pub(crate) fn totals(&self) -> (u64, u64) {
        if self.is_dir() {
            self.children.iter().fold((0, 0), |(f, b), c| {
                let (cf, cb) = c.totals();
                (f + cf, b + cb)
            })
        } else {
            (1, self.entry.size)
        }
    }

    /// Number of entries including directories (for delete progress).
    fn entry_count(&self) -> u64 {
        1 + self.children.iter().map(Node::entry_count).sum::<u64>()
    }
}

/// A top-level source after scanning.
enum Top {
    Node(Node),
    /// Finished in an earlier run (e.g. moved or deleted already).
    Done,
    /// Missing / unreadable (reported).
    Failed,
}

pub(crate) struct Runner<'a> {
    slot: &'a Arc<JobSlot>,
    shared: &'a Arc<Shared>,
    pub(crate) buf: Vec<u8>,
    /// The job's policy (changed by "apply to all").
    conflict: ConflictPolicy,
    /// The answer for the file being copied (`Ask`), used again at the
    /// final rename.
    file_choice: Option<ConflictPolicy>,
    move_mode: bool,
    /// A backend of this job is remote: while offline, errors count as
    /// network errors.
    remote: bool,
    window_start: Instant,
    window_bytes: u64,
    speed: f64,
}

/// Why streaming one file stopped.
enum StreamError {
    Stop(Stop),
    Open(VfsError),
    Read(VfsError),
    Write(VfsError),
}

impl<'a> Runner<'a> {
    fn new(slot: &'a Arc<JobSlot>, shared: &'a Arc<Shared>) -> Self {
        Runner {
            slot,
            shared,
            buf: Vec::new(),
            conflict: ConflictPolicy::Overwrite,
            file_choice: None,
            move_mode: false,
            remote: false,
            window_start: Instant::now(),
            window_bytes: 0,
            speed: 0.0,
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.slot.cancel.load(Ordering::Relaxed)
    }

    pub(crate) fn check(&self) -> Result<(), Cancelled> {
        if self.cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    /// Cancel or pause requested?
    fn check_stop(&self) -> Result<(), Stop> {
        if self.cancelled() {
            Err(Stop::Cancelled)
        } else if self.slot.pause.load(Ordering::Relaxed) {
            Err(Stop::Paused)
        } else {
            Ok(())
        }
    }

    /// Should this error stop the job to wait for the network?
    fn is_transient(&self, e: &VfsError) -> bool {
        e.is_transient()
            || (self.remote
                && !self.shared.is_online()
                && !matches!(
                    e,
                    VfsError::PermissionDenied(_)
                        | VfsError::AlreadyExists(_)
                        | VfsError::NotADirectory(_)
                        | VfsError::DirectoryNotEmpty(_)
                        | VfsError::Unsupported(_)
                        | VfsError::Cancelled
                ))
    }

    fn transient(&self, what: &dyn std::fmt::Display, e: &VfsError) -> Option<Stop> {
        self.is_transient(e)
            .then(|| Stop::Transient(format!("{what}: {e}")))
    }

    pub(crate) fn status(&self) -> MutexGuard<'_, JobStatus> {
        lock(&self.slot.status)
    }

    fn progress(&self) -> MutexGuard<'_, Progress> {
        lock(&self.slot.progress)
    }

    pub(crate) fn error(&self, msg: String) {
        log::warn!("transfer job {}: {msg}", self.slot.id);
        let mut st = self.status();
        match st.errors.len() {
            n if n < MAX_ERRORS => st.errors.push(msg),
            n if n == MAX_ERRORS => st
                .errors
                .push("too many errors, further errors omitted".into()),
            _ => {}
        }
    }

    /// A note for the job log (not an error).
    fn note(&self, msg: String) {
        log::info!("transfer job {}: {msg}", self.slot.id);
        let mut st = self.status();
        if st.resume.log.len() < MAX_NOTES {
            st.resume.log.push(msg);
        }
        drop(st);
        self.slot.signal.urgent();
    }

    /// Stop in `WaitingForDecision` until the user answers `q`. Cancel (the
    /// answer or the job's) and pause end the wait with a `Stop`.
    fn ask(&mut self, q: ConflictQuestion) -> Result<(ConflictChoice, bool), Stop> {
        *lock(&self.slot.answer) = None;
        {
            let mut st = self.status();
            st.state = JobState::WaitingForDecision(Box::new(q));
            st.speed = 0.0;
        }
        self.slot.signal.urgent();
        self.shared.decision_started();
        let result = loop {
            if let Err(stop) = self.check_stop() {
                break Err(stop);
            }
            let mut answer = lock(&self.slot.answer);
            if let Some(a) = answer.take() {
                break Ok(a);
            }
            let _ = self
                .slot
                .answer_cv
                .wait_timeout(answer, Duration::from_millis(100));
        };
        self.shared.decision_ended();
        // Count the time spent waiting out of the speed.
        self.window_start = Instant::now();
        self.window_bytes = 0;
        match result {
            Ok((ConflictChoice::Cancel, _)) => {
                self.slot.cancel.store(true, Ordering::SeqCst);
                Err(Stop::Cancelled)
            }
            Ok(a) => {
                self.status().state = JobState::Running;
                self.slot.signal.urgent();
                Ok(a)
            }
            Err(stop) => Err(stop),
        }
    }

    /// "Apply to all" for files: the job's policy from now on (persisted).
    fn set_policy(&mut self, policy: ConflictPolicy) {
        self.conflict = policy;
        self.progress().conflict = Some(policy);
        self.slot.signal.urgent();
    }

    /// Ask about one conflicting file. Returns the policy for this file.
    fn ask_file(
        &mut self,
        src: &Entry,
        dst: &Entry,
        dst_path: &VfsPath,
        choices: Vec<ConflictChoice>,
    ) -> Result<ConflictPolicy, Stop> {
        let q = ConflictQuestion {
            src: item(src, &src.path),
            dst: item(dst, dst_path),
            choices,
        };
        let (choice, all) = self.ask(q)?;
        let policy = choice.policy().unwrap_or(ConflictPolicy::Skip);
        if all {
            self.set_policy(policy);
        }
        Ok(policy)
    }

    pub(crate) fn set_current(&self, p: &VfsPath) {
        self.status().current_file = Some(p.to_string());
    }

    pub(crate) fn add_totals(&self, files: u64, bytes: u64) {
        let mut st = self.status();
        st.files_total += files;
        st.bytes_total += bytes;
    }

    pub(crate) fn add_bytes(&mut self, n: u64) {
        self.window_bytes += n;
        let el = self.window_start.elapsed();
        let mut st = lock(&self.slot.status);
        st.bytes_done += n;
        if st.bytes_done > st.bytes_total {
            st.bytes_total = st.bytes_done;
        }
        if el >= SPEED_WINDOW {
            let inst = self.window_bytes as f64 / el.as_secs_f64();
            self.speed = if self.speed == 0.0 {
                inst
            } else {
                0.5 * self.speed + 0.5 * inst
            };
            st.speed = self.speed;
            self.window_bytes = 0;
            self.window_start = Instant::now();
        }
        drop(st);
        self.slot.signal.touch();
    }

    pub(crate) fn file_done(&self) {
        self.status().files_done += 1;
    }

    /// Account a whole subtree as processed without transferring it.
    pub(crate) fn skip_node(&mut self, node: &Node) {
        let (files, bytes) = node.totals();
        self.status().files_done += files;
        let window = self.window_bytes;
        self.add_bytes(bytes);
        // Skipped bytes aren't transferred: keep them out of the speed.
        self.window_bytes = window;
    }

    fn run(&mut self, work: &Work) -> Result<(), Fatal> {
        let job = match work {
            Work::Compress(c) => return self.run_compress(c),
            Work::Transfer(job) => job,
        };
        let chosen = self.progress().conflict;
        self.conflict = chosen.unwrap_or(job.conflict);
        let is_remote = |fs: &SharedVfs| fs.protocol() != manta_core::Protocol::Local;
        self.remote = is_remote(&job.src_fs) || job.dst_fs.as_ref().is_some_and(is_remote);
        match job.kind {
            JobKind::Delete => self.run_delete(job),
            JobKind::Copy | JobKind::Move | JobKind::Extract => self.run_transfer(job),
            JobKind::Compress => Err(Fatal::Failed("use enqueue_compress for archives".into())),
        }
    }

    /// Replace the job's title (e.g. once the final archive name is known).
    pub(crate) fn set_title(&self, title: String) {
        self.status().title = title;
    }

    /// A (re-)run starts: counters are rebuilt by the scan; the previous
    /// numbers stay visible until the scan is done.
    fn begin_counting(&self, files_total: u64, bytes_total: u64) {
        let mut st = self.status();
        st.files_total = files_total;
        st.bytes_total = bytes_total;
        st.files_done = 0;
        st.bytes_done = 0;
        st.speed = 0.0;
        st.resume.resumed_from = None;
    }

    // ------------------------------------------------------------- scanning

    fn scan(&mut self, fs: &dyn Vfs, entry: Entry) -> Result<Node, Stop> {
        self.check_stop()?;
        let mut node = Node {
            entry,
            children: Vec::new(),
            list_failed: false,
        };
        if node.is_dir() {
            self.set_current(&node.entry.path);
            match fs.list(&node.entry.path) {
                Ok(mut entries) => {
                    entries.sort_by(|a, b| a.name.cmp(&b.name));
                    for e in entries {
                        if e.name == "." || e.name == ".." || crate::resume::is_part_file(&e.name) {
                            continue;
                        }
                        let child = self.scan(fs, e)?;
                        node.children.push(child);
                    }
                }
                Err(e) => {
                    if let Some(stop) = self.transient(&node.entry.path, &e) {
                        return Err(stop);
                    }
                    node.list_failed = true;
                    self.error(format!("{}: {e}", node.entry.path));
                }
            }
        }
        Ok(node)
    }

    /// Stat + scan a top-level source; the entry path is forced to `src`.
    fn scan_top(&mut self, fs: &dyn Vfs, src: &VfsPath) -> Result<Top, Stop> {
        match fs.stat(src) {
            Ok(mut e) => {
                e.path = src.clone();
                let node = self.scan(fs, e)?;
                if node.list_failed {
                    // Top-level directory unreadable: nothing to do for it.
                    return Ok(Top::Failed);
                }
                Ok(Top::Node(node))
            }
            Err(VfsError::NotFound(_)) if self.progress().completed.contains(src.as_str()) => {
                Ok(Top::Done)
            }
            Err(err) => {
                if let Some(stop) = self.transient(src, &err) {
                    return Err(stop);
                }
                self.error(format!("{src}: {err}"));
                Ok(Top::Failed)
            }
        }
    }

    // --------------------------------------------------------------- delete

    fn run_delete(&mut self, job: &Job) -> Result<(), Fatal> {
        let fs = &*job.src_fs;
        let mut nodes = Vec::new();
        let mut done_before = 0u64;
        let mut entries = 0u64;
        for src in &job.sources {
            if src.is_root() {
                self.error("refusing to delete the root directory".into());
                continue;
            }
            match self.scan_top(fs, src)? {
                Top::Node(n) => {
                    entries += n.entry_count();
                    nodes.push(n);
                }
                Top::Done => done_before += 1,
                Top::Failed => {}
            }
        }
        self.begin_counting(entries + done_before, 0);
        self.status().files_done = done_before;
        if nodes.is_empty() && done_before == 0 && !job.sources.is_empty() {
            return Err(Fatal::Failed(self.first_error("nothing to delete")));
        }
        for n in &nodes {
            if self.delete_node(fs, n)? {
                self.progress().completed.insert(n.entry.path.to_string());
                self.slot.signal.touch();
            }
        }
        Ok(())
    }

    fn delete_node(&mut self, fs: &dyn Vfs, node: &Node) -> Result<bool, Stop> {
        self.check_stop()?;
        let path = &node.entry.path;
        let ok = if node.is_dir() {
            // Backends with a fast bulk delete (S3) remove the whole subtree
            // at once; the rest report `Unsupported` and we walk it.
            if !node.list_failed {
                self.set_current(path);
                match fs.remove_tree(path) {
                    Ok(()) => {
                        self.status().files_done += node.entry_count();
                        return Ok(true);
                    }
                    Err(VfsError::Unsupported(_)) => {}
                    Err(e) => {
                        if let Some(stop) = self.transient(path, &e) {
                            return Err(stop);
                        }
                        log::debug!("remove_tree {path}: {e}; deleting entry by entry")
                    }
                }
            }
            let mut all = !node.list_failed;
            for c in &node.children {
                all &= self.delete_node(fs, c)?;
            }
            self.set_current(path);
            if all {
                self.report_stop(fs.remove_dir(path), path)?
            } else {
                false
            }
        } else {
            self.set_current(path);
            self.report_stop(fs.remove_file(path), path)?
        };
        self.file_done();
        Ok(ok)
    }

    fn report(&self, r: Result<(), VfsError>, path: &VfsPath) -> bool {
        match r {
            Ok(()) => true,
            Err(e) => {
                self.error(format!("{path}: {e}"));
                false
            }
        }
    }

    /// Like [`Self::report`], but a network error stops the job.
    fn report_stop(&self, r: Result<(), VfsError>, path: &VfsPath) -> Result<bool, Stop> {
        match r {
            Err(e) if self.is_transient(&e) => Err(Stop::Transient(format!("{path}: {e}"))),
            r => Ok(self.report(r, path)),
        }
    }

    pub(crate) fn first_error(&self, fallback: &str) -> String {
        self.status()
            .errors
            .first()
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    }

    // ------------------------------------------------------- copy and move

    fn run_transfer(&mut self, job: &Job) -> Result<(), Fatal> {
        let (Some(dst_fs), Some(dst_dir)) = (job.dst_fs.as_ref(), job.dst_dir.as_ref()) else {
            return Err(Fatal::Failed("no destination given".into()));
        };
        self.move_mode = job.kind == JobKind::Move;
        let src_fs = &job.src_fs;
        let same = same_fs(src_fs, dst_fs);

        if let Err(e) = ensure_dir(&**dst_fs, dst_dir) {
            return Err(match self.transient(dst_dir, &e) {
                Some(stop) => stop.into(),
                None => Fatal::Failed(format!("destination {dst_dir}: {e}")),
            });
        }

        let mut pending: Vec<&VfsPath> = Vec::new();
        let mut handled = 0u64;
        for src in &job.sources {
            self.check_stop()?;
            let dst = dst_dir.join(&display_name(src));
            if src.is_root() {
                self.error("cannot copy or move the root directory".into());
                continue;
            }
            if same && dst_dir.starts_with(src) {
                self.error(format!("{src}: cannot copy a folder into itself"));
                continue;
            }
            if self.move_mode && same {
                if dst == *src || self.progress().completed.contains(src.as_str()) {
                    // Moving onto itself, or moved by an earlier run.
                    handled += 1;
                    continue;
                }
                if matches!(dst_fs.stat(&dst), Err(VfsError::NotFound(_))) {
                    self.set_current(src);
                    if src_fs.rename(src, &dst).is_ok() {
                        self.progress().completed.insert(src.to_string());
                        self.slot.signal.urgent();
                        handled += 1;
                        continue;
                    }
                }
            }
            if !self.move_mode && same && job.kind == JobKind::Copy {
                if self.progress().completed.contains(src.as_str()) {
                    handled += 1;
                    continue;
                }
                // A copy within one server (WebDAV COPY): the server copies,
                // nothing goes through Manta. Falls back to a normal copy.
                if matches!(dst_fs.stat(&dst), Err(VfsError::NotFound(_))) {
                    self.set_current(src);
                    if src_fs.server_copy(src, &dst).is_ok() {
                        self.progress().completed.insert(src.to_string());
                        self.slot.signal.urgent();
                        handled += 1;
                        continue;
                    }
                }
            }
            pending.push(src);
        }

        let mut nodes = Vec::new();
        let (mut files, mut bytes) = (0u64, 0u64);
        for src in pending {
            match self.scan_top(&**src_fs, src)? {
                Top::Node(n) => {
                    let (f, b) = n.totals();
                    files += f;
                    bytes += b;
                    nodes.push(n);
                }
                Top::Done => handled += 1,
                Top::Failed => {}
            }
        }
        self.begin_counting(files + handled, bytes);
        self.status().files_done = handled;
        if nodes.is_empty() && handled == 0 && !job.sources.is_empty() {
            return Err(Fatal::Failed(self.first_error("nothing to transfer")));
        }

        self.buf = vec![0u8; COPY_BUFFER_SIZE];
        for n in &nodes {
            let dst = dst_dir.join(n.entry.path.file_name());
            self.copy_node(src_fs, n, dst_fs, dst, true, same)?;
        }
        Ok(())
    }

    /// Remember the outcome for a source so a re-run doesn't redo it.
    fn mark(&self, src: &VfsPath, ok: bool) {
        let mut p = self.progress();
        if ok {
            p.completed.insert(src.to_string());
        } else {
            p.failed.insert(src.to_string());
        }
        drop(p);
        self.slot.signal.touch();
    }

    /// Copy (and in move mode delete) `node` to `dst`. Returns true if the
    /// whole subtree was transferred (only then is the source removed).
    fn copy_node(
        &mut self,
        src_fs: &SharedVfs,
        node: &Node,
        dst_fs: &SharedVfs,
        mut dst: VfsPath,
        top: bool,
        same: bool,
    ) -> Result<bool, Stop> {
        self.check_stop()?;
        let src = &node.entry.path;
        let key = src.to_string();

        // Finished (or given up) in an earlier run.
        self.file_choice = None;
        if top && node.is_dir() && self.progress().failed.contains(&key) {
            // A folder skipped by a conflict answer in an earlier run.
            self.skip_node(node);
            return Ok(false);
        }
        if !node.is_dir() {
            if self.progress().completed.contains(&key) {
                self.skip_node(node);
                if self.move_mode {
                    // The source may still be there if removing it was
                    // interrupted.
                    return match src_fs.stat(src) {
                        Ok(_) => self.report_stop(src_fs.remove_file(src), src),
                        Err(_) => Ok(true),
                    };
                }
                return Ok(true);
            }
            if self.progress().failed.contains(&key) {
                self.skip_node(node);
                return Ok(false);
            }
        }

        // A destination chosen by an earlier run ("keep both").
        let mapped = self.progress().dst_map.get(&key).map(|d| VfsPath::new(d));
        let resuming = self
            .progress()
            .partial
            .clone()
            .filter(|p| p.src == key && !node.is_dir());
        if let Some(p) = &resuming {
            dst = VfsPath::new(&p.dst);
        } else if let Some(m) = &mapped {
            dst = m.clone();
        }
        let existing = match dst_fs.stat(&dst) {
            Ok(e) => Some(e),
            Err(e) => {
                if let Some(stop) = self.transient(&dst, &e) {
                    return Err(stop);
                }
                None
            }
        };

        if node.is_dir() {
            if let (None, true, Some(ex)) = (&mapped, top, &existing) {
                let same_dir = same && dst == *src;
                let mut answer = self.progress().folders;
                if answer.is_none() && self.conflict == ConflictPolicy::Ask {
                    let choices = if ex.is_dir() && !same_dir {
                        vec![
                            ConflictChoice::Merge,
                            ConflictChoice::KeepBoth,
                            ConflictChoice::Skip,
                        ]
                    } else {
                        vec![ConflictChoice::KeepBoth, ConflictChoice::Skip]
                    };
                    let q = ConflictQuestion {
                        src: item(&node.entry, src),
                        dst: item(ex, &dst),
                        choices,
                    };
                    let (choice, all) = self.ask(q)?;
                    if all {
                        self.progress().folders = Some(choice);
                        if let Some(p @ (ConflictPolicy::Skip | ConflictPolicy::Rename)) =
                            choice.policy()
                        {
                            self.set_policy(p);
                        }
                        self.slot.signal.urgent();
                    }
                    answer = Some(choice);
                }
                match answer {
                    Some(ConflictChoice::KeepBoth) => {
                        dst = unique_name(&**dst_fs, &dst, true);
                        self.progress().dst_map.insert(key.clone(), dst.to_string());
                    }
                    Some(ConflictChoice::Merge) => {
                        self.progress().dst_map.insert(key.clone(), dst.to_string());
                    }
                    Some(ConflictChoice::Skip) => {
                        self.skip_node(node);
                        self.mark(src, false);
                        return Ok(false);
                    }
                    _ => {}
                }
            }
            let mapped =
                mapped.or_else(|| self.progress().dst_map.get(&key).map(|d| VfsPath::new(d)));
            if mapped.is_none() {
                if existing.is_some() && top && self.conflict == ConflictPolicy::Rename {
                    dst = unique_name(&**dst_fs, &dst, true);
                } else if same && dst == *src {
                    // Same-location copy without "keep both": nothing sensible to do.
                    self.error(format!("{src}: source and destination are the same"));
                    self.skip_node(node);
                    return Ok(false);
                }
                if top {
                    self.progress().dst_map.insert(key.clone(), dst.to_string());
                }
            }
            if same && dst.starts_with(src) {
                self.error(format!("{src}: cannot copy a folder into itself"));
                self.skip_node(node);
                return Ok(false);
            }
            match dst_fs.stat(&dst) {
                Ok(e) if e.is_dir() => {}
                Ok(_) => {
                    self.error(format!("{dst}: a file with this name already exists"));
                    self.skip_node(node);
                    return Ok(false);
                }
                Err(_) => {
                    if let Err(e) = dst_fs.create_dir(&dst) {
                        if let Some(stop) = self.transient(&dst, &e) {
                            return Err(stop);
                        }
                        self.error(format!("{dst}: {e}"));
                        self.skip_node(node);
                        return Ok(false);
                    }
                }
            }
            let mut all = !node.list_failed;
            for c in &node.children {
                let child_dst = dst.join(c.entry.path.file_name());
                all &= self.copy_node(src_fs, c, dst_fs, child_dst, false, same)?;
            }
            if let Some(mode) = node.entry.permissions {
                let _ = dst_fs.set_permissions(&dst, mode);
            }
            if self.move_mode && all {
                all = self.report_stop(src_fs.remove_dir(src), src)?;
            }
            if top && all {
                self.mark(src, true);
            }
            return Ok(all);
        }

        if node.entry.kind == EntryKind::Other {
            self.error(format!("{src}: skipping special file"));
            self.skip_node(node);
            self.mark(src, false);
            return Ok(false);
        }

        // Regular file or symlink (copied as its content).
        let same_file = same && dst == *src;
        if resuming.is_none() && mapped.is_none() && (existing.is_some() || same_file) {
            let dst_entry = existing.as_ref();
            let dst_is_dir = dst_entry.is_some_and(|e| e.is_dir());
            let mut policy = self.conflict;
            if policy == ConflictPolicy::Ask {
                let choices = if same_file || dst_is_dir {
                    vec![ConflictChoice::KeepBoth, ConflictChoice::Skip]
                } else {
                    vec![
                        ConflictChoice::Replace,
                        ConflictChoice::ReplaceIfNewer,
                        ConflictChoice::KeepBoth,
                        ConflictChoice::Skip,
                    ]
                };
                let ex = dst_entry.cloned().unwrap_or_else(|| node.entry.clone());
                policy = self.ask_file(&node.entry, &ex, &dst, choices)?;
                self.file_choice = Some(policy);
            }
            let proceed = match policy {
                ConflictPolicy::Rename => {
                    dst = unique_name(&**dst_fs, &dst, false);
                    true
                }
                _ if same_file => {
                    self.error(format!("{src}: source and destination are the same"));
                    false
                }
                _ if dst_is_dir => {
                    self.error(format!("{dst}: a folder with this name already exists"));
                    false
                }
                ConflictPolicy::Overwrite => true,
                ConflictPolicy::Skip | ConflictPolicy::Ask => false,
                ConflictPolicy::OverwriteIfNewer => {
                    dst_entry.is_some_and(|d| source_is_newer(&node.entry, d))
                }
            };
            if !proceed {
                self.skip_node(node);
                self.mark(src, false);
                return Ok(false);
            }
        }
        if top {
            self.progress().dst_map.insert(key, dst.to_string());
        }

        let mut ok = self.copy_file(&**src_fs, &node.entry, &**dst_fs, &dst, resuming)?;
        self.mark(src, ok);
        if ok && self.move_mode {
            ok = self.report_stop(src_fs.remove_file(src), src)?;
        }
        Ok(ok)
    }

    fn set_partial(&self, p: Option<Partial>) {
        self.progress().partial = p;
        self.slot.signal.urgent();
    }

    /// Persists upload-session changes into the job's progress.
    fn checkpoint(&self) -> UploadCheckpoint {
        let slot = self.slot.clone();
        Arc::new(move |s: &UploadSession| {
            if let Some(p) = lock(&slot.progress).partial.as_mut() {
                p.session = Some(s.clone());
            }
            slot.signal.urgent();
        })
    }

    /// Copy one file through a part file (or upload session), continuing
    /// `resuming` when possible. Ok(true): copied; Ok(false): failed for
    /// good (reported); Err: the job stops.
    fn copy_file(
        &mut self,
        src_fs: &dyn Vfs,
        src: &Entry,
        dst_fs: &dyn Vfs,
        dst: &VfsPath,
        resuming: Option<Partial>,
    ) -> Result<bool, Stop> {
        self.set_current(&src.path);
        self.status().resume.resumed_from = None;
        let caps = dst_fs.supports_resume();

        // Can the earlier attempt be continued?
        let mut continued = None;
        if let Some(p) = resuming {
            let tag = if p.tag.is_some() {
                src_fs.content_tag(&src.path)
            } else {
                None
            };
            if p.source_unchanged(src, tag.as_deref()) && p.part.is_none() == caps.upload_session {
                continued = Some(p);
            } else {
                self.note(format!(
                    "{}: the source changed since the interruption; copying it again from the start",
                    src.path
                ));
                discard_partial_files(Some(dst_fs), None, &p);
            }
        }
        let resumed = continued.is_some();
        let partial = match continued {
            Some(p) => {
                self.file_choice = p.choice;
                p
            }
            None => {
                let tag = if src.size >= TAG_MIN_SIZE {
                    src_fs.content_tag(&src.path)
                } else {
                    None
                };
                Partial {
                    src: src.path.to_string(),
                    dst: dst.to_string(),
                    part: (!caps.upload_session).then(|| part_path(dst).to_string()),
                    size: src.size,
                    mtime: src.modified,
                    tag,
                    session: None,
                    choice: self.file_choice,
                }
            }
        };
        self.set_partial(Some(partial.clone()));

        let mut copied = 0u64;
        let result = self.stream(src_fs, src, dst_fs, &partial, resumed, &mut copied);
        let stopped = matches!(result, Err(StreamError::Stop(_)))
            || matches!(&result, Err(StreamError::Open(e) | StreamError::Read(e) | StreamError::Write(e)) if self.is_transient(e));

        let final_dst = match result {
            Ok(()) => match &partial.part {
                Some(part) => self.finalize(dst_fs, &VfsPath::new(part), dst, src)?,
                None => Some(dst.clone()),
            },
            Err(StreamError::Stop(Stop::Cancelled)) => {
                discard_partial_files(Some(dst_fs), None, &partial);
                self.progress().partial = None;
                return Err(Stop::Cancelled);
            }
            Err(StreamError::Stop(stop)) => return Err(stop),
            Err(StreamError::Open(e) | StreamError::Read(e) | StreamError::Write(e)) if stopped => {
                return Err(Stop::Transient(format!("{}: {e}", src.path)));
            }
            Err(StreamError::Open(e)) | Err(StreamError::Read(e)) => {
                self.error(format!("{}: {e}", src.path));
                discard_partial_files(Some(dst_fs), None, &partial);
                None
            }
            Err(StreamError::Write(e)) => {
                self.error(format!("{dst}: {e}"));
                discard_partial_files(Some(dst_fs), None, &partial);
                None
            }
        };
        self.set_partial(None);
        // Keep the progress consistent if the file was shorter than scanned
        // or the copy failed half-way.
        self.add_bytes(src.size.saturating_sub(copied));
        self.file_done();
        let Some(final_dst) = final_dst else {
            return Ok(false);
        };
        if let Some(mode) = src.permissions {
            match dst_fs.set_permissions(&final_dst, mode) {
                Ok(()) | Err(VfsError::Unsupported(_)) => {}
                Err(e) => log::debug!("set_permissions {final_dst}: {e}"),
            }
        }
        if let Some(mtime) = src.modified {
            match dst_fs.set_modified(&final_dst, mtime) {
                Ok(()) | Err(VfsError::Unsupported(_)) => {}
                Err(e) => log::debug!("set_modified {final_dst}: {e}"),
            }
        }
        Ok(true)
    }

    /// Move the finished part file to `dst`, applying the conflict policy
    /// to a file that appeared there in the meantime. `None`: skipped.
    fn finalize(
        &mut self,
        fs: &dyn Vfs,
        part: &VfsPath,
        dst: &VfsPath,
        src: &Entry,
    ) -> Result<Option<VfsPath>, Stop> {
        let mut target = dst.clone();
        match fs.stat(dst) {
            Ok(existing) if existing.is_dir() => {
                self.error(format!("{dst}: a folder with this name already exists"));
                let _ = fs.remove_file(part);
                return Ok(None);
            }
            Ok(existing) => {
                let mut policy = self.file_choice.unwrap_or(self.conflict);
                if policy == ConflictPolicy::Ask {
                    // The file appeared while it was copied.
                    let choices = vec![
                        ConflictChoice::Replace,
                        ConflictChoice::ReplaceIfNewer,
                        ConflictChoice::KeepBoth,
                        ConflictChoice::Skip,
                    ];
                    policy = self.ask_file(src, &existing, dst, choices)?;
                    self.file_choice = Some(policy);
                }
                let replace = match policy {
                    ConflictPolicy::Overwrite => true,
                    ConflictPolicy::OverwriteIfNewer => source_is_newer(src, &existing),
                    ConflictPolicy::Skip | ConflictPolicy::Ask => false,
                    ConflictPolicy::Rename => {
                        target = unique_name(fs, dst, false);
                        false
                    }
                };
                if replace {
                    if let Err(e) = fs.remove_file(dst) {
                        if let Some(stop) = self.transient(dst, &e) {
                            return Err(stop);
                        }
                    }
                } else if target == *dst {
                    let _ = fs.remove_file(part);
                    return Ok(None);
                }
            }
            Err(e) => {
                if let Some(stop) = self.transient(dst, &e) {
                    return Err(stop);
                }
            }
        }
        match fs.rename(part, &target) {
            Ok(()) => Ok(Some(target)),
            Err(e) => {
                if let Some(stop) = self.transient(&target, &e) {
                    return Err(stop);
                }
                // No rename on this backend: copy the part over, then drop it.
                match copy_within(fs, part, &target) {
                    Ok(()) => {
                        let _ = fs.remove_file(part);
                        Ok(Some(target))
                    }
                    Err(e2) => {
                        if let Some(stop) = self.transient(&target, &e2) {
                            return Err(stop);
                        }
                        self.error(format!("{target}: {e}"));
                        let _ = fs.remove_file(part);
                        Ok(None)
                    }
                }
            }
        }
    }

    /// Stream the bytes into the part file / upload, starting where the
    /// earlier attempt stopped when `resumed`. The writer is dropped
    /// (closed) before returning.
    fn stream(
        &mut self,
        src_fs: &dyn Vfs,
        src: &Entry,
        dst_fs: &dyn Vfs,
        partial: &Partial,
        resumed: bool,
        copied: &mut u64,
    ) -> Result<(), StreamError> {
        let dst = VfsPath::new(&partial.dst);
        let (mut writer, offset) = match &partial.part {
            None => {
                let session = if resumed {
                    partial.session.as_ref()
                } else {
                    None
                };
                let up = dst_fs
                    .open_upload(&dst, src.size, session, self.checkpoint())
                    .map_err(StreamError::Write)?;
                if resumed && up.offset == 0 && session.is_some() {
                    self.note(format!(
                        "{}: the upload session expired; uploading again from the start",
                        src.path
                    ));
                }
                (up.writer, up.offset)
            }
            Some(part) => {
                let part = VfsPath::new(part);
                let caps = dst_fs.supports_resume();
                let mut offset = 0;
                if resumed {
                    match dst_fs.stat(&part) {
                        Ok(e) if !caps.append && e.size > 0 => self.note(format!(
                            "{}: {} can't continue partial files; copying it again from the start",
                            src.path,
                            dst_fs.label()
                        )),
                        Ok(e) if e.size > src.size => self.note(format!(
                            "{}: the partial file is larger than the source; copying it again from the start",
                            src.path
                        )),
                        Ok(e) => offset = e.size,
                        Err(VfsError::NotFound(_)) => {}
                        Err(e) => return Err(StreamError::Write(e)),
                    }
                }
                let w = if offset > 0 {
                    dst_fs.open_append(&part)
                } else {
                    dst_fs.open_write(&part)
                }
                .map_err(StreamError::Write)?;
                (w, offset)
            }
        };
        let mut reader = src_fs
            .open_read_at(&src.path, offset)
            .map_err(StreamError::Open)?;
        if offset > 0 {
            log::info!(
                "transfer job {}: continuing {} at byte {offset}",
                self.slot.id,
                src.path
            );
            self.status().resume.resumed_from = Some(offset);
            *copied += offset;
            let window = self.window_bytes;
            self.add_bytes(offset);
            self.window_bytes = window;
        }
        let mut buf = std::mem::take(&mut self.buf);
        if buf.is_empty() {
            buf = vec![0u8; COPY_BUFFER_SIZE];
        }
        let result = loop {
            if let Err(stop) = self.check_stop() {
                break Err(StreamError::Stop(stop));
            }
            let n = match reader.read(&mut buf) {
                Ok(0) => break writer.flush().map_err(|e| StreamError::Write(e.into())),
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => break Err(StreamError::Read(e.into())),
            };
            if let Err(e) = writer.write_all(&buf[..n]) {
                break Err(StreamError::Write(e.into()));
            }
            *copied += n as u64;
            self.add_bytes(n as u64);
        };
        self.buf = buf;
        drop(writer);
        drop(reader);
        result
    }
}

/// Copy `from` to `to` on the same backend by streaming (no rename).
fn copy_within(fs: &dyn Vfs, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
    let mut r = fs.open_read(from)?;
    let mut w = fs.open_write(to)?;
    std::io::copy(&mut r, &mut w)?;
    w.flush()?;
    Ok(())
}

/// One side of a conflict question.
fn item(e: &Entry, path: &VfsPath) -> ConflictItem {
    let is_dir = e.is_dir();
    ConflictItem {
        path: path.clone(),
        is_dir,
        size: (!is_dir).then_some(e.size),
        modified: e.modified,
    }
}

fn source_is_newer(src: &Entry, dst: &Entry) -> bool {
    match (src.modified, dst.modified) {
        (Some(s), Some(d)) => (s - d).num_milliseconds() >= NEWER_TOLERANCE_SECS * 1000,
        _ => src.size != dst.size,
    }
}

/// Create `dir` and missing parents; error if it exists but isn't a directory.
pub(crate) fn ensure_dir(fs: &dyn Vfs, dir: &VfsPath) -> Result<(), VfsError> {
    match fs.stat(dir) {
        Ok(e) if e.is_dir() => Ok(()),
        Ok(_) => Err(VfsError::NotADirectory(dir.to_string())),
        Err(e) if e.is_transient() => Err(e),
        Err(_) => {
            if let Some(p) = dir.parent() {
                ensure_dir(fs, &p)?;
            }
            match fs.create_dir(dir) {
                Err(VfsError::AlreadyExists(_)) => Ok(()),
                r => r,
            }
        }
    }
}
