//! Book-keeping for the file actions reported to plugins
//! ([`manta_plugin::PluginEvent::FileAction`]): ids, start times, the
//! transfer jobs behind an action and their combined result.

use std::collections::HashMap;
use std::time::Instant;

use manta_ops::{JobId, JobState, JobStatus};
use manta_plugin::{FileAction, FileActionPhase};

/// An action that started and has not finished yet.
struct Running {
    action: FileAction,
    started: Instant,
    /// Transfer jobs still running for it.
    jobs: usize,
    files: u64,
    bytes: u64,
    errors: Vec<String>,
    cancelled: bool,
    failed: bool,
}

#[derive(Default)]
pub struct ActionTracker {
    next: u64,
    running: HashMap<u64, Running>,
    /// Transfer job → action id.
    jobs: HashMap<JobId, u64>,
}

impl ActionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// Remember a started action; returns it (phase `Started`) for sending.
    pub fn start(&mut self, mut action: FileAction) -> FileAction {
        action.phase = FileActionPhase::Started;
        self.running.insert(
            action.id,
            Running {
                action: action.clone(),
                started: Instant::now(),
                jobs: 0,
                files: 0,
                bytes: 0,
                errors: Vec::new(),
                cancelled: false,
                failed: false,
            },
        );
        action
    }

    /// The action `id` is carried out by the transfer job `job`.
    pub fn attach_job(&mut self, id: u64, job: JobId) {
        if let Some(r) = self.running.get_mut(&id) {
            r.jobs += 1;
            self.jobs.insert(job, id);
        }
    }

    #[cfg(test)]
    pub fn is_tracked_job(&self, job: JobId) -> bool {
        self.jobs.contains_key(&job)
    }

    /// Jobs we wait for.
    pub fn tracked_jobs(&self) -> Vec<JobId> {
        self.jobs.keys().copied().collect()
    }

    /// A tracked transfer job reached a final state. Returns the finished
    /// action once its last job is done.
    pub fn job_finished(&mut self, st: &JobStatus) -> Option<FileAction> {
        let id = self.jobs.remove(&st.id)?;
        let r = self.running.get_mut(&id)?;
        r.jobs = r.jobs.saturating_sub(1);
        r.files += st.files_done;
        r.bytes += st.bytes_done;
        match &st.state {
            JobState::Failed(e) => {
                r.failed = true;
                r.errors.push(e.clone());
            }
            JobState::Cancelled => r.cancelled = true,
            _ => {}
        }
        r.errors.extend(st.errors.iter().cloned());
        if r.jobs == 0 {
            self.finish(id, None)
        } else {
            None
        }
    }

    /// A tracked job vanished without being seen finished (cleared): stop
    /// waiting for it.
    pub fn forget_job(&mut self, job: JobId) -> Option<FileAction> {
        let id = self.jobs.remove(&job)?;
        let r = self.running.get_mut(&id)?;
        r.jobs = r.jobs.saturating_sub(1);
        if r.jobs == 0 {
            self.finish(id, None)
        } else {
            None
        }
    }

    /// Finish an action with a file count and per-item errors (batch
    /// rename).
    pub fn finish_with(&mut self, id: u64, files: u64, errors: Vec<String>) -> Option<FileAction> {
        let r = self.running.get_mut(&id)?;
        r.files += files;
        r.failed |= !errors.is_empty();
        r.errors.extend(errors);
        self.finish(id, None)
    }

    /// Finish an action that is not a transfer job (rename, new folder,
    /// open, connect, …). `outcome`: `Some(Ok(files))` or `Some(Err(e))`;
    /// `None` uses what the jobs reported.
    pub fn finish(&mut self, id: u64, outcome: Option<Result<u64, String>>) -> Option<FileAction> {
        let mut r = self.running.remove(&id)?;
        match outcome {
            Some(Ok(files)) => r.files += files,
            Some(Err(e)) => {
                r.failed = true;
                r.errors.push(e);
            }
            None => {}
        }
        let mut a = r.action;
        a.phase = FileActionPhase::Finished {
            ok: !r.failed && !r.cancelled && r.errors.is_empty(),
            files: r.files,
            bytes: r.bytes,
            errors: r.errors,
            cancelled: r.cancelled,
            elapsed_ms: r.started.elapsed().as_millis() as u64,
        };
        Some(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::VfsPath;
    use manta_ops::JobKind;
    use manta_plugin::{FileActionKind, PaneId};

    fn status(id: JobId, state: JobState, files: u64, bytes: u64, errors: &[&str]) -> JobStatus {
        JobStatus {
            id,
            kind: JobKind::Copy,
            title: String::new(),
            state,
            current_file: None,
            bytes_done: bytes,
            bytes_total: bytes,
            files_done: files,
            files_total: files,
            speed: 0.0,
            errors: errors.iter().map(|s| s.to_string()).collect(),
            resume: Default::default(),
        }
    }

    fn action(t: &mut ActionTracker, kind: FileActionKind) -> u64 {
        let id = t.next_id();
        let a = t.start(FileAction::new(
            id,
            PaneId::Left,
            kind,
            vec![VfsPath::new("/a")],
        ));
        assert_eq!(a.phase, FileActionPhase::Started);
        id
    }

    #[test]
    fn sync_combines_its_jobs() {
        let mut t = ActionTracker::new();
        let id = action(&mut t, FileActionKind::Sync);
        t.attach_job(id, 10);
        t.attach_job(id, 11);
        assert!(t
            .job_finished(&status(10, JobState::Done, 2, 100, &[]))
            .is_none());
        let a = t
            .job_finished(&status(11, JobState::Done, 1, 50, &["skipped x"]))
            .unwrap();
        assert_eq!(a.id, id);
        let FileActionPhase::Finished {
            ok,
            files,
            bytes,
            errors,
            cancelled,
            ..
        } = a.phase
        else {
            panic!()
        };
        assert!(!ok && !cancelled);
        assert_eq!((files, bytes), (3, 150));
        assert_eq!(errors, vec!["skipped x".to_string()]);
        assert!(!t.is_tracked_job(10) && !t.is_tracked_job(11));
    }

    #[test]
    fn failed_cancelled_and_worker_results() {
        let mut t = ActionTracker::new();
        let id = action(&mut t, FileActionKind::Copy);
        t.attach_job(id, 1);
        let a = t
            .job_finished(&status(1, JobState::Failed("no space".into()), 0, 0, &[]))
            .unwrap();
        assert!(
            matches!(a.phase, FileActionPhase::Finished { ok: false, ref errors, .. } if errors[0] == "no space")
        );

        let id = action(&mut t, FileActionKind::Move);
        t.attach_job(id, 2);
        let a = t
            .job_finished(&status(2, JobState::Cancelled, 1, 5, &[]))
            .unwrap();
        assert!(matches!(
            a.phase,
            FileActionPhase::Finished {
                ok: false,
                cancelled: true,
                ..
            }
        ));

        let id = action(&mut t, FileActionKind::Rename);
        let a = t.finish(id, Some(Ok(0))).unwrap();
        assert!(matches!(
            a.phase,
            FileActionPhase::Finished {
                ok: true,
                files: 0,
                ..
            }
        ));
        assert!(t.finish(id, Some(Ok(0))).is_none(), "only once");
        let id = action(&mut t, FileActionKind::NewFolder);
        let a = t.finish(id, Some(Err("exists".into()))).unwrap();
        assert!(matches!(
            a.phase,
            FileActionPhase::Finished { ok: false, .. }
        ));
    }
}
