//! Unpacking a verified ZIP and checking the app inside.

use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use crate::bundle::{self, Signing};
use crate::download::VerifiedDownload;
use crate::error::{Result, UpdateError};

/// An unpacked, checked app, ready for [`crate::install`]. The temp folder
/// lives as long as this value.
#[derive(Debug)]
pub struct PreparedApp {
    pub app_path: PathBuf,
    pub version: Version,
    pub bundle_id: String,
    pub signing: Signing,
    _dir: tempfile::TempDir,
}

/// What [`prepare_with`] checks the new app against.
#[derive(Clone, Debug)]
pub struct PrepareOptions {
    pub expected_bundle_id: String,
    /// The running app, for the Team ID rule. `None`: treated as ad-hoc.
    pub current_bundle: Option<PathBuf>,
    /// Where the temp folder is created (default: next to the ZIP).
    pub work_dir: Option<PathBuf>,
}

/// [`prepare_with`] for the running app (bundle ID `com.manta-filemanager.manta` unless the
/// running bundle says otherwise).
pub fn prepare(download: &VerifiedDownload) -> Result<PreparedApp> {
    let current = crate::install::running_bundle().ok();
    let expected = current
        .as_deref()
        .and_then(|b| bundle::read_bundle_info(b).ok())
        .map(|i| i.bundle_id)
        .unwrap_or_else(|| crate::manifest::DEFAULT_APP_ID.to_string());
    prepare_with(
        download,
        &PrepareOptions {
            expected_bundle_id: expected,
            current_bundle: current,
            work_dir: None,
        },
    )
}

/// Unpacks with `ditto -x -k` into a temp folder and checks: exactly one
/// `.app`, bundle ID, version = release version, `codesign --verify --deep
/// --strict`, and the Team ID rule ([`bundle::check_team_policy`]). Removes
/// the quarantine attribute.
pub fn prepare_with(download: &VerifiedDownload, opts: &PrepareOptions) -> Result<PreparedApp> {
    let zip = download.path();
    let release = download.release();
    let parent = opts
        .work_dir
        .clone()
        .unwrap_or_else(|| zip.parent().unwrap_or(Path::new("/tmp")).to_path_buf());
    std::fs::create_dir_all(&parent).map_err(|e| UpdateError::io(parent.display(), e))?;
    let dir = tempfile::Builder::new()
        .prefix("manta-update-")
        .tempdir_in(&parent)
        .map_err(|e| UpdateError::io(parent.display(), e))?;

    let out = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(zip)
        .arg(dir.path())
        .output()
        .map_err(|e| UpdateError::io("ditto", e))?;
    if !out.status.success() {
        return Err(UpdateError::Bundle(format!(
            "could not unpack ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    let apps: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .map_err(|e| UpdateError::io(dir.path().display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "app"))
        .collect();
    let app = match apps.as_slice() {
        [one] => one.clone(),
        [] => return Err(UpdateError::Bundle("the ZIP contains no .app".into())),
        _ => {
            return Err(UpdateError::Bundle(
                "the ZIP contains more than one .app".into(),
            ))
        }
    };
    if std::fs::symlink_metadata(&app)
        .map(|m| !m.is_dir())
        .unwrap_or(true)
    {
        return Err(UpdateError::Bundle("the .app is not a folder".into()));
    }

    let info = bundle::read_bundle_info(&app)?;
    if info.bundle_id != opts.expected_bundle_id {
        return Err(UpdateError::Bundle(format!(
            "bundle ID is {}, expected {}",
            info.bundle_id, opts.expected_bundle_id
        )));
    }
    let version = Version::parse(&info.version).map_err(|_| {
        UpdateError::Bundle(format!(
            "version \"{}\" is not a valid version",
            info.version
        ))
    })?;
    if version != release.version {
        return Err(UpdateError::Bundle(format!(
            "it is version {version}, but the feed promised {}",
            release.version
        )));
    }

    // Quarantine first, so the check sees the bundle as it will be installed.
    let _ = Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&app)
        .output();
    bundle::verify_code_signature(&app)?;
    let new_signing = bundle::signing_of(&app)?;
    let current_signing = match &opts.current_bundle {
        Some(b) => bundle::signing_of(b)?,
        None => Signing::AdHoc,
    };
    bundle::check_team_policy(&current_signing, &new_signing)?;

    Ok(PreparedApp {
        app_path: app,
        version,
        bundle_id: info.bundle_id,
        signing: new_signing,
        _dir: dir,
    })
}
