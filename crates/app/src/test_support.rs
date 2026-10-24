//! Shared knobs for the unit and UI tests.

use std::time::Duration;

/// How long a test waits for background work (listings, copies, thumbnail
/// workers, plugin threads) before it fails.
///
/// Waits end as soon as their condition holds, so a generous limit costs
/// nothing when things work; it only has to be far above what a loaded CI
/// runner needs (`scripts/stress-tests.sh` runs the suite next to busy
/// loops). `MANTA_TEST_WAIT_SECS` overrides it.
pub fn wait() -> Duration {
    std::env::var("MANTA_TEST_WAIT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(60))
}
