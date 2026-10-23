//! The update feed (`update.json`), its validation and the canonical messages
//! that are signed with Ed25519.
//!
//! ```json
//! {
//!   "schema": 1,
//!   "app_id": "com.manta-filemanager.manta",
//!   "channel": "stable",
//!   "generated": "2026-10-21T10:00:00Z",
//!   "releases": [{
//!     "version": "0.2.0",
//!     "channel": "stable",
//!     "published": "2026-10-21T10:00:00Z",
//!     "min_macos": "11.0",
//!     "notes": { "en": "Markdown", "de": "Markdown" },
//!     "url": "Manta-0.2.0.zip",
//!     "size": 12345678,
//!     "sha256": "<64 lowercase hex>",
//!     "signature": "<base64 Ed25519 over release_message()>"
//!   }]
//! }
//! ```
//!
//! The whole file is signed as well: `update.json.sig` next to it holds the
//! base64 Ed25519 signature over `manifest_message(<exact bytes of update.json>)`.
//! Signing the raw bytes avoids any JSON canonicalisation question.

use chrono::{DateTime, FixedOffset};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::error::{Result, UpdateError};

/// The schema this build understands. A feed with a higher schema is refused
/// with a hint to update manually.
pub const SCHEMA: u32 = 1;
/// Default `CFBundleIdentifier` / `app_id`.
pub const DEFAULT_APP_ID: &str = "com.manta-filemanager.manta";
/// Domain separation for release signatures.
pub const RELEASE_CONTEXT: &str = "manta-update-release-v1";
/// Domain separation for the manifest signature.
pub const MANIFEST_CONTEXT: &str = "manta-update-manifest-v1";
/// Refuse absurd sizes before downloading anything.
pub const MAX_RELEASE_SIZE: u64 = 2 << 30;

/// Update channel. `Stable` sees stable releases only, `Beta` sees both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    /// Whether a client on `self` is offered a release on `release`.
    pub fn accepts(self, release: Channel) -> bool {
        match self {
            Channel::Stable => release == Channel::Stable,
            Channel::Beta => true,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    pub fn parse(s: &str) -> Option<Channel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            _ => None,
        }
    }
}

/// Release notes in Markdown. English is required, German optional.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notes {
    pub en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub de: Option<String>,
}

impl Notes {
    /// Notes for a UI language code (`"de"`, `"de-DE"`, `"en"`, …); falls back to English.
    pub fn for_language(&self, lang: &str) -> &str {
        if lang.to_ascii_lowercase().starts_with("de") {
            if let Some(de) = self.de.as_deref().filter(|s| !s.trim().is_empty()) {
                return de;
            }
        }
        &self.en
    }
}

/// One downloadable version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: Version,
    /// Missing: `beta` for pre-release versions (`1.2.0-beta.1`), else `stable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,
    pub published: DateTime<FixedOffset>,
    /// Minimum macOS version, e.g. `"11.0"`.
    pub min_macos: String,
    pub notes: Notes,
    /// Absolute `https://` URL, or relative to the feed URL.
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the ZIP.
    pub sha256: String,
    /// Base64 Ed25519 signature over [`release_message`].
    pub signature: String,
}

impl Release {
    pub fn effective_channel(&self) -> Channel {
        self.channel.unwrap_or(if self.version.pre.is_empty() {
            Channel::Stable
        } else {
            Channel::Beta
        })
    }

    /// File name used for the download (`Manta-<version>.zip`).
    pub fn file_name(&self) -> String {
        // semver only allows [0-9A-Za-z.+-], so this is a safe file name.
        format!("Manta-{}.zip", self.version)
    }
}

/// The parsed `update.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub app_id: String,
    /// The feed's default channel (informational; each release carries its own).
    #[serde(default)]
    pub channel: Channel,
    /// When the publish script wrote this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<DateTime<FixedOffset>>,
    pub releases: Vec<Release>,
}

impl Manifest {
    /// Parses and validates `update.json`. Does NOT check the signature; see
    /// [`crate::keys::verify_manifest`].
    pub fn parse(bytes: &[u8]) -> Result<Manifest> {
        // Peek at the schema first, so a future format gives a useful message
        // instead of a serde error about some field.
        #[derive(Deserialize)]
        struct Head {
            schema: Option<u32>,
        }
        let head: Head = serde_json::from_slice(bytes)
            .map_err(|e| UpdateError::Manifest(format!("not valid JSON ({e})")))?;
        match head.schema {
            None => return Err(UpdateError::Manifest("\"schema\" is missing".into())),
            Some(s) if s > SCHEMA => {
                return Err(UpdateError::Manifest(format!(
                    "feed format {s} needs a newer Manta; please download the latest version from the website"
                )))
            }
            Some(s) if s < 1 => return Err(UpdateError::Manifest(format!("unknown feed format {s}"))),
            _ => {}
        }
        let m: Manifest =
            serde_json::from_slice(bytes).map_err(|e| UpdateError::Manifest(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    /// Structural checks: hashes, signatures, sizes, duplicates, versions.
    pub fn validate(&self) -> Result<()> {
        let bad = |msg: String| Err(UpdateError::Manifest(msg));
        if self.schema != SCHEMA {
            return bad(format!("unsupported schema {}", self.schema));
        }
        if self.app_id.trim().is_empty() {
            return bad("\"app_id\" is empty".into());
        }
        let mut seen = std::collections::HashSet::new();
        for r in &self.releases {
            let v = &r.version;
            if !seen.insert(v.clone()) {
                return bad(format!("version {v} is listed twice"));
            }
            if r.sha256.len() != 64
                || !r
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return bad(format!("{v}: sha256 must be 64 lowercase hex characters"));
            }
            if r.size == 0 || r.size > MAX_RELEASE_SIZE {
                return bad(format!("{v}: implausible size {}", r.size));
            }
            if r.url.trim().is_empty() {
                return bad(format!("{v}: url is empty"));
            }
            if crate::check::OsVersion::parse(&r.min_macos).is_none() {
                return bad(format!("{v}: invalid min_macos \"{}\"", r.min_macos));
            }
            if r.notes.en.trim().is_empty() {
                return bad(format!("{v}: English release notes are missing"));
            }
            crate::keys::decode_signature(&r.signature)
                .map_err(|e| UpdateError::Manifest(format!("{v}: {e}")))?;
        }
        Ok(())
    }

    /// Pretty JSON as written by the publish tool (newest release first).
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("manifest serialises");
        s.push('\n');
        s
    }
}

/// The exact bytes a release signature covers. Binds the app, version,
/// channel, minimum OS, size and hash, so a signature can't be replayed for
/// another version, another app or a relabelled channel. The URL is not
/// included (mirrors are fine; the hash pins the content).
pub fn release_message(app_id: &str, release: &Release) -> Vec<u8> {
    format!(
        "{RELEASE_CONTEXT}\napp_id={}\nversion={}\nchannel={}\nmin_macos={}\nsize={}\nsha256={}\n",
        app_id,
        release.version,
        release.effective_channel().as_str(),
        release.min_macos.trim(),
        release.size,
        release.sha256.to_ascii_lowercase(),
    )
    .into_bytes()
}

/// The exact bytes the manifest signature (`update.json.sig`) covers.
pub fn manifest_message(manifest_bytes: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(MANIFEST_CONTEXT.len() + 1 + manifest_bytes.len());
    m.extend_from_slice(MANIFEST_CONTEXT.as_bytes());
    m.push(b'\n');
    m.extend_from_slice(manifest_bytes);
    m
}
