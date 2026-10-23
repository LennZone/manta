//! VFS backends: local file system, SFTP, FTP/FTPS, S3 (and S3-compatible
//! object stores, see [`s3`]), Docker containers and Kubernetes pods (see
//! [`container`]), Google Drive (see [`gdrive`]), and read-only archives
//! ([`ArchiveFs`]: zip, tar, compressed tar, single compressed files).
//!
//! PUBLIC API CONTRACT (used by `manta-ops` and `manta-app`; do not change signatures):
//! - `LocalFs::new() -> LocalFs`  (implements `Vfs`)
//! - `connect(&ConnectionConfig) -> VfsResult<SharedVfs>`  (dispatches on protocol; Local -> LocalFs)
//!
//! Notes for callers:
//! - All backends are `Send + Sync` and may be used concurrently from several
//!   threads. FTP keeps a small pool of control connections so that e.g. a
//!   directory listing can run while a transfer is in progress, and a copy
//!   within the same FTP server (read + write at once) does not deadlock.
//! - Writers returned by `open_write` finalize the upload when dropped. Call
//!   `flush()` before dropping to observe errors: for FTP, `flush()` completes
//!   the transfer and reports the server's verdict (writing again afterwards
//!   transparently appends).
//! - Readers report transfer errors (FTP completion reply) at end of stream.
//! - `rename` has native semantics of the backend (local: replaces an
//!   existing target file); check for existence first if that matters.

use std::sync::Arc;

use manta_core::{ConnectionConfig, Protocol, SharedVfs, VfsResult};

pub mod archive;
pub mod cloud_local;
pub mod container;
pub mod ftp;
pub mod gdrive;
pub mod known_hosts;
pub mod local;
pub mod s3;
pub mod sftp;

pub use archive::{ArchiveFormat, ArchiveFs};
pub use cloud_local::{discover_cloud_locations, CloudLocation, CloudProvider, SyncState};
pub use container::{ContainerFs, DockerFs, K8sFs};
pub use ftp::FtpFs;
pub use gdrive::GDriveFs;
pub use local::LocalFs;
pub use s3::S3Fs;
pub use sftp::SftpFs;

/// Default timeout for establishing network connections.
pub(crate) const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Open a connection for the given config. Blocking; call off the UI thread.
///
/// `cfg.initial_path` is ignored here; the app navigates there after connecting.
pub fn connect(cfg: &ConnectionConfig) -> VfsResult<SharedVfs> {
    Ok(match cfg.protocol {
        Protocol::Local => Arc::new(LocalFs::new()),
        Protocol::Sftp => Arc::new(SftpFs::connect(cfg)?),
        Protocol::Ftp => ftp::connect_plain(cfg)?,
        Protocol::Ftps => ftp::connect_secure(cfg)?,
        Protocol::S3 => Arc::new(S3Fs::connect(cfg)?),
        Protocol::Docker | Protocol::Kubernetes => Arc::new(ContainerFs::connect(cfg)?),
        Protocol::GoogleDrive => Arc::new(gdrive::connect(cfg)?),
    })
}

/// Resolve `host:port` and connect with [`CONNECT_TIMEOUT`], trying every
/// resolved address in turn.
pub(crate) fn tcp_connect(host: &str, port: u16) -> VfsResult<std::net::TcpStream> {
    use manta_core::VfsError;
    use std::net::ToSocketAddrs;

    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| VfsError::Connection(format!("cannot resolve {host}: {e}")))?;
    let mut last_err = None;
    for addr in addrs {
        match std::net::TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    Err(VfsError::Connection(match last_err {
        Some(e) => format!("cannot connect to {host}:{port}: {e}"),
        None => format!("no address found for {host}"),
    }))
}

/// Effective port: `cfg.port`, or the protocol default when it is 0.
pub(crate) fn effective_port(cfg: &ConnectionConfig) -> u16 {
    if cfg.port == 0 {
        ConnectionConfig::default_port(cfg.protocol)
    } else {
        cfg.port
    }
}

/// Convert a unix timestamp (seconds) to a UTC datetime.
pub(crate) fn unix_to_utc(secs: i64) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::from_timestamp(secs, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::{Auth, VfsError};

    fn cfg(protocol: Protocol, port: u16) -> ConnectionConfig {
        ConnectionConfig {
            name: "t".into(),
            protocol,
            host: "127.0.0.1".into(),
            port,
            username: "nobody".into(),
            auth: Auth::None,
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
        }
    }

    #[test]
    fn connect_local() {
        let v = connect(&cfg(Protocol::Local, 0)).expect("local");
        assert_eq!(v.protocol(), Protocol::Local);
        assert!(v.local_path(&manta_core::VfsPath::root()).is_some());
    }

    #[test]
    fn connect_refused_is_connection_error() {
        // Grab a free port, then close it so nothing listens there.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .expect("free port");
        for proto in [Protocol::Sftp, Protocol::Ftp, Protocol::Ftps] {
            match connect(&cfg(proto, port)) {
                Err(VfsError::Connection(_)) => {}
                Err(other) => panic!("{proto}: unexpected error {other:?}"),
                Ok(_) => panic!("{proto}: unexpectedly connected"),
            }
        }
    }
}
