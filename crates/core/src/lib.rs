//! Shared types and the virtual file system (VFS) abstraction used by all
//! Manta crates. Every backend (local, SFTP, FTP, ...) implements [`Vfs`];
//! the operations layer and the GUI only ever talk to `Arc<dyn Vfs>`.

use std::fmt;
use std::io::{Read, Write};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod path;

pub use path::VfsPath;

/// Errors produced by any VFS backend or operation.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("not a directory: {0}")]
    NotADirectory(String),
    #[error("directory not empty: {0}")]
    DirectoryNotEmpty(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    /// SFTP: the server's host key is not in `known_hosts` yet. The app
    /// asks the user (host, key type, fingerprint) and, if accepted, trusts
    /// the key and connects again.
    #[error("unknown host key: {} {} {}", .0.host_label(), .0.key_type, .0.fingerprint)]
    UnknownHostKey(Box<HostKeyInfo>),
    /// SFTP: the server's host key differs from the one in `known_hosts`
    /// (server reinstalled — or a man-in-the-middle). Never connects.
    #[error("host key changed (possible man-in-the-middle attack): {} {} {}", .0.host_label(), .0.key_type, .0.fingerprint)]
    HostKeyMismatch(Box<HostKeyInfo>),
    #[error("operation not supported: {0}")]
    Unsupported(String),
    #[error("cancelled")]
    Cancelled,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type VfsResult<T> = Result<T, VfsError>;

/// An SSH server's host key as presented during a connection
/// ([`VfsError::UnknownHostKey`], [`VfsError::HostKeyMismatch`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    /// OpenSSH key type name, e.g. `ssh-ed25519`, `ecdsa-sha2-nistp256`.
    pub key_type: String,
    /// OpenSSH fingerprint: `SHA256:` + unpadded base64 of the key's SHA-256.
    pub fingerprint: String,
    /// The raw public key blob (SSH wire format), for `known_hosts`.
    pub key: Vec<u8>,
    /// The `known_hosts` file that was checked (shown in the warning).
    pub known_hosts: String,
}

impl HostKeyInfo {
    /// `host` for port 22, `[host]:port` otherwise (as in `known_hosts`).
    pub fn host_label(&self) -> String {
        if self.port == 22 || self.port == 0 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }
}

/// Transient vs permanent failures, for retrying transfers after a network
/// drop ([`VfsError::is_transient`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Worth retrying: connection reset / refused / aborted, timeouts, DNS,
    /// TLS EOF, HTTP 5xx / 429, no network.
    Transient,
    /// Retrying won't help: permission, not found, disk full, auth, ...
    Permanent,
}

impl VfsError {
    /// Classify this error (see [`ErrorClass`]).
    pub fn class(&self) -> ErrorClass {
        let transient = match self {
            VfsError::Connection(_) => true,
            VfsError::Io(e) => io_is_transient(e),
            VfsError::Other(m) => message_is_transient(m),
            VfsError::NotFound(_)
            | VfsError::AlreadyExists(_)
            | VfsError::PermissionDenied(_)
            | VfsError::NotADirectory(_)
            | VfsError::DirectoryNotEmpty(_)
            | VfsError::Auth(_)
            | VfsError::UnknownHostKey(_)
            | VfsError::HostKeyMismatch(_)
            | VfsError::Unsupported(_)
            | VfsError::Cancelled => false,
        };
        if transient {
            ErrorClass::Transient
        } else {
            ErrorClass::Permanent
        }
    }

    /// Shorthand for `class() == ErrorClass::Transient`.
    pub fn is_transient(&self) -> bool {
        self.class() == ErrorClass::Transient
    }
}

/// Network-ish `io::Error`s (by kind, then by message for `Other` errors
/// that wrap a backend's message).
pub fn io_is_transient(e: &std::io::Error) -> bool {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::ConnectionReset
        | K::ConnectionRefused
        | K::ConnectionAborted
        | K::NotConnected
        | K::BrokenPipe
        | K::TimedOut
        | K::UnexpectedEof
        | K::AddrNotAvailable
        | K::NetworkDown
        | K::NetworkUnreachable
        | K::HostUnreachable
        | K::WouldBlock => true,
        K::NotFound
        | K::PermissionDenied
        | K::AlreadyExists
        | K::StorageFull
        | K::QuotaExceeded
        | K::ReadOnlyFilesystem
        | K::FileTooLarge
        | K::InvalidInput
        | K::IsADirectory
        | K::NotADirectory
        | K::DirectoryNotEmpty
        | K::Unsupported
        | K::OutOfMemory => false,
        _ => {
            // `io::Error::other(VfsError)` and friends: look at the source,
            // then at the text.
            if let Some(inner) = e.get_ref() {
                if let Some(v) = inner.downcast_ref::<VfsError>() {
                    return v.is_transient();
                }
                if let Some(io) = inner.downcast_ref::<std::io::Error>() {
                    return io_is_transient(io);
                }
            }
            message_is_transient(&e.to_string())
        }
    }
}

/// Heuristic for errors that only exist as text (FTP replies, HTTP
/// statuses, libssh2 / TLS messages).
pub fn message_is_transient(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    // Permanent conditions win over generic words like "connection".
    const PERMANENT: &[&str] = &[
        "permission denied",
        "access denied",
        "not found",
        "no such file",
        "no space",
        "disk full",
        "quota",
        "storage full",
        "authentication",
        "login incorrect",
        "not logged in",
        "unauthorized",
        "forbidden",
        "invalid credentials",
        "certificate",
        "set the region to",
        "wrong region",
        "clock is too far",
        "blocks encrypted ftp data",
        "http 400",
        "http 401",
        "http 403",
        "http 404",
        "552 ",
        "553 ",
        "550 ",
        "530 ",
    ];
    if PERMANENT.iter().any(|p| m.contains(p)) {
        return false;
    }
    const TRANSIENT: &[&str] = &[
        "connection error",
        "connection reset",
        "connection refused",
        "connection aborted",
        "connection closed",
        "connection lost",
        "broken pipe",
        "timed out",
        "timeout",
        "network is unreachable",
        "network is down",
        "host is unreachable",
        "no route to host",
        "not connected",
        "failed to lookup",
        "cannot resolve",
        "name resolution",
        "nodename nor servname",
        "dns",
        "unexpected eof",
        "early eof",
        "end of file",
        "tls",
        "close_notify",
        "socket",
        "transport",
        "offline",
        "internet connection",
        "http 429",
        "http 500",
        "http 502",
        "http 503",
        "http 504",
        "status 429",
        "status 500",
        "status 502",
        "status 503",
        "status 504",
        "too many requests",
        "slow down",
        "slowdown",
        "service unavailable",
        "internal error",
        "bad gateway",
        "gateway timeout",
        "421 ",
        "425 ",
        "426 ",
        "data connection",
    ];
    TRANSIENT.iter().any(|t| m.contains(t))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

/// A single directory entry / file metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// File name only (no directory component).
    pub name: String,
    /// Absolute path inside the backend.
    pub path: VfsPath,
    pub kind: EntryKind,
    /// Size in bytes (0 for directories if unknown).
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    /// Unix permission bits, if the backend knows them.
    pub permissions: Option<u32>,
    /// Target of a symlink, if any.
    pub link_target: Option<String>,
    pub hidden: bool,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}

/// Which protocol a connection uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Protocol {
    Local,
    Sftp,
    Ftp,
    Ftps,
    /// Amazon S3 or an S3-compatible object store (R2, MinIO, B2, ...).
    /// Settings live in [`ConnectionConfig::s3`].
    S3,
    /// A Docker container, through the `docker` CLI. Settings live in
    /// [`ConnectionConfig::container`].
    Docker,
    /// A container in a Kubernetes pod, through `kubectl`. Settings live in
    /// [`ConnectionConfig::container`].
    Kubernetes,
    /// Google Drive through the Drive API v3 (OAuth 2.0). `username` is the
    /// Google account's email address (the refresh token lives in the macOS
    /// Keychain under service `manta-gdrive`, account = email); `host` is
    /// `drive.google.com`; `initial_path` is the optional starting folder.
    /// See [`ConnectionConfig::google_drive`].
    GoogleDrive,
}

impl Protocol {
    /// Docker or Kubernetes: a container browsed through its CLI.
    pub fn is_container(self) -> bool {
        matches!(self, Protocol::Docker | Protocol::Kubernetes)
    }

    /// A cloud storage API (S3 or Google Drive), not a server login.
    pub fn is_cloud(self) -> bool {
        matches!(self, Protocol::S3 | Protocol::GoogleDrive)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Protocol::Local => "Local",
            Protocol::Sftp => "SFTP",
            Protocol::Ftp => "FTP",
            Protocol::Ftps => "FTPS",
            Protocol::S3 => "S3",
            Protocol::Docker => "Docker",
            Protocol::Kubernetes => "Kubernetes",
            Protocol::GoogleDrive => "Google Drive",
        })
    }
}

/// How to log in. `Debug` never shows secrets (configs may end up in logs).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Auth {
    None,
    Password(String),
    /// Path to a private key file, optional passphrase.
    KeyFile {
        path: String,
        passphrase: Option<String>,
    },
    /// Use the running ssh-agent.
    Agent,
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hide = |s: &str| if s.is_empty() { "" } else { "<redacted>" };
        match self {
            Auth::None => f.write_str("None"),
            Auth::Password(p) => f.debug_tuple("Password").field(&hide(p)).finish(),
            Auth::KeyFile { path, passphrase } => f
                .debug_struct("KeyFile")
                .field("path", path)
                .field("passphrase", &passphrase.as_deref().map(hide))
                .finish(),
            Auth::Agent => f.write_str("Agent"),
        }
    }
}

/// Everything needed to open a connection. Persisted as a "favorite".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    /// Directory to open after connecting.
    pub initial_path: Option<String>,
    /// S3 settings (only for [`Protocol::S3`]). For S3, `username` is the
    /// access key ID and `Auth::Password` holds the secret access key;
    /// `host` mirrors the endpoint's host name for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3: Option<Box<S3Options>>,
    /// Container settings (only for [`Protocol::Docker`] and
    /// [`Protocol::Kubernetes`]). `host` mirrors the container name (Docker)
    /// or `namespace/pod` (Kubernetes) for display and matching; `username`
    /// is the pod's container name (Kubernetes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<Box<ContainerOptions>>,
    /// FTP/FTPS settings (only for [`Protocol::Ftp`] / [`Protocol::Ftps`]).
    /// `None` means the defaults ([`FtpOptions::default`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ftp: Option<Box<FtpOptions>>,
}

/// Settings of an FTP / FTPS connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FtpOptions {
    /// FTPS only: how the data connections (listings, transfers) are protected.
    pub data_channel: FtpDataChannel,
}

/// FTPS data channel protection (`PROT P` / `PROT C`). The control
/// connection (login, password, commands) is always encrypted with FTPS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FtpDataChannel {
    /// Encrypt the data connections; if the network resets encrypted data
    /// connections (some firewalls do), continue with clear data
    /// connections and report [`ConnectionNotice::ClearDataChannel`].
    #[default]
    Auto,
    /// Always encrypt; fail if that's not possible.
    Require,
    /// Clear data connections from the start (login stays encrypted).
    Off,
}

/// Something about a live connection the user should know (shown in the
/// pane's location chip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionNotice {
    /// FTPS fell back to clear data connections because the network blocks
    /// encrypted ones; the login stays encrypted.
    ClearDataChannel,
}

/// Settings of a Docker or Kubernetes container connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContainerOptions {
    /// Docker: container name (preferred: it survives re-creation) or ID.
    /// Kubernetes: the container inside the pod ("" = the pod's default).
    pub container: String,
    /// Kubernetes: the kubeconfig context ("" = the current context).
    pub context: String,
    /// Kubernetes: the namespace ("" = the context's default).
    pub namespace: String,
    /// Kubernetes: the pod name.
    pub pod: String,
}

/// Settings of an S3 / S3-compatible connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct S3Options {
    /// Provider preset id the dialog used ("aws", "r2", "minio", ...).
    /// Informational only; the backend uses the explicit fields below.
    pub provider: String,
    /// Endpoint URL such as `https://s3.eu-central-1.amazonaws.com` or
    /// `http://localhost:9000`. Empty: AWS endpoint for `region`.
    pub endpoint: String,
    /// Signing region. Empty: `us-east-1` (or the profile's region).
    pub region: String,
    /// Restrict the connection to one bucket. Empty: `/` lists all buckets.
    pub bucket: String,
    /// Address buckets as `endpoint/bucket/key` instead of
    /// `bucket.endpoint/key` (MinIO and most self-hosted servers need this).
    pub path_style: bool,
    /// Take credentials (and the region, if empty) from this AWS CLI
    /// profile in `~/.aws/credentials` / `~/.aws/config`. `Some("")` means
    /// `$AWS_PROFILE` or `default`.
    pub profile: Option<String>,
    /// Cloudflare R2 account ID (used to build the endpoint).
    pub account_id: String,
}

impl ConnectionConfig {
    pub fn default_port(protocol: Protocol) -> u16 {
        match protocol {
            Protocol::Local => 0,
            Protocol::Sftp => 22,
            Protocol::Ftp | Protocol::Ftps => 21,
            Protocol::S3 | Protocol::GoogleDrive => 443,
            Protocol::Docker | Protocol::Kubernetes => 0,
        }
    }

    /// Host shown for Google Drive connections.
    pub const GOOGLE_DRIVE_HOST: &'static str = "drive.google.com";

    /// A Google Drive connection for the account `email` (signed in before
    /// with `manta_vfs::gdrive::sign_in`). `start_folder` is a Manta path
    /// such as `/My Drive/Projects`; `None` opens My Drive.
    pub fn google_drive(email: &str, start_folder: Option<&str>) -> ConnectionConfig {
        ConnectionConfig {
            name: format!("Google Drive ({email})"),
            protocol: Protocol::GoogleDrive,
            host: Self::GOOGLE_DRIVE_HOST.into(),
            port: 0,
            username: email.trim().to_string(),
            auth: Auth::None,
            initial_path: start_folder
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty()),
            s3: None,
            container: None,
            ftp: None,
        }
    }
}

/// What a backend can do to continue an interrupted transfer
/// ([`Vfs::supports_resume`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResumeCaps {
    /// [`Vfs::open_read_at`] seeks natively (no download-and-discard).
    /// The default implementation is always *correct*, just slower.
    pub read_at: bool,
    /// [`Vfs::open_append`] works: a partial file can be continued.
    pub append: bool,
    /// Uploads go through a server-side session that stays invisible until
    /// it completes and can be continued after a network drop
    /// ([`Vfs::open_upload`]: S3 multipart, Google Drive resumable uploads).
    /// Such backends need no `.manta-part` file.
    pub upload_session: bool,
}

/// Serializable state of a resumable upload (S3 multipart upload id and
/// completed parts, Google Drive session URI, ...). `kind` names the
/// backend; `data` is the backend's own encoding (JSON). Holds no secrets,
/// so it can be persisted in the transfer journal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSession {
    pub kind: String,
    pub data: String,
    /// Bytes the server has confirmed so far.
    #[serde(default)]
    pub committed: u64,
}

/// Called by a resumable upload whenever its [`UploadSession`] changes
/// (session created, part completed), so the caller can persist it.
pub type UploadCheckpoint = Arc<dyn Fn(&UploadSession) + Send + Sync>;

/// A writer for a (possibly continued) upload ([`Vfs::open_upload`]).
pub struct ResumableUpload {
    pub writer: Box<dyn Write + Send>,
    /// Byte offset the caller has to continue at: write the source's bytes
    /// from here on (0 for a fresh upload).
    pub offset: u64,
}

/// The virtual file system interface. Implementations must be thread-safe;
/// they are shared via `Arc<dyn Vfs>` between the GUI thread and transfer
/// worker threads. Blocking calls are fine — callers never invoke them on the
/// UI thread.
pub trait Vfs: Send + Sync {
    /// Human-readable label, e.g. "Local" or "user@host (SFTP)".
    fn label(&self) -> String;
    fn protocol(&self) -> Protocol;
    /// The directory to show initially (home dir for local, login dir remote).
    fn home(&self) -> VfsResult<VfsPath>;

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>>;
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry>;

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()>;
    /// Remove a file or symlink.
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()>;
    /// Remove an *empty* directory.
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()>;
    /// Remove a directory and everything below it in one go, for backends
    /// that do this much faster than a per-entry walk (S3: batch
    /// DeleteObjects). Default `Unsupported`; callers then walk the tree.
    fn remove_tree(&self, _path: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported("remove_tree".into()))
    }
    /// Rename / move within the same backend.
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()>;

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>>;
    /// Create or truncate a file for writing.
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>>;

    fn set_permissions(&self, _path: &VfsPath, _mode: u32) -> VfsResult<()> {
        Err(VfsError::Unsupported("set_permissions".into()))
    }

    /// Set the modification time. Used after copying so that a later compare
    /// sees source and copy as identical.
    fn set_modified(&self, _path: &VfsPath, _mtime: DateTime<Utc>) -> VfsResult<()> {
        Err(VfsError::Unsupported("set_modified".into()))
    }

    /// For local backends: the real OS path (used for "open with default app",
    /// reveal in Finder, etc.). Remote backends return `None`.
    fn local_path(&self, _path: &VfsPath) -> Option<std::path::PathBuf> {
        None
    }

    /// True if nothing can be written to this backend (e.g. an archive
    /// opened as a folder). The GUI disables paste, rename, delete and new
    /// folder for such backends; write calls fail with `Unsupported`.
    fn is_read_only(&self) -> bool {
        false
    }

    /// Downcasting hook for backend-specific features (e.g. the archive
    /// backend's format and encrypted-entry flags). `None` by default.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// A notice about the live connection (e.g. FTPS continued with clear
    /// data connections). Cheap; the GUI polls it. `None` by default.
    fn notice(&self) -> Option<ConnectionNotice> {
        None
    }

    // ---------------------------------------------------------------- resume

    /// Read `path` starting at byte `offset`. The default reads and discards
    /// the first `offset` bytes, so it is correct on every backend; backends
    /// that can seek override it (and report `ResumeCaps::read_at`).
    /// An offset beyond the end of the file is an error.
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        let mut r = self.open_read(path)?;
        if offset > 0 {
            let skipped = std::io::copy(&mut (&mut r).take(offset), &mut std::io::sink())?;
            if skipped < offset {
                return Err(VfsError::Other(format!(
                    "{path}: file is shorter ({skipped} bytes) than the resume offset {offset}"
                )));
            }
        }
        Ok(r)
    }

    /// Open an existing file for appending (create it if missing). Used to
    /// continue a partial file. Default `Unsupported`.
    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        Err(VfsError::Unsupported(format!("append: {path}")))
    }

    /// What this backend can do to continue interrupted transfers.
    fn supports_resume(&self) -> ResumeCaps {
        ResumeCaps::default()
    }

    /// Start (`resume: None`) or continue a server-side resumable upload
    /// to `path` (only when `ResumeCaps::upload_session`). The returned
    /// offset is where the caller continues; it may be lower than
    /// `session.committed` if the server lost data, and 0 if the session
    /// expired. `checkpoint` is called with the new state whenever it
    /// changes. Dropping the writer without `flush()` keeps the session on
    /// the server (so it can be continued); [`Vfs::abort_upload`] discards it.
    fn open_upload(
        &self,
        path: &VfsPath,
        _size: u64,
        _resume: Option<&UploadSession>,
        _checkpoint: UploadCheckpoint,
    ) -> VfsResult<ResumableUpload> {
        Err(VfsError::Unsupported(format!("resumable upload: {path}")))
    }

    /// Discard a resumable upload session (cancelled transfer). Best effort.
    fn abort_upload(&self, _path: &VfsPath, _session: &UploadSession) -> VfsResult<()> {
        Ok(())
    }

    /// A tag that changes whenever the file's content changes (S3 ETag,
    /// Drive md5Checksum), if the backend has one. Used together with size
    /// and mtime to decide whether a partial transfer can be continued.
    fn content_tag(&self, _path: &VfsPath) -> Option<String> {
        None
    }

    /// Re-establish the connection after a network drop (remote backends
    /// drop dead sessions / pooled connections and log in again). Called
    /// before a transfer is retried. Default: nothing to do.
    fn reconnect(&self) -> VfsResult<()> {
        Ok(())
    }
}

pub type SharedVfs = Arc<dyn Vfs>;

#[cfg(test)]
mod resume_tests {
    use super::*;
    use std::io::{Cursor, ErrorKind};

    #[test]
    fn error_classification() {
        let t = |e: VfsError| e.is_transient();
        assert!(t(VfsError::Connection("lost".into())));
        assert!(t(VfsError::Io(std::io::Error::from(
            ErrorKind::ConnectionReset
        ))));
        assert!(t(VfsError::Io(std::io::Error::from(ErrorKind::TimedOut))));
        assert!(t(VfsError::Io(std::io::Error::from(
            ErrorKind::UnexpectedEof
        ))));
        assert!(t(VfsError::Io(std::io::Error::from(
            ErrorKind::ConnectionRefused
        ))));
        assert!(t(VfsError::Other(
            "failed to lookup address information: nodename nor servname provided".into()
        )));
        assert!(t(VfsError::Other("HTTP 503 Service Unavailable".into())));
        assert!(t(VfsError::Other(
            "SlowDown: please reduce your request rate (HTTP 429)".into()
        )));
        assert!(t(VfsError::Other(
            "TLS: peer closed connection without sending close_notify".into()
        )));
        assert!(t(VfsError::Io(std::io::Error::other(
            VfsError::Connection("x".into())
        ))));
        assert!(t(VfsError::Io(std::io::Error::other(
            "connection error: reset by peer"
        ))));

        assert!(!t(VfsError::PermissionDenied("/x".into())));
        assert!(!t(VfsError::NotFound("/x".into())));
        assert!(!t(VfsError::Auth("bad password".into())));
        assert!(!t(VfsError::Io(std::io::Error::from(
            ErrorKind::StorageFull
        ))));
        assert!(!t(VfsError::Io(std::io::Error::from(
            ErrorKind::PermissionDenied
        ))));
        assert!(!t(VfsError::Other("550 Permission denied".into())));
        assert!(!t(VfsError::Other("HTTP 403 Forbidden".into())));
        assert!(!t(VfsError::Other("No space left on device".into())));
        assert!(!t(VfsError::Cancelled));
        assert_eq!(
            VfsError::Connection("x".into()).class(),
            ErrorClass::Transient
        );
    }

    struct Bytes(Vec<u8>);
    impl Vfs for Bytes {
        fn label(&self) -> String {
            "b".into()
        }
        fn protocol(&self) -> Protocol {
            Protocol::Local
        }
        fn home(&self) -> VfsResult<VfsPath> {
            Ok(VfsPath::root())
        }
        fn list(&self, _: &VfsPath) -> VfsResult<Vec<Entry>> {
            Ok(Vec::new())
        }
        fn stat(&self, p: &VfsPath) -> VfsResult<Entry> {
            Err(VfsError::NotFound(p.to_string()))
        }
        fn create_dir(&self, _: &VfsPath) -> VfsResult<()> {
            Ok(())
        }
        fn remove_file(&self, _: &VfsPath) -> VfsResult<()> {
            Ok(())
        }
        fn remove_dir(&self, _: &VfsPath) -> VfsResult<()> {
            Ok(())
        }
        fn rename(&self, _: &VfsPath, _: &VfsPath) -> VfsResult<()> {
            Ok(())
        }
        fn open_read(&self, _: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
            Ok(Box::new(Cursor::new(self.0.clone())))
        }
        fn open_write(&self, _: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
            Err(VfsError::Unsupported("w".into()))
        }
    }

    #[test]
    fn auth_debug_hides_secrets() {
        let s = format!("{:?}", Auth::Password("hunter2".into()));
        assert!(!s.contains("hunter2"), "{s}");
        let s = format!(
            "{:?}",
            Auth::KeyFile {
                path: "~/.ssh/id".into(),
                passphrase: Some("open sesame".into())
            }
        );
        assert!(!s.contains("open sesame") && s.contains("~/.ssh/id"), "{s}");
    }

    #[test]
    fn default_read_at_skips() {
        let fs = Bytes((0u8..100).collect());
        let mut out = Vec::new();
        fs.open_read_at(&VfsPath::root(), 40)
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, (40u8..100).collect::<Vec<_>>());
        assert!(fs.open_read_at(&VfsPath::root(), 101).is_err());
        assert!(matches!(
            fs.open_append(&VfsPath::root()),
            Err(VfsError::Unsupported(_))
        ));
        assert_eq!(fs.supports_resume(), ResumeCaps::default());
        assert!(fs.reconnect().is_ok());
    }
}
