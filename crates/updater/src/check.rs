//! Fetching the feed and choosing the release to offer.

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use semver::Version;

use crate::error::{Result, UpdateError};
use crate::keys;
use crate::manifest::{Channel, Manifest, Release, DEFAULT_APP_ID};

/// Largest `update.json` we read.
const MAX_MANIFEST_BYTES: u64 = 2 << 20;

// ---------------------------------------------------------------- URLs

/// The HTTPS-only rule: `https://` anywhere, `http://` only to 127.0.0.1,
/// `[::1]` or localhost (local tests).
pub fn validate_url(url: &str) -> Result<()> {
    let lower = url.trim().to_ascii_lowercase();
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(UpdateError::InvalidUrl(url.to_string()));
    }
    if let Some(rest) = lower.strip_prefix("https://") {
        return if host_of(rest).is_empty() {
            Err(UpdateError::InvalidUrl(url.into()))
        } else {
            Ok(())
        };
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = host_of(rest);
        return if matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
            Ok(())
        } else {
            Err(UpdateError::InsecureUrl(url.to_string()))
        };
    }
    Err(UpdateError::InsecureUrl(url.to_string()))
}

/// Host part of `host[:port]/path` (no userinfo allowed).
fn host_of(rest: &str) -> &str {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return ""; // user:pass@host is never needed and only confuses
    }
    if authority.starts_with('[') {
        return authority.split_inclusive(']').next().unwrap_or("");
    }
    authority.split(':').next().unwrap_or("")
}

/// Resolves a release URL against the feed URL: absolute URLs stay, a relative
/// one (`Manta-1.2.0.zip`) is taken relative to the feed's folder.
pub fn resolve_url(feed_url: &str, url: &str) -> String {
    if url.contains("://") {
        return url.to_string();
    }
    let base = feed_url.split(['?', '#']).next().unwrap_or(feed_url);
    if let Some(path) = url.strip_prefix('/') {
        // Absolute path on the same host.
        let after_scheme = base.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end = base[after_scheme..]
            .find('/')
            .map(|i| i + after_scheme)
            .unwrap_or(base.len());
        return format!("{}/{}", &base[..host_end], path);
    }
    match base.rfind('/') {
        Some(i) if i > base.find("://").map(|j| j + 2).unwrap_or(0) => {
            format!("{}/{}", &base[..i], url)
        }
        _ => format!("{}/{}", base.trim_end_matches('/'), url),
    }
}

/// Where the detached manifest signature lives: `<feed>.sig`.
pub fn signature_url(feed_url: &str) -> String {
    match feed_url.split_once('?') {
        Some((path, query)) => format!("{path}.sig?{query}"),
        None => format!("{feed_url}.sig"),
    }
}

// ---------------------------------------------------------------- macOS version

/// A dotted OS version like `11.0` or `14.6.1`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OsVersion(pub Vec<u32>);

impl OsVersion {
    pub fn parse(s: &str) -> Option<OsVersion> {
        let s = s.trim();
        if s.is_empty() || s.split('.').count() > 4 {
            return None;
        }
        let parts: Option<Vec<u32>> = s
            .split('.')
            .map(|p| {
                if p.bytes().all(|b| b.is_ascii_digit()) {
                    p.parse().ok()
                } else {
                    None
                }
            })
            .collect();
        let mut parts = parts?;
        while parts.len() > 1 && parts.last() == Some(&0) {
            parts.pop(); // 11.0 == 11
        }
        Some(OsVersion(parts))
    }
}

impl std::fmt::Display for OsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: Vec<String> = self.0.iter().map(u32::to_string).collect();
        f.write_str(&s.join("."))
    }
}

/// The running macOS version (`sw_vers -productVersion`), cached.
pub fn macos_version() -> Option<OsVersion> {
    static V: OnceLock<Option<OsVersion>> = OnceLock::new();
    V.get_or_init(|| {
        let out = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        OsVersion::parse(String::from_utf8_lossy(&out.stdout).trim())
    })
    .clone()
}

// ---------------------------------------------------------------- selection

/// The release to offer: the newest one that is newer than `current`, on an
/// accepted channel, newer than a skipped version, and runs on `os` (`None`:
/// unknown OS, no check). Releases needing a newer macOS are passed over in
/// favour of the newest compatible one.
pub fn select_release(
    manifest: &Manifest,
    current: &Version,
    channel: Channel,
    skipped: Option<&Version>,
    os: Option<&OsVersion>,
) -> Option<Release> {
    manifest
        .releases
        .iter()
        .filter(|r| r.version > *current)
        .filter(|r| channel.accepts(r.effective_channel()))
        .filter(|r| skipped.is_none_or(|s| r.version > *s))
        .filter(|r| match (os, OsVersion::parse(&r.min_macos)) {
            (Some(os), Some(min)) => *os >= min,
            _ => true,
        })
        .max_by(|a, b| a.version.cmp(&b.version))
        .cloned()
}

// ---------------------------------------------------------------- HTTP

pub(crate) fn agent(url: &str, global_timeout: Option<Duration>) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        // Refuse redirects to http:// for https feeds.
        .https_only(url.to_ascii_lowercase().starts_with("https://"))
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_send_request(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(global_timeout)
        .user_agent(concat!("Manta-Updater/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

pub(crate) fn net_error(url: &str, e: ureq::Error) -> UpdateError {
    let host = url
        .split("://")
        .nth(1)
        .map(|r| host_of(r).to_string())
        .unwrap_or_default();
    match e {
        ureq::Error::HostNotFound => {
            UpdateError::Network(format!("cannot resolve {host} (offline?)"))
        }
        ureq::Error::ConnectionFailed => UpdateError::Network(format!("cannot connect to {host}")),
        ureq::Error::Timeout(t) => UpdateError::Network(format!("{host} timed out ({t})")),
        ureq::Error::RequireHttpsOnly(_) => UpdateError::InsecureUrl(url.to_string()),
        ureq::Error::Io(io) => UpdateError::Network(format!("{host}: {io}")),
        other => UpdateError::Network(format!("{host}: {other}")),
    }
}

/// GET a small resource into memory.
pub(crate) fn get_small(url: &str, limit: u64) -> Result<Vec<u8>> {
    validate_url(url)?;
    let agent = agent(url, Some(Duration::from_secs(60)));
    let mut resp = agent
        .get(url)
        .header("Cache-Control", "no-cache")
        .call()
        .map_err(|e| net_error(url, e))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(UpdateError::Http {
            status,
            url: url.to_string(),
        });
    }
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|e| UpdateError::Network(format!("{url}: {e}")))?;
    if buf.len() as u64 > limit {
        return Err(UpdateError::Manifest(format!(
            "{url} is larger than {limit} bytes"
        )));
    }
    Ok(buf)
}

/// Downloads `update.json` and `update.json.sig`, verifies the signature and
/// parses the manifest. If the signature doesn't match, both files are fetched
/// once more after a short pause (an upload may be half done).
pub fn fetch_manifest(feed_url: &str, key: &VerifyingKey, app_id: &str) -> Result<Manifest> {
    validate_url(feed_url)?;
    let mut attempt = 0;
    let bytes = loop {
        attempt += 1;
        let bytes = get_small(feed_url, MAX_MANIFEST_BYTES)?;
        let sig = match get_small(&signature_url(feed_url), 4096) {
            Ok(s) => s,
            Err(UpdateError::Http { status: 404, .. }) => {
                return Err(UpdateError::Signature(
                    "update.json.sig is missing on the server".into(),
                ))
            }
            Err(e) => return Err(e),
        };
        match keys::verify_manifest(key, &bytes, &String::from_utf8_lossy(&sig)) {
            Ok(()) => break bytes,
            Err(e) if attempt >= 2 => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(1500)),
        }
    };
    let manifest = Manifest::parse(&bytes)?;
    if manifest.app_id != app_id {
        return Err(UpdateError::Manifest(format!(
            "the feed is for \"{}\", not \"{app_id}\"",
            manifest.app_id
        )));
    }
    Ok(manifest)
}

/// Options for [`check_with`].
#[derive(Clone, Debug)]
pub struct CheckOptions {
    pub key: VerifyingKey,
    pub app_id: String,
    /// Override the detected macOS version (tests).
    pub os_version: Option<OsVersion>,
}

/// Checks the feed with the built-in key and app ID. `Ok(None)`: up to date.
pub fn check(
    feed_url: &str,
    current_version: &Version,
    channel: Channel,
    skipped: Option<Version>,
) -> Result<Option<Release>> {
    let opts = CheckOptions {
        key: keys::embedded_public_key()?,
        app_id: DEFAULT_APP_ID.into(),
        os_version: macos_version(),
    };
    check_with(&opts, feed_url, current_version, channel, skipped.as_ref())
}

/// [`check`] with an explicit key, app ID and OS version. The returned
/// release's signature is verified and its `url` is absolute.
pub fn check_with(
    opts: &CheckOptions,
    feed_url: &str,
    current_version: &Version,
    channel: Channel,
    skipped: Option<&Version>,
) -> Result<Option<Release>> {
    let manifest = fetch_manifest(feed_url, &opts.key, &opts.app_id)?;
    let Some(mut release) = select_release(
        &manifest,
        current_version,
        channel,
        skipped,
        opts.os_version.as_ref(),
    ) else {
        return Ok(None);
    };
    keys::verify_release(&opts.key, &opts.app_id, &release)?;
    release.url = resolve_url(feed_url, &release.url);
    validate_url(&release.url)?;
    Ok(Some(release))
}
