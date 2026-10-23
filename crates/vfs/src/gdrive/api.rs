//! Drive API v3 transport: bearer tokens with automatic refresh (on expiry
//! and on HTTP 401), retries with exponential backoff (429, 5xx, Drive's
//! `rateLimitExceeded` / `userRateLimitExceeded`), and mapping of Drive's
//! JSON errors to [`VfsError`] with friendly text.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use manta_core::{VfsError, VfsResult};
use serde::Deserialize;
use ureq::http;

use super::oauth::{self, OAuthClient};
use super::store::TokenStore;

pub(crate) type Response = http::Response<ureq::Body>;

/// Retry behaviour; tests shorten the delays.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    /// Attempts per request (1 + retries).
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 6,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(32),
        }
    }
}

impl RetryPolicy {
    fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(d) = retry_after {
            return d.min(Duration::from_secs(60));
        }
        let exp = self
            .base_delay
            .saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1).min(10)));
        let jitter_ms = u64::from(chrono::Utc::now().timestamp_subsec_millis() % 250);
        let jitter = Duration::from_millis(jitter_ms).min(self.base_delay);
        exp.min(self.max_delay) + jitter
    }
}

/// Request body.
pub(crate) enum Body<'a> {
    Empty,
    Json(String),
    Bytes(&'a [u8]),
}

/// One API call. `url` is absolute (API base + path, or an upload session URI).
pub(crate) struct Call<'a> {
    pub method: &'static str,
    pub url: String,
    pub query: Vec<(&'static str, String)>,
    pub headers: Vec<(&'static str, String)>,
    pub body: Body<'a>,
    /// Treat `308 Resume Incomplete` as success (resumable uploads).
    pub accept_308: bool,
    /// Don't retry 5xx (the caller recovers, e.g. upload chunks re-query).
    pub no_retry: bool,
}

impl<'a> Call<'a> {
    pub fn new(method: &'static str, url: String) -> Call<'a> {
        Call {
            method,
            url,
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::Empty,
            accept_308: false,
            no_retry: false,
        }
    }
    pub fn query(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.query.push((k, v.into()));
        self
    }
    pub fn header(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.headers.push((k, v.into()));
        self
    }
    pub fn json(mut self, v: &serde_json::Value) -> Self {
        self.body = Body::Json(v.to_string());
        self
    }
    pub fn bytes(mut self, b: &'a [u8]) -> Self {
        self.body = Body::Bytes(b);
        self
    }
    fn full_url(&self) -> String {
        if self.query.is_empty() {
            return self.url.clone();
        }
        let sep = if self.url.contains('?') { '&' } else { '?' };
        let q: Vec<String> = self
            .query
            .iter()
            .map(|(k, v)| format!("{}={}", k, oauth::pct_encode(v)))
            .collect();
        format!("{}{sep}{}", self.url, q.join("&"))
    }
}

struct Access {
    token: String,
    expires_at: Instant,
}

/// Authenticated Drive API client, shared by the backend and its writers.
pub(crate) struct Api {
    agent: ureq::Agent,
    pub client: OAuthClient,
    pub email: String,
    refresh_token: Mutex<String>,
    access: Mutex<Option<Access>>,
    store: Arc<dyn TokenStore>,
    pub retry: RetryPolicy,
}

impl Api {
    pub fn new(
        client: OAuthClient,
        email: String,
        refresh_token: String,
        access: Option<(String, Instant)>,
        store: Arc<dyn TokenStore>,
    ) -> Api {
        Api {
            agent: oauth::http_agent(),
            client,
            email,
            refresh_token: Mutex::new(refresh_token),
            access: Mutex::new(access.map(|(token, expires_at)| Access { token, expires_at })),
            store,
            retry: RetryPolicy::default(),
        }
    }

    pub fn base(&self) -> &str {
        &self.client.endpoints.api_base
    }

    /// A valid access token, refreshed when it expires within a minute.
    /// `stale`: the token that just got a 401, forces a refresh.
    fn access_token(&self, stale: Option<&str>) -> VfsResult<String> {
        let mut guard = self.access.lock().unwrap();
        if let Some(a) = guard.as_ref() {
            let fresh = a.expires_at > Instant::now() + Duration::from_secs(60);
            if fresh && stale != Some(a.token.as_str()) {
                return Ok(a.token.clone());
            }
        }
        let refresh = self.refresh_token.lock().unwrap().clone();
        let (token, ttl, rotated) =
            oauth::refresh_access_token(&self.agent, &self.client, &refresh)?;
        if let Some(r) = rotated.filter(|r| !r.is_empty() && *r != refresh) {
            if let Err(e) = self.store.save(&self.email, &r) {
                log::warn!("Google Drive: storing the rotated refresh token failed: {e}");
            }
            *self.refresh_token.lock().unwrap() = r;
        }
        log::debug!("Google Drive: access token refreshed for {}", self.email);
        *guard = Some(Access {
            token: token.clone(),
            expires_at: Instant::now() + ttl,
        });
        Ok(token)
    }

    /// Verify the sign-in now (refresh if needed).
    pub fn ensure_token(&self) -> VfsResult<()> {
        self.access_token(None).map(|_| ())
    }

    /// Send `call`, retrying transient failures and refreshing the access
    /// token once on 401. Only 2xx (and 308 if accepted) come back as `Ok`.
    pub fn send(&self, call: &Call<'_>, ctx: &str) -> VfsResult<Response> {
        let url = call.full_url();
        let mut attempt = 0;
        let mut stale: Option<String> = None;
        let mut refreshed = false;
        loop {
            attempt += 1;
            let token = self.access_token(stale.as_deref())?;
            let mut b = http::Request::builder()
                .method(call.method)
                .uri(&url)
                .header("authorization", format!("Bearer {token}"));
            for (k, v) in &call.headers {
                b = b.header(*k, v.as_str());
            }
            let result = match &call.body {
                Body::Empty if matches!(call.method, "GET" | "DELETE" | "HEAD") => {
                    b.body(()).map(|r| self.agent.run(r))
                }
                Body::Empty => b.body(&[][..]).map(|r| self.agent.run(r)),
                Body::Json(s) => b
                    .header("content-type", "application/json; charset=UTF-8")
                    .body(s.as_bytes())
                    .map(|r| self.agent.run(r)),
                Body::Bytes(d) => b.body(*d).map(|r| self.agent.run(r)),
            };
            let result =
                result.map_err(|e| VfsError::Other(format!("{ctx}: invalid request: {e}")))?;
            let mut resp = match result {
                Ok(r) => r,
                Err(e) => {
                    let transient = matches!(
                        e,
                        ureq::Error::Io(_)
                            | ureq::Error::Timeout(_)
                            | ureq::Error::ConnectionFailed
                    );
                    if transient && !call.no_retry && attempt < self.retry.max_attempts {
                        log::debug!("Google Drive {} {ctx}: {e}; retrying", call.method);
                        std::thread::sleep(self.retry.delay(attempt, None));
                        continue;
                    }
                    return Err(transport_error(e, &self.client.endpoints.api_base));
                }
            };
            let status = resp.status().as_u16();
            if (200..300).contains(&status) || (status == 308 && call.accept_308) {
                return Ok(resp);
            }
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            let text = if call.method == "HEAD" {
                String::new()
            } else {
                resp.body_mut()
                    .with_config()
                    .limit(1024 * 1024)
                    .read_to_string()
                    .unwrap_or_default()
            };
            let err = parse_error(&text);
            if status == 401 && !refreshed {
                refreshed = true;
                stale = Some(token);
                attempt -= 1;
                continue;
            }
            let rate_limited = status == 429
                || (status == 403
                    && matches!(
                        err.reason.as_str(),
                        "rateLimitExceeded" | "userRateLimitExceeded"
                    ));
            let server = matches!(status, 500 | 502 | 503 | 504);
            if (rate_limited || (server && !call.no_retry)) && attempt < self.retry.max_attempts {
                log::debug!(
                    "Google Drive {} {ctx}: HTTP {status} {}; backing off",
                    call.method,
                    err.reason
                );
                std::thread::sleep(self.retry.delay(attempt, retry_after));
                continue;
            }
            return Err(map_error(status, &err.reason, &err.message, ctx));
        }
    }

    /// Send and parse the JSON answer.
    pub fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        call: &Call<'_>,
        ctx: &str,
    ) -> VfsResult<T> {
        let mut resp = self.send(call, ctx)?;
        let text = resp
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| VfsError::Connection(format!("{ctx}: reading the answer failed: {e}")))?;
        serde_json::from_str(&text)
            .map_err(|e| VfsError::Other(format!("{ctx}: unexpected answer from Drive: {e}")))
    }
}

fn transport_error(e: ureq::Error, base: &str) -> VfsError {
    let host = base
        .split("://")
        .nth(1)
        .unwrap_or(base)
        .trim_end_matches('/')
        .to_string();
    match e {
        ureq::Error::HostNotFound => {
            VfsError::Connection(format!("cannot resolve {host} (offline?)"))
        }
        ureq::Error::Timeout(t) => VfsError::Connection(format!("Google Drive timed out ({t})")),
        ureq::Error::ConnectionFailed => VfsError::Connection(format!("cannot connect to {host}")),
        ureq::Error::Io(io) => VfsError::Connection(format!("Google Drive: {io}")),
        other => VfsError::Connection(format!("Google Drive: {other}")),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ApiError {
    pub reason: String,
    pub message: String,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    errors: Vec<ErrorItem>,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct ErrorItem {
    #[serde(default)]
    reason: String,
}

/// Drive's `{"error": {"errors": [{"reason": ...}], "message": ...}}`.
pub(crate) fn parse_error(text: &str) -> ApiError {
    match serde_json::from_str::<ErrorEnvelope>(text) {
        Ok(env) => ApiError {
            reason: env
                .error
                .errors
                .into_iter()
                .map(|e| e.reason)
                .find(|r| !r.is_empty())
                .unwrap_or(env.error.status),
            message: env.error.message,
        },
        Err(_) => ApiError::default(),
    }
}

/// Map a Drive error to a [`VfsError`] about `ctx` (usually the path).
pub(crate) fn map_error(status: u16, reason: &str, message: &str, ctx: &str) -> VfsError {
    let detail = if message.is_empty() {
        String::new()
    } else {
        format!(" ({message})")
    };
    match reason {
        "notFound" => return VfsError::NotFound(ctx.to_string()),
        "storageQuotaExceeded" | "quotaExceeded" | "teamDriveFileLimitExceeded" | "numChildrenInNonRootLimitExceeded" => {
            return VfsError::Other(format!("{ctx}: your Google Drive storage is full{detail}"))
        }
        "dailyLimitExceeded" | "dailyLimitExceededUnreg" | "uploadLimitExceeded" => {
            return VfsError::Connection(format!("{ctx}: Google Drive's daily limit is reached; try again later{detail}"))
        }
        "rateLimitExceeded" | "userRateLimitExceeded" => {
            return VfsError::Connection(format!("{ctx}: Google Drive is rate-limiting requests; try again in a minute"))
        }
        "accessNotConfigured" => {
            return VfsError::Auth("the Google Drive API is not enabled for Manta's Google Cloud project".into())
        }
        "exportSizeLimitExceeded" => {
            return VfsError::Unsupported(format!(
                "{ctx}: this Google document is too large to export (Google's limit is 10 MB); open it in the browser"
            ))
        }
        "cannotDownloadAbusiveFile" => {
            return VfsError::PermissionDenied(format!("{ctx}: Google flagged this file as malware or spam"))
        }
        "fileNotDownloadable" | "cannotExportFile" => {
            return VfsError::Unsupported(format!("{ctx}: this file can't be downloaded from Google Drive{detail}"))
        }
        "insufficientFilePermissions" | "insufficientPermissions" | "forbidden" | "appNotAuthorizedToFile"
        | "domainPolicy" | "teamDrivesFolderMoveInNotSupported" | "cannotMoveTrashedItemIntoTeamDrive" => {
            return VfsError::PermissionDenied(format!("{ctx}: Google Drive doesn't allow this{detail}"))
        }
        "authError" | "invalidCredentials" => {
            return VfsError::Auth("the Google Drive sign-in is no longer valid; please sign in again".into())
        }
        _ => {}
    }
    match status {
        404 => VfsError::NotFound(ctx.to_string()),
        401 => VfsError::Auth(
            "the Google Drive sign-in is no longer valid; please sign in again".into(),
        ),
        403 => VfsError::PermissionDenied(format!("{ctx}: access denied by Google Drive{detail}")),
        429 => VfsError::Connection(format!(
            "{ctx}: Google Drive is rate-limiting requests; try again in a minute"
        )),
        s if s >= 500 => {
            VfsError::Connection(format!("{ctx}: Google Drive server error (HTTP {s})"))
        }
        s => VfsError::Other(format!("{ctx}: Google Drive error (HTTP {s}){detail}")),
    }
}

/// A file resource (only the fields Manta asks for).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileMeta {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    /// int64 as a string; absent for folders and Google-native files.
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub drive_id: Option<String>,
    #[serde(default)]
    pub shortcut_details: Option<ShortcutDetails>,
    #[serde(default)]
    pub parents: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShortcutDetails {
    #[serde(default)]
    pub target_id: String,
    #[serde(default)]
    pub target_mime_type: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileList {
    #[serde(default)]
    pub files: Vec<FileMeta>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DriveMeta {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveList {
    #[serde(default)]
    pub drives: Vec<DriveMeta>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// Fields requested for files.
pub(crate) const FILE_FIELDS: &str =
    "id,name,mimeType,size,modifiedTime,createdTime,driveId,shortcutDetails(targetId,targetMimeType)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_drive_errors() {
        let t = r#"{"error":{"errors":[{"domain":"usageLimits","reason":"userRateLimitExceeded","message":"x"}],"code":403,"message":"User Rate Limit Exceeded"}}"#;
        assert_eq!(
            parse_error(t),
            ApiError {
                reason: "userRateLimitExceeded".into(),
                message: "User Rate Limit Exceeded".into()
            }
        );
        assert_eq!(parse_error("<html>"), ApiError::default());
        let t = r#"{"error":{"code":429,"message":"slow","status":"RESOURCE_EXHAUSTED"}}"#;
        assert_eq!(parse_error(t).reason, "RESOURCE_EXHAUSTED");
    }

    #[test]
    fn maps_errors() {
        assert!(matches!(
            map_error(404, "notFound", "", "/a"),
            VfsError::NotFound(_)
        ));
        assert!(matches!(
            map_error(404, "", "", "/a"),
            VfsError::NotFound(_)
        ));
        assert!(matches!(map_error(401, "", "", "/a"), VfsError::Auth(_)));
        assert!(matches!(
            map_error(403, "insufficientFilePermissions", "", "/a"),
            VfsError::PermissionDenied(_)
        ));
        match map_error(
            403,
            "storageQuotaExceeded",
            "The user's Drive storage quota has been exceeded.",
            "/a",
        ) {
            VfsError::Other(m) => assert!(m.contains("storage is full"), "{m}"),
            e => panic!("{e:?}"),
        }
        assert!(matches!(
            map_error(403, "exportSizeLimitExceeded", "", "/a"),
            VfsError::Unsupported(_)
        ));
        assert!(matches!(
            map_error(503, "", "", "/a"),
            VfsError::Connection(_)
        ));
        assert!(matches!(
            map_error(403, "accessNotConfigured", "", "/a"),
            VfsError::Auth(_)
        ));
    }

    #[test]
    fn backoff_grows_and_caps() {
        let p = RetryPolicy {
            max_attempts: 9,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
        };
        assert!(p.delay(1, None) < Duration::from_millis(201));
        assert!(p.delay(3, None) >= Duration::from_millis(400));
        assert!(p.delay(8, None) <= Duration::from_millis(1100));
        assert_eq!(
            p.delay(1, Some(Duration::from_secs(7))),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn call_urls() {
        let c = Call::new("GET", "http://h/drive/v3/files".into())
            .query("q", "'root' in parents")
            .query("pageSize", "2");
        assert_eq!(
            c.full_url(),
            "http://h/drive/v3/files?q=%27root%27%20in%20parents&pageSize=2"
        );
    }
}
