//! The transfer journal: unfinished jobs persisted as JSON so they survive a
//! quit, crash or reboot (`~/Library/Application Support/Manta/transfers.json`
//! in the app).
//!
//! Format (version 1):
//! ```json
//! { "version": 1,
//!   "jobs": [ { "id": 7, "kind": "copy", "title": "Copying \"a.iso\" to /dl",
//!               "state": "waiting", "conflict": "overwrite",
//!               "src": {"type": "remote", "config": {...no secrets...}},
//!               "dst": {"type": "local"},
//!               "sources": ["/pub/a.iso"], "dst_dir": "/Users/me/dl",
//!               "progress": { "completed": [], "failed": [], "dst_map": {},
//!                             "partial": { "src": "/pub/a.iso", "dst": "/Users/me/dl/a.iso",
//!                                          "part": "/Users/me/dl/.a.iso.manta-part",
//!                                          "size": 734003200, "mtime": "...", "tag": null,
//!                                          "session": null } },
//!               "bytes_done": 52428800, "bytes_total": 734003200,
//!               "files_done": 0, "files_total": 1, "errors": [], "log": [] } ] }
//! ```
//!
//! Writes are atomic (temp file, fsync, rename), so a crash leaves either
//! the old or the new journal, never a torn one. A journal that can't be
//! parsed is moved aside (`transfers.json.corrupt`) and treated as empty.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::resume::{Endpoint, Progress};
use crate::{ConflictPolicy, JobKind};

pub const JOURNAL_VERSION: u32 = 1;

/// How a job stood when the journal was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    Queued,
    Running,
    Paused,
    Waiting,
    NeedsConnection,
    /// Waiting for a conflict decision ([`crate::ConflictPolicy::Ask`]);
    /// asks again after a restore.
    Deciding,
}

/// One unfinished job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalJob {
    pub id: u64,
    pub kind: JobKind,
    pub title: String,
    pub state: JournalState,
    pub conflict: ConflictPolicy,
    pub src: Endpoint,
    #[serde(default)]
    pub dst: Option<Endpoint>,
    pub sources: Vec<String>,
    #[serde(default)]
    pub dst_dir: Option<String>,
    #[serde(default)]
    pub progress: Progress,
    #[serde(default)]
    pub bytes_done: u64,
    #[serde(default)]
    pub bytes_total: u64,
    #[serde(default)]
    pub files_done: u64,
    #[serde(default)]
    pub files_total: u64,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub log: Vec<String>,
}

impl JournalJob {
    /// Endpoints this job needs, source first.
    pub fn endpoints(&self) -> Vec<&Endpoint> {
        std::iter::once(&self.src)
            .chain(self.dst.as_ref())
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalFile {
    pub version: u32,
    #[serde(default)]
    pub jobs: Vec<JournalJob>,
}

/// Reads and atomically writes the journal file.
pub struct Journal {
    path: PathBuf,
    /// Serializes writers (the throttled background writer and the final
    /// write at shutdown) so they never share the temp file.
    write_lock: Mutex<()>,
}

impl Journal {
    pub fn new(path: PathBuf) -> Journal {
        Journal {
            path,
            write_lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the jobs; a missing file is empty, an unreadable one is moved
    /// aside and treated as empty.
    pub fn load(&self) -> Vec<JournalJob> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                log::warn!("transfer journal {}: {e}", self.path.display());
                return Vec::new();
            }
        };
        match serde_json::from_str::<JournalFile>(&text) {
            Ok(f) if f.version <= JOURNAL_VERSION => f.jobs,
            Ok(f) => {
                log::warn!(
                    "transfer journal version {} is newer than {JOURNAL_VERSION}; ignored",
                    f.version
                );
                Vec::new()
            }
            Err(e) => {
                log::warn!(
                    "transfer journal {} is damaged ({e}); moved aside",
                    self.path.display()
                );
                let _ = std::fs::rename(&self.path, self.path.with_extension("json.corrupt"));
                Vec::new()
            }
        }
    }

    /// Replace the journal atomically. An empty job list removes the file.
    pub fn save(&self, jobs: &[JournalJob]) -> std::io::Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        if jobs.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = JournalFile {
            version: JOURNAL_VERSION,
            jobs: jobs.to_vec(),
        };
        let json = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resume::Partial;
    use manta_core::{Auth, ConnectionConfig, Protocol, UploadSession};

    fn sample() -> JournalJob {
        let cfg = ConnectionConfig {
            name: "Bucket".into(),
            protocol: Protocol::S3,
            host: "s3.example.com".into(),
            port: 0,
            username: "AKIA".into(),
            auth: Auth::Password("very-secret".into()),
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
            webdav: None,
            smb: None,
        };
        let mut progress = Progress::default();
        progress.completed.insert("/a/1.txt".into());
        progress.dst_map.insert("/a".into(), "/dst/a 2".into());
        progress.partial = Some(Partial {
            src: "/a/big.iso".into(),
            dst: "/b/a 2/big.iso".into(),
            part: None,
            size: 1 << 30,
            mtime: chrono::DateTime::from_timestamp(1_700_000_000, 0),
            tag: Some("\"abc\"".into()),
            session: Some(UploadSession {
                kind: "s3".into(),
                data: "{\"upload_id\":\"u1\"}".into(),
                committed: 32 << 20,
            }),
            choice: Some(ConflictPolicy::Overwrite),
        });
        progress.conflict = Some(ConflictPolicy::Skip);
        JournalJob {
            id: 3,
            kind: JobKind::Copy,
            title: "Copying".into(),
            state: JournalState::Waiting,
            conflict: ConflictPolicy::Rename,
            src: Endpoint::Local,
            dst: Some(Endpoint::remote(&cfg)),
            sources: vec!["/a".into()],
            dst_dir: Some("/b".into()),
            progress,
            bytes_done: 5,
            bytes_total: 10,
            files_done: 1,
            files_total: 2,
            errors: vec!["x".into()],
            log: vec!["restarted".into()],
        }
    }

    #[test]
    fn round_trip_without_secrets() {
        let td = tempfile::tempdir().unwrap();
        let j = Journal::new(td.path().join("Manta/transfers.json"));
        assert!(j.load().is_empty(), "missing file is empty");
        let jobs = vec![sample()];
        j.save(&jobs).unwrap();
        let text = std::fs::read_to_string(j.path()).unwrap();
        assert!(!text.contains("very-secret"), "{text}");
        assert_eq!(j.load(), jobs);
        // An empty queue removes the file.
        j.save(&[]).unwrap();
        assert!(!j.path().exists());
        assert!(j.load().is_empty());
    }

    #[test]
    fn crash_safety() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("transfers.json");
        let j = Journal::new(path.clone());
        let jobs = vec![sample()];
        j.save(&jobs).unwrap();
        // A crash while writing leaves a half-written temp file behind: the
        // journal itself is untouched.
        std::fs::write(
            path.with_extension("json.tmp"),
            b"{\"version\":1,\"jobs\":[{\"id\":",
        )
        .unwrap();
        assert_eq!(j.load(), jobs);
        // Saving again replaces the stale temp file.
        j.save(&jobs).unwrap();
        assert_eq!(j.load(), jobs);
        // A torn journal (written by something else / disk trouble) is moved
        // aside instead of crashing or blocking new jobs.
        let full = std::fs::read(&path).unwrap();
        std::fs::write(&path, &full[..full.len() / 2]).unwrap();
        assert!(j.load().is_empty());
        assert!(path.with_extension("json.corrupt").exists());
        assert!(!path.exists());
        // A newer format is ignored, not misread.
        std::fs::write(&path, b"{\"version\":99,\"jobs\":[]}").unwrap();
        assert!(j.load().is_empty());
    }
}
