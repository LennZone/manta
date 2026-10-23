//! When to check, and the small state the app persists between launches.

use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::manifest::{Channel, Release};

/// Check interval for callers that throttle checks themselves (the app's
/// default is "every launch", see [`UpdateState::interval_hours`]).
pub const DEFAULT_CHECK_INTERVAL: Duration = Duration::hours(24);

/// Whether an automatic check is due. Checks when there was none yet, when the
/// interval has passed, or when the clock went backwards by more than a day
/// (the stored time is then unreliable).
pub fn should_check(
    last_check: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    interval: Duration,
) -> bool {
    match last_check {
        None => true,
        Some(last) if last > now + Duration::days(1) => true,
        Some(last) => now.signed_duration_since(last) >= interval,
    }
}

/// A ZIP that was downloaded and verified but not installed yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedUpdate {
    pub release: Release,
    pub zip: PathBuf,
}

/// Updater state the app stores (e.g. as JSON next to its settings).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    /// Check automatically (at launch, see [`UpdateState::interval_hours`]).
    pub auto_check: bool,
    /// Minimum hours between automatic checks; 0 (the default) checks on
    /// every launch. Internal: the app has no UI for it.
    pub interval_hours: u32,
    pub channel: Channel,
    pub last_check: Option<DateTime<Utc>>,
    /// "Skip This Version": releases up to and including it aren't offered.
    pub skipped_version: Option<Version>,
    pub downloaded: Option<DownloadedUpdate>,
}

impl Default for UpdateState {
    fn default() -> Self {
        UpdateState {
            auto_check: true,
            interval_hours: 0,
            channel: Channel::Stable,
            last_check: None,
            skipped_version: None,
            downloaded: None,
        }
    }
}

impl UpdateState {
    pub fn interval(&self) -> Duration {
        Duration::hours(i64::from(self.interval_hours))
    }

    /// Whether an automatic check should run now (always, when
    /// `interval_hours` is 0 and `auto_check` is on).
    pub fn check_due(&self, now: DateTime<Utc>) -> bool {
        self.auto_check
            && (self.interval_hours == 0 || should_check(self.last_check, now, self.interval()))
    }

    /// Drops a remembered download that is gone or not newer than `current`
    /// (e.g. after the update was installed). Returns true if something changed.
    pub fn forget_stale_download(&mut self, current: &Version) -> bool {
        let stale = self
            .downloaded
            .as_ref()
            .is_some_and(|d| d.release.version <= *current || !d.zip.is_file());
        if stale {
            if let Some(d) = self.downloaded.take() {
                let _ = std::fs::remove_file(&d.zip);
            }
        }
        stale
    }
}
