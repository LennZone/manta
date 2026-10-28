//! Free space in the status bar ("18 GB available", like Finder): the disk
//! of a local folder, the iCloud or Google Drive account's quota, a server's
//! disk (SFTP, SMB) or WebDAV quota. Asked off the UI thread, kept for a
//! while per folder and asked again after a transfer.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use manta_core::Space;

use crate::pane::Location;

/// How long an answer is shown before it is asked again.
const LOCAL_TTL: Duration = Duration::from_secs(10);
const REMOTE_TTL: Duration = Duration::from_secs(60);

/// Connection (`None`: this Mac) and folder.
pub type Key = (Option<u64>, String);

#[derive(Default)]
pub struct SpaceCache {
    known: HashMap<Key, (Instant, Option<Space>)>,
    asking: HashSet<Key>,
}

impl SpaceCache {
    /// Archives have no free space of their own.
    pub fn key(loc: &Location) -> Option<Key> {
        loc.archive
            .is_none()
            .then(|| (loc.conn, loc.path.as_str().to_string()))
    }

    /// The last answer for `key` (also while a newer one is asked for).
    pub fn get(&self, key: &Key) -> Option<Space> {
        self.known.get(key).and_then(|(_, s)| *s)
    }

    /// Whether `key` should be asked now (unknown or old, not in flight);
    /// marks it as asked.
    pub fn should_ask(&mut self, key: &Key, now: Instant) -> bool {
        if self.asking.contains(key) {
            return false;
        }
        let ttl = if key.0.is_none() {
            LOCAL_TTL
        } else {
            REMOTE_TTL
        };
        let fresh = self
            .known
            .get(key)
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) < ttl);
        if fresh {
            return false;
        }
        self.asking.insert(key.clone());
        true
    }

    /// An answer arrived (errors count as "unknown").
    pub fn answer(&mut self, key: Key, space: Option<Space>, now: Instant) {
        self.asking.remove(&key);
        if self.known.len() > 256 {
            self.known.clear();
        }
        self.known.insert(key, (now, space));
    }

    /// Something was copied, moved or deleted: ask again next time, but
    /// keep showing the old numbers until the new ones arrive.
    pub fn stale(&mut self) {
        let old = Instant::now()
            .checked_sub(REMOTE_TTL + Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        for (at, _) in self.known.values_mut() {
            *at = old;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        (None, "/tmp".into())
    }

    #[test]
    fn asks_once_until_answered_then_after_the_ttl() {
        let mut c = SpaceCache::default();
        let t0 = Instant::now();
        assert!(c.should_ask(&key(), t0));
        assert!(!c.should_ask(&key(), t0), "already in flight");
        let s = Space {
            available: 5,
            total: Some(10),
        };
        c.answer(key(), Some(s), t0);
        assert_eq!(c.get(&key()), Some(s));
        assert!(!c.should_ask(&key(), t0 + Duration::from_secs(1)));
        assert!(c.should_ask(&key(), t0 + LOCAL_TTL + Duration::from_secs(1)));
        // The old number stays while the new one is asked.
        assert_eq!(c.get(&key()), Some(s));
    }

    #[test]
    fn stale_asks_again_but_keeps_the_number() {
        let mut c = SpaceCache::default();
        let now = Instant::now();
        let remote: Key = (Some(3), "/".into());
        assert!(c.should_ask(&remote, now));
        c.answer(
            remote.clone(),
            Some(Space {
                available: 1,
                total: None,
            }),
            now,
        );
        assert!(!c.should_ask(&remote, Instant::now()));
        c.stale();
        assert!(c.should_ask(&remote, Instant::now()));
        assert!(c.get(&remote).is_some());
    }
}
