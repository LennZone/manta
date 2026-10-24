//! The HTTP side of the WebDAV backend: URLs, Basic and Digest
//! authentication, retries with backoff (dropped connections, 5xx, 429),
//! one redirect (a folder addressed without its trailing slash) and HTTP
//! status mapping. Keep-alive connections are pooled by the `ureq::Agent`.
//! Certificates are checked by macOS (rustls-platform-verifier): Keychain
//! trust settings apply, like in Safari.

use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use manta_core::VfsError;
use ureq::http;

pub type Response = http::Response<ureq::Body>;

/// Attempts per request (1 + retries), for requests whose body can be sent
/// again.
const MAX_ATTEMPTS: u32 = 4;
const BACKOFF_BASE: Duration = Duration::from_millis(250);

/// A request body.
pub enum Body<'a> {
    None,
    Bytes(&'a [u8]),
    /// Streamed with chunked transfer encoding. Can't be sent twice, so
    /// such requests are never retried.
    Stream(Box<dyn Read + Send + 'static>),
}

/// One `WWW-Authenticate` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// Lower case: `basic`, `digest`, ...
    pub scheme: String,
    /// Parameter names in lower case, values unquoted.
    pub params: Vec<(String, String)>,
}

impl Challenge {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse `WWW-Authenticate` header values (several challenges per value
/// are allowed: `Basic realm="a", Digest realm="b", nonce="c"`).
pub fn parse_challenges(values: &[&str]) -> Vec<Challenge> {
    let mut out: Vec<Challenge> = Vec::new();
    for v in values {
        let b = v.as_bytes();
        let mut i = 0;
        while i < b.len() {
            while i < b.len() && (b[i] == b' ' || b[i] == b',' || b[i] == b'\t') {
                i += 1;
            }
            let start = i;
            while i < b.len() && !matches!(b[i], b' ' | b'\t' | b',' | b'=') {
                i += 1;
            }
            if start == i {
                i += 1;
                continue;
            }
            let token = v[start..i].to_string();
            let mut j = i;
            while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                j += 1;
            }
            if j < b.len() && b[j] == b'=' {
                // A parameter of the current challenge.
                j += 1;
                while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                    j += 1;
                }
                let mut value = String::new();
                if j < b.len() && b[j] == b'"' {
                    j += 1;
                    while j < b.len() && b[j] != b'"' {
                        if b[j] == b'\\' && j + 1 < b.len() {
                            j += 1;
                        }
                        // Values are ASCII in practice; keep UTF-8 intact.
                        let ch_len = utf8_len(b[j]);
                        value.push_str(&v[j..(j + ch_len).min(b.len())]);
                        j += ch_len;
                    }
                    j += 1;
                } else {
                    let s = j;
                    while j < b.len() && b[j] != b',' {
                        j += 1;
                    }
                    value = v[s..j].trim().to_string();
                }
                if let Some(c) = out.last_mut() {
                    c.params.push((token.to_ascii_lowercase(), value));
                }
                i = j;
            } else {
                // token68 after a scheme (`Negotiate abc==`) is ignored.
                let is_token68 = out
                    .last()
                    .is_some_and(|c| c.params.is_empty() && token.ends_with('='));
                if !is_token68 {
                    out.push(Challenge {
                        scheme: token.to_ascii_lowercase(),
                        params: Vec::new(),
                    });
                }
                i = j;
            }
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

/// How requests are signed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum AuthState {
    /// No challenge yet: requests go out without credentials.
    #[default]
    Unknown,
    Basic,
    Digest(Digest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Digest {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    /// As the server spelled it (`MD5`, `SHA-256`, `MD5-sess`, ...).
    algorithm: String,
    /// `auth` when the server offers it.
    qop: Option<String>,
    /// Nonce count.
    nc: u32,
}

impl Digest {
    fn from_challenge(c: &Challenge) -> Option<Digest> {
        let algorithm = c.param("algorithm").unwrap_or("MD5").to_string();
        let a = algorithm.to_ascii_uppercase();
        if !matches!(a.as_str(), "MD5" | "MD5-SESS" | "SHA-256" | "SHA-256-SESS") {
            return None;
        }
        let qop = c.param("qop").and_then(|q| {
            q.split(',')
                .map(str::trim)
                .any(|q| q.eq_ignore_ascii_case("auth"))
                .then(|| "auth".to_string())
        });
        Some(Digest {
            realm: c.param("realm").unwrap_or_default().to_string(),
            nonce: c.param("nonce")?.to_string(),
            opaque: c.param("opaque").map(str::to_string),
            algorithm,
            qop,
            nc: 0,
        })
    }

    fn hash(&self, s: &str) -> String {
        let alg = self.algorithm.to_ascii_uppercase();
        if alg.starts_with("SHA-256") {
            use sha2::Digest as _;
            hex(&sha2::Sha256::digest(s.as_bytes()))
        } else {
            use md5::Digest as _;
            hex(&md5::Md5::digest(s.as_bytes()))
        }
    }

    /// The `Authorization` value for one request (`uri`: the request target).
    fn authorization(&mut self, user: &str, password: &str, method: &str, uri: &str) -> String {
        self.nc += 1;
        let nc = format!("{:08x}", self.nc);
        let cnonce = random_hex(16);
        let mut ha1 = self.hash(&format!("{user}:{}:{password}", self.realm));
        if self.algorithm.to_ascii_uppercase().ends_with("-SESS") {
            ha1 = self.hash(&format!("{ha1}:{}:{cnonce}", self.nonce));
        }
        let ha2 = self.hash(&format!("{method}:{uri}"));
        let response = match &self.qop {
            Some(q) => self.hash(&format!("{ha1}:{}:{nc}:{cnonce}:{q}:{ha2}", self.nonce)),
            None => self.hash(&format!("{ha1}:{}:{ha2}", self.nonce)),
        };
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut out = format!(
            "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={}, response=\"{response}\"",
            q(user),
            q(&self.realm),
            q(&self.nonce),
            q(uri),
            self.algorithm
        );
        if let Some(o) = &self.opaque {
            out.push_str(&format!(", opaque=\"{}\"", q(o)));
        }
        if let Some(qop) = &self.qop {
            out.push_str(&format!(", qop={qop}, nc={nc}, cnonce=\"{cnonce}\""));
        }
        out
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    if getrandom::fill(&mut buf).is_err() {
        // Not secret, only unique: fall back to the clock.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        buf = t.to_le_bytes()[..n.min(16)].to_vec();
    }
    hex(&buf)
}

/// Percent-encode one path segment (everything but RFC 3986 unreserved).
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode `%XX` escapes (invalid UTF-8 is replaced).
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hexval = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encode a decoded `/a/b c` path segment by segment.
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// The path part of an `href` (absolute URL or absolute path), decoded,
/// without query and trailing slash (`""` for the root).
pub fn href_path(href: &str) -> String {
    let mut h = href.trim();
    if let Some(i) = h.find("://") {
        h = &h[i + 3..];
        h = h.find('/').map_or("", |j| &h[j..]);
    }
    if let Some(i) = h.find(['?', '#']) {
        h = &h[..i];
    }
    decode(h).trim_end_matches('/').to_string()
}

/// A WebDAV server: origin, root folder and credentials.
pub struct Client {
    agent: ureq::Agent,
    /// `https://host[:port]`.
    pub origin: String,
    /// Host name, for messages.
    pub host: String,
    /// Decoded root folder (`/remote.php/dav/files/alice`), no trailing
    /// slash; `""` for the server root.
    pub root: String,
    user: String,
    password: Option<String>,
    auth: Mutex<AuthState>,
}

impl Client {
    pub fn new(
        https: bool,
        host: &str,
        port: u16,
        root: &str,
        user: &str,
        password: Option<String>,
    ) -> Client {
        let default_port = if https { 443 } else { 80 };
        let host_part = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let origin = if port == 0 || port == default_port {
            format!("{}://{host_part}", if https { "https" } else { "http" })
        } else {
            format!(
                "{}://{host_part}:{port}",
                if https { "https" } else { "http" }
            )
        };
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .allow_non_standard_methods(true)
            .timeout_connect(Some(crate::CONNECT_TIMEOUT))
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(120)))
            .max_idle_connections_per_host(8)
            .user_agent(concat!("Manta/", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build();
        Client {
            agent: ureq::Agent::new_with_config(config),
            origin,
            host: host.to_string(),
            root: normalize_root(root),
            user: user.to_string(),
            password,
            auth: Mutex::new(AuthState::Unknown),
        }
    }

    /// The URL of `path` (a Manta path below the root); folders get a
    /// trailing slash.
    pub fn url(&self, path: &str, dir: bool) -> String {
        let p = path.trim_matches('/');
        let mut u = format!("{}{}", self.origin, encode_path(&self.root));
        if !p.is_empty() {
            u.push('/');
            u.push_str(&encode_path(p));
        }
        if dir || p.is_empty() {
            u.push('/');
        }
        u
    }

    /// Decoded server path of a URL made by [`Client::url`], without the
    /// trailing slash.
    pub fn server_path(&self, url: &str) -> String {
        href_path(url)
    }

    fn has_credentials(&self) -> bool {
        !self.user.is_empty() || self.password.is_some()
    }

    fn authorization(&self, method: &str, url: &str) -> Option<String> {
        let mut state = self.auth.lock().unwrap_or_else(|e| e.into_inner());
        let password = self.password.as_deref().unwrap_or("");
        match &mut *state {
            AuthState::Unknown => None,
            AuthState::Basic => Some(format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{password}", self.user))
            )),
            AuthState::Digest(d) => {
                let target = request_target(url);
                Some(d.authorization(&self.user, password, method, &target))
            }
        }
    }

    /// Take the server's challenges. True if the request is worth sending
    /// again (a scheme we can do, or a fresh nonce).
    fn accept_challenges(&self, resp: &Response) -> bool {
        let values: Vec<&str> = resp
            .headers()
            .get_all("www-authenticate")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        let challenges = parse_challenges(&values);
        let mut state = self.auth.lock().unwrap_or_else(|e| e.into_inner());
        let digest = challenges
            .iter()
            .filter(|c| c.scheme == "digest")
            .find_map(Digest::from_challenge);
        let new = if let Some(d) = digest {
            let stale = challenges.iter().any(|c| {
                c.scheme == "digest"
                    && c.param("stale")
                        .is_some_and(|s| s.eq_ignore_ascii_case("true"))
            });
            match &*state {
                // Same nonce refused again and not stale: wrong password.
                AuthState::Digest(old) if old.nonce == d.nonce && !stale => return false,
                _ => AuthState::Digest(d),
            }
        } else if challenges.iter().any(|c| c.scheme == "basic") {
            if *state == AuthState::Basic {
                return false;
            }
            AuthState::Basic
        } else {
            return false;
        };
        *state = new;
        true
    }

    /// Forget the negotiated authentication (reconnect).
    pub fn reset_auth(&self) {
        *self.auth.lock().unwrap_or_else(|e| e.into_inner()) = AuthState::Unknown;
    }

    /// Send a request with authentication, retries and one same-origin
    /// redirect. Any final status is returned (see [`Client::send`]).
    pub fn send_raw(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        mut body: Body<'_>,
        ctx: &str,
    ) -> Result<Response, VfsError> {
        let replayable = !matches!(body, Body::Stream(_));
        let mut url = url.to_string();
        let mut attempt = 0;
        let mut auth_rounds = 0;
        let mut redirected = false;
        loop {
            attempt += 1;
            let mut b = http::Request::builder().method(method).uri(&url);
            for (k, v) in headers {
                b = b.header(*k, v.as_str());
            }
            if let Some(a) = self.authorization(method, &url) {
                b = b.header("authorization", a);
            }
            let built = match &mut body {
                Body::None => b.body(()).map(|r| self.agent.run(r)),
                Body::Bytes(d) => {
                    let d: &[u8] = d;
                    b.body(d).map(|r| self.agent.run(r))
                }
                Body::Stream(_) => {
                    let Body::Stream(r) = std::mem::replace(&mut body, Body::None) else {
                        unreachable!()
                    };
                    b.body(ureq::SendBody::from_owned_reader(r))
                        .map(|r| self.agent.run(r))
                }
            };
            let result =
                built.map_err(|e| VfsError::Other(format!("{ctx}: invalid request: {e}")))?;
            let resp = match result {
                Ok(r) => r,
                Err(e) => {
                    let retry = matches!(
                        e,
                        ureq::Error::Io(_)
                            | ureq::Error::Timeout(_)
                            | ureq::Error::ConnectionFailed
                    );
                    if retry && replayable && attempt < MAX_ATTEMPTS {
                        log::debug!("WebDAV {method} {ctx}: {e}; retrying");
                        backoff(attempt);
                        continue;
                    }
                    return Err(self.transport_error(e));
                }
            };
            let status = resp.status().as_u16();
            if status == 401
                && self.has_credentials()
                && auth_rounds < 2
                && self.accept_challenges(&resp)
            {
                auth_rounds += 1;
                if replayable {
                    attempt -= 1;
                    continue;
                }
                // The streamed body is gone; the next attempt (a transfer
                // retry) signs right away.
                return Err(VfsError::Connection(format!(
                    "{ctx}: the server asked to sign in again; connection lost"
                )));
            }
            if matches!(status, 301 | 302 | 307 | 308) && replayable && !redirected {
                if let Some(loc) = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|l| self.same_origin(l))
                {
                    redirected = true;
                    url = loc;
                    attempt -= 1;
                    continue;
                }
            }
            let retry_status =
                (status >= 500 && !matches!(status, 501 | 505 | 507)) || status == 429;
            if retry_status && replayable && attempt < MAX_ATTEMPTS {
                log::debug!("WebDAV {method} {ctx}: HTTP {status}; retrying");
                backoff(attempt);
                continue;
            }
            return Ok(resp);
        }
    }

    /// [`Client::send_raw`], with every non-2xx status turned into an error.
    pub fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Body<'_>,
        ctx: &str,
    ) -> Result<Response, VfsError> {
        let resp = self.send_raw(method, url, headers, body, ctx)?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            Ok(resp)
        } else {
            Err(status_error(status, ctx))
        }
    }

    /// Send and read the whole (XML) response body.
    pub fn send_text(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Body<'_>,
        ctx: &str,
    ) -> Result<(u16, String), VfsError> {
        let mut resp = self.send(method, url, headers, body, ctx)?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .with_config()
            .limit(256 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| VfsError::Connection(format!("{ctx}: reading the reply failed: {e}")))?;
        Ok((status, text))
    }

    /// A redirect target on this server, as an absolute URL.
    fn same_origin(&self, location: &str) -> Option<String> {
        if location.starts_with('/') {
            Some(format!("{}{location}", self.origin))
        } else if location
            .to_ascii_lowercase()
            .starts_with(&format!("{}/", self.origin.to_ascii_lowercase()))
        {
            Some(location.to_string())
        } else {
            None
        }
    }

    fn transport_error(&self, e: ureq::Error) -> VfsError {
        let host = &self.host;
        match e {
            ureq::Error::HostNotFound => VfsError::Connection(format!("cannot resolve {host}")),
            ureq::Error::Timeout(t) => VfsError::Connection(format!("{host}: timed out ({t})")),
            ureq::Error::ConnectionFailed => {
                VfsError::Connection(format!("cannot connect to {host}: connection refused"))
            }
            ureq::Error::Io(io) => VfsError::Connection(format!("{host}: {io}")),
            ureq::Error::Tls(m) => VfsError::Other(format!("{host}: TLS: {m}")),
            ureq::Error::Rustls(r) => VfsError::Other(format!("{host}: TLS: {r}")),
            other => VfsError::Connection(format!("{host}: {other}")),
        }
    }
}

/// `/remote.php/dav/files/alice` from user input such as
/// `remote.php/dav/files/alice/` or `/dav%20files`.
pub fn normalize_root(root: &str) -> String {
    let decoded = decode(root.trim());
    let parts: Vec<&str> = decoded
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Path and query of a URL (the request target Digest signs).
fn request_target(url: &str) -> String {
    match url.find("://") {
        Some(i) => {
            let rest = &url[i + 3..];
            rest.find('/')
                .map_or_else(|| "/".to_string(), |j| rest[j..].to_string())
        }
        None => url.to_string(),
    }
}

fn backoff(attempt: u32) {
    let base = BACKOFF_BASE * 2u32.pow(attempt.saturating_sub(1).min(5));
    let jitter = Duration::from_millis(u64::from(
        chrono::Utc::now().timestamp_subsec_millis() % 100,
    ));
    std::thread::sleep(base + jitter);
}

/// Map an HTTP error status to a [`VfsError`] about `ctx`.
pub fn status_error(status: u16, ctx: &str) -> VfsError {
    match status {
        401 => VfsError::Auth(format!(
            "{ctx}: authentication failed (HTTP 401): user name or password refused"
        )),
        403 => VfsError::PermissionDenied(format!("{ctx}: forbidden (HTTP 403)")),
        404 | 410 => VfsError::NotFound(ctx.to_string()),
        409 => VfsError::NotFound(format!(
            "{ctx}: the parent folder does not exist (HTTP 409)"
        )),
        412 => VfsError::AlreadyExists(ctx.to_string()),
        423 => VfsError::PermissionDenied(format!("{ctx}: locked on the server (HTTP 423)")),
        507 => VfsError::Other(format!(
            "{ctx}: no space left on the server, quota exceeded (HTTP 507)"
        )),
        405 => VfsError::Unsupported(format!("{ctx}: not allowed by the server (HTTP 405)")),
        s => VfsError::Other(format!("{ctx}: HTTP {s} {}", reason(s))),
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenges() {
        let c = parse_challenges(&[
            r#"Digest realm="files@example.com", qop="auth,auth-int", nonce="dcd98b", opaque="5ccc", algorithm=MD5"#,
            r#"Basic realm="x y""#,
        ]);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].scheme, "digest");
        assert_eq!(c[0].param("realm"), Some("files@example.com"));
        assert_eq!(c[0].param("qop"), Some("auth,auth-int"));
        assert_eq!(c[0].param("algorithm"), Some("MD5"));
        assert_eq!(c[1].scheme, "basic");
        assert_eq!(c[1].param("realm"), Some("x y"));
        // Several challenges in one header value.
        let c = parse_challenges(&[r#"Basic realm="a", Digest realm="b", nonce="n""#]);
        assert_eq!(
            c.iter().map(|c| c.scheme.as_str()).collect::<Vec<_>>(),
            ["basic", "digest"]
        );
        assert_eq!(c[1].param("nonce"), Some("n"));
        let c = parse_challenges(&["Negotiate abc==, Basic realm=\"r\""]);
        assert_eq!(c.len(), 2);
    }

    /// RFC 2617 section 3.5 example.
    #[test]
    fn digest_rfc2617_example() {
        let mut d = Digest {
            realm: "testrealm@host.com".into(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".into(),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".into()),
            algorithm: "MD5".into(),
            qop: Some("auth".into()),
            nc: 0,
        };
        let ha1 = d.hash("Mufasa:testrealm@host.com:Circle Of Life");
        let ha2 = d.hash("GET:/dir/index.html");
        let expected = d.hash(&format!(
            "{ha1}:dcd98b7102dd2f0e8b11d0f600bfb0c093:00000001:0a4f113b:auth:{ha2}"
        ));
        assert_eq!(expected, "6629fae49393a05397450978507c4ef1");
        let h = d.authorization("Mufasa", "Circle Of Life", "GET", "/dir/index.html");
        assert!(h.starts_with("Digest username=\"Mufasa\""), "{h}");
        assert!(h.contains("nc=00000001") && h.contains("qop=auth"), "{h}");
        assert!(h.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));
        let h2 = d.authorization("Mufasa", "Circle Of Life", "GET", "/dir/index.html");
        assert!(h2.contains("nc=00000002"), "{h2}");
    }

    #[test]
    fn sha256_digest() {
        let c =
            parse_challenges(&[r#"Digest realm="r", nonce="n", algorithm=SHA-256, qop="auth""#]);
        let d = Digest::from_challenge(&c[0]).unwrap();
        assert_eq!(d.hash("abc").len(), 64);
        let c = parse_challenges(&[r#"Digest realm="r", nonce="n", algorithm=SHA-512-256"#]);
        assert!(Digest::from_challenge(&c[0]).is_none());
    }

    #[test]
    fn urls_and_paths() {
        let c = Client::new(
            true,
            "cloud.example.com",
            443,
            "remote.php/dav/files/al ice/",
            "al ice",
            None,
        );
        assert_eq!(c.root, "/remote.php/dav/files/al ice");
        assert_eq!(
            c.url("/", true),
            "https://cloud.example.com/remote.php/dav/files/al%20ice/"
        );
        assert_eq!(
            c.url("/Fotos/Ü 1.jpg", false),
            "https://cloud.example.com/remote.php/dav/files/al%20ice/Fotos/%C3%9C%201.jpg"
        );
        assert_eq!(
            c.url("/Fotos", true),
            "https://cloud.example.com/remote.php/dav/files/al%20ice/Fotos/"
        );
        let c = Client::new(false, "nas", 8080, "", "", None);
        assert_eq!(c.url("/a#b?c", false), "http://nas:8080/a%23b%3Fc");
        assert_eq!(c.url("/", true), "http://nas:8080/");
        let c = Client::new(false, "::1", 8080, "/dav", "", None);
        assert_eq!(c.url("/", true), "http://[::1]:8080/dav/");

        assert_eq!(href_path("/dav/a%20b/"), "/dav/a b");
        assert_eq!(href_path("https://h:1/dav/x?y=1"), "/dav/x");
        assert_eq!(href_path("http://h"), "");
        assert_eq!(decode("%E2%82%AC%2"), "€%2");
        assert_eq!(normalize_root("%2Fdav%20x//y/"), "/dav x/y");
        assert_eq!(request_target("http://h:1/a/b?c"), "/a/b?c");
    }

    #[test]
    fn status_mapping() {
        assert!(matches!(status_error(404, "x"), VfsError::NotFound(_)));
        assert!(matches!(status_error(401, "x"), VfsError::Auth(_)));
        assert!(matches!(
            status_error(403, "x"),
            VfsError::PermissionDenied(_)
        ));
        assert!(matches!(status_error(412, "x"), VfsError::AlreadyExists(_)));
        assert!(status_error(503, "x").is_transient());
        assert!(!status_error(507, "x").is_transient());
        assert!(!status_error(400, "x").is_transient());
    }
}
