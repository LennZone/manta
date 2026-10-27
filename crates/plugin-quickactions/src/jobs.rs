//! Running actions: at most [`MAX_PARALLEL`] background jobs at a time,
//! each command in its own process group (`/bin/sh -c`, stdin closed) so
//! Cancel stops the whole pipeline. Jobs run on their own threads and
//! report through the shared state and the plugin handle.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_plugin::{HostRequest, PaneId, ToastLevel};

use crate::core::{self, Action, Mode, Output};
use crate::i18n::tr;
use crate::{Shared, PANEL};

pub const MAX_PARALLEL: usize = 2;
/// Finished runs kept for the panel.
pub const HISTORY: usize = 8;
/// Characters of output kept per run.
pub const TAIL: usize = 4000;
/// Before a "Running…" toast appears.
pub const PROGRESS_AFTER: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Queued,
    Running,
    Ok,
    Failed,
    Cancelled,
    /// Handed to the terminal (nothing to track).
    Terminal,
}

impl Status {
    pub fn busy(self) -> bool {
        matches!(self, Status::Queued | Status::Running)
    }
}

pub struct Job {
    pub id: u64,
    pub action: Action,
    pub pane: PaneId,
    /// [(command, cwd)]
    pub commands: Vec<(String, String)>,
    pub count: usize,
    pub refresh_panes: Vec<PaneId>,
    pub status: Status,
    pub step: usize,
    pub started: Instant,
    pub ended: Option<Instant>,
    pub stdout: String,
    pub stderr: String,
    pub returncode: Option<i32>,
    pub cancelled: Arc<AtomicBool>,
    /// Process group of the running command.
    pub pgid: Option<i32>,
}

impl Job {
    pub fn new(
        id: u64,
        action: Action,
        pane: PaneId,
        commands: Vec<(String, String)>,
        count: usize,
    ) -> Job {
        Job {
            id,
            action,
            pane,
            commands,
            count,
            refresh_panes: Vec::new(),
            status: Status::Queued,
            step: 0,
            started: Instant::now(),
            ended: None,
            stdout: String::new(),
            stderr: String::new(),
            returncode: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            pgid: None,
        }
    }

    pub fn display_command(&self) -> String {
        let i = if self.step < self.commands.len() {
            self.step
        } else {
            self.commands.len().saturating_sub(1)
        };
        let cmd = self.commands.get(i).map(|c| c.0.as_str()).unwrap_or("");
        let more = self.commands.len().saturating_sub(1);
        if more > 0 {
            // Not a plural: "(+{count} more)".
            format!(
                "{cmd}   {}",
                crate::i18n::CATALOG.fmt("status.more", &[("count", &more)])
            )
        } else {
            cmd.to_string()
        }
    }

    /// The last `lines` lines of stdout + stderr.
    pub fn tail(&self, lines: usize) -> String {
        let text = format!("{}{}", self.stdout, self.stderr);
        let text = text.trim_end_matches('\n');
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    /// Stop the job: SIGTERM to the process group of its command.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(pg) = self.pgid {
            kill_group(pg);
        }
    }
}

/// SIGTERM to the whole group, with the group stopped meanwhile: the kernel
/// signals one member after the other, and a shell whose child died first
/// could still start its next command (`sleep 9; touch x`) before its own
/// SIGTERM arrives. Stopped, nobody runs until every member has it pending.
fn kill_group(pgid: i32) {
    if pgid > 1 {
        // SAFETY: plain syscalls; a negative pid addresses the process group.
        unsafe {
            libc::kill(-pgid, libc::SIGSTOP);
            libc::kill(-pgid, libc::SIGTERM);
            libc::kill(-pgid, libc::SIGCONT);
        }
    }
}

fn keep_tail(s: String) -> String {
    let n = s.chars().count();
    if n > TAIL {
        s.chars().skip(n - TAIL).collect()
    } else {
        s
    }
}

/// Add a job (newest first) and forget old finished runs.
pub fn add_job(shared: &Shared, job: Job) {
    let mut st = shared.lock();
    st.shown_job = Some(job.id);
    st.jobs.insert(0, job);
    let finished: Vec<u64> = st
        .jobs
        .iter()
        .filter(|j| !j.status.busy())
        .map(|j| j.id)
        .collect();
    for old in finished.iter().skip(HISTORY) {
        st.jobs.retain(|j| j.id != *old);
    }
}

/// Start queued jobs while fewer than [`MAX_PARALLEL`] run.
pub fn schedule(shared: &Arc<crate::SharedState>) {
    let start: Vec<u64> = {
        let mut st = shared.lock();
        let running = st
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running)
            .count();
        let queued: Vec<u64> = st
            .jobs
            .iter()
            .rev()
            .filter(|j| j.status == Status::Queued)
            .map(|j| j.id)
            .collect();
        let start: Vec<u64> = queued
            .into_iter()
            .take(MAX_PARALLEL.saturating_sub(running))
            .collect();
        for j in st.jobs.iter_mut().filter(|j| start.contains(&j.id)) {
            j.status = Status::Running; // claimed; run_job sets the start time
        }
        start
    };
    for id in start {
        let shared = shared.clone();
        let _ = std::thread::Builder::new()
            .name(format!("qa-job-{id}"))
            .spawn(move || run_job(&shared, id));
    }
}

/// A job thread: run the job's commands one after another.
fn run_job(shared: &Arc<crate::SharedState>, id: u64) {
    let _lang = shared.ui_lang();
    let (commands, cancelled) = {
        let mut st = shared.lock();
        let Some(job) = st.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        job.started = Instant::now();
        (job.commands.clone(), job.cancelled.clone())
    };
    shared.repaint();
    let mut ok = true;
    for (i, (cmd, cwd)) in commands.iter().enumerate() {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        shared.with_job(id, |j| j.step = i);
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(cmd).current_dir(cwd);
        for k in [
            "MANTA_PLUGIN_ID",
            "MANTA_PLUGIN_DATA_DIR",
            "MANTA_API_VERSION",
            "PYTHONUNBUFFERED",
        ] {
            c.env_remove(k);
        }
        // Own process group, so Cancel stops the whole pipeline; stdin is
        // closed (nothing to type into a hidden command).
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                shared.with_job(id, |j| {
                    j.stderr = keep_tail(format!("{}{e}\n", j.stderr));
                    j.returncode = Some(127);
                });
                ok = false;
                break;
            }
        };
        let pgid = child.id() as i32;
        shared.with_job(id, |j| j.pgid = Some(pgid));
        if cancelled.load(Ordering::SeqCst) {
            kill_group(pgid);
        }
        let reader = |r: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_end(&mut buf);
                }
                buf
            })
        };
        let out_t = reader(
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        );
        let err_t = reader(
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        );
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    if cancelled.load(Ordering::SeqCst) {
                        kill_group(pgid);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break None,
            }
        };
        let out = out_t.join().unwrap_or_default();
        let err = err_t.join().unwrap_or_default();
        let code = status.and_then(|s| {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                s.code().or_else(|| s.signal().map(|sig| -sig))
            }
            #[cfg(not(unix))]
            s.code()
        });
        shared.with_job(id, |j| {
            j.stdout = keep_tail(format!("{}{}", j.stdout, String::from_utf8_lossy(&out)));
            j.stderr = keep_tail(format!("{}{}", j.stderr, String::from_utf8_lossy(&err)));
            j.returncode = code;
            j.pgid = None;
        });
        if code != Some(0) {
            ok = false;
            break;
        }
    }
    shared.with_job(id, |j| {
        j.ended = Some(Instant::now());
        j.status = if j.cancelled.load(Ordering::SeqCst) {
            Status::Cancelled
        } else if ok {
            Status::Ok
        } else {
            Status::Failed
        };
    });
    finish(shared, id);
    schedule(shared);
}

/// The result toast (or panel) of a finished job, and pane refreshes.
fn finish(shared: &Shared, id: u64) {
    let Some((action, status, stdout, stderr, code, count, panes)) = shared.with_job(id, |j| {
        (
            j.action.clone(),
            j.status,
            j.stdout.clone(),
            j.stderr.clone(),
            j.returncode,
            j.count,
            j.refresh_panes.clone(),
        )
    }) else {
        return;
    };
    let title = core::title(&action);
    match status {
        Status::Cancelled => shared.toast(ToastLevel::Info, tr!("toast.cancelled", title = title)),
        Status::Failed => {
            let err = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            let lines: Vec<&str> = err.lines().collect();
            let why = if lines.is_empty() {
                tr!(
                    "toast.exit_status",
                    code = code.map(|c| c.to_string()).unwrap_or_else(|| "None".into())
                )
            } else {
                lines[lines.len().saturating_sub(3)..].join("\n")
            };
            shared.toast(
                ToastLevel::Error,
                tr!("toast.failed", title = title, why = why),
            );
        }
        _ if action.output == Output::Toast => {
            let mut lines: Vec<String> = stdout
                .trim()
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() {
                lines.push(tr!("toast.no_output").to_string());
            }
            let out = if lines.len() > 8 {
                let mut v: Vec<String> = lines[..3].to_vec();
                v.push("…".into());
                v.extend_from_slice(&lines[lines.len() - 4..]);
                v.join("\n")
            } else {
                lines.join("\n")
            };
            shared.toast(ToastLevel::Success, out);
        }
        _ if action.output == Output::Panel => {
            shared.lock().shown_job = Some(id);
            shared.send(HostRequest::SetPanelOpen {
                panel: PANEL.into(),
                pane: None,
                open: true,
            });
        }
        _ => {
            if let Some(m) = core::message(&action, count) {
                shared.toast(ToastLevel::Success, m);
            } else if action.mode != Mode::Silent {
                shared.toast(ToastLevel::Success, tr!("toast.done", title = title));
            }
        }
    }
    if action.refresh {
        for pane in panes {
            shared.send(HostRequest::RefreshPane { pane: Some(pane) });
        }
    }
    shared.repaint();
}

/// After [`PROGRESS_AFTER`]: a "Running…" toast with Cancel if the job
/// still runs (background mode only).
pub fn progress_toast_later(shared: Arc<crate::SharedState>, id: u64) {
    let _ = std::thread::Builder::new()
        .name("qa-progress".into())
        .spawn(move || {
            std::thread::sleep(PROGRESS_AFTER);
            let _lang = shared.ui_lang();
            let Some((busy, mode, title, count)) = shared.with_job(id, |j| {
                (
                    j.status.busy(),
                    j.action.mode,
                    core::title(&j.action),
                    j.count,
                )
            }) else {
                return;
            };
            if busy && mode == Mode::Background {
                shared.send(HostRequest::ActionToast {
                    level: ToastLevel::Info,
                    text: tr!(
                        "toast.running",
                        title = title,
                        what = tr!("items", count = count)
                    ),
                    button: tr!("cancel").to_string(),
                    command: format!("cancel-{id}"),
                });
            }
        });
}

/// Cancel a running or waiting job.
pub fn cancel_job(shared: &Shared, id: u64) {
    {
        let mut st = shared.lock();
        let Some(job) = st.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        match job.status {
            Status::Queued => {
                job.status = Status::Cancelled;
                job.ended = Some(Instant::now());
                job.cancelled.store(true, Ordering::SeqCst);
            }
            Status::Running => job.cancel(),
            _ => return,
        }
    }
    shared.repaint();
}
