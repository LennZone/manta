//! FTP / FTPS (explicit, `AUTH TLS`) backend on top of `suppaftp` (sync API).
//!
//! FTP allows only one command (and one data transfer) at a time per control
//! connection, so this backend keeps a small pool of logged-in connections.
//! Every operation checks one out exclusively; readers and writers returned by
//! `open_read`/`open_write` hold their connection until the transfer has been
//! finalized. This keeps the GUI responsive during transfers and lets a copy
//! within the same server (download + upload at once) work.
//!
//! Network robustness (see `docs/updates.md` and the README):
//! - FTPS uses our own rustls connector ([`tls`]): data connections resume
//!   the control connection's TLS session (servers that require session
//!   reuse), and the handshake runs eagerly with a timeout.
//! - Data channel policy ([`FtpDataChannel`]): `Auto` falls back to clear
//!   data connections (`PROT C`, login stays encrypted) when the network
//!   resets encrypted ones, and reports [`ConnectionNotice::ClearDataChannel`];
//!   `Require` never does; `Off` starts with clear data connections.
//! - Passive mode always connects to the control connection's host (the
//!   address in a `PASV` reply is ignored: NAT, private or bogus addresses),
//!   and falls back between `PASV` and `EPSV` when the server refuses one.

#[cfg(test)]
mod fake_server;
#[cfg(test)]
mod resume_tests;
mod tls;
#[cfg(test)]
mod transport_tests;

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDateTime, Utc};
use manta_core::{
    Auth, ConnectionConfig, ConnectionNotice, Entry, EntryKind, FtpDataChannel, Protocol,
    SharedVfs, Vfs, VfsError, VfsPath, VfsResult,
};
use suppaftp::list::{ListParser, PosixPexQuery};
use suppaftp::types::FileType as TransferType;
use suppaftp::{FtpError, ImplFtpStream, Mode, Status, TlsStream, TransferStream};

use self::tls::DataSwitch;

use crate::{effective_port, tcp_connect, CONNECT_TIMEOUT};

/// Maximum number of simultaneous control connections per server.
const MAX_CONNECTIONS: usize = 4;
/// Idle connections older than this are probed with NOOP before reuse.
const PROBE_AFTER: Duration = Duration::from_secs(20);
/// How long to wait for a free pooled connection.
const CHECKOUT_TIMEOUT: Duration = Duration::from_secs(300);
/// Read/write timeout on control and data sockets.
const IO_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// The error (after `connection error: `) when the data channel policy is
/// `Require` and the network blocks encrypted data connections. The app
/// translates this exact text.
pub const DATA_TLS_BLOCKED: &str = "the network blocks encrypted FTP data connections (the TLS \
     handshake was reset or timed out). Set the data channel to Automatic to continue with \
     clear data connections; the login stays encrypted";

/// Map an FTP error that occurred while operating on `path`.
pub(crate) fn map_ftp(err: FtpError, path: &VfsPath) -> VfsError {
    let p = path.to_string();
    match err {
        FtpError::ConnectionError(e) => VfsError::Connection(format!("{p}: {e}")),
        FtpError::SecureError(e) => {
            // suppaftp nests its "Secure error: " prefix when it wraps ours.
            let mut msg = e.as_str();
            while let Some(rest) = msg.strip_prefix("Secure error: ") {
                msg = rest;
            }
            VfsError::Connection(format!("TLS error: {msg}"))
        }
        FtpError::InvalidAddress(e) => VfsError::Connection(format!("invalid address: {e}")),
        FtpError::BadResponse => VfsError::Other(format!("{p}: malformed server response")),
        FtpError::DataConnectionAlreadyOpen => {
            VfsError::Other(format!("{p}: a data transfer is already in progress"))
        }
        FtpError::UnexpectedResponse(resp) => {
            let code = resp.status.code();
            let msg = resp.as_string().unwrap_or_default();
            map_reply(code, &msg, p)
        }
    }
}

fn map_reply(code: u32, msg: &str, p: String) -> VfsError {
    let lower = msg.to_lowercase();
    let detail = || format!("{p}: {msg}");
    match code {
        421 | 425 | 426 => VfsError::Connection(detail()),
        530 | 532 => VfsError::Auth(detail()),
        500 | 502 | 504 | 202 => VfsError::Unsupported(detail()),
        450 | 550 | 551 | 553 | 552 | 451 | 452 => {
            if [
                "no such file",
                "not found",
                "does not exist",
                "doesn't exist",
                "cannot find",
                "can't find",
                "can't check for file existence",
            ]
            .iter()
            .any(|s| lower.contains(s))
            {
                VfsError::NotFound(p)
            } else if [
                "permission denied",
                "access denied",
                "not permitted",
                "access is denied",
            ]
            .iter()
            .any(|s| lower.contains(s))
            {
                VfsError::PermissionDenied(p)
            } else if lower.contains("not empty") {
                VfsError::DirectoryNotEmpty(p)
            } else if lower.contains("not a directory") {
                VfsError::NotADirectory(p)
            } else if lower.contains("exists") {
                VfsError::AlreadyExists(p)
            } else {
                VfsError::Other(detail())
            }
        }
        _ => VfsError::Other(format!("{p}: [{code}] {msg}")),
    }
}

/// Whether the error means the control connection is unusable.
fn is_fatal(err: &FtpError) -> bool {
    match err {
        FtpError::ConnectionError(_) | FtpError::SecureError(_) | FtpError::BadResponse => true,
        FtpError::UnexpectedResponse(r) => r.status.code() == 421,
        _ => false,
    }
}

fn to_io(err: FtpError, path: &VfsPath) -> io::Error {
    io::Error::other(map_ftp(err, path).to_string())
}

// ---------------------------------------------------------------------------
// Connection pool
// ---------------------------------------------------------------------------

type Factory<T> = dyn Fn() -> VfsResult<Conn<T>> + Send + Sync;

/// A logged-in control connection.
struct Conn<T: TlsStream> {
    ftp: ImplFtpStream<T>,
    /// FTPS: whether this connection's data connections are clear.
    switch: Option<Arc<DataSwitch>>,
}

struct PoolState<T: TlsStream> {
    idle: Vec<(Conn<T>, Instant)>,
    /// Connections in existence (idle + checked out + being opened).
    total: usize,
}

struct Pool<T: TlsStream> {
    state: Mutex<PoolState<T>>,
    cv: Condvar,
    factory: Box<Factory<T>>,
}

impl<T: TlsStream> Pool<T> {
    fn lock(&self) -> MutexGuard<'_, PoolState<T>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn checkout(self: &Arc<Self>) -> VfsResult<Lease<T>> {
        let deadline = Instant::now() + CHECKOUT_TIMEOUT;
        let mut st = self.lock();
        loop {
            if let Some((mut conn, last_used)) = st.idle.pop() {
                if last_used.elapsed() < PROBE_AFTER {
                    return Ok(Lease::new(conn, self));
                }
                drop(st);
                if conn.ftp.noop().is_ok() {
                    return Ok(Lease::new(conn, self));
                }
                log::debug!("discarding stale FTP connection");
                st = self.lock();
                st.total -= 1;
                continue;
            }
            if st.total < MAX_CONNECTIONS {
                st.total += 1;
                drop(st);
                return match (self.factory)() {
                    Ok(conn) => Ok(Lease::new(conn, self)),
                    Err(e) => {
                        self.lock().total -= 1;
                        self.cv.notify_one();
                        Err(e)
                    }
                };
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(VfsError::Connection(
                    "timed out waiting for a free FTP connection".into(),
                ));
            }
            st = self
                .cv
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

/// Exclusive use of one pooled connection; returned to the pool on drop.
struct Lease<T: TlsStream> {
    conn: Option<Conn<T>>,
    pool: Arc<Pool<T>>,
    /// Set when the connection must not be reused.
    broken: bool,
    /// A data connection's TLS handshake failed (reset, timeout, refused).
    /// Only data connections can fail like this once the control
    /// connection is up, see [`tls`].
    data_tls_failed: bool,
}

impl<T: TlsStream> Lease<T> {
    fn new(conn: Conn<T>, pool: &Arc<Pool<T>>) -> Self {
        Lease {
            conn: Some(conn),
            pool: Arc::clone(pool),
            broken: false,
            data_tls_failed: false,
        }
    }

    fn slot(&mut self) -> &mut Conn<T> {
        self.conn
            .as_mut()
            .expect("lease holds a connection until dropped")
    }

    fn conn(&mut self) -> &mut ImplFtpStream<T> {
        &mut self.slot().ftp
    }

    /// Run a command; marks the connection broken on fatal errors.
    fn run<R>(
        &mut self,
        f: impl FnOnce(&mut ImplFtpStream<T>) -> Result<R, FtpError>,
    ) -> Result<R, FtpError> {
        let res = f(self.conn());
        if let Err(e) = &res {
            if is_fatal(e) {
                self.broken = true;
            }
            if matches!(e, FtpError::SecureError(_)) {
                self.data_tls_failed = true;
            }
        }
        res
    }
}

impl<T: TlsStream> Drop for Lease<T> {
    fn drop(&mut self) {
        let Some(conn) = self.conn.take() else { return };
        let mut st = self.pool.lock();
        if self.broken {
            st.total -= 1;
            drop(st);
            drop(conn);
        } else {
            st.idle.push((conn, Instant::now()));
        }
        self.pool.cv.notify_one();
    }
}

// ---------------------------------------------------------------------------
// Listing parsers
// ---------------------------------------------------------------------------

/// Parse one MLSD/MLST entry line. Returns `None` for `.`/`..` (cdir/pdir)
/// and unparsable lines. `name_override` replaces the line's pathname (MLST
/// returns full paths).
fn parse_mlsx(line: &str, dir: &VfsPath, name_override: Option<&str>) -> Option<Entry> {
    // `facts SP name`; MLST reply lines may carry a leading space, and an
    // entry without facts is just `SP name`.
    let (facts, name) = match line.find("; ") {
        Some(end) => (line[..=end].trim_start(), &line[end + 2..]),
        None => ("", line.strip_prefix(' ')?),
    };
    let mut kind = EntryKind::File;
    let mut size = 0u64;
    let mut modified = None;
    let mut permissions = None;
    let mut link_target = None;
    for fact in facts.split(';') {
        let Some((key, value)) = fact.split_once('=') else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "type" => {
                let v = value.to_ascii_lowercase();
                kind = match v.as_str() {
                    "cdir" | "pdir" => return None,
                    "dir" => EntryKind::Directory,
                    "file" => EntryKind::File,
                    _ if v.contains("link") => {
                        // e.g. "OS.unix=slink:/target" or "OS.unix=symlink"
                        if let Some((_, t)) = value.split_once(':') {
                            if !t.is_empty() {
                                link_target = Some(t.to_string());
                            }
                        }
                        EntryKind::Symlink
                    }
                    _ => EntryKind::Other,
                };
            }
            "size" | "sizd" => size = value.parse().unwrap_or(0),
            "modify" => {
                let base = value.split('.').next().unwrap_or(value);
                modified = NaiveDateTime::parse_from_str(base, "%Y%m%d%H%M%S")
                    .ok()
                    .map(|t| t.and_utc());
            }
            "unix.mode" => permissions = u32::from_str_radix(value, 8).ok().map(|m| m & 0o7777),
            _ => {}
        }
    }
    let name = match name_override {
        Some(n) => n.to_string(),
        None => name.rsplit('/').next().unwrap_or(name).to_string(),
    };
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(Entry {
        path: dir.join(&name),
        hidden: name.starts_with('.'),
        size: if kind == EntryKind::Directory {
            0
        } else {
            size
        },
        name,
        kind,
        modified,
        permissions,
        link_target,
    })
}

/// Parse one `LIST` line (Unix or DOS style).
fn parse_list_line(line: &str, dir: &VfsPath) -> Option<Entry> {
    let (file, posix) = match ListParser::parse_posix(line) {
        Ok(f) => (f, true),
        Err(_) => (ListParser::parse_dos(line).ok()?, false),
    };
    let name = file.name().to_string();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let kind = if file.is_directory() {
        EntryKind::Directory
    } else if file.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    let permissions = posix.then(|| {
        let bits = |who: PosixPexQuery| {
            (u32::from(file.can_read(who)) << 2)
                | (u32::from(file.can_write(who)) << 1)
                | u32::from(file.can_execute(who))
        };
        (bits(PosixPexQuery::Owner) << 6)
            | (bits(PosixPexQuery::Group) << 3)
            | bits(PosixPexQuery::Others)
    });
    Some(Entry {
        path: dir.join(&name),
        hidden: name.starts_with('.'),
        size: if kind == EntryKind::Directory {
            0
        } else {
            file.size() as u64
        },
        modified: Some(DateTime::<Utc>::from(file.modified())),
        permissions,
        link_target: file.symlink().map(|p| p.to_string_lossy().into_owned()),
        name,
        kind,
    })
}

// ---------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------

/// The data channel policy of one FTPS connection (shared by its pool).
#[derive(Debug)]
pub(crate) struct DataPolicy {
    policy: FtpDataChannel,
    /// Data connections are clear (policy `Off`, or `Auto` after a fallback).
    clear: AtomicBool,
}

impl DataPolicy {
    pub(crate) fn new(policy: FtpDataChannel) -> Self {
        DataPolicy {
            policy,
            clear: AtomicBool::new(policy == FtpDataChannel::Off),
        }
    }

    /// Whether new data connections should be clear (`PROT C`).
    pub(crate) fn wants_clear(&self) -> bool {
        self.clear.load(Ordering::Acquire)
    }

    /// An encrypted data connection failed during the TLS handshake.
    /// Returns whether to retry the operation with a clear data channel.
    pub(crate) fn on_tls_failure(&self) -> bool {
        match self.policy {
            FtpDataChannel::Auto => {
                if !self.clear.swap(true, Ordering::AcqRel) {
                    log::warn!(
                        "FTPS: the network resets encrypted data connections; continuing with \
                         clear data connections (the login stays encrypted)"
                    );
                }
                true
            }
            FtpDataChannel::Require | FtpDataChannel::Off => false,
        }
    }

    /// What the pane should tell the user.
    pub(crate) fn notice(&self) -> Option<ConnectionNotice> {
        (self.policy == FtpDataChannel::Auto && self.wants_clear())
            .then_some(ConnectionNotice::ClearDataChannel)
    }

    /// The error for a data connection whose TLS handshake failed when no
    /// fallback is allowed.
    pub(crate) fn tls_failure_error(&self, err: VfsError) -> VfsError {
        match self.policy {
            FtpDataChannel::Require => {
                log::warn!("FTPS data connection failed: {err}");
                VfsError::Connection(DATA_TLS_BLOCKED.to_string())
            }
            _ => err,
        }
    }
}

/// Passive mode flavours, stored in an `AtomicU8`.
const PASSIVE: u8 = 0;
const EXTENDED_PASSIVE: u8 = 1;

fn mode_of(code: u8) -> Mode {
    if code == EXTENDED_PASSIVE {
        Mode::ExtendedPassive
    } else {
        Mode::Passive
    }
}

/// Passive modes to try, best first: `EPSV` for IPv6 (`PASV` can't express
/// IPv6 addresses), else `PASV` (some firewalls and old servers mishandle
/// `EPSV`; its address is replaced by the control host anyway).
fn passive_order(ipv6: bool) -> [u8; 2] {
    if ipv6 {
        [EXTENDED_PASSIVE, PASSIVE]
    } else {
        [PASSIVE, EXTENDED_PASSIVE]
    }
}

/// Outcome of asking the server for a passive port.
#[derive(Debug, PartialEq, Eq)]
enum Probe {
    /// The server answered with a usable port.
    Works,
    /// The server refused the command (5xx): try the other one.
    Refused,
    /// Anything else (odd reply, connection trouble): the control
    /// connection may be out of step; don't reuse it.
    Broken,
}

fn classify_probe(res: &Result<suppaftp::types::Response, FtpError>, extended: bool) -> Probe {
    match res {
        Ok(r) => {
            let body = r.as_string().unwrap_or_default();
            let ok = if extended {
                body.contains("|||") || body.contains('(')
            } else {
                body.matches(',').count() >= 5
            };
            if ok {
                Probe::Works
            } else {
                Probe::Broken
            }
        }
        Err(FtpError::UnexpectedResponse(r)) if (500..600).contains(&r.status.code()) => {
            Probe::Refused
        }
        Err(_) => Probe::Broken,
    }
}

/// Pick the passive mode the server accepts. Returns the mode and a usable
/// connection (a fresh one if probing left `conn` in an unknown state).
fn probe_passive<T: TlsStream>(
    mut conn: Conn<T>,
    ipv6: bool,
    factory: &Factory<T>,
) -> VfsResult<(u8, Conn<T>)> {
    let order = passive_order(ipv6);
    for mode in order {
        let extended = mode == EXTENDED_PASSIVE;
        let res = if extended {
            conn.ftp
                .custom_command("EPSV", &[Status::ExtendedPassiveMode])
        } else {
            conn.ftp.custom_command("PASV", &[Status::PassiveMode])
        };
        match classify_probe(&res, extended) {
            Probe::Works => return Ok((mode, conn)),
            Probe::Refused => {
                log::info!(
                    "FTP server refuses {}",
                    if extended { "EPSV" } else { "PASV" }
                );
            }
            Probe::Broken => {
                log::info!("unexpected passive mode reply ({res:?}); reconnecting");
                drop(conn);
                return Ok((order[0], factory()?));
            }
        }
    }
    Ok((order[0], conn))
}

/// An FTP or FTPS connection (pool). `T` is the TLS stream type of suppaftp.
pub struct FtpFs<T: TlsStream> {
    label: String,
    protocol: Protocol,
    pool: Arc<Pool<T>>,
    home: VfsPath,
    /// Server supports MLSD/MLST (RFC 3659). Cleared if it turns out not to.
    mlsx: AtomicBool,
    /// FTPS data channel policy (`None` for plain FTP).
    data: Option<Arc<DataPolicy>>,
    /// `PASSIVE` or `EXTENDED_PASSIVE`.
    passive: AtomicU8,
}

/// Connect to a plain FTP server.
pub(crate) fn connect_plain(cfg: &ConnectionConfig) -> VfsResult<SharedVfs> {
    let cfg2 = cfg.clone();
    let factory = move || {
        let tcp = tcp_connect(&cfg2.host, effective_port(&cfg2))?;
        tcp.set_read_timeout(Some(IO_TIMEOUT)).ok();
        tcp.set_write_timeout(Some(IO_TIMEOUT)).ok();
        let stream = suppaftp::FtpStream::connect_with_stream(tcp)
            .map_err(|e| map_ftp(e, &VfsPath::root()))?;
        Ok::<_, VfsError>(Conn {
            ftp: setup(stream, &cfg2)?,
            switch: None,
        })
    };
    Ok(Arc::new(FtpFs::new(
        cfg,
        Protocol::Ftp,
        Box::new(factory),
        None,
    )?))
}

/// Connect to an FTPS server using explicit TLS (`AUTH TLS`).
pub(crate) fn connect_secure(cfg: &ConnectionConfig) -> VfsResult<SharedVfs> {
    let base = tls::default_client_config().map_err(VfsError::Connection)?;
    Ok(Arc::new(connect_secure_with(cfg, base)?))
}

/// [`connect_secure`] with a given TLS client configuration (tests).
pub(crate) fn connect_secure_with(
    cfg: &ConnectionConfig,
    base: Arc<rustls::ClientConfig>,
) -> VfsResult<FtpFs<tls::Stream>> {
    let policy = cfg.ftp.as_ref().map(|o| o.data_channel).unwrap_or_default();
    let cfg2 = cfg.clone();
    let factory = move || {
        let tcp = tcp_connect(&cfg2.host, effective_port(&cfg2))?;
        tcp.set_read_timeout(Some(IO_TIMEOUT)).ok();
        tcp.set_write_timeout(Some(IO_TIMEOUT)).ok();
        let switch = Arc::new(DataSwitch::default());
        let connector = tls::Connector::new(&base, Arc::clone(&switch));
        let stream = ImplFtpStream::<tls::Stream>::connect_with_stream(tcp)
            .map_err(|e| map_ftp(e, &VfsPath::root()))?
            .into_secure(connector, &cfg2.host)
            .map_err(|e| match e {
                FtpError::UnexpectedResponse(r) => VfsError::Connection(format!(
                    "server does not support explicit FTPS (AUTH TLS): {r}"
                )),
                other => map_ftp(other, &VfsPath::root()),
            })?;
        Ok(Conn {
            ftp: setup(stream, &cfg2)?,
            switch: Some(switch),
        })
    };
    FtpFs::new(
        cfg,
        Protocol::Ftps,
        Box::new(factory),
        Some(Arc::new(DataPolicy::new(policy))),
    )
}

/// Log in and configure a freshly connected stream.
fn setup<T: TlsStream>(
    stream: ImplFtpStream<T>,
    cfg: &ConnectionConfig,
) -> VfsResult<ImplFtpStream<T>> {
    let mut stream = stream.passive_stream_builder(|addr| {
        let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(FtpError::ConnectionError)?;
        s.set_read_timeout(Some(IO_TIMEOUT)).ok();
        s.set_write_timeout(Some(IO_TIMEOUT)).ok();
        Ok(s)
    });
    // Always connect data connections to the control connection's host: the
    // address in a PASV reply is often private or bogus behind NAT.
    stream.set_passive_nat_workaround(true);

    let (user, pass) = match (&cfg.auth, cfg.username.is_empty()) {
        (Auth::Password(pw), true) => ("anonymous".to_string(), pw.clone()),
        (Auth::Password(pw), false) => (cfg.username.clone(), pw.clone()),
        (_, true) => ("anonymous".to_string(), "anonymous@".to_string()),
        (_, false) => (cfg.username.clone(), String::new()),
    };
    stream
        .login(user.as_str(), pass.as_str())
        .map_err(|e| match e {
            FtpError::UnexpectedResponse(r) => VfsError::Auth(format!("login failed: {r}")),
            other => map_ftp(other, &VfsPath::root()),
        })?;
    stream
        .transfer_type(TransferType::Binary)
        .map_err(|e| map_ftp(e, &VfsPath::root()))?;
    Ok(stream)
}

impl<T: TlsStream + Send + 'static> FtpFs<T> {
    fn new(
        cfg: &ConnectionConfig,
        protocol: Protocol,
        factory: Box<Factory<T>>,
        data: Option<Arc<DataPolicy>>,
    ) -> VfsResult<Self> {
        let mut first = factory()?;
        let features = first.ftp.feat().unwrap_or_default();
        let mlsx = features.keys().any(|k| k.eq_ignore_ascii_case("MLST"));
        if features.keys().any(|k| k.eq_ignore_ascii_case("UTF8")) {
            let _ = first.ftp.opts("UTF8", Some("ON"));
        }
        let home = first
            .ftp
            .pwd()
            .map(|p| VfsPath::new(&p))
            .unwrap_or_else(|_| VfsPath::root());
        let ipv6 = first.ftp.get_ref().peer_addr().is_ok_and(|a| a.is_ipv6());
        let (passive, first) = probe_passive(first, ipv6, &*factory)?;
        let pool = Arc::new(Pool {
            state: Mutex::new(PoolState {
                idle: vec![(first, Instant::now())],
                total: 1,
            }),
            cv: Condvar::new(),
            factory,
        });
        let user = if cfg.username.is_empty() {
            "anonymous"
        } else {
            cfg.username.as_str()
        };
        Ok(FtpFs {
            label: format!("{user}@{} ({protocol})", cfg.host),
            protocol,
            pool,
            home,
            mlsx: AtomicBool::new(mlsx),
            data,
            passive: AtomicU8::new(passive),
        })
    }

    /// Check out a connection, set up for the current passive mode and data
    /// channel protection.
    fn lease(&self) -> VfsResult<Lease<T>> {
        let mut lease = self.pool.checkout()?;
        lease
            .conn()
            .set_mode(mode_of(self.passive.load(Ordering::Relaxed)));
        if let Some(policy) = &self.data {
            let switch = lease.slot().switch.clone();
            if let Some(switch) = switch.filter(|s| policy.wants_clear() && !s.is_clear()) {
                lease
                    .run(|c| c.custom_command("PROT C", &[Status::CommandOk]))
                    .map_err(|e| map_ftp(e, &VfsPath::root()))?;
                switch.set_clear();
            }
        }
        Ok(lease)
    }

    /// Run an operation that opens a data connection. If the data
    /// connection's TLS handshake fails and the policy allows it, retry once
    /// on a fresh connection with a clear data channel. Returns the result
    /// and the lease it ran on (transfers keep it).
    fn with_data<R>(
        &self,
        mut op: impl FnMut(&mut Lease<T>) -> VfsResult<R>,
    ) -> VfsResult<(R, Lease<T>)> {
        let mut lease = self.lease()?;
        match op(&mut lease) {
            Ok(r) => Ok((r, lease)),
            Err(e) if lease.data_tls_failed => {
                // The server is still waiting for the data connection: never
                // reuse this control connection.
                lease.broken = true;
                drop(lease);
                let Some(policy) = &self.data else {
                    return Err(e);
                };
                if !policy.on_tls_failure() {
                    return Err(policy.tls_failure_error(e));
                }
                let mut lease = self.lease()?;
                let r = op(&mut lease)?;
                Ok((r, lease))
            }
            Err(e) => Err(e),
        }
    }

    /// Run a single control-channel command on a pooled connection.
    fn cmd<R>(
        &self,
        path: &VfsPath,
        f: impl FnOnce(&mut ImplFtpStream<T>) -> Result<R, FtpError>,
    ) -> VfsResult<R> {
        let mut lease = self.lease()?;
        lease.run(f).map_err(|e| map_ftp(e, path))
    }

    fn list_with(&self, lease: &mut Lease<T>, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        if let Err(e) = lease.run(|c| c.cwd(dir.as_str())) {
            let mapped = map_ftp(e, dir);
            if matches!(mapped, VfsError::Other(_) | VfsError::NotFound(_)) {
                if let Ok(entry) = self.stat_with(lease, dir) {
                    if !entry.is_dir() {
                        return Err(VfsError::NotADirectory(dir.to_string()));
                    }
                }
            }
            return Err(mapped);
        }
        let mut entries = None;
        if self.mlsx.load(Ordering::Relaxed) {
            match lease.run(|c| c.mlsd(None)) {
                Ok(lines) => {
                    entries = Some(
                        lines
                            .iter()
                            .filter_map(|l| parse_mlsx(l, dir, None))
                            .collect(),
                    );
                }
                Err(FtpError::UnexpectedResponse(r))
                    if [500, 501, 502, 504].contains(&r.status.code()) =>
                {
                    log::info!("MLSD not supported, falling back to LIST");
                    self.mlsx.store(false, Ordering::Relaxed);
                }
                Err(e) => return Err(map_ftp(e, dir)),
            }
        }
        let mut entries: Vec<Entry> = match entries {
            Some(e) => e,
            None => lease
                .run(|c| c.list(None))
                .map_err(|e| map_ftp(e, dir))?
                .iter()
                .filter_map(|l| parse_list_line(l, dir))
                .collect(),
        };
        // Symlinks that point to directories should be navigable.
        for e in entries.iter_mut().filter(|e| e.kind == EntryKind::Symlink) {
            if lease.run(|c| c.cwd(e.path.as_str())).is_ok() {
                e.kind = EntryKind::Directory;
                e.size = 0;
            }
        }
        Ok(entries)
    }

    fn stat_with(&self, lease: &mut Lease<T>, path: &VfsPath) -> VfsResult<Entry> {
        if path.is_root() {
            return Ok(Entry {
                name: "/".into(),
                path: VfsPath::root(),
                kind: EntryKind::Directory,
                size: 0,
                modified: None,
                permissions: None,
                link_target: None,
                hidden: false,
            });
        }
        let name = path.file_name().to_string();
        let parent = path.parent().unwrap_or_else(VfsPath::root);
        if self.mlsx.load(Ordering::Relaxed) {
            match lease.run(|c| c.mlst(Some(path.as_str()))) {
                Ok(line) => {
                    if let Some(mut e) = parse_mlsx(&line, &parent, Some(&name)) {
                        if e.kind == EntryKind::Symlink
                            && lease.run(|c| c.cwd(path.as_str())).is_ok()
                        {
                            e.kind = EntryKind::Directory;
                            e.size = 0;
                        }
                        return Ok(e);
                    }
                }
                Err(FtpError::UnexpectedResponse(r))
                    if [500, 501, 502, 504].contains(&r.status.code()) => {}
                Err(e) => return Err(map_ftp(e, path)),
            }
        }
        // Fallback: list the parent directory and look the entry up.
        let entries = self.list_with(lease, &parent)?;
        entries
            .into_iter()
            .find(|e| e.name == name)
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }
}

impl<T: TlsStream + Send + 'static> Vfs for FtpFs<T> {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn protocol(&self) -> Protocol {
        self.protocol
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(self.home.clone())
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.with_data(|lease| self.list_with(lease, dir))
            .map(|(r, _)| r)
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        // MLST needs no data connection, but the LIST fallback does.
        self.with_data(|lease| self.stat_with(lease, path))
            .map(|(r, _)| r)
    }

    fn notice(&self) -> Option<ConnectionNotice> {
        self.data.as_ref().and_then(|d| d.notice())
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let mut lease = self.lease()?;
        lease.run(|c| c.mkdir(path.as_str())).map_err(|e| {
            let mapped = map_ftp(e, path);
            if matches!(mapped, VfsError::Other(_) | VfsError::PermissionDenied(_))
                && self.stat_with(&mut lease, path).is_ok()
            {
                VfsError::AlreadyExists(path.to_string())
            } else {
                mapped
            }
        })
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.cmd(path, |c| c.rm(path.as_str()))
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let mut lease = self.lease()?;
        lease.run(|c| c.rmdir(path.as_str())).map_err(|e| {
            let mapped = map_ftp(e, path);
            if matches!(mapped, VfsError::Other(_)) {
                match self.stat_with(&mut lease, path) {
                    Ok(entry) if !entry.is_dir() => {
                        return VfsError::NotADirectory(path.to_string())
                    }
                    Ok(_) => {
                        if self
                            .list_with(&mut lease, path)
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
        self.cmd(from, |c| c.rename(from.as_str(), to.as_str()))
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let (stream, lease) = self.with_data(|lease| {
            lease.run(|c| c.retr_as_stream(path.as_str())).map_err(|e| {
                let mapped = map_ftp(e, path);
                if lease.data_tls_failed {
                    return mapped;
                }
                match (&mapped, self.stat_with(lease, path)) {
                    (VfsError::Other(_), Ok(entry)) if entry.is_dir() => {
                        VfsError::Other(format!("is a directory: {path}"))
                    }
                    (VfsError::Other(_), Err(VfsError::NotFound(_))) => {
                        VfsError::NotFound(path.to_string())
                    }
                    _ => mapped,
                }
            })
        })?;
        Ok(Box::new(FtpReader {
            stream: Some(stream),
            lease,
            path: path.clone(),
        }))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let (stream, lease) = self.with_data(|lease| {
            lease
                .run(|c| c.put_with_stream(path.as_str()))
                .map_err(|e| map_ftp(e, path))
        })?;
        Ok(Box::new(FtpWriter {
            stream: Some(stream),
            lease,
            path: path.clone(),
        }))
    }

    /// `REST offset` + `RETR`. Servers without `REST` fall back to reading
    /// and discarding the first `offset` bytes.
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        if offset == 0 {
            return self.open_read(path);
        }
        let rest = usize::try_from(offset)
            .map_err(|_| VfsError::Other(format!("{path}: offset {offset} too large")))?;
        let res = self.with_data(|lease| {
            lease
                .run(|c| {
                    c.resume_transfer(rest)?;
                    c.retr_as_stream(path.as_str())
                })
                .map_err(|e| {
                    // A refused REST leaves the passive data socket unused:
                    // don't reuse this control connection.
                    lease.broken = true;
                    map_ftp(e, path)
                })
        });
        match res {
            Ok((stream, lease)) => Ok(Box::new(FtpReader {
                stream: Some(stream),
                lease,
                path: path.clone(),
            })),
            Err(VfsError::Unsupported(m)) => {
                log::info!("FTP server refuses REST ({m}); skipping {offset} bytes instead");
                let mut r = self.open_read(path)?;
                let skipped = io::copy(&mut (&mut r).take(offset), &mut io::sink())?;
                if skipped < offset {
                    return Err(VfsError::Other(format!(
                        "{path}: file is shorter ({skipped} bytes) than the resume offset {offset}"
                    )));
                }
                Ok(r)
            }
            Err(e) => Err(e),
        }
    }

    /// `APPE`: the server appends (creating the file if needed).
    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let (stream, lease) = self.with_data(|lease| {
            lease
                .run(|c| c.append_with_stream(path.as_str()))
                .map_err(|e| map_ftp(e, path))
        })?;
        Ok(Box::new(FtpWriter {
            stream: Some(stream),
            lease,
            path: path.clone(),
        }))
    }

    fn supports_resume(&self) -> manta_core::ResumeCaps {
        manta_core::ResumeCaps {
            read_at: true,
            append: true,
            upload_session: false,
        }
    }

    /// Drop the idle pooled control connections (after a network drop they
    /// are dead, and fresh ones would be reused without a probe), then log
    /// in once to check the server is reachable again. Data channel policy
    /// and TLS session reuse apply to the new connections as before.
    fn reconnect(&self) -> VfsResult<()> {
        let stale = {
            let mut st = self.pool.lock();
            let idle = std::mem::take(&mut st.idle);
            st.total -= idle.len();
            idle
        };
        drop(stale);
        self.pool.cv.notify_all();
        let mut lease = self.lease()?;
        lease
            .run(|c| c.noop())
            .map_err(|e| map_ftp(e, &VfsPath::root()))
    }

    fn set_permissions(&self, path: &VfsPath, mode: u32) -> VfsResult<()> {
        self.cmd(path, |c| {
            c.site(format!("CHMOD {:o} {}", mode & 0o7777, path.as_str()))
        })
        .map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// Transfer streams
// ---------------------------------------------------------------------------

/// Download stream. The completion reply is checked at end of stream; an
/// unfinished download is aborted (`ABOR`) on drop.
struct FtpReader<T: TlsStream> {
    stream: Option<TransferStream<T>>,
    lease: Lease<T>,
    path: VfsPath,
}

impl<T: TlsStream> Read for FtpReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(stream) = self.stream.as_mut() else {
            return Ok(0);
        };
        let n = match stream.read(buf) {
            Ok(n) => n,
            Err(e) => {
                self.lease.broken = true;
                return Err(e);
            }
        };
        if n == 0 && !buf.is_empty() {
            if let Some(stream) = self.stream.take() {
                stream.finish().map_err(|e| {
                    if is_fatal(&e) {
                        self.lease.broken = true;
                    }
                    to_io(e, &self.path)
                })?;
            }
        }
        Ok(n)
    }
}

impl<T: TlsStream> Drop for FtpReader<T> {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            if let Err(e) = self.lease.conn().abort(stream) {
                log::debug!("aborting FTP download of {} failed: {e}", self.path);
                self.lease.broken = true;
            }
        }
    }
}

/// Upload stream. `flush()` completes the transfer and reports the server's
/// verdict; writing afterwards continues with `APPE`. Drop finalizes
/// best-effort.
struct FtpWriter<T: TlsStream> {
    stream: Option<TransferStream<T>>,
    lease: Lease<T>,
    path: VfsPath,
}

impl<T: TlsStream> FtpWriter<T> {
    fn finalize(&mut self) -> io::Result<()> {
        let Some(mut stream) = self.stream.take() else {
            return Ok(());
        };
        if let Err(e) = stream.flush() {
            self.lease.broken = true;
            return Err(e);
        }
        stream.finish().map_err(|e| {
            if is_fatal(&e) {
                self.lease.broken = true;
            }
            to_io(e, &self.path)
        })
    }
}

impl<T: TlsStream> Write for FtpWriter<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.stream.is_none() {
            let path = self.path.clone();
            let s = self
                .lease
                .run(|c| c.append_with_stream(path.as_str()))
                .map_err(|e| to_io(e, &path))?;
            self.stream = Some(s);
        }
        let Some(stream) = self.stream.as_mut() else {
            return Err(io::Error::other("FTP upload stream missing"));
        };
        stream.write(buf).inspect_err(|_| self.lease.broken = true)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.finalize()
    }
}

impl<T: TlsStream> Drop for FtpWriter<T> {
    fn drop(&mut self) {
        if let Err(e) = self.finalize() {
            log::warn!("finalizing FTP upload of {} failed: {e}", self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlsd_parsing() {
        let dir = VfsPath::new("/pub");
        let e = parse_mlsx(
            "type=file;size=1234;modify=20201019151930.123;UNIX.mode=0644; my file.txt",
            &dir,
            None,
        )
        .expect("entry");
        assert_eq!(e.name, "my file.txt");
        assert_eq!(e.path.as_str(), "/pub/my file.txt");
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.size, 1234);
        assert_eq!(e.permissions, Some(0o644));
        assert_eq!(
            e.modified.map(|m| m.to_rfc3339()).as_deref(),
            Some("2020-10-19T15:19:30+00:00")
        );

        let d = parse_mlsx("type=dir;modify=20201019151930; .config", &dir, None).expect("dir");
        assert!(d.is_dir());
        assert!(d.hidden);
        assert_eq!(d.permissions, None);

        assert!(parse_mlsx("type=cdir; .", &dir, None).is_none());
        assert!(parse_mlsx("type=pdir; ..", &dir, None).is_none());

        let l = parse_mlsx("type=OS.unix=slink:/etc/x;size=5; link", &dir, None).expect("link");
        assert_eq!(l.kind, EntryKind::Symlink);
        assert_eq!(l.link_target.as_deref(), Some("/etc/x"));

        let m = parse_mlsx(" type=file;size=3; /pub/a.txt", &dir, Some("a.txt")).expect("mlst");
        assert_eq!(m.name, "a.txt");
        assert_eq!(m.size, 3);
    }

    #[test]
    fn list_parsing() {
        let dir = VfsPath::new("/");
        let e = parse_list_line("-rw-r--r-- 1 user group 1234 Nov 5 13:46 example.txt", &dir)
            .expect("posix");
        assert_eq!(e.name, "example.txt");
        assert_eq!(e.size, 1234);
        assert_eq!(e.permissions, Some(0o644));
        assert_eq!(e.kind, EntryKind::File);

        let d = parse_list_line("drwxr-xr-x 2 user group 4096 Nov 5 2018 docs", &dir).expect("dir");
        assert!(d.is_dir());
        assert_eq!(d.permissions, Some(0o755));

        let l = parse_list_line("lrwxrwxrwx 1 user group 7 Nov 5 2018 link -> target", &dir)
            .expect("link");
        assert_eq!(l.kind, EntryKind::Symlink);
        assert_eq!(l.name, "link");
        assert_eq!(l.link_target.as_deref(), Some("target"));

        let dos = parse_list_line("10-19-20  03:19PM  <DIR>  windows", &dir).expect("dos");
        assert!(dos.is_dir());
        assert_eq!(dos.permissions, None);
    }

    #[test]
    fn reply_mapping() {
        let p = || "/x".to_string();
        assert!(matches!(
            map_reply(550, "No such file or directory", p()),
            VfsError::NotFound(_)
        ));
        assert!(matches!(
            map_reply(550, "Permission denied", p()),
            VfsError::PermissionDenied(_)
        ));
        assert!(matches!(
            map_reply(550, "Directory not empty", p()),
            VfsError::DirectoryNotEmpty(_)
        ));
        assert!(matches!(
            map_reply(550, "File exists", p()),
            VfsError::AlreadyExists(_)
        ));
        assert!(matches!(
            map_reply(530, "Not logged in", p()),
            VfsError::Auth(_)
        ));
        assert!(matches!(
            map_reply(421, "Timeout", p()),
            VfsError::Connection(_)
        ));
        assert!(matches!(
            map_reply(502, "Not implemented", p()),
            VfsError::Unsupported(_)
        ));
        assert!(matches!(map_reply(550, "Failed", p()), VfsError::Other(_)));
    }
}
