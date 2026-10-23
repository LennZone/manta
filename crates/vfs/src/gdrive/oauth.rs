//! OAuth 2.0 for installed apps (RFC 8252): authorization code flow with
//! PKCE (RFC 7636, `S256`) and a loopback redirect on `127.0.0.1:<random>`.
//!
//! [`authorize`] is blocking (call it on a background thread): it binds the
//! loopback port, hands the consent URL to `open_url` (the UI opens the
//! system browser), waits for Google to redirect the browser back with the
//! authorization code, answers with a small "return to Manta" page and
//! exchanges the code for tokens. [`authorize_with`] takes an [`AuthCancel`]
//! and a timeout (default [`AUTHORIZE_TIMEOUT`], 5 minutes).
//!
//! Tokens are never logged; `Debug` of [`Tokens`] and [`OAuthClient`]
//! redacts the secrets.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use manta_core::{VfsError, VfsResult};
use serde::Deserialize;
use sha2::Digest;

/// Full Drive access. A *restricted* scope: an app that is used by more than
/// the 100 test users of its OAuth consent screen needs Google's
/// verification (incl. a security assessment), see docs/cloud.md.
pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

/// How long [`authorize`] waits for the browser.
pub const AUTHORIZE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What the UI shows when the user has set up no OAuth client of their own.
pub const NOT_CONFIGURED: &str = "Google Drive sign-in needs your own OAuth client ID (Settings → General → Cloud → Expert); or use Google Drive for desktop";

/// Where the OAuth and Drive endpoints live. Tests point these at a fake
/// server on 127.0.0.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// Consent page, e.g. `https://accounts.google.com/o/oauth2/v2/auth`.
    pub auth_url: String,
    /// Token endpoint, e.g. `https://oauth2.googleapis.com/token`.
    pub token_url: String,
    /// Token revocation endpoint.
    pub revoke_url: String,
    /// API base for `/drive/v3/...` and `/upload/drive/v3/...`,
    /// e.g. `https://www.googleapis.com`.
    pub api_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            revoke_url: "https://oauth2.googleapis.com/revoke".into(),
            api_base: "https://www.googleapis.com".into(),
        }
    }
}

impl Endpoints {
    /// All endpoints on one test server, e.g. `http://127.0.0.1:4711`.
    pub fn local(base: &str) -> Endpoints {
        let base = base.trim_end_matches('/');
        Endpoints {
            auth_url: format!("{base}/o/oauth2/v2/auth"),
            token_url: format!("{base}/token"),
            revoke_url: format!("{base}/revoke"),
            api_base: base.to_string(),
        }
    }
}

/// Where the client id came from (for the settings / about UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientSource {
    /// `MANTA_GDRIVE_CLIENT_ID` in the environment of the running app.
    RuntimeEnv,
    /// `<config dir>/Manta/gdrive-client.json` (Settings → Cloud → Expert:
    /// the user's own client).
    ConfigFile,
    /// Set in code ([`OAuthClient::new`]).
    Explicit,
}

/// A Google OAuth client of type "Desktop app". Its secret is not
/// confidential (Google says so for installed apps); PKCE protects the flow.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
    pub endpoints: Endpoints,
    pub source: ClientSource,
}

impl fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthClient")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("endpoints", &self.endpoints)
            .field("source", &self.source)
            .finish()
    }
}

/// Runtime override file, `{"client_id": "...", "client_secret": "..."}`.
#[derive(Deserialize)]
struct ClientFile {
    client_id: String,
    #[serde(default)]
    client_secret: String,
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl OAuthClient {
    pub fn new(client_id: &str, client_secret: &str) -> OAuthClient {
        OAuthClient {
            client_id: client_id.trim().to_string(),
            client_secret: client_secret.trim().to_string(),
            endpoints: Endpoints::default(),
            source: ClientSource::Explicit,
        }
    }

    pub fn with_endpoints(mut self, endpoints: Endpoints) -> OAuthClient {
        self.endpoints = endpoints;
        self
    }

    /// Path of the runtime override file:
    /// `~/Library/Application Support/Manta/gdrive-client.json`.
    pub fn config_file() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|d| d.join("Manta").join("gdrive-client.json"))
    }

    /// The client the user set up, first match wins:
    /// 1. `MANTA_GDRIVE_CLIENT_ID` / `MANTA_GDRIVE_CLIENT_SECRET` in the
    ///    environment of the running app (development),
    /// 2. [`OAuthClient::config_file`] (Settings → Cloud → Expert).
    ///
    /// Manta never ships a client of its own: without one of these, Google
    /// Drive is offered only through Google Drive for desktop (the File
    /// Provider folder under `~/Library/CloudStorage`). `None`: not set up
    /// ([`NOT_CONFIGURED`]).
    pub fn configured() -> Option<OAuthClient> {
        let env = |k: &str| nonempty(std::env::var(k).ok());
        if let Some(id) = env("MANTA_GDRIVE_CLIENT_ID") {
            let secret = env("MANTA_GDRIVE_CLIENT_SECRET").unwrap_or_default();
            return Some(OAuthClient {
                source: ClientSource::RuntimeEnv,
                ..OAuthClient::new(&id, &secret)
            });
        }
        Self::from_file(&Self::config_file()?)
    }

    /// The client saved in `path` (`{"client_id": …, "client_secret": …}`);
    /// `None` if the file is missing, unreadable or has no id.
    pub fn from_file(path: &std::path::Path) -> Option<OAuthClient> {
        let c = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<ClientFile>(&t).ok())
            .filter(|c| !c.client_id.trim().is_empty())?;
        Some(OAuthClient {
            source: ClientSource::ConfigFile,
            ..OAuthClient::new(&c.client_id, &c.client_secret)
        })
    }

    /// Save the user's own client to `path` (owner-only permissions), or
    /// remove the file when `client_id` is empty.
    pub fn save_file(
        path: &std::path::Path,
        client_id: &str,
        client_secret: &str,
    ) -> std::io::Result<()> {
        if client_id.trim().is_empty() {
            return match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::json!({ "client_id": client_id.trim(), "client_secret": client_secret.trim() });
        std::fs::write(path, serde_json::to_vec_pretty(&json).unwrap_or_default())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

/// Is an OAuth client available ([`OAuthClient::configured`])?
pub fn is_configured() -> bool {
    OAuthClient::configured().is_some()
}

/// The result of a successful sign-in.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// When `access_token` expires (local clock).
    pub expires_at: Instant,
    /// The Google account's email address (from Drive `about.get`).
    pub email: String,
    /// The account's display name ("" if unknown).
    pub display_name: String,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .finish()
    }
}

/// Cancels a running [`authorize_with`] from another thread (the UI's
/// "Cancel" button). Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct AuthCancel(Arc<AtomicBool>);

impl AuthCancel {
    pub fn new() -> AuthCancel {
        AuthCancel::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Sign in with the system browser; blocking, at most
/// [`AUTHORIZE_TIMEOUT`]. `open_url` receives the consent URL and should
/// open it in the default browser (`open <url>`).
pub fn authorize(client: &OAuthClient, open_url: impl Fn(&str)) -> VfsResult<Tokens> {
    authorize_with(client, open_url, &AuthCancel::new(), AUTHORIZE_TIMEOUT)
}

/// [`authorize`] with a cancel handle and a timeout. Returns
/// `VfsError::Cancelled` when cancelled, `VfsError::Auth` when the user
/// denied access or the code exchange failed, `VfsError::Other` on timeout.
pub fn authorize_with(
    client: &OAuthClient,
    open_url: impl Fn(&str),
    cancel: &AuthCancel,
    timeout: Duration,
) -> VfsResult<Tokens> {
    if client.client_id.is_empty() {
        return Err(VfsError::Unsupported(NOT_CONFIGURED.into()));
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| VfsError::Other(format!("cannot open the sign-in port on 127.0.0.1: {e}")))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = random_token(48);
    let challenge = pkce_challenge(&verifier);
    let state = random_token(24);
    let url = consent_url(client, &redirect_uri, &challenge, &state);
    open_url(&url);

    let code = wait_for_code(&listener, &state, cancel, Instant::now() + timeout)?;
    drop(listener);
    let agent = http_agent();
    let resp = token_request(
        &agent,
        client,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", &verifier),
            ("client_id", &client.client_id),
            ("client_secret", &client.client_secret),
        ],
    )?;
    let refresh_token = resp
        .refresh_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            VfsError::Auth(
            "Google returned no refresh token; remove Manta at myaccount.google.com/permissions \
             and sign in again"
                .into(),
        )
        })?;
    let (email, display_name) = fetch_user(&agent, &client.endpoints, &resp.access_token)?;
    Ok(Tokens {
        access_token: resp.access_token,
        refresh_token,
        expires_at: Instant::now() + Duration::from_secs(resp.expires_in.unwrap_or(3600)),
        email,
        display_name,
    })
}

/// The consent page URL.
pub(crate) fn consent_url(
    client: &OAuthClient,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> String {
    let q = [
        ("client_id", client.client_id.as_str()),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", DRIVE_SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        // A refresh token every time (also when the user consented before).
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    format!("{}?{}", client.endpoints.auth_url, form_encode(&q))
}

/// `BASE64URL(SHA256(verifier))` without padding (RFC 7636 `S256`).
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()))
}

/// URL-safe random string from `bytes` random bytes.
pub(crate) fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if getrandom::fill(&mut buf).is_err() {
        // Practically unreachable on macOS; still never fall back to zeros.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let digest = sha2::Sha256::digest(format!("{t}-{:p}", &buf).as_bytes());
        for (i, b) in buf.iter_mut().enumerate() {
            *b = digest[i % digest.len()];
        }
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

const SUCCESS_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Manta</title>\
<style>body{font:15px -apple-system,BlinkMacSystemFont,sans-serif;display:flex;align-items:center;\
justify-content:center;height:90vh;margin:0;color:#1d1d1f;background:#f5f5f7}\
@media(prefers-color-scheme:dark){body{color:#f5f5f7;background:#1d1d1f}}\
main{text-align:center;max-width:28em}h1{font-size:20px;font-weight:600}p{opacity:.75}</style></head>\
<body><main><h1>Angemeldet bei Google Drive</h1><p>Du kannst dieses Fenster schließen und zu Manta zurückkehren.</p>\
<h1 lang=\"en\">Signed in to Google Drive</h1><p lang=\"en\">You can close this window and return to Manta.</p></main></body></html>";

fn failure_page(reason: &str) -> String {
    let reason = reason
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Manta</title></head>\
         <body style=\"font:15px -apple-system,sans-serif;text-align:center;margin-top:20vh\">\
         <h1 style=\"font-size:20px\">Anmeldung fehlgeschlagen · Sign-in failed</h1><p>{reason}</p>\
         <p>Du kannst dieses Fenster schließen und zu Manta zurückkehren. · You can close this window \
         and return to Manta.</p></body></html>"
    )
}

/// Accept browser requests on the loopback port until one carries the
/// authorization code (or an error) for our `state`.
fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &AuthCancel,
    deadline: Instant,
) -> VfsResult<String> {
    loop {
        if cancel.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(VfsError::Other(
                "the Google sign-in timed out; please try again".into(),
            ));
        }
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(VfsError::Io(e)),
        };
        match handle_redirect(stream, state) {
            Redirect::Code(c) => return Ok(c),
            Redirect::Denied(e) => {
                let msg = if e == "access_denied" {
                    "access to Google Drive was denied in the browser".to_string()
                } else {
                    format!("Google sign-in failed: {e}")
                };
                return Err(VfsError::Auth(msg));
            }
            Redirect::Ignored => {}
        }
    }
}

enum Redirect {
    Code(String),
    Denied(String),
    /// favicon, a stale tab with an old `state`, garbage, ...
    Ignored,
}

fn handle_redirect(mut stream: TcpStream, state: &str) -> Redirect {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&head);
    let target = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params = parse_query(query);
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let (status, body, result) = match (get("state"), get("code"), get("error")) {
        (Some(s), Some(code), _) if s == state => {
            ("200 OK", SUCCESS_PAGE.to_string(), Redirect::Code(code))
        }
        (Some(s), None, Some(err)) if s == state => {
            ("200 OK", failure_page(&err), Redirect::Denied(err))
        }
        (Some(_), _, _) => (
            "400 Bad Request",
            failure_page("This sign-in link is outdated; start the sign-in in Manta again."),
            Redirect::Ignored,
        ),
        _ => ("404 Not Found", String::new(), Redirect::Ignored),
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
    result
}

#[derive(Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Deserialize, Default)]
struct OAuthError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

/// POST to the token endpoint (code exchange or refresh).
pub(crate) fn token_request(
    agent: &ureq::Agent,
    client: &OAuthClient,
    form: &[(&str, &str)],
) -> VfsResult<TokenResponse> {
    let body = form_encode(form);
    let req = ureq::http::Request::builder()
        .method("POST")
        .uri(&client.endpoints.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body.as_bytes())
        .map_err(|e| VfsError::Other(format!("token request: {e}")))?;
    let mut resp = agent
        .run(req)
        .map_err(|e| VfsError::Connection(format!("cannot reach Google's sign-in service: {e}")))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .unwrap_or_default();
    if (200..300).contains(&status) {
        return serde_json::from_str(&text).map_err(|e| {
            VfsError::Other(format!(
                "unexpected answer from Google's token service: {e}"
            ))
        });
    }
    let err: OAuthError = serde_json::from_str(&text).unwrap_or_default();
    Err(match err.error.as_str() {
        "invalid_grant" => VfsError::Auth(
            "the Google sign-in has expired or was revoked; please sign in to Google Drive again"
                .into(),
        ),
        "invalid_client" | "unauthorized_client" => VfsError::Auth(format!(
            "Google rejected Manta's OAuth client ({}); check the client id and secret",
            if err.error_description.is_empty() {
                &err.error
            } else {
                &err.error_description
            }
        )),
        _ if status >= 500 => {
            VfsError::Connection(format!("Google's sign-in service failed (HTTP {status})"))
        }
        "" => VfsError::Auth(format!("Google sign-in failed (HTTP {status})")),
        e => VfsError::Auth(
            format!("Google sign-in failed: {e} {}", err.error_description)
                .trim()
                .to_string(),
        ),
    })
}

/// Exchange a refresh token for a new access token. Returns
/// (access token, lifetime, rotated refresh token if Google sent one).
pub(crate) fn refresh_access_token(
    agent: &ureq::Agent,
    client: &OAuthClient,
    refresh_token: &str,
) -> VfsResult<(String, Duration, Option<String>)> {
    let r = token_request(
        agent,
        client,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &client.client_id),
            ("client_secret", &client.client_secret),
        ],
    )?;
    Ok((
        r.access_token,
        Duration::from_secs(r.expires_in.unwrap_or(3600)),
        r.refresh_token,
    ))
}

/// Revoke a token (sign out). Best effort.
pub(crate) fn revoke(client: &OAuthClient, token: &str) -> VfsResult<()> {
    let agent = http_agent();
    let body = form_encode(&[("token", token)]);
    let req = ureq::http::Request::builder()
        .method("POST")
        .uri(&client.endpoints.revoke_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body.as_bytes())
        .map_err(|e| VfsError::Other(format!("revoke: {e}")))?;
    let resp = agent
        .run(req)
        .map_err(|e| VfsError::Connection(format!("revoke: {e}")))?;
    match resp.status().as_u16() {
        200..=299 | 400 => Ok(()), // 400: already invalid
        s => Err(VfsError::Other(format!(
            "revoking the Google token failed (HTTP {s})"
        ))),
    }
}

#[derive(Deserialize)]
struct About {
    user: AboutUser,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutUser {
    #[serde(default)]
    email_address: String,
    #[serde(default)]
    display_name: String,
}

/// The signed-in account (`about.get`).
pub(crate) fn fetch_user(
    agent: &ureq::Agent,
    ep: &Endpoints,
    access_token: &str,
) -> VfsResult<(String, String)> {
    let url = format!(
        "{}/drive/v3/about?fields=user(emailAddress%2CdisplayName)",
        ep.api_base
    );
    let req = ureq::http::Request::builder()
        .method("GET")
        .uri(&url)
        .header("authorization", format!("Bearer {access_token}"))
        .body(())
        .map_err(|e| VfsError::Other(format!("about: {e}")))?;
    let mut resp = agent
        .run(req)
        .map_err(|e| VfsError::Connection(format!("Google Drive: {e}")))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .unwrap_or_default();
    if status == 403 && text.contains("accessNotConfigured") {
        return Err(VfsError::Auth(
            "the Google Drive API is not enabled for Manta's Google Cloud project".into(),
        ));
    }
    if !(200..300).contains(&status) {
        return Err(VfsError::Auth(format!(
            "Google Drive refused the new sign-in (HTTP {status})"
        )));
    }
    let about: About = serde_json::from_str(&text)
        .map_err(|e| VfsError::Other(format!("unexpected Drive answer: {e}")))?;
    if about.user.email_address.is_empty() {
        return Err(VfsError::Auth(
            "Google did not tell which account signed in".into(),
        ));
    }
    Ok((about.user.email_address, about.user.display_name))
}

/// The HTTP agent shared by the OAuth calls and the Drive client.
pub(crate) fn http_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_connect(Some(crate::CONNECT_TIMEOUT))
        .timeout_send_request(Some(Duration::from_secs(60)))
        .timeout_recv_response(Some(Duration::from_secs(90)))
        .max_idle_connections_per_host(8)
        .user_agent(concat!("Manta/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

/// Percent-encode everything but RFC 3986 unreserved characters.
pub(crate) fn pct_encode(s: &str) -> String {
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

pub(crate) fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                    16,
                ) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub(crate) fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (pct_decode(k), pct_decode(v)),
            None => (pct_decode(p), String::new()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_vector() {
        // RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_tokens_differ_and_are_url_safe() {
        let a = random_token(48);
        let b = random_token(48);
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    }

    #[test]
    fn query_roundtrip() {
        let enc = form_encode(&[("scope", DRIVE_SCOPE), ("x", "a b&c=d/ä")]);
        assert_eq!(
            parse_query(&enc),
            vec![
                ("scope".to_string(), DRIVE_SCOPE.to_string()),
                ("x".to_string(), "a b&c=d/ä".to_string())
            ]
        );
        assert_eq!(pct_decode("100%"), "100%");
        assert_eq!(pct_decode("a+b%2"), "a b%2");
    }

    #[test]
    fn consent_url_has_pkce_and_offline() {
        let c = OAuthClient::new("id.apps.googleusercontent.com", "s");
        let u = consent_url(&c, "http://127.0.0.1:5000", "chal", "st");
        assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        let q = parse_query(u.split_once('?').unwrap().1);
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("code_challenge_method"), Some("S256"));
        assert_eq!(get("code_challenge"), Some("chal"));
        assert_eq!(get("access_type"), Some("offline"));
        assert_eq!(get("scope"), Some(DRIVE_SCOPE));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:5000"));
    }

    #[test]
    fn secrets_are_redacted_in_debug() {
        let c = OAuthClient::new("id", "very-secret");
        assert!(!format!("{c:?}").contains("very-secret"));
        let t = Tokens {
            access_token: "ya29.secret".into(),
            refresh_token: "1//refresh".into(),
            expires_at: Instant::now(),
            email: "a@b.c".into(),
            display_name: String::new(),
        };
        let d = format!("{t:?}");
        assert!(!d.contains("ya29") && !d.contains("1//") && d.contains("a@b.c"));
    }

    #[test]
    fn cancel_stops_waiting() {
        let c = OAuthClient::new("id", "s").with_endpoints(Endpoints::local("http://127.0.0.1:9"));
        let cancel = AuthCancel::new();
        let c2 = cancel.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c2.cancel();
        });
        let start = Instant::now();
        let r = authorize_with(&c, |_| {}, &cancel, Duration::from_secs(30));
        t.join().unwrap();
        assert!(matches!(r, Err(VfsError::Cancelled)), "{r:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn times_out() {
        let c = OAuthClient::new("id", "s");
        let r = authorize_with(&c, |_| {}, &AuthCancel::new(), Duration::from_millis(100));
        assert!(
            matches!(r, Err(VfsError::Other(ref m)) if m.contains("timed out")),
            "{r:?}"
        );
    }

    #[test]
    fn client_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Manta/gdrive-client.json");
        assert!(OAuthClient::from_file(&path).is_none());
        OAuthClient::save_file(&path, " my-id.apps.googleusercontent.com ", "sec").unwrap();
        let c = OAuthClient::from_file(&path).unwrap();
        assert_eq!(
            (c.client_id.as_str(), c.client_secret.as_str()),
            ("my-id.apps.googleusercontent.com", "sec")
        );
        assert_eq!(c.source, ClientSource::ConfigFile);
        OAuthClient::save_file(&path, "  ", "").unwrap();
        assert!(!path.exists() && OAuthClient::from_file(&path).is_none());
        OAuthClient::save_file(&path, "", "").unwrap();
    }

    #[test]
    fn no_client_id_is_not_configured() {
        let c = OAuthClient::new("", "");
        let r = authorize(&c, |_| panic!("must not open a browser"));
        assert!(matches!(r, Err(VfsError::Unsupported(ref m)) if m == NOT_CONFIGURED));
    }
}
