//! FTPS transport: a rustls connector for suppaftp's TLS hook.
//!
//! Why our own connector instead of suppaftp's native-tls / rustls ones:
//!
//! - **TLS session reuse.** Servers such as ProFTPD (`TLSOptions
//!   RequireSessionReuse`, the default on many hosters), vsftpd
//!   (`require_ssl_reuse`) and FileZilla Server only accept an encrypted data
//!   connection that resumes the TLS session of its control connection.
//!   native-tls (Security.framework) can't do that. Every control connection
//!   here gets its own rustls session cache, so its data connections resume
//!   exactly its session (never the session of another pooled connection).
//! - **Clear data channel.** suppaftp wraps every data connection in TLS once
//!   the control connection is secure. After `PROT C` the connector hands the
//!   plain TCP socket through instead ([`DataSwitch`]).
//! - **Eager handshake.** The handshake runs inside `connect` with a short
//!   timeout, so a network that resets or swallows encrypted data
//!   connections shows up as a `FtpError::SecureError` of the data command
//!   (which the backend turns into the clear-data fallback), not as a broken
//!   read later on.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::Resumption;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, HandshakeKind, StreamOwned};
use suppaftp::{FtpError, FtpResult, TlsConnector, TlsStream};

/// Upper bound for one TLS handshake (control or data connection).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// Sessions kept per control connection (TLS 1.3 tickets are single-use).
const SESSIONS_PER_CONNECTION: usize = 8;

/// The default client configuration: ring, TLS 1.2 + 1.3, certificates
/// verified by macOS (Keychain trust settings, like Safari and the old
/// native-tls backend).
pub(crate) fn default_client_config() -> Result<Arc<ClientConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
        .map_err(|e| format!("cannot initialize TLS: {e}"))?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("cannot initialize TLS: {e}"))?
        .dangerous() // only to plug in the platform verifier; nothing is disabled
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Per control connection: whether its data connections are clear (the
/// server has acknowledged `PROT C`) or TLS (`PROT P`, the default).
#[derive(Debug, Default)]
pub(crate) struct DataSwitch {
    clear: AtomicBool,
}

impl DataSwitch {
    pub(crate) fn is_clear(&self) -> bool {
        self.clear.load(Ordering::Acquire)
    }

    pub(crate) fn set_clear(&self) {
        self.clear.store(true, Ordering::Release);
    }
}

/// suppaftp TLS connector for one control connection. The first `connect`
/// secures the control connection; every later one is a data connection.
pub(crate) struct Connector {
    config: Arc<ClientConfig>,
    control_secured: AtomicBool,
    switch: Arc<DataSwitch>,
}

impl fmt::Debug for Connector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connector")
            .field("clear_data", &self.switch.is_clear())
            .finish_non_exhaustive()
    }
}

impl Connector {
    /// A connector with its own session cache, so data connections resume
    /// this control connection's session.
    pub(crate) fn new(base: &Arc<ClientConfig>, switch: Arc<DataSwitch>) -> Self {
        let mut config = ClientConfig::clone(base);
        config.resumption = Resumption::in_memory_sessions(SESSIONS_PER_CONNECTION);
        Connector {
            config: Arc::new(config),
            control_secured: AtomicBool::new(false),
            switch,
        }
    }
}

impl TlsConnector for Connector {
    type Stream = Stream;

    fn connect(&self, domain: &str, mut tcp: TcpStream) -> FtpResult<Stream> {
        let data = self.control_secured.swap(true, Ordering::AcqRel);
        let what = if data {
            "data connection"
        } else {
            "control connection"
        };
        if data && self.switch.is_clear() {
            return Ok(Stream::Clear(tcp));
        }
        // Brackets around IPv6 literals are not part of the server name.
        let name = domain
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let name = ServerName::try_from(name)
            .map_err(|e| FtpError::SecureError(format!("invalid server name {domain}: {e}")))?;
        let mut conn = ClientConnection::new(Arc::clone(&self.config), name)
            .map_err(|e| FtpError::SecureError(e.to_string()))?;
        let old_timeouts = (
            tcp.read_timeout().ok().flatten(),
            tcp.write_timeout().ok().flatten(),
        );
        tcp.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).ok();
        tcp.set_write_timeout(Some(HANDSHAKE_TIMEOUT)).ok();
        while conn.is_handshaking() {
            if let Err(e) = conn.complete_io(&mut tcp) {
                return Err(FtpError::SecureError(handshake_error(what, &e)));
            }
        }
        tcp.set_read_timeout(old_timeouts.0).ok();
        tcp.set_write_timeout(old_timeouts.1).ok();
        if data {
            log::debug!(
                "FTPS data connection: TLS {:?}, session {}",
                conn.protocol_version(),
                if conn.handshake_kind() == Some(HandshakeKind::Resumed) {
                    "resumed"
                } else {
                    "new"
                }
            );
        }
        Ok(Stream::Tls(Box::new(TlsSock {
            inner: StreamOwned::new(conn, tcp),
            close_notify: true,
        })))
    }
}

fn handshake_error(what: &str, e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::UnexpectedEof => {
            format!("TLS handshake on the {what} was reset ({e})")
        }
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
            format!("TLS handshake on the {what} timed out")
        }
        _ => format!("TLS handshake on the {what} failed: {e}"),
    }
}

/// A TLS socket that says goodbye (`close_notify`) when dropped; servers
/// treat an upload without it as truncated.
#[derive(Debug)]
pub(crate) struct TlsSock {
    inner: StreamOwned<ClientConnection, TcpStream>,
    close_notify: bool,
}

impl Drop for TlsSock {
    fn drop(&mut self) {
        if self.close_notify {
            let _ = self.inner.flush();
            self.inner.conn.send_close_notify();
            let _ = self.inner.conn.write_tls(&mut self.inner.sock);
        }
    }
}

/// A control or data connection: TLS, or (data after `PROT C`) plain TCP.
#[derive(Debug)]
pub(crate) enum Stream {
    Tls(Box<TlsSock>),
    Clear(TcpStream),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tls(s) => s.inner.read(buf),
            Stream::Clear(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Tls(s) => s.inner.write(buf),
            Stream::Clear(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Tls(s) => s.inner.flush(),
            Stream::Clear(s) => s.flush(),
        }
    }
}

impl TlsStream for Stream {
    type InnerStream = Stream;

    fn tcp_stream(self) -> FtpResult<TcpStream> {
        match self {
            Stream::Tls(mut s) => {
                s.close_notify = false;
                let _ = s.inner.flush();
                s.inner.sock.try_clone().map_err(FtpError::ConnectionError)
            }
            Stream::Clear(s) => Ok(s),
        }
    }

    fn get_ref(&self) -> &TcpStream {
        match self {
            Stream::Tls(s) => s.inner.get_ref(),
            Stream::Clear(s) => s,
        }
    }

    fn mut_ref(&mut self) -> &mut Stream {
        self
    }
}
