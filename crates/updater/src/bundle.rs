//! Reading app bundles: Info.plist values and code-signature details.

use std::path::Path;
use std::process::Command;

use crate::error::{Result, UpdateError};

/// The Info.plist values the updater cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleInfo {
    pub bundle_id: String,
    /// `CFBundleShortVersionString`
    pub version: String,
    pub executable: Option<String>,
    /// `MantaUpdateFeedURL`, written by `scripts/build-release.sh`.
    pub feed_url: Option<String>,
}

/// Reads `<app>/Contents/Info.plist` (XML or binary) through `plutil`.
pub fn read_bundle_info(app: &Path) -> Result<BundleInfo> {
    let plist = app.join("Contents/Info.plist");
    if !plist.is_file() {
        return Err(UpdateError::Bundle(format!(
            "{} has no Contents/Info.plist",
            app.display()
        )));
    }
    let out = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .map_err(|e| UpdateError::io("plutil", e))?;
    if !out.status.success() {
        return Err(UpdateError::Bundle(format!(
            "unreadable Info.plist: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| UpdateError::Bundle(format!("unreadable Info.plist: {e}")))?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    Ok(BundleInfo {
        bundle_id: s("CFBundleIdentifier")
            .ok_or_else(|| UpdateError::Bundle("CFBundleIdentifier is missing".into()))?,
        version: s("CFBundleShortVersionString")
            .ok_or_else(|| UpdateError::Bundle("CFBundleShortVersionString is missing".into()))?,
        executable: s("CFBundleExecutable"),
        feed_url: s("MantaUpdateFeedURL").filter(|u| !u.trim().is_empty()),
    })
}

/// How a bundle is code-signed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signing {
    Unsigned,
    /// `codesign -s -` (no identity, no Team ID).
    AdHoc,
    /// Signed with a certificate; the Team ID if there is one.
    Identity {
        team_id: Option<String>,
    },
}

/// Parses `codesign -dv --verbose=2` output (it prints to stderr).
pub fn parse_codesign_display(text: &str) -> Signing {
    if text.contains("not signed at all") {
        return Signing::Unsigned;
    }
    let mut adhoc = false;
    let mut team = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "Signature=adhoc"
            || (line.starts_with("CodeDirectory")
                && line.contains("flags=")
                && line.contains("adhoc"))
        {
            adhoc = true;
        }
        if let Some(t) = line.strip_prefix("TeamIdentifier=") {
            if t != "not set" && !t.is_empty() {
                team = Some(t.to_string());
            }
        }
    }
    if adhoc && team.is_none() {
        Signing::AdHoc
    } else {
        Signing::Identity { team_id: team }
    }
}

/// Signature details of a bundle.
pub fn signing_of(app: &Path) -> Result<Signing> {
    let out = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=2"])
        .arg(app)
        .output()
        .map_err(|e| UpdateError::io("codesign", e))?;
    Ok(parse_codesign_display(&String::from_utf8_lossy(
        &out.stderr,
    )))
}

/// `codesign --verify --deep --strict`.
pub fn verify_code_signature(app: &Path) -> Result<()> {
    let out = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()
        .map_err(|e| UpdateError::io("codesign", e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(UpdateError::Bundle(format!(
            "its code signature is invalid ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// The Team ID rule: if the running app is signed by a team, the update must
/// be signed by the same team. An ad-hoc (or unsigned dev) running app accepts
/// an ad-hoc or team-signed update (the Ed25519 signature is what counts then).
pub fn check_team_policy(current: &Signing, new: &Signing) -> Result<()> {
    match (current, new) {
        (_, Signing::Unsigned) => Err(UpdateError::Bundle("the new app is not code-signed".into())),
        (Signing::Identity { team_id: Some(cur) }, Signing::Identity { team_id: Some(new) })
            if cur == new =>
        {
            Ok(())
        }
        (Signing::Identity { team_id: Some(cur) }, other) => Err(UpdateError::Bundle(format!(
            "the new app is signed by {}, but this copy of Manta is signed by team {cur}",
            match other {
                Signing::Identity { team_id: Some(t) } => format!("team {t}"),
                Signing::AdHoc => "nobody (ad-hoc)".into(),
                _ => "an unknown identity".into(),
            }
        ))),
        _ => Ok(()),
    }
}
