//! # manta-updater
//!
//! Self-update engine for Manta (no UI). Updates come from a static feed on
//! any HTTPS web space: `update.json` + `update.json.sig` + one ZIP per
//! version (`ditto -c -k --keepParent Manta.app`). See `docs/updates.md`.
//!
//! Pipeline: [`check()`] (signed feed, semver, channel, `min_macos`, skipped
//! version) → [`download()`] (streaming, resumable, size + SHA-256 + Ed25519)
//! → [`prepare()`] (unzip with `ditto`, bundle ID, version, `codesign`, Team
//! ID) → [`install()`] + [`InstallPlan::apply`] (atomic swap, Trash, optional
//! admin prompt) → [`relaunch_after_exit`].
//!
//! The UI talks to the [`Updater`] facade: all work runs on background
//! threads; [`Updater::poll`] returns [`UpdateStatus`] changes.

pub mod bundle;
pub mod check;
pub mod download;
pub mod error;
pub mod install;
pub mod keys;
pub mod manifest;
pub mod prepare;
pub mod schedule;
pub mod updater;

pub use check::{
    check, check_with, fetch_manifest, select_release, validate_url, CheckOptions, OsVersion,
};
pub use download::{download, download_with, verify_file, VerifiedDownload};
pub use error::{Result, UpdateError};
pub use install::{
    install, relaunch_after_exit, BackupDisposal, InstallOutcome, InstallPlan, LocationProblem,
};
pub use manifest::{Channel, Manifest, Notes, Release, DEFAULT_APP_ID, SCHEMA};
pub use prepare::{prepare, prepare_with, PrepareOptions, PreparedApp};
pub use schedule::{should_check, DownloadedUpdate, UpdateState, DEFAULT_CHECK_INTERVAL};
pub use semver::Version;
pub use updater::{
    configured_feed_url, current_version, default_download_dir, UpdateStatus, Updater,
    UpdaterConfig,
};

#[cfg(test)]
mod unit_tests;
