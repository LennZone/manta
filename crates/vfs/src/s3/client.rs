//! A small synchronous S3 REST client: SigV4 signing, retries with
//! exponential backoff (5xx, throttling, dropped connections), automatic
//! bucket-region correction and S3 error-code mapping. HTTP keep-alive
//! connections are pooled by the shared `ureq::Agent`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use chrono::Utc;
use manta_core::VfsError;
use ureq::http;

use super::sigv4::{self, Credentials, SignInput};
use super::xml;

/// Attempts per request (1 + retries).
const MAX_ATTEMPTS: u32 = 4;
const BACKOFF_BASE: Duration = Duration::from_millis(250);

pub type Response = http::Response<ureq::Body>;

/// Scheme and `host[:port]` of the service endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub https: bool,
    /// Host name plus port when it isn't the scheme's default.
    pub host: String,
}

impl Endpoint {
    /// Parse `https://host:port`, `host` (https assumed) or `http://host`.
    pub fn parse(s: &str) -> Result<Endpoint, String> {
        let s = s.trim().trim_end_matches('/');
        let (https, rest) = if let Some(r) = s.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = s.strip_prefix("http://") {
            (false, r)
        } else if s.contains("://") {
            return Err(format!(
                "unsupported endpoint scheme in “{s}” (use http or https)"
            ));
        } else {
            (true, s)
        };
        if rest.contains('/') {
            return Err(format!("the endpoint “{s}” must not contain a path"));
        }
        if rest.is_empty() {
            return Err("the endpoint is empty".into());
        }
        let host = match rest.rsplit_once(':') {
            Some((h, "443")) if https && !h.contains(':') => h.to_string(),
            Some((h, "80")) if !https && !h.contains(':') => h.to_string(),
            _ => rest.to_string(),
        };
        Ok(Endpoint { https, host })
    }

    pub fn aws(region: &str) -> Endpoint {
        Endpoint {
            https: true,
            host: format!("s3.{region}.amazonaws.com"),
        }
    }

    pub fn url(&self) -> String {
        format!(
            "{}://{}",
            if self.https { "https" } else { "http" },
            self.host
        )
    }

    /// Host name without the port.
    pub fn hostname(&self) -> &str {
        match self.host.rsplit_once(':') {
            Some((h, p)) if !h.ends_with(']') && p.chars().all(|c| c.is_ascii_digit()) => h,
            _ => &self.host,
        }
    }

    fn is_aws(&self) -> bool {
        self.hostname().ends_with(".amazonaws.com")
    }

    /// IP literals and single-label hosts (localhost) can't take a bucket
    /// sub-domain.
    fn supports_virtual_hosts(&self) -> bool {
        let h = self.hostname();
        h.contains('.') && h.parse::<std::net::IpAddr>().is_err() && !h.starts_with('[')
    }
}

/// Can `bucket` be used as a DNS label (virtual-hosted-style addressing)?
pub fn dns_compatible(bucket: &str, https: bool) -> bool {
    (3..=63).contains(&bucket.len())
        && bucket.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || (b == b'.' && !https)
        })
        && !bucket.starts_with('-')
        && !bucket.ends_with('-')
}

/// One API call. `bucket: None` addresses the service (ListBuckets).
pub struct Request<'a> {
    pub method: &'static str,
    pub bucket: Option<&'a str>,
    pub key: &'a str,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: &'a [u8],
}

impl<'a> Request<'a> {
    pub fn new(method: &'static str, bucket: Option<&'a str>, key: &'a str) -> Self {
        Request {
            method,
            bucket,
            key,
            query: Vec::new(),
            headers: Vec::new(),
            body: &[],
        }
    }

    pub fn query(mut self, k: &str, v: impl Into<String>) -> Self {
        self.query.push((k.to_string(), v.into()));
        self
    }

    pub fn header(mut self, k: &str, v: impl Into<String>) -> Self {
        self.headers.push((k.to_ascii_lowercase(), v.into()));
        self
    }

    pub fn body(mut self, b: &'a [u8]) -> Self {
        self.body = b;
        self
    }
}

pub struct Client {
    agent: ureq::Agent,
    pub endpoint: Endpoint,
    pub region: String,
    path_style: bool,
    /// `None`: anonymous (unsigned) requests for public buckets.
    creds: Option<Credentials>,
    /// Buckets found to live in another region: bucket -> (region, host).
    bucket_regions: Mutex<HashMap<String, (String, Option<String>)>>,
}

impl Client {
    pub fn new(
        endpoint: Endpoint,
        region: String,
        path_style: bool,
        creds: Option<Credentials>,
    ) -> Client {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(crate::CONNECT_TIMEOUT))
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .max_idle_connections_per_host(8)
            .user_agent(concat!("Manta/", env!("CARGO_PKG_VERSION")))
            .build();
        Client {
            agent: ureq::Agent::new_with_config(config),
            endpoint,
            region,
            path_style,
            creds,
            bucket_regions: Mutex::new(HashMap::new()),
        }
    }

    /// Host, canonical (encoded) URI and signing region for an address.
    pub fn target(&self, bucket: Option<&str>, key: &str) -> (String, String, String) {
        let mut host = self.endpoint.host.clone();
        let mut region = self.region.clone();
        if let Some(b) = bucket {
            if let Some((r, h)) = self.bucket_regions.lock().unwrap().get(b) {
                region = r.clone();
                if let Some(h) = h {
                    host = h.clone();
                }
            }
        }
        let key_enc = sigv4::uri_encode(key, true);
        match bucket {
            None => (host, "/".into(), region),
            Some(b)
                if !self.path_style
                    && self.endpoint.supports_virtual_hosts()
                    && dns_compatible(b, self.endpoint.https) =>
            {
                (format!("{b}.{host}"), format!("/{key_enc}"), region)
            }
            Some(b) => {
                let b = sigv4::uri_encode(b, false);
                let uri = if key.is_empty() {
                    format!("/{b}")
                } else {
                    format!("/{b}/{key_enc}")
                };
                (host, uri, region)
            }
        }
    }

    /// Send `req`, retrying transient failures. Only 2xx responses are
    /// returned; everything else becomes a [`VfsError`] about `ctx`.
    pub fn send(&self, req: &Request<'_>, ctx: &str) -> Result<Response, VfsError> {
        let payload_hash = sigv4::sha256_hex(req.body);
        let mut attempt = 0;
        let mut region_fixed = false;
        loop {
            attempt += 1;
            let (host, uri, region) = self.target(req.bucket, req.key);
            let query = sigv4::canonical_query(&req.query);
            let scheme = if self.endpoint.https { "https" } else { "http" };
            let url = if query.is_empty() {
                format!("{scheme}://{host}{uri}")
            } else {
                format!("{scheme}://{host}{uri}?{query}")
            };
            let amz_date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let mut headers = req.headers.clone();
            headers.push(("x-amz-date".into(), amz_date.clone()));
            headers.push(("x-amz-content-sha256".into(), payload_hash.clone()));
            if let Some(t) = self.creds.as_ref().and_then(|c| c.session_token.clone()) {
                headers.push(("x-amz-security-token".into(), t));
            }
            let mut builder = http::Request::builder().method(req.method).uri(&url);
            if let Some(creds) = &self.creds {
                let mut signed = headers.clone();
                signed.push(("host".into(), host.clone()));
                let auth = sigv4::authorization(
                    creds,
                    &region,
                    "s3",
                    &amz_date,
                    &SignInput {
                        method: req.method,
                        canonical_uri: &uri,
                        query: &req.query,
                        headers: &signed,
                        payload_hash: &payload_hash,
                    },
                );
                builder = builder.header("authorization", auth);
            }
            for (k, v) in &headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            let result = if req.body.is_empty() && matches!(req.method, "GET" | "HEAD" | "DELETE") {
                builder.body(()).map(|r| self.agent.run(r))
            } else {
                builder.body(req.body).map(|r| self.agent.run(r))
            };
            let result = match result {
                Ok(r) => r,
                Err(e) => return Err(VfsError::Other(format!("{ctx}: invalid request: {e}"))),
            };
            let mut resp = match result {
                Ok(resp) => resp,
                Err(e) => {
                    let retry = matches!(
                        e,
                        ureq::Error::Io(_)
                            | ureq::Error::Timeout(_)
                            | ureq::Error::ConnectionFailed
                    );
                    if retry && attempt < MAX_ATTEMPTS {
                        log::debug!("S3 {} {ctx}: {e}; retrying", req.method);
                        backoff(attempt);
                        continue;
                    }
                    return Err(transport_error(e, &self.endpoint));
                }
            };
            let status = resp.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(resp);
            }
            let header_region = resp
                .headers()
                .get("x-amz-bucket-region")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = if req.method == "HEAD" {
                String::new()
            } else {
                resp.body_mut()
                    .with_config()
                    .limit(1024 * 1024)
                    .read_to_string()
                    .unwrap_or_default()
            };
            let err = xml::parse_error(&body).unwrap_or_default();

            // Bucket in another region: remember it and try again there.
            let region_error = status == 301
                || matches!(
                    err.code.as_str(),
                    "AuthorizationHeaderMalformed"
                        | "PermanentRedirect"
                        | "IllegalLocationConstraintException"
                );
            if let (true, false, Some(b)) = (region_error, region_fixed, req.bucket) {
                if let Some(r) = header_region
                    .clone()
                    .or(err.region.clone())
                    .filter(|r| *r != region)
                {
                    log::info!("S3 bucket {b} is in region {r}");
                    let host = self.endpoint.is_aws().then(|| Endpoint::aws(&r).host);
                    self.bucket_regions
                        .lock()
                        .unwrap()
                        .insert(b.to_string(), (r, host));
                    region_fixed = true;
                    attempt -= 1;
                    continue;
                }
            }

            let throttled = matches!(
                err.code.as_str(),
                "SlowDown"
                    | "Throttling"
                    | "ThrottlingException"
                    | "RequestTimeout"
                    | "RequestLimitExceeded"
                    | "InternalError"
                    | "ServiceUnavailable"
            );
            if (status >= 500 || status == 429 || throttled) && attempt < MAX_ATTEMPTS {
                log::debug!(
                    "S3 {} {ctx}: HTTP {status} {}; retrying",
                    req.method,
                    err.code
                );
                backoff(attempt);
                continue;
            }
            return Err(map_error(
                status,
                &err.code,
                &err.message,
                header_region.or(err.region),
                ctx,
            ));
        }
    }

    /// Send and read the whole (XML) response body.
    pub fn send_text(&self, req: &Request<'_>, ctx: &str) -> Result<String, VfsError> {
        let mut resp = self.send(req, ctx)?;
        resp.body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| VfsError::Connection(format!("{ctx}: reading response failed: {e}")))
    }
}

fn backoff(attempt: u32) {
    // Exponential backoff with a little jitter from the clock.
    let base = BACKOFF_BASE * 2u32.pow(attempt.saturating_sub(1).min(5));
    let jitter = Duration::from_millis(u64::from(Utc::now().timestamp_subsec_millis() % 100));
    std::thread::sleep(base + jitter);
}

fn transport_error(e: ureq::Error, ep: &Endpoint) -> VfsError {
    let host = ep.hostname().to_string();
    match e {
        ureq::Error::HostNotFound => VfsError::Connection(format!("cannot resolve {host}")),
        ureq::Error::Timeout(t) => VfsError::Connection(format!("{host}: timed out ({t})")),
        ureq::Error::ConnectionFailed => {
            VfsError::Connection(format!("cannot connect to {host}: connection refused"))
        }
        ureq::Error::Io(io) => VfsError::Connection(format!("{host}: {io}")),
        other => VfsError::Connection(format!("{host}: {other}")),
    }
}

/// Map an S3 error response to a [`VfsError`].
pub fn map_error(
    status: u16,
    code: &str,
    message: &str,
    region: Option<String>,
    ctx: &str,
) -> VfsError {
    let detail = if message.is_empty() {
        code.to_string()
    } else {
        format!("{message} ({code})")
    };
    let msg = |fallback: &str| {
        if detail.is_empty() {
            format!("{ctx}: {fallback}")
        } else {
            format!("{ctx}: {detail}")
        }
    };
    match code {
        "NoSuchKey" | "NoSuchBucket" | "NoSuchUpload" | "NotFound" => {
            VfsError::NotFound(msg("not found"))
        }
        "AccessDenied" | "AllAccessDisabled" | "AccountProblem" => {
            VfsError::PermissionDenied(msg("access denied"))
        }
        "InvalidAccessKeyId"
        | "SignatureDoesNotMatch"
        | "ExpiredToken"
        | "InvalidToken"
        | "TokenRefreshRequired"
        | "MissingSecurityHeader"
        | "InvalidSecurity" => {
            let hint = match code {
                "InvalidAccessKeyId" => "the access key ID is unknown to this service",
                "SignatureDoesNotMatch" => "the secret access key doesn't match the access key ID",
                _ => "the session token is invalid or expired",
            };
            VfsError::Auth(format!("{hint} — {detail}"))
        }
        "RequestTimeTooSkewed" => VfsError::Connection(format!(
            "your Mac's clock is too far off from the server's time; turn on “Set time and date \
             automatically” in System Settings › General › Date & Time ({detail})"
        )),
        "BucketAlreadyExists" | "BucketAlreadyOwnedByYou" => {
            VfsError::AlreadyExists(msg("bucket exists"))
        }
        "BucketNotEmpty" => VfsError::DirectoryNotEmpty(msg("bucket is not empty")),
        "NotImplemented" | "XNotImplemented" => {
            VfsError::Unsupported(msg("not implemented by this server"))
        }
        "AuthorizationHeaderMalformed"
        | "PermanentRedirect"
        | "IllegalLocationConstraintException" => VfsError::Connection(match region {
            Some(r) => format!("{ctx}: the bucket is in region {r}; set the region to {r}"),
            None => msg("wrong region or endpoint for this bucket"),
        }),
        _ => match status {
            404 => VfsError::NotFound(msg("not found")),
            401 => VfsError::Auth(msg("unauthorized")),
            403 => VfsError::PermissionDenied(msg("access denied")),
            301 | 307 => VfsError::Connection(match region {
                Some(r) => format!("{ctx}: the bucket is in region {r}; set the region to {r}"),
                None => msg("wrong region or endpoint for this bucket"),
            }),
            s if s >= 500 => VfsError::Connection(msg(&format!("server error (HTTP {s})"))),
            s => VfsError::Other(msg(&format!("HTTP {s}"))),
        },
    }
}

/// Base64 MD5 of a body (required by DeleteObjects).
pub fn content_md5(body: &[u8]) -> String {
    use md5::Digest;
    base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        let e = Endpoint::parse("http://localhost:9000/").unwrap();
        assert_eq!(
            e,
            Endpoint {
                https: false,
                host: "localhost:9000".into()
            }
        );
        assert_eq!(e.hostname(), "localhost");
        assert!(!e.supports_virtual_hosts());
        let e = Endpoint::parse("s3.eu-central-003.backblazeb2.com").unwrap();
        assert!(e.https && e.supports_virtual_hosts());
        assert_eq!(
            Endpoint::parse("https://x.example.com:443").unwrap().host,
            "x.example.com"
        );
        assert!(Endpoint::parse("ftp://x").is_err());
        assert!(Endpoint::parse("https://x.com/path").is_err());
        assert!(!Endpoint::parse("http://127.0.0.1:9000")
            .unwrap()
            .supports_virtual_hosts());
    }

    #[test]
    fn addressing_styles() {
        let aws = Client::new(Endpoint::aws("eu-west-1"), "eu-west-1".into(), false, None);
        assert_eq!(
            aws.target(Some("photos"), "2024/a b.jpg"),
            (
                "photos.s3.eu-west-1.amazonaws.com".into(),
                "/2024/a%20b.jpg".into(),
                "eu-west-1".into()
            )
        );
        // Dotted bucket over https: path style (the wildcard cert wouldn't match).
        assert_eq!(aws.target(Some("my.bucket"), "k").1, "/my.bucket/k");
        assert_eq!(aws.target(None, "").1, "/");
        let minio = Client::new(
            Endpoint::parse("http://localhost:9000").unwrap(),
            "us-east-1".into(),
            false,
            None,
        );
        assert_eq!(minio.target(Some("b"), "").0, "localhost:9000");
        assert_eq!(minio.target(Some("bkt"), "").1, "/bkt");
        let forced = Client::new(Endpoint::aws("us-east-1"), "us-east-1".into(), true, None);
        assert_eq!(forced.target(Some("photos"), "x/y").1, "/photos/x/y");
        // Region override after a redirect.
        aws.bucket_regions.lock().unwrap().insert(
            "photos".into(),
            (
                "us-west-2".into(),
                Some("s3.us-west-2.amazonaws.com".into()),
            ),
        );
        let (host, _, region) = aws.target(Some("photos"), "k");
        assert_eq!(
            (host.as_str(), region.as_str()),
            ("photos.s3.us-west-2.amazonaws.com", "us-west-2")
        );
    }

    #[test]
    fn error_codes() {
        assert!(matches!(
            map_error(404, "NoSuchKey", "", None, "/b/k"),
            VfsError::NotFound(_)
        ));
        assert!(matches!(
            map_error(404, "NoSuchBucket", "", None, "/b"),
            VfsError::NotFound(_)
        ));
        assert!(matches!(
            map_error(403, "AccessDenied", "", None, "/b"),
            VfsError::PermissionDenied(_)
        ));
        assert!(matches!(
            map_error(403, "InvalidAccessKeyId", "x", None, "/"),
            VfsError::Auth(_)
        ));
        assert!(matches!(
            map_error(403, "SignatureDoesNotMatch", "x", None, "/"),
            VfsError::Auth(_)
        ));
        match map_error(403, "RequestTimeTooSkewed", "x", None, "/") {
            VfsError::Connection(m) => assert!(m.contains("clock"), "{m}"),
            other => panic!("{other:?}"),
        }
        match map_error(
            400,
            "AuthorizationHeaderMalformed",
            "",
            Some("eu-west-1".into()),
            "/b",
        ) {
            VfsError::Connection(m) => assert!(m.contains("eu-west-1")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            map_error(409, "BucketNotEmpty", "", None, "/b"),
            VfsError::DirectoryNotEmpty(_)
        ));
        assert!(matches!(
            map_error(404, "", "", None, "/b/k"),
            VfsError::NotFound(_)
        ));
        assert!(matches!(
            map_error(503, "", "", None, "/b/k"),
            VfsError::Connection(_)
        ));
    }

    #[test]
    fn md5_header() {
        assert_eq!(content_md5(b""), "1B2M2Y8AsgTpgAmY7PhCfg==");
    }

    #[test]
    fn bucket_names() {
        assert!(dns_compatible("my-bucket-01", true));
        assert!(!dns_compatible("My_Bucket", true));
        assert!(!dns_compatible("a.b.c", true));
        assert!(dns_compatible("a.b.c", false));
        assert!(!dns_compatible("ab", false));
    }
}
