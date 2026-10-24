//! The community plugin directory (`plugins/registry.json` in the Manta
//! repository) for Settings → Plugins → Discover.
//!
//! **Privacy.** Manta loads the directory only when the user opens the
//! Discover tab (or clicks Refresh there): one HTTPS GET of [`DEFAULT_URL`]
//! from GitHub, nothing else is sent. The answer is kept in
//! `plugin-registry.json` next to `plugins.json`, so the tab shows the last
//! list offline and installed cards can offer updates without a network
//! call. Installing downloads the release archive from the entry's `url`
//! (usually a GitHub release) and checks its SHA-256 against the entry
//! before anything is unpacked; then the usual review and trust sheets
//! apply ([`crate::install`]).
//!
//! `MANTA_PLUGIN_REGISTRY_URL` points Manta at another directory (tests,
//! mirrors). Only `https://` URLs are fetched (`http://` to the local
//! machine for tests).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bundled::compare_versions;
use crate::manifest::{valid_id, MANTA_VERSION};

/// The directory in the Manta repository on GitHub.
pub const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/web-workers-com/manta/main/plugins/registry.json";
/// Environment variable with another directory URL.
pub const URL_ENV: &str = "MANTA_PLUGIN_REGISTRY_URL";
/// The cached directory, next to `plugins.json`.
pub const CACHE_FILE: &str = "plugin-registry.json";
/// Largest directory we read.
pub const MAX_REGISTRY_BYTES: u64 = 4 << 20;
/// Largest release archive we download.
pub const MAX_DOWNLOAD_BYTES: u64 = 256 << 20;

/// The directory URL: [`URL_ENV`] if set, else [`DEFAULT_URL`].
pub fn registry_url() -> String {
    std::env::var(URL_ENV)
        .ok()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// `latest` of an entry: the release to install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    /// A `.mantaplugin` or `.zip` file (`https://`).
    pub url: String,
    /// Lowercase hex SHA-256 of that file.
    pub sha256: String,
}

/// One plugin in the directory (see `plugins/registry.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Language code (`en`, `de`, `zh-Hans` …) → text; `en` is required.
    pub name: BTreeMap<String, String>,
    pub description: BTreeMap<String, String>,
    pub author: String,
    pub repository: String,
    #[serde(default)]
    pub homepage: Option<String>,
    pub license: String,
    pub latest: Release,
    pub min_manta_version: String,
    pub platforms: Vec<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Why an entry can't be installed in this Manta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incompatible {
    /// It needs this Manta version or newer.
    NeedsManta(String),
    /// None of its platforms is this Mac.
    Platform(Vec<String>),
}

/// The text for the app's language: the exact code, its language (`zh` →
/// `zh-Hans`), then English.
fn localized(map: &BTreeMap<String, String>) -> String {
    let locale = crate::locale();
    let lang = locale.split(['-', '_']).next().unwrap_or("en").to_string();
    let candidates = [
        locale.clone(),
        lang.clone(),
        if lang == "zh" { "zh-Hans".into() } else { lang },
        "en".into(),
    ];
    candidates
        .iter()
        .find_map(|k| map.get(k).filter(|t| !t.trim().is_empty()))
        .or_else(|| map.values().next())
        .cloned()
        .unwrap_or_default()
}

fn https(u: &str) -> bool {
    u.starts_with("https://")
        && u.len() > 8
        && u.len() <= 2000
        && !u.chars().any(char::is_whitespace)
}

impl Entry {
    /// Name in the app's language.
    pub fn name(&self) -> String {
        localized(&self.name)
    }

    /// Description in the app's language.
    pub fn description(&self) -> String {
        localized(&self.description)
    }

    /// The rules the app relies on (the full format check is
    /// `scripts/plugin-registry.py`).
    pub fn check(&self) -> Result<(), String> {
        valid_id(&self.id)?;
        if self.name.get("en").is_none_or(|n| n.trim().is_empty()) {
            return Err(format!("{}: no English name", self.id));
        }
        // (`http://` to this machine only for tests, as in `check_url`.)
        if !https(&self.latest.url)
            && !(self.latest.url.starts_with("http://") && check_url(&self.latest.url).is_ok())
        {
            return Err(format!("{}: latest.url must be an https:// URL", self.id));
        }
        if !https(&self.repository) {
            return Err(format!("{}: repository must be an https:// URL", self.id));
        }
        if self.homepage.as_deref().is_some_and(|h| !https(h)) {
            return Err(format!("{}: homepage must be an https:// URL", self.id));
        }
        let sha = &self.latest.sha256;
        if sha.len() != 64
            || !sha
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(format!(
                "{}: latest.sha256 must be 64 lowercase hex digits",
                self.id
            ));
        }
        if self.latest.version.trim().is_empty() {
            return Err(format!("{}: latest.version is empty", self.id));
        }
        Ok(())
    }

    /// `None` if it runs on this Mac with this Manta.
    pub fn incompatibility(&self) -> Option<Incompatible> {
        if compare_versions(&self.min_manta_version, MANTA_VERSION) == std::cmp::Ordering::Greater {
            return Some(Incompatible::NeedsManta(self.min_manta_version.clone()));
        }
        let here = cfg!(all(target_os = "macos", target_arch = "aarch64"));
        let ok = self
            .platforms
            .iter()
            .any(|p| p == "any" || (p == "macos-arm64" && here));
        (!ok).then(|| Incompatible::Platform(self.platforms.clone()))
    }
}

/// A parsed directory: the usable entries, sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    pub plugins: Vec<Entry>,
    /// Entries left out, with the reason (logged).
    pub skipped: Vec<String>,
}

/// Parse `registry.json`. Broken entries are skipped (one bad entry
/// doesn't hide the others); a broken file is an error.
pub fn parse(bytes: &[u8]) -> Result<Registry, String> {
    #[derive(Deserialize)]
    struct Raw {
        schema: u32,
        plugins: Vec<serde_json::Value>,
    }
    let raw: Raw = serde_json::from_slice(bytes)
        .map_err(|e| format!("the plugin directory is damaged: {e}"))?;
    if raw.schema != 1 {
        return Err(format!(
            "the plugin directory has format {}, this Manta reads format 1",
            raw.schema
        ));
    }
    let mut reg = Registry::default();
    for (n, v) in raw.plugins.into_iter().enumerate() {
        let entry = serde_json::from_value::<Entry>(v)
            .map_err(|e| format!("entry {n}: {e}"))
            .and_then(|e| e.check().map(|()| e));
        match entry {
            Ok(e) if reg.plugins.iter().any(|x| x.id == e.id) => {
                reg.skipped.push(format!("{}: listed twice", e.id))
            }
            Ok(e) => reg.plugins.push(e),
            Err(why) => reg.skipped.push(why),
        }
    }
    reg.plugins.sort_by_key(|e| e.name().to_lowercase());
    Ok(reg)
}

// ------------------------------------------------------------------ HTTP

/// `https://` anywhere; `http://` only to this machine (tests).
pub fn check_url(url: &str) -> Result<(), String> {
    let lower = url.trim().to_ascii_lowercase();
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("invalid URL: {url}"));
    }
    if lower.starts_with("https://") && lower.len() > 8 {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = if authority.starts_with('[') {
            authority.split_inclusive(']').next().unwrap_or("")
        } else {
            authority.split(':').next().unwrap_or("")
        };
        if !authority.contains('@') && matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
            return Ok(());
        }
    }
    Err(format!("only https:// URLs are allowed: {url}"))
}

fn agent(url: &str) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(url.to_ascii_lowercase().starts_with("https://"))
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(300)))
        .user_agent(concat!("Manta/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split(['/', '?', '#']).next())
        .unwrap_or(url)
        .to_string()
}

fn net_error(url: &str, e: ureq::Error) -> String {
    let host = host_of(url);
    match e {
        ureq::Error::HostNotFound => format!("can't reach {host} (offline?)"),
        ureq::Error::ConnectionFailed => format!("can't connect to {host}"),
        ureq::Error::Timeout(_) => format!("{host} didn't answer in time"),
        ureq::Error::RequireHttpsOnly(_) => format!("{host} redirected to an insecure URL"),
        other => format!("{host}: {other}"),
    }
}

/// GET `url` into `out`, at most `limit` bytes. Returns the byte count.
fn get_into(url: &str, limit: u64, out: &mut dyn Write) -> Result<u64, String> {
    check_url(url)?;
    let mut resp = agent(url)
        .get(url)
        .header("Cache-Control", "no-cache")
        .call()
        .map_err(|e| net_error(url, e))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("{} answered with HTTP {status}", host_of(url)));
    }
    let mut reader = resp.body_mut().as_reader().take(limit + 1);
    let n = std::io::copy(&mut reader, out).map_err(|e| format!("{}: {e}", host_of(url)))?;
    if n > limit {
        return Err(format!("{url} is larger than {} MB", limit >> 20));
    }
    Ok(n)
}

/// What [`fetch`] got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub registry: Registry,
    /// The download failed and this is the saved copy.
    pub from_cache: bool,
    /// When the list was downloaded (the cache file's date for a saved copy).
    pub fetched_at: Option<SystemTime>,
    /// Why the download failed (with `from_cache`).
    pub error: Option<String>,
}

/// Read the saved directory, if there is a usable one.
pub fn load_cache(cache: &Path) -> Option<Fetched> {
    let bytes = std::fs::read(cache).ok()?;
    let registry = parse(&bytes).ok()?;
    Some(Fetched {
        registry,
        from_cache: true,
        fetched_at: std::fs::metadata(cache).and_then(|m| m.modified()).ok(),
        error: None,
    })
}

/// Download the directory (blocking; call on a background thread) and save
/// it to `cache`. Offline or broken: the saved copy with the error, or the
/// error if there is none.
pub fn fetch(url: &str, cache: Option<&Path>) -> Result<Fetched, String> {
    let online = (|| {
        let mut buf = Vec::new();
        get_into(url, MAX_REGISTRY_BYTES, &mut buf)?;
        let registry = parse(&buf)?;
        Ok::<_, String>((buf, registry))
    })();
    match online {
        Ok((bytes, registry)) => {
            if let Some(path) = cache {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, &bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, path);
                }
            }
            for s in &registry.skipped {
                log::warn!("plugin directory: skipped {s}");
            }
            Ok(Fetched {
                registry,
                from_cache: false,
                fetched_at: Some(SystemTime::now()),
                error: None,
            })
        }
        Err(e) => {
            log::info!("plugin directory: {e}");
            match cache.and_then(load_cache) {
                Some(mut f) => {
                    f.error = Some(e);
                    Ok(f)
                }
                None => Err(e),
            }
        }
    }
}

/// Lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex(&h.finalize()))
}

/// Writes to a file and hashes what passes through.
struct Hashing<'a> {
    file: &'a mut std::fs::File,
    hash: Sha256,
}

impl Write for Hashing<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Download a release archive to `dest` (blocking) and verify its SHA-256
/// against `sha256` from the directory. On a mismatch nothing is kept and
/// the error says so; the file is never unpacked.
pub fn download(url: &str, sha256: &str, dest: &Path) -> Result<(), String> {
    let part = dest.with_extension("part");
    let result = (|| {
        let mut f =
            std::fs::File::create(&part).map_err(|e| format!("can't save the download: {e}"))?;
        let mut w = Hashing {
            file: &mut f,
            hash: Sha256::new(),
        };
        get_into(url, MAX_DOWNLOAD_BYTES, &mut w)?;
        let got = hex(&w.hash.finalize());
        f.sync_all().ok();
        if !got.eq_ignore_ascii_case(sha256.trim()) {
            return Err(format!(
                "SHA-256 mismatch: the plugin directory says {sha256}, the downloaded file is {got}. \
                 Nothing was installed."
            ));
        }
        std::fs::rename(&part, dest).map_err(|e| format!("can't save the download: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
pub(crate) mod tests;
