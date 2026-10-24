//! Data channel policy, TLS session reuse and passive mode fallbacks against
//! the scripted fake server ([`super::fake_server`]).

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use manta_core::{
    Auth, ConnectionConfig, ConnectionNotice, FtpDataChannel, FtpOptions, Protocol, Vfs, VfsError,
    VfsPath,
};

use super::fake_server::{client_config, Options, Server};
use super::*;

fn config(server: &Server, protocol: Protocol, policy: FtpDataChannel) -> ConnectionConfig {
    ConnectionConfig {
        name: "fake".into(),
        protocol,
        host: "127.0.0.1".into(),
        port: server.port,
        username: "manta".into(),
        auth: Auth::Password("secret".into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: Some(Box::new(FtpOptions {
            data_channel: policy,
        })),
        webdav: None,
        smb: None,
    }
}

fn secure(server: &Server, policy: FtpDataChannel) -> FtpFs<tls::Stream> {
    connect_secure_with(&config(server, Protocol::Ftps, policy), client_config()).expect("connect")
}

fn put(fs: &dyn Vfs, name: &str, body: &[u8]) {
    let mut w = fs.open_write(&VfsPath::new(name)).expect("open_write");
    w.write_all(body).expect("write");
    w.flush().expect("finish upload");
}

fn get(fs: &dyn Vfs, name: &str) -> Vec<u8> {
    let mut r = fs.open_read(&VfsPath::new(name)).expect("open_read");
    let mut out = Vec::new();
    r.read_to_end(&mut out).expect("read");
    out
}

fn names(fs: &dyn Vfs) -> Vec<String> {
    fs.list(&VfsPath::root())
        .expect("list")
        .into_iter()
        .map(|e| e.name)
        .collect()
}

fn seed(server: &Server) {
    server
        .files
        .lock()
        .unwrap()
        .insert("hello.txt".into(), b"hello".to_vec());
}

#[test]
fn tls_data_connections_resume_the_control_session() {
    let server = Server::start(Options::default());
    seed(&server);
    let fs = secure(&server, FtpDataChannel::Auto);
    assert_eq!(names(&fs), ["hello.txt"]);
    put(&fs, "up.bin", &[7u8; 100_000]);
    assert_eq!(get(&fs, "up.bin"), vec![7u8; 100_000]);
    assert_eq!(get(&fs, "hello.txt"), b"hello");
    assert_eq!(fs.notice(), None);
    let seen = server.seen.lock().unwrap();
    assert_eq!(
        seen.tls_data.len(),
        4,
        "every data connection was encrypted"
    );
    assert!(
        seen.tls_data.iter().all(|resumed| *resumed),
        "data connections resume the session: {:?}",
        seen.tls_data
    );
    assert!(!seen.commands.iter().any(|c| c == "PROT C"));
}

#[test]
fn auto_falls_back_to_clear_data_when_the_network_resets_tls() {
    let server = Server::start(Options {
        reset_tls_data: true,
        ..Options::default()
    });
    seed(&server);
    let fs = secure(&server, FtpDataChannel::Auto);
    assert_eq!(
        fs.notice(),
        None,
        "nothing to report before the first transfer"
    );
    assert_eq!(names(&fs), ["hello.txt"]);
    assert_eq!(fs.notice(), Some(ConnectionNotice::ClearDataChannel));
    // Transfers keep working on the clear data channel, also on new pooled connections.
    put(&fs, "a.txt", b"payload");
    assert_eq!(get(&fs, "a.txt"), b"payload");
    let seen = server.seen.lock().unwrap();
    assert_eq!(
        seen.resets, 1,
        "only the first encrypted attempt hits the firewall"
    );
    assert!(seen.tls_data.is_empty());
    // The login itself stayed encrypted: AUTH TLS came before USER/PASS.
    let auth = seen
        .commands
        .iter()
        .position(|c| c == "AUTH TLS")
        .expect("AUTH TLS");
    let user = seen
        .commands
        .iter()
        .position(|c| c.starts_with("USER"))
        .expect("USER");
    assert!(auth < user);
    assert!(seen.commands.iter().any(|c| c == "PROT C"));
}

#[test]
fn require_never_falls_back() {
    let server = Server::start(Options {
        reset_tls_data: true,
        ..Options::default()
    });
    seed(&server);
    let fs = secure(&server, FtpDataChannel::Require);
    match fs.list(&VfsPath::root()) {
        Err(VfsError::Connection(msg)) => assert_eq!(msg, DATA_TLS_BLOCKED),
        other => panic!("expected the blocked-TLS error, got {other:?}"),
    }
    assert_eq!(fs.notice(), None);
    // Still failing (and still not falling back) on the next try.
    assert!(fs.list(&VfsPath::root()).is_err());
    assert!(!server.commands().iter().any(|c| c == "PROT C"));
}

#[test]
fn off_uses_clear_data_from_the_start() {
    let server = Server::start(Options {
        reset_tls_data: true,
        ..Options::default()
    });
    seed(&server);
    let fs = secure(&server, FtpDataChannel::Off);
    assert_eq!(names(&fs), ["hello.txt"]);
    assert_eq!(
        fs.notice(),
        None,
        "Off is the user's choice, not a fallback"
    );
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.resets, 0, "no encrypted data connection was tried");
    assert!(seen.commands.iter().any(|c| c == "AUTH TLS"));
}

#[test]
fn passive_address_is_ignored_in_favour_of_the_control_host() {
    // The fake PASV reply points at an unroutable TEST-NET address.
    let server = Server::start(Options {
        tls: false,
        ..Options::default()
    });
    seed(&server);
    let fs = connect_plain(&config(&server, Protocol::Ftp, FtpDataChannel::Auto)).expect("connect");
    let start = Instant::now();
    assert_eq!(names(&*fs), ["hello.txt"]);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(server.commands().iter().any(|c| c == "PASV"));
    assert!(!server.commands().iter().any(|c| c == "EPSV"));
}

#[test]
fn falls_back_to_epsv_when_pasv_is_refused() {
    let server = Server::start(Options {
        tls: false,
        pasv: false,
        ..Options::default()
    });
    seed(&server);
    let fs = connect_plain(&config(&server, Protocol::Ftp, FtpDataChannel::Auto)).expect("connect");
    assert_eq!(names(&*fs), ["hello.txt"]);
    put(&*fs, "b.txt", b"via epsv");
    assert_eq!(get(&*fs, "b.txt"), b"via epsv");
    let cmds = server.commands();
    assert_eq!(
        cmds.iter().filter(|c| *c == "PASV").count(),
        1,
        "PASV is probed once, then avoided: {cmds:?}"
    );
    assert!(cmds.iter().filter(|c| *c == "EPSV").count() >= 4);
}

#[test]
fn falls_back_to_pasv_when_epsv_is_refused_over_tls() {
    let server = Server::start(Options {
        epsv: false,
        ..Options::default()
    });
    seed(&server);
    let fs = secure(&server, FtpDataChannel::Auto);
    assert_eq!(names(&fs), ["hello.txt"]);
    assert!(
        !server.commands().iter().any(|c| c == "EPSV"),
        "IPv4: PASV first, EPSV never needed"
    );
}

#[test]
fn data_policy_decisions() {
    let auto = DataPolicy::new(FtpDataChannel::Auto);
    assert!(!auto.wants_clear());
    assert_eq!(auto.notice(), None);
    assert!(auto.on_tls_failure());
    assert!(auto.wants_clear());
    assert_eq!(auto.notice(), Some(ConnectionNotice::ClearDataChannel));
    assert!(
        auto.on_tls_failure(),
        "a second failing connection retries too"
    );

    let require = DataPolicy::new(FtpDataChannel::Require);
    assert!(!require.on_tls_failure());
    assert!(!require.wants_clear());
    assert_eq!(require.notice(), None);
    let err = require.tls_failure_error(VfsError::Connection("TLS error: reset".into()));
    assert!(matches!(err, VfsError::Connection(m) if m == DATA_TLS_BLOCKED));

    let off = DataPolicy::new(FtpDataChannel::Off);
    assert!(off.wants_clear());
    assert!(!off.on_tls_failure());
    assert_eq!(off.notice(), None);
}

#[test]
fn passive_mode_order_and_probe_classification() {
    assert_eq!(passive_order(false), [PASSIVE, EXTENDED_PASSIVE]);
    assert_eq!(passive_order(true), [EXTENDED_PASSIVE, PASSIVE]);
    assert_eq!(mode_of(PASSIVE), Mode::Passive);
    assert_eq!(mode_of(EXTENDED_PASSIVE), Mode::ExtendedPassive);
    let refused = Err(FtpError::UnexpectedResponse(
        suppaftp::types::Response::new(Status::from(502), b"502 not implemented".to_vec()),
    ));
    assert_eq!(classify_probe(&refused, false), Probe::Refused);
    // A transfer reply where a PASV reply belongs: out of step (curl's
    // "Bad PASV/EPSV response: 150").
    let odd = Err(FtpError::UnexpectedResponse(
        suppaftp::types::Response::new(Status::from(150), b"150 Opening".to_vec()),
    ));
    assert_eq!(classify_probe(&odd, true), Probe::Broken);
}
