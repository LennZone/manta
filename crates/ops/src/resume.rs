//! Building blocks for resumable transfers: retry backoff, `.manta-part`
//! naming, per-job progress (what the journal persists) and the connection
//! a job needs ([`Endpoint`]).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use manta_core::{Auth, ConnectionConfig, Entry, Protocol, SharedVfs, UploadSession, VfsPath};
use serde::{Deserialize, Serialize};

use crate::{ConflictChoice, ConflictPolicy};

/// Suffix of partial files (`.NAME.manta-part` next to the target).
pub const PART_SUFFIX: &str = ".manta-part";

/// The hidden partial file for `dst`: `.NAME.manta-part` in the same folder.
pub fn part_path(dst: &VfsPath) -> VfsPath {
    let parent = dst.parent().unwrap_or_else(VfsPath::root);
    let name = dst.file_name();
    let hidden = if name.starts_with('.') {
        name.to_string()
    } else {
        format!(".{name}")
    };
    parent.join(&format!("{hidden}{PART_SUFFIX}"))
}

/// True for names written by [`part_path`].
pub fn is_part_file(name: &str) -> bool {
    name.ends_with(PART_SUFFIX)
}

/// Exponential retry backoff: `base`, 2·`base`, 4·`base`, ... capped at `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    /// 1 s → 2 → 4 → ... → 60 s.
    fn default() -> Self {
        Backoff {
            base: Duration::from_secs(1),
            max: Duration::from_secs(60),
        }
    }
}

impl Backoff {
    /// Delay before retry number `attempt` (1-based).
    pub fn delay(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(30);
        self.base.saturating_mul(1u32 << shift).min(self.max)
    }
}

/// Which connection a job's source or destination lives on, as persisted in
/// the journal. Never contains secrets: passwords and key passphrases are
/// stripped (see [`Endpoint::remote`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Endpoint {
    /// The local file system.
    Local,
    /// A server / cloud connection.
    Remote { config: Box<ConnectionConfig> },
    /// Something that can't be reopened after a restart (an archive opened
    /// as a folder, a test backend, ...).
    Unknown { label: String },
}

impl Endpoint {
    /// A remote endpoint for `cfg`, without secrets.
    pub fn remote(cfg: &ConnectionConfig) -> Endpoint {
        let mut config = cfg.clone();
        config.auth = match &cfg.auth {
            Auth::Password(_) => Auth::None,
            Auth::KeyFile { path, .. } => Auth::KeyFile {
                path: path.clone(),
                passphrase: None,
            },
            other => other.clone(),
        };
        if cfg.protocol == Protocol::Local {
            return Endpoint::Local;
        }
        Endpoint::Remote {
            config: Box::new(config),
        }
    }

    /// Best guess without a config: the local file system if `fs` is a
    /// writable local backend, otherwise [`Endpoint::Unknown`].
    pub fn guess(fs: &SharedVfs) -> Endpoint {
        if fs.protocol() == Protocol::Local
            && !fs.is_read_only()
            && fs.local_path(&VfsPath::root()).is_some()
        {
            Endpoint::Local
        } else {
            Endpoint::Unknown { label: fs.label() }
        }
    }

    /// Short name for the UI ("user@host (SFTP)").
    pub fn label(&self) -> String {
        match self {
            Endpoint::Local => "Local".into(),
            Endpoint::Unknown { label } => label.clone(),
            Endpoint::Remote { config: c } => {
                if !c.name.trim().is_empty() {
                    format!("{} ({})", c.name.trim(), c.protocol)
                } else if c.username.is_empty() || c.protocol == Protocol::S3 {
                    format!("{} ({})", c.host, c.protocol)
                } else {
                    format!("{}@{} ({})", c.username, c.host, c.protocol)
                }
            }
        }
    }

    /// The connection config to reconnect with (secrets stripped).
    pub fn config(&self) -> Option<&ConnectionConfig> {
        match self {
            Endpoint::Remote { config } => Some(config),
            _ => None,
        }
    }

    /// Same server account / bucket / container (ignores name, start
    /// folder and secrets).
    pub fn same_connection(&self, other: &Endpoint) -> bool {
        match (self, other) {
            (Endpoint::Local, Endpoint::Local) => true,
            (Endpoint::Remote { config: a }, Endpoint::Remote { config: b }) => {
                same_connection(a, b)
            }
            _ => false,
        }
    }

    /// Whether this endpoint matches the connection `cfg`.
    pub fn matches(&self, cfg: &ConnectionConfig) -> bool {
        self.same_connection(&Endpoint::remote(cfg))
    }
}

/// Same account on the same server (see [`Endpoint::same_connection`]).
pub fn same_connection(a: &ConnectionConfig, b: &ConnectionConfig) -> bool {
    let port = |c: &ConnectionConfig| {
        if c.port == 0 {
            ConnectionConfig::default_port(c.protocol)
        } else {
            c.port
        }
    };
    a.protocol == b.protocol
        && a.host.eq_ignore_ascii_case(&b.host)
        && port(a) == port(b)
        && a.username == b.username
        && a.s3.as_ref().map(|s| (&s.endpoint, &s.bucket, &s.profile))
            == b.s3.as_ref().map(|s| (&s.endpoint, &s.bucket, &s.profile))
        && a.container == b.container
        && webdav_root(a) == webdav_root(b)
        && a.smb.as_ref().map(|s| s.share.trim().to_lowercase())
            == b.smb.as_ref().map(|s| s.share.trim().to_lowercase())
}

/// The WebDAV folder of a connection, without slashes around it.
fn webdav_root(c: &ConnectionConfig) -> String {
    c.webdav
        .as_ref()
        .map(|w| w.root.trim().trim_matches('/').to_string())
        .unwrap_or_default()
}

/// Where an interrupted file stands (persisted in the journal).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partial {
    /// Source path.
    pub src: String,
    /// Final destination path (after conflict handling).
    pub dst: String,
    /// The `.manta-part` file; `None` when the destination uses an upload
    /// session (S3, Drive) instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// Source fingerprint when the file was started.
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// S3 multipart / Drive resumable upload state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<UploadSession>,
    /// The answer given for this file under [`ConflictPolicy::Ask`], so a
    /// resumed file doesn't ask again at the final rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<ConflictPolicy>,
}

impl Partial {
    /// Whether the source still looks like when the transfer began. `tag`
    /// is only compared when both sides have one.
    pub fn source_unchanged(&self, now: &Entry, tag: Option<&str>) -> bool {
        let mtime_same = match (self.mtime, now.modified) {
            (Some(a), Some(b)) => a == b,
            (None, None) => true,
            _ => false,
        };
        let tag_same = match (self.tag.as_deref(), tag) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
        self.size == now.size && mtime_same && tag_same
    }
}

/// What a job has already done; lets a paused, network-interrupted or
/// restored job continue where it stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Source paths that are finished (copied / moved / deleted).
    #[serde(default)]
    pub completed: BTreeSet<String>,
    /// Source paths that were skipped or failed for good (not retried).
    #[serde(default)]
    pub failed: BTreeSet<String>,
    /// Destination chosen for a source (e.g. "photos 2" with "keep both"),
    /// so a re-run doesn't pick yet another name.
    #[serde(default)]
    pub dst_map: BTreeMap<String, String>,
    /// The file that was in flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<Partial>,
    /// "Apply to all" answered a conflict question: the policy for the
    /// job's further file conflicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictPolicy>,
    /// "Apply to all" answered a folder conflict: Merge, KeepBoth or Skip
    /// for further existing top-level folders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folders: Option<ConflictChoice>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::EntryKind;

    #[test]
    fn backoff_schedule() {
        let b = Backoff::default();
        let secs: Vec<u64> = (1..=9).map(|a| b.delay(a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(b.delay(0), Duration::from_secs(1));
        assert_eq!(b.delay(1000), Duration::from_secs(60));
        let fast = Backoff {
            base: Duration::from_millis(10),
            max: Duration::from_millis(50),
        };
        assert_eq!(fast.delay(3), Duration::from_millis(40));
        assert_eq!(fast.delay(4), Duration::from_millis(50));
    }

    #[test]
    fn part_paths() {
        assert_eq!(
            part_path(&VfsPath::new("/a/b.zip")).as_str(),
            "/a/.b.zip.manta-part"
        );
        assert_eq!(
            part_path(&VfsPath::new("/a/.env")).as_str(),
            "/a/.env.manta-part"
        );
        assert_eq!(part_path(&VfsPath::new("/x")).as_str(), "/.x.manta-part");
        assert!(is_part_file(".b.zip.manta-part"));
        assert!(!is_part_file("b.zip"));
    }

    fn entry(size: u64, secs: i64) -> Entry {
        Entry {
            name: "f".into(),
            path: VfsPath::new("/f"),
            kind: EntryKind::File,
            size,
            modified: DateTime::from_timestamp(secs, 0),
            permissions: None,
            link_target: None,
            hidden: false,
        }
    }

    #[test]
    fn source_changed_detection() {
        let p = Partial {
            src: "/f".into(),
            dst: "/g".into(),
            size: 100,
            mtime: DateTime::from_timestamp(5, 0),
            tag: Some("\"etag1\"".into()),
            ..Default::default()
        };
        assert!(p.source_unchanged(&entry(100, 5), Some("\"etag1\"")));
        assert!(p.source_unchanged(&entry(100, 5), None));
        assert!(!p.source_unchanged(&entry(101, 5), None), "size changed");
        assert!(!p.source_unchanged(&entry(100, 6), None), "mtime changed");
        assert!(
            !p.source_unchanged(&entry(100, 5), Some("\"etag2\"")),
            "etag changed"
        );
    }

    #[test]
    fn endpoint_strips_secrets_and_matches() {
        let cfg = ConnectionConfig {
            name: "Web".into(),
            protocol: Protocol::Sftp,
            host: "Example.com".into(),
            port: 22,
            username: "me".into(),
            auth: Auth::Password("hunter2".into()),
            initial_path: Some("/srv".into()),
            s3: None,
            container: None,
            ftp: None,
            webdav: None,
            smb: None,
        };
        let ep = Endpoint::remote(&cfg);
        let json = serde_json::to_string(&ep).unwrap();
        assert!(!json.contains("hunter2"), "{json}");
        let key = Endpoint::remote(&ConnectionConfig {
            auth: Auth::KeyFile {
                path: "/k".into(),
                passphrase: Some("secret".into()),
            },
            ..cfg.clone()
        });
        assert!(!serde_json::to_string(&key).unwrap().contains("secret"));
        let mut other = cfg.clone();
        other.host = "example.com".into();
        other.port = 0;
        other.name = "Renamed".into();
        other.initial_path = None;
        assert!(ep.matches(&other));
        other.username = "you".into();
        assert!(!ep.matches(&other));
        assert_eq!(ep.label(), "Web (SFTP)");
        assert!(Endpoint::Local.same_connection(&Endpoint::Local));
        assert!(!Endpoint::Local.same_connection(&ep));
    }
}
