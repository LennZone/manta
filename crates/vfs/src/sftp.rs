//! SFTP backend on top of `ssh2` (libssh2).
//!
//! `ssh2::Session`, `ssh2::Sftp` and `ssh2::File` are `Send + Sync` and
//! `'static`: every call locks the session internally, so a single session
//! can be shared between the GUI and worker threads, and open files can be
//! moved to other threads freely.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};
use manta_core::{
    Auth, ConnectionConfig, Entry, EntryKind, Protocol, Vfs, VfsError, VfsPath, VfsResult,
};
use ssh2::{ErrorCode, FileStat, FileType, Session, Sftp};

use crate::{effective_port, tcp_connect, unix_to_utc};

/// Timeout (ms) for individual blocking libssh2 calls.
const SESSION_TIMEOUT_MS: u32 = 60_000;

// SFTP status codes (draft-ietf-secsh-filexfer).
const FX_EOF: i32 = 1;
const FX_NO_SUCH_FILE: i32 = 2;
const FX_PERMISSION_DENIED: i32 = 3;
const FX_FAILURE: i32 = 4;
const FX_NO_CONNECTION: i32 = 6;
const FX_CONNECTION_LOST: i32 = 7;
const FX_OP_UNSUPPORTED: i32 = 8;
const FX_NO_SUCH_PATH: i32 = 10;
const FX_FILE_ALREADY_EXISTS: i32 = 11;
const FX_WRITE_PROTECT: i32 = 12;
const FX_DIR_NOT_EMPTY: i32 = 18;
const FX_NOT_A_DIRECTORY: i32 = 19;

/// An SFTP connection.
pub struct SftpFs {
    label: String,
    /// The live session; replaced by [`Vfs::reconnect`] after a network drop.
    conn: RwLock<Arc<Conn>>,
    /// Kept to log in again on `reconnect` (in memory only).
    cfg: ConnectionConfig,
    /// Cached login directory.
    home: Mutex<Option<VfsPath>>,
}

/// One SSH session with its SFTP channel. Open files keep it alive.
struct Conn {
    session: Session,
    sftp: Arc<Sftp>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        let _ = self.session.disconnect(None, "bye", None);
    }
}

/// Connect, check the host key, authenticate and open the SFTP channel.
fn open_conn(cfg: &ConnectionConfig) -> VfsResult<Conn> {
    let port = effective_port(cfg);
    let tcp = tcp_connect(&cfg.host, port)?;
    let mut session = Session::new()
        .map_err(|e| VfsError::Connection(format!("cannot create SSH session: {e}")))?;
    session.set_tcp_stream(tcp);
    session.set_timeout(SESSION_TIMEOUT_MS);
    session
        .handshake()
        .map_err(|e| VfsError::Connection(format!("SSH handshake failed: {e}")))?;
    check_host_key(&session, &cfg.host, port)?;
    // Keep NAT/firewall state alive on idle connections.
    session.set_keepalive(true, 30);

    authenticate(&session, cfg)?;
    if !session.authenticated() {
        return Err(VfsError::Auth(format!(
            "authentication failed for {}",
            cfg.username
        )));
    }

    let sftp = session
        .sftp()
        .map_err(|e| VfsError::Connection(format!("cannot start SFTP subsystem: {e}")))?;
    Ok(Conn {
        session,
        sftp: Arc::new(sftp),
    })
}

/// Timeout (ms) for the liveness probe in `reconnect`.
const PROBE_TIMEOUT_MS: u32 = 5_000;

/// Map an ssh2 error that occurred while operating on `path`.
pub(crate) fn map_ssh(err: ssh2::Error, path: &VfsPath) -> VfsError {
    let p = path.to_string();
    match err.code() {
        ErrorCode::SFTP(code) => match code {
            FX_NO_SUCH_FILE | FX_NO_SUCH_PATH => VfsError::NotFound(p),
            FX_PERMISSION_DENIED | FX_WRITE_PROTECT => VfsError::PermissionDenied(p),
            FX_FILE_ALREADY_EXISTS => VfsError::AlreadyExists(p),
            FX_DIR_NOT_EMPTY => VfsError::DirectoryNotEmpty(p),
            FX_NOT_A_DIRECTORY => VfsError::NotADirectory(p),
            FX_NO_CONNECTION | FX_CONNECTION_LOST => {
                VfsError::Connection(format!("{p}: {}", err.message()))
            }
            FX_OP_UNSUPPORTED => VfsError::Unsupported(format!("{p}: {}", err.message())),
            FX_FAILURE => VfsError::Other(format!("operation failed: {p}")),
            FX_EOF => VfsError::Other(format!("unexpected end of file: {p}")),
            _ => VfsError::Other(format!("{p}: {err}")),
        },
        ErrorCode::Session(_) => VfsError::Connection(format!("{p}: {}", err.message())),
    }
}

fn ssh_err_to_io(err: ssh2::Error) -> io::Error {
    io::Error::other(err)
}

fn kind_of(st: &FileStat) -> EntryKind {
    match st.file_type() {
        FileType::Directory => EntryKind::Directory,
        FileType::RegularFile => EntryKind::File,
        FileType::Symlink => EntryKind::Symlink,
        _ => EntryKind::Other,
    }
}

impl SftpFs {
    /// Connect and authenticate. Blocking.
    pub fn connect(cfg: &ConnectionConfig) -> VfsResult<SftpFs> {
        let conn = open_conn(cfg)?;
        let label = if cfg.username.is_empty() {
            format!("{} (SFTP)", cfg.host)
        } else {
            format!("{}@{} (SFTP)", cfg.username, cfg.host)
        };
        Ok(SftpFs {
            label,
            conn: RwLock::new(Arc::new(conn)),
            cfg: cfg.clone(),
            home: Mutex::new(None),
        })
    }

    fn conn(&self) -> Arc<Conn> {
        self.conn.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn sftp(&self) -> Arc<Sftp> {
        self.conn().sftp.clone()
    }

    fn p(path: &VfsPath) -> &Path {
        Path::new(path.as_str())
    }

    /// Build an entry from `lstat` data, resolving symlinks.
    fn make_entry(&self, path: &VfsPath, name: String, lst: &FileStat) -> Entry {
        let mut kind = kind_of(lst);
        let mut link_target = None;
        let mut attrs = lst.clone();
        if kind == EntryKind::Symlink {
            link_target = self
                .sftp()
                .readlink(Self::p(path))
                .ok()
                .map(|t| t.to_string_lossy().into_owned());
            if let Ok(target) = self.sftp().stat(Self::p(path)) {
                if target.is_dir() {
                    kind = EntryKind::Directory;
                }
                attrs = target;
            }
        }
        Entry {
            hidden: name.starts_with('.'),
            name,
            path: path.clone(),
            size: if kind == EntryKind::Directory {
                0
            } else {
                attrs.size.unwrap_or(0)
            },
            modified: attrs
                .mtime
                .and_then(|t| i64::try_from(t).ok())
                .and_then(unix_to_utc),
            permissions: attrs.perm.map(|m| m & 0o7777),
            link_target,
            kind,
        }
    }
}

/// Host key check against `known_hosts` (see [`crate::known_hosts`]):
/// unknown and changed keys are errors the app handles.
fn check_host_key(session: &Session, host: &str, port: u16) -> VfsResult<()> {
    let Some((key, _)) = session.host_key() else {
        return Err(VfsError::Connection(
            "server did not provide a host key".into(),
        ));
    };
    crate::known_hosts::verify(host, port, key)
}

fn authenticate(session: &Session, cfg: &ConnectionConfig) -> VfsResult<()> {
    let user = cfg.username.as_str();
    let auth_err = |what: &str, e: ssh2::Error| VfsError::Auth(format!("{what}: {}", e.message()));
    match &cfg.auth {
        Auth::Password(pw) => session
            .userauth_password(user, pw)
            .map_err(|e| auth_err("password authentication failed", e)),
        Auth::KeyFile { path, passphrase } => {
            let key = expand_tilde(path);
            if !key.exists() {
                return Err(VfsError::Auth(format!(
                    "key file not found: {}",
                    key.display()
                )));
            }
            // Use a matching .pub file if one exists; libssh2 can derive it otherwise.
            let mut pubkey = key.clone().into_os_string();
            pubkey.push(".pub");
            let pubkey = PathBuf::from(pubkey);
            let pubkey = pubkey.exists().then_some(pubkey);
            session
                .userauth_pubkey_file(user, pubkey.as_deref(), &key, passphrase.as_deref())
                .map_err(|e| auth_err("public key authentication failed", e))
        }
        Auth::Agent | Auth::None => agent_auth(session, user),
    }
}

fn agent_auth(session: &Session, user: &str) -> VfsResult<()> {
    let mut agent = session
        .agent()
        .map_err(|e| VfsError::Auth(format!("ssh-agent unavailable: {e}")))?;
    agent
        .connect()
        .map_err(|e| VfsError::Auth(format!("cannot connect to ssh-agent: {}", e.message())))?;
    agent
        .list_identities()
        .map_err(|e| VfsError::Auth(format!("ssh-agent: {}", e.message())))?;
    let identities = agent
        .identities()
        .map_err(|e| VfsError::Auth(format!("ssh-agent: {}", e.message())))?;
    if identities.is_empty() {
        return Err(VfsError::Auth("ssh-agent has no identities".into()));
    }
    for id in &identities {
        if agent.userauth(user, id).is_ok() && session.authenticated() {
            let _ = agent.disconnect();
            return Ok(());
        }
    }
    let _ = agent.disconnect();
    Err(VfsError::Auth("no ssh-agent identity was accepted".into()))
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

impl Vfs for SftpFs {
    /// `fstatvfs@openssh.com` on the folder, opened like a file (a
    /// directory handle has no descriptor to ask). Servers without the
    /// extension: unknown.
    fn space(&self, path: &VfsPath) -> VfsResult<Option<manta_core::Space>> {
        let Ok(mut dir) = self.sftp().open(Self::p(path)) else {
            return Ok(None);
        };
        let Ok(st) = dir.statvfs() else {
            return Ok(None);
        };
        let unit = if st.f_frsize > 0 {
            st.f_frsize
        } else {
            st.f_bsize
        };
        Ok(Some(manta_core::Space {
            available: st.f_bavail.saturating_mul(unit),
            total: Some(st.f_blocks.saturating_mul(unit)).filter(|t| *t > 0),
        }))
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn protocol(&self) -> Protocol {
        Protocol::Sftp
    }

    fn home(&self) -> VfsResult<VfsPath> {
        let mut cached = self.home.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(h) = cached.as_ref() {
            return Ok(h.clone());
        }
        let real = self
            .sftp()
            .realpath(Path::new("."))
            .map_err(|e| map_ssh(e, &VfsPath::new(".")))?;
        let h = VfsPath::new(&real.to_string_lossy());
        *cached = Some(h.clone());
        Ok(h)
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let items = self.sftp().readdir(Self::p(dir)).map_err(|e| {
            let mapped = map_ssh(e, dir);
            // OpenSSH reports opendir on a file as a generic failure.
            if matches!(mapped, VfsError::Other(_)) {
                if let Ok(st) = self.sftp().stat(Self::p(dir)) {
                    if !st.is_dir() {
                        return VfsError::NotADirectory(dir.to_string());
                    }
                }
            }
            mapped
        })?;
        Ok(items
            .into_iter()
            .map(|(p, st)| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let path = dir.join(&name);
                self.make_entry(&path, name, &st)
            })
            .filter(|e| !e.name.is_empty())
            .collect())
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let st = self
            .sftp()
            .lstat(Self::p(path))
            .map_err(|e| map_ssh(e, path))?;
        let name = if path.is_root() {
            "/".to_string()
        } else {
            path.file_name().to_string()
        };
        Ok(self.make_entry(path, name, &st))
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.sftp().mkdir(Self::p(path), 0o755).map_err(|e| {
            let mapped = map_ssh(e, path);
            // OpenSSH answers "failure" when the directory already exists.
            if matches!(mapped, VfsError::Other(_)) && self.sftp().lstat(Self::p(path)).is_ok() {
                VfsError::AlreadyExists(path.to_string())
            } else {
                mapped
            }
        })
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.sftp().unlink(Self::p(path)).map_err(|e| {
            let mapped = map_ssh(e, path);
            if matches!(mapped, VfsError::Other(_)) {
                if let Ok(st) = self.sftp().lstat(Self::p(path)) {
                    if st.is_dir() {
                        return VfsError::Other(format!("is a directory: {path}"));
                    }
                }
            }
            mapped
        })
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.sftp().rmdir(Self::p(path)).map_err(|e| {
            let mapped = map_ssh(e, path);
            // OpenSSH answers "failure" for non-empty or non-directory paths.
            if matches!(mapped, VfsError::Other(_)) {
                match self.sftp().lstat(Self::p(path)) {
                    Ok(st) if !st.is_dir() => return VfsError::NotADirectory(path.to_string()),
                    Ok(_) => {
                        if self
                            .sftp()
                            .readdir(Self::p(path))
                            .is_ok_and(|v| !v.is_empty())
                        {
                            return VfsError::DirectoryNotEmpty(path.to_string());
                        }
                    }
                    Err(_) => {}
                }
            }
            mapped
        })
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.sftp()
            .rename(Self::p(from), Self::p(to), None)
            .map_err(|e| {
                let mapped = map_ssh(e, from);
                if matches!(mapped, VfsError::Other(_)) && self.sftp().lstat(Self::p(to)).is_ok() {
                    VfsError::AlreadyExists(to.to_string())
                } else {
                    mapped
                }
            })
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let st = self
            .sftp()
            .stat(Self::p(path))
            .map_err(|e| map_ssh(e, path))?;
        if st.is_dir() {
            return Err(VfsError::Other(format!("is a directory: {path}")));
        }
        let f = self
            .sftp()
            .open(Self::p(path))
            .map_err(|e| map_ssh(e, path))?;
        Ok(Box::new(f))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let f = self
            .sftp()
            .create(Self::p(path))
            .map_err(|e| map_ssh(e, path))?;
        Ok(Box::new(SftpWriter {
            sftp: self.sftp(),
            path: PathBuf::from(path.as_str()),
            file: Some(f),
            offset: 0,
        }))
    }

    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        use std::io::{Seek, SeekFrom};
        let sftp = self.sftp();
        let st = sftp.stat(Self::p(path)).map_err(|e| map_ssh(e, path))?;
        if st.is_dir() {
            return Err(VfsError::Other(format!("is a directory: {path}")));
        }
        let size = st.size.unwrap_or(0);
        if offset > size {
            return Err(VfsError::Other(format!(
                "{path}: file is shorter ({size} bytes) than the resume offset {offset}"
            )));
        }
        let mut f = sftp.open(Self::p(path)).map_err(|e| map_ssh(e, path))?;
        f.seek(SeekFrom::Start(offset))?;
        Ok(Box::new(f))
    }

    /// Opens with WRITE|CREAT (no TRUNC) and writes at the current size:
    /// OpenSSH ignores `SSH_FXF_APPEND` for plain `write` requests, an
    /// explicit offset works everywhere.
    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let sftp = self.sftp();
        let size = match sftp.stat(Self::p(path)) {
            Ok(st) if st.is_dir() => {
                return Err(VfsError::Other(format!("is a directory: {path}")))
            }
            Ok(st) => st.size.unwrap_or(0),
            Err(e) => match map_ssh(e, path) {
                VfsError::NotFound(_) => 0,
                other => return Err(other),
            },
        };
        let mut f = sftp
            .open_mode(
                Self::p(path),
                ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE,
                0o644,
                ssh2::OpenType::File,
            )
            .map_err(|e| map_ssh(e, path))?;
        {
            use std::io::{Seek, SeekFrom};
            f.seek(SeekFrom::Start(size))?;
        }
        Ok(Box::new(SftpWriter {
            sftp,
            path: PathBuf::from(path.as_str()),
            file: Some(f),
            offset: size,
        }))
    }

    fn supports_resume(&self) -> manta_core::ResumeCaps {
        manta_core::ResumeCaps {
            read_at: true,
            append: true,
            upload_session: false,
        }
    }

    /// Probes the session (short timeout); if it's dead, logs in again and
    /// swaps in the new session. Readers / writers of the old session fail
    /// and are reopened by the caller.
    fn reconnect(&self) -> VfsResult<()> {
        let old = self.conn();
        old.session.set_timeout(PROBE_TIMEOUT_MS);
        let alive =
            old.sftp.stat(Path::new("/")).is_ok() || old.sftp.realpath(Path::new(".")).is_ok();
        old.session.set_timeout(SESSION_TIMEOUT_MS);
        if alive {
            return Ok(());
        }
        log::info!("{}: SFTP session lost; reconnecting", self.label);
        let fresh = open_conn(&self.cfg)?;
        *self.conn.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(fresh);
        Ok(())
    }

    fn set_permissions(&self, path: &VfsPath, mode: u32) -> VfsResult<()> {
        let stat = FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: Some(mode & 0o7777),
            atime: None,
            mtime: None,
        };
        self.sftp()
            .setstat(Self::p(path), stat)
            .map_err(|e| map_ssh(e, path))
    }

    fn set_modified(&self, path: &VfsPath, mtime: DateTime<Utc>) -> VfsResult<()> {
        // libssh2 always sends atime and mtime together.
        let secs = mtime.timestamp().max(0) as u64;
        let stat = FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: None,
            atime: Some(secs),
            mtime: Some(secs),
        };
        self.sftp()
            .setstat(Self::p(path), stat)
            .map_err(|e| map_ssh(e, path))
    }
}

/// Upload handle. `flush()` closes the remote handle so that errors are
/// reported; writing again afterwards transparently reopens the file and
/// continues at the current offset. Dropping closes best-effort.
struct SftpWriter {
    sftp: Arc<Sftp>,
    path: PathBuf,
    file: Option<ssh2::File>,
    offset: u64,
}

impl Write for SftpWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.file.is_none() {
            use std::io::{Seek, SeekFrom};
            let mut f = self
                .sftp
                .open_mode(
                    &self.path,
                    ssh2::OpenFlags::WRITE,
                    0o644,
                    ssh2::OpenType::File,
                )
                .map_err(ssh_err_to_io)?;
            f.seek(SeekFrom::Start(self.offset))?;
            self.file = Some(f);
        }
        let f = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("SFTP upload handle missing"))?;
        let n = f.write(buf)?;
        self.offset += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(mut f) = self.file.take() {
            f.close().map_err(ssh_err_to_io)?;
        }
        Ok(())
    }
}
