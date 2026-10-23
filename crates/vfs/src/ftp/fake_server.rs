//! A tiny scripted FTP / FTPS server on localhost for the transport tests:
//! just enough of RFC 959 / 2228 / 2428 / 3659 for the backend, plus knobs
//! for the failure modes seen in the wild (a firewall that resets encrypted
//! data connections, bogus PASV addresses, servers refusing PASV or EPSV).

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{HandshakeKind, ServerConfig, ServerConnection, StreamOwned};

/// Test-only self-signed certificate (CN=localhost, 127.0.0.1).
const CERT: &[u8] = include_bytes!("testdata/cert.pem");
const KEY: &[u8] = include_bytes!("testdata/key.pem");

#[derive(Debug, Clone, Copy)]
pub(crate) struct Options {
    /// Offer `AUTH TLS`.
    pub tls: bool,
    /// Reset every data connection that should be encrypted (`PROT P`), like
    /// the firewall in the field: the TLS handshake never completes.
    pub reset_tls_data: bool,
    /// Answer `PASV` (with a bogus, unroutable address) or refuse it (502).
    pub pasv: bool,
    /// Answer `EPSV` or refuse it (502).
    pub epsv: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tls: true,
            reset_tls_data: false,
            pasv: true,
            epsv: true,
        }
    }
}

/// What the server saw, for assertions.
#[derive(Debug, Default)]
pub(crate) struct Seen {
    /// Commands (verb + argument; the password is masked).
    pub commands: Vec<String>,
    /// Data connections that were TLS: whether the session was resumed.
    pub tls_data: Vec<bool>,
    /// Encrypted data connections the "firewall" reset.
    pub resets: usize,
}

pub(crate) struct Server {
    pub port: u16,
    pub files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    pub seen: Arc<Mutex<Seen>>,
    pub faults: Arc<Faults>,
}

/// Network trouble to simulate (resume tests).
#[derive(Debug, Default)]
pub(crate) struct Faults {
    /// Cut the next download after this many bytes (426, like a reset).
    pub cut_download: Mutex<Option<usize>>,
    /// Cut the next upload after this many bytes.
    pub cut_upload: Mutex<Option<usize>>,
    /// Refuse new control connections (the server is "unreachable").
    pub down: std::sync::atomic::AtomicBool,
}

impl Server {
    pub(crate) fn start(opts: Options) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let files: Arc<Mutex<BTreeMap<String, Vec<u8>>>> = Arc::default();
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let tls = Arc::new(server_config());
        let faults: Arc<Faults> = Arc::default();
        let (f, s, fa) = (files.clone(), seen.clone(), faults.clone());
        thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { return };
                if fa.down.load(std::sync::atomic::Ordering::SeqCst) {
                    drop(conn);
                    continue;
                }
                let session = Session {
                    opts,
                    tls: tls.clone(),
                    files: f.clone(),
                    seen: s.clone(),
                    faults: fa.clone(),
                    private: false,
                    pending: None,
                    rename_from: None,
                    restart: 0,
                };
                thread::spawn(move || session.run(conn));
            }
        });
        Server {
            port,
            files,
            seen,
            faults,
        }
    }

    pub(crate) fn commands(&self) -> Vec<String> {
        self.seen.lock().unwrap().commands.clone()
    }
}

fn server_config() -> ServerConfig {
    let certs = vec![CertificateDer::from_pem_slice(CERT).expect("cert")];
    let key = PrivateKeyDer::from_pem_slice(KEY).expect("key");
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("server config")
}

/// A TLS client config that accepts the fake server's certificate.
pub(crate) fn client_config() -> Arc<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
        .with_no_client_auth();
    Arc::new(config)
}

#[derive(Debug)]
struct AcceptAny(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

enum Io {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ServerConnection, TcpStream>>),
}

impl Read for Io {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Io::Plain(s) => s.read(buf),
            Io::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Io {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Io::Plain(s) => s.write(buf),
            Io::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Io::Plain(s) => s.flush(),
            Io::Tls(s) => s.flush(),
        }
    }
}

impl Io {
    fn close(self) {
        match self {
            Io::Plain(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            Io::Tls(mut s) => {
                s.conn.send_close_notify();
                let _ = s.flush();
                let _ = s.sock.shutdown(Shutdown::Write);
            }
        }
    }
}

struct Session {
    opts: Options,
    tls: Arc<ServerConfig>,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    seen: Arc<Mutex<Seen>>,
    /// `PROT P` in effect.
    private: bool,
    pending: Option<TcpListener>,
    rename_from: Option<String>,
    faults: Arc<Faults>,
    /// `REST` offset for the next transfer.
    restart: usize,
}

fn read_line(io: &mut Io) -> Option<String> {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match io.read(&mut b) {
            Ok(0) | Err(_) => return None,
            Ok(_) if b[0] == b'\n' => break,
            Ok(_) => line.push(b[0]),
        }
    }
    Some(
        String::from_utf8_lossy(&line)
            .trim_end_matches('\r')
            .to_string(),
    )
}

fn reply(io: &mut Io, text: &str) {
    let _ = io.write_all(format!("{text}\r\n").as_bytes());
    let _ = io.flush();
}

fn name_of(arg: &str) -> String {
    arg.trim_start_matches('/').to_string()
}

impl Session {
    fn run(mut self, sock: TcpStream) {
        sock.set_read_timeout(Some(Duration::from_secs(30))).ok();
        let mut io = Io::Plain(sock);
        reply(&mut io, "220 fake FTP ready");
        while let Some(line) = read_line(&mut io) {
            let (verb, arg) = match line.split_once(' ') {
                Some((v, a)) => (v.to_ascii_uppercase(), a.to_string()),
                None => (line.to_ascii_uppercase(), String::new()),
            };
            let logged = if verb == "PASS" {
                "PASS ***".to_string()
            } else {
                line.clone()
            };
            self.seen.lock().unwrap().commands.push(logged);
            match verb.as_str() {
                "AUTH" if self.opts.tls => {
                    reply(&mut io, "234 AUTH TLS successful");
                    let Io::Plain(sock) = io else { return };
                    let conn = ServerConnection::new(self.tls.clone()).expect("server conn");
                    io = Io::Tls(Box::new(StreamOwned::new(conn, sock)));
                }
                "PBSZ" => reply(&mut io, "200 PBSZ 0 successful"),
                "PROT" => {
                    self.private = arg.eq_ignore_ascii_case("P");
                    reply(&mut io, "200 Protection set");
                }
                "USER" => reply(&mut io, "331 Password required"),
                "PASS" => reply(&mut io, "230 User logged in"),
                "TYPE" | "OPTS" | "NOOP" => reply(&mut io, "200 OK"),
                "FEAT" => reply(&mut io, "211-Features:\r\n MLST type*;size*;modify*;\r\n REST STREAM\r\n UTF8\r\n211 End"),
                "REST" => match arg.trim().parse::<usize>() {
                    Ok(n) => {
                        self.restart = n;
                        reply(&mut io, &format!("350 Restarting at {n}"));
                    }
                    Err(_) => reply(&mut io, "501 Bad offset"),
                },
                "PWD" => reply(&mut io, "257 \"/\" is the current directory"),
                "CWD" => {
                    if arg == "/" || arg.is_empty() {
                        reply(&mut io, "250 CWD successful");
                    } else {
                        reply(&mut io, &format!("550 {arg}: No such file or directory"));
                    }
                }
                "PASV" if self.opts.pasv => {
                    let l = TcpListener::bind("127.0.0.1:0").expect("bind data");
                    let port = l.local_addr().expect("addr").port();
                    self.pending = Some(l);
                    // TEST-NET-1: unroutable, so a client that used it would hang.
                    reply(&mut io, &format!("227 Entering Passive Mode (192,0,2,1,{},{})", port >> 8, port & 0xff));
                }
                "EPSV" if self.opts.epsv => {
                    let l = TcpListener::bind("127.0.0.1:0").expect("bind data");
                    let port = l.local_addr().expect("addr").port();
                    self.pending = Some(l);
                    reply(&mut io, &format!("229 Entering Extended Passive Mode (|||{port}|)"));
                }
                "MLSD" | "LIST" | "RETR" | "STOR" | "APPE" => self.transfer(&mut io, &verb, &arg),
                "SIZE" => match self.files.lock().unwrap().get(&name_of(&arg)) {
                    Some(d) => reply(&mut io, &format!("213 {}", d.len())),
                    None => reply(&mut io, &format!("550 {arg}: No such file or directory")),
                },
                "MLST" => {
                    let name = name_of(&arg);
                    match self.files.lock().unwrap().get(&name) {
                        Some(d) => reply(&mut io, &format!("250-Listing {arg}\r\n type=file;size={}; /{name}\r\n250 End", d.len())),
                        None => reply(&mut io, &format!("550 {arg}: No such file or directory")),
                    }
                }
                "DELE" => match self.files.lock().unwrap().remove(&name_of(&arg)) {
                    Some(_) => reply(&mut io, "250 DELE successful"),
                    None => reply(&mut io, &format!("550 {arg}: No such file or directory")),
                },
                "RNFR" => {
                    if self.files.lock().unwrap().contains_key(&name_of(&arg)) {
                        self.rename_from = Some(name_of(&arg));
                        reply(&mut io, "350 Ready for RNTO");
                    } else {
                        reply(&mut io, &format!("550 {arg}: No such file or directory"));
                    }
                }
                "RNTO" => match self.rename_from.take() {
                    Some(from) => {
                        let mut files = self.files.lock().unwrap();
                        let data = files.remove(&from).unwrap_or_default();
                        files.insert(name_of(&arg), data);
                        reply(&mut io, "250 Rename successful");
                    }
                    None => reply(&mut io, "503 Bad sequence of commands"),
                },
                "QUIT" => {
                    reply(&mut io, "221 Goodbye");
                    io.close();
                    return;
                }
                _ => reply(&mut io, &format!("502 {verb} not implemented")),
            }
        }
    }

    fn transfer(&mut self, io: &mut Io, verb: &str, arg: &str) {
        let Some(listener) = self.pending.take() else {
            reply(io, "425 Use PASV or EPSV first");
            return;
        };
        let name = name_of(arg);
        let restart = std::mem::take(&mut self.restart);
        if verb == "RETR" && !self.files.lock().unwrap().contains_key(&name) {
            reply(io, &format!("550 {arg}: No such file or directory"));
            return;
        }
        reply(io, "150 Opening data connection");
        let Ok((sock, _)) = listener.accept() else {
            reply(io, "425 Cannot open data connection");
            return;
        };
        sock.set_read_timeout(Some(Duration::from_secs(10))).ok();
        let mut data = if self.private {
            if self.opts.reset_tls_data {
                // Wait for the ClientHello, then close without reading it:
                // unread data makes the kernel answer with a TCP reset.
                let mut probe = [0u8; 1];
                let _ = sock.peek(&mut probe);
                drop(sock);
                self.seen.lock().unwrap().resets += 1;
                reply(
                    io,
                    "425 Unable to build data connection: Connection reset by peer",
                );
                return;
            }
            let conn = ServerConnection::new(self.tls.clone()).expect("server conn");
            let mut tls = StreamOwned::new(conn, sock);
            while tls.conn.is_handshaking() {
                if tls.conn.complete_io(&mut tls.sock).is_err() {
                    reply(io, "425 TLS negotiation failed");
                    return;
                }
            }
            let resumed = tls.conn.handshake_kind() == Some(HandshakeKind::Resumed);
            self.seen.lock().unwrap().tls_data.push(resumed);
            Io::Tls(Box::new(tls))
        } else {
            Io::Plain(sock)
        };
        match verb {
            "MLSD" | "LIST" => {
                let files = self.files.lock().unwrap().clone();
                for (n, d) in files {
                    let line = if verb == "MLSD" {
                        format!("type=file;size={};modify=20240102030405; {n}\r\n", d.len())
                    } else {
                        format!("-rw-r--r-- 1 user group {} Jan 02 2024 {n}\r\n", d.len())
                    };
                    let _ = data.write_all(line.as_bytes());
                }
                data.close();
            }
            "RETR" => {
                let body = self
                    .files
                    .lock()
                    .unwrap()
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                let body = body.get(restart..).unwrap_or_default();
                if let Some(n) = self.faults.cut_download.lock().unwrap().take() {
                    let _ = data.write_all(&body[..n.min(body.len())]);
                    data.close();
                    reply(io, "426 Connection closed; transfer aborted");
                    return;
                }
                let _ = data.write_all(body);
                data.close();
            }
            _ => {
                let mut body = Vec::new();
                let mut buf = [0u8; 8192];
                let cut = self.faults.cut_upload.lock().unwrap().take();
                loop {
                    if cut.is_some_and(|c| body.len() >= c) {
                        break;
                    }
                    match data.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => body.extend_from_slice(&buf[..n]),
                    }
                }
                if let Some(c) = cut {
                    body.truncate(c);
                    let mut files = self.files.lock().unwrap();
                    if verb == "APPE" {
                        files.entry(name).or_default().extend_from_slice(&body);
                    } else {
                        files.insert(name, body);
                    }
                    drop(files);
                    // Unread data makes the kernel answer with a reset.
                    drop(data);
                    reply(io, "426 Connection closed; transfer aborted");
                    return;
                }
                let mut files = self.files.lock().unwrap();
                if verb == "APPE" {
                    files.entry(name).or_default().extend_from_slice(&body);
                } else {
                    files.insert(name, body);
                }
                drop(files);
                data.close();
            }
        }
        reply(io, "226 Transfer complete");
    }
}
