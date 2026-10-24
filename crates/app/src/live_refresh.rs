//! Live folder refresh (like Finder): local folders shown in a pane update
//! by themselves when other apps create, delete, rename or change files.
//!
//! - Watches (macOS FSEvents through `notify`) the local folders of every
//!   open tab, plus the folders of the Columns view. Folders no tab shows
//!   any more are unwatched. A tab in the background is not reloaded: it
//!   is marked stale (`Tab::stale`) and reloads quietly as soon as it is
//!   shown again, so switching to it needs no ⌘R.
//!   Servers, containers, cloud APIs and archives are never watched (⌘R).
//! - Events are coalesced per folder: a folder is reloaded once it has been
//!   quiet for [`QUIET`], or at the latest [`MAX_DELAY`] after its first
//!   event, so copying thousands of files (by Manta or anyone else) reloads
//!   a pane about once a second instead of thousands of times.
//! - The app reloads quietly (no spinner) and keeps selection, cursor,
//!   scroll position and type-ahead (`Tab::apply_listing` for the same
//!   folder). It defers while a rename is being typed in that folder and
//!   skips a reload when a listing started after the last event anyway.
//!
//! The clock is passed in ([`LiveRefresh::due`]) so the debounce logic is
//! tested without sleeping.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;

/// A folder is reloaded after this long without further events.
pub const QUIET: Duration = Duration::from_millis(200);
/// ... but no later than this after its first event (steady streams).
pub const MAX_DELAY: Duration = Duration::from_millis(1000);

/// Events of one folder since its last reload.
#[derive(Debug, Clone, Copy)]
struct Burst {
    first: Instant,
    last: Instant,
}

pub struct LiveRefresh {
    watcher: Option<Box<dyn notify::Watcher + Send>>,
    rx: Receiver<PathBuf>,
    /// Watched folders: canonical path -> path as the pane shows it.
    watched: HashMap<PathBuf, PathBuf>,
    /// The folders asked for last (as shown), to skip unchanged syncs.
    wanted: BTreeSet<PathBuf>,
    pending: HashMap<PathBuf, Burst>,
}

impl LiveRefresh {
    /// `enabled: false` (unit tests by default) watches nothing.
    pub fn new(ctx: &egui::Context, enabled: bool) -> Self {
        let (tx, rx) = channel::<PathBuf>();
        let watcher = if enabled {
            let ctx = ctx.clone();
            let handler = move |res: notify::Result<notify::Event>| {
                let Ok(ev) = res else { return };
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for p in ev.paths {
                    // The folder that lists `p`, and `p` itself (a watched
                    // folder that was renamed or changed).
                    if let Some(parent) = p.parent() {
                        let _ = tx.send(parent.to_path_buf());
                    }
                    let _ = tx.send(p);
                }
                // Wake the UI; it reloads once the burst is quiet.
                ctx.request_repaint_after(QUIET);
            };
            match notify::recommended_watcher(handler) {
                Ok(w) => Some(Box::new(w) as Box<dyn notify::Watcher + Send>),
                Err(e) => {
                    log::warn!("live refresh: cannot start the file system watcher: {e}");
                    None
                }
            }
        } else {
            None
        };
        LiveRefresh {
            watcher,
            rx,
            watched: HashMap::new(),
            wanted: BTreeSet::new(),
            pending: HashMap::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.watcher.is_some()
    }

    /// Watch exactly `folders` (as the panes show them).
    pub fn sync(&mut self, folders: BTreeSet<PathBuf>) {
        if folders == self.wanted {
            return;
        }
        let Some(w) = self.watcher.as_mut() else {
            self.wanted = folders;
            return;
        };
        let canon: HashMap<PathBuf, PathBuf> = folders
            .iter()
            .filter_map(|f| std::fs::canonicalize(f).ok().map(|c| (c, f.clone())))
            .collect();
        let gone: Vec<PathBuf> = self
            .watched
            .keys()
            .filter(|c| !canon.contains_key(*c))
            .cloned()
            .collect();
        for c in gone {
            let _ = w.unwatch(&c);
            self.watched.remove(&c);
            self.pending.remove(&c);
        }
        for (c, shown) in canon {
            if let Some(s) = self.watched.get_mut(&c) {
                *s = shown;
                continue;
            }
            match w.watch(&c, notify::RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watched.insert(c, shown);
                }
                Err(e) => log::debug!("live refresh: cannot watch {}: {e}", c.display()),
            }
        }
        log::debug!("live refresh: watching {} folder(s)", self.watched.len());
        self.wanted = folders;
    }

    /// Record events that arrived (call once per frame).
    pub fn poll(&mut self, now: Instant) {
        while let Ok(p) = self.rx.try_recv() {
            self.note(&p, now);
        }
    }

    /// A change in (or of) `path`, if it is a watched folder.
    fn note(&mut self, path: &Path, now: Instant) {
        if !self.watched.contains_key(path) {
            return;
        }
        self.pending
            .entry(path.to_path_buf())
            .and_modify(|b| b.last = now)
            .or_insert(Burst {
                first: now,
                last: now,
            });
    }

    /// Folders (as shown) whose burst of events is over at `now`, with the
    /// time of their last event, and when to look again for the others.
    pub fn due(&mut self, now: Instant) -> (Vec<(PathBuf, Instant)>, Option<Duration>) {
        let mut out = Vec::new();
        let mut next: Option<Duration> = None;
        self.pending.retain(|c, b| {
            let ready_at = (b.last + QUIET).min(b.first + MAX_DELAY);
            if now >= ready_at {
                if let Some(shown) = self.watched.get(c) {
                    out.push((shown.clone(), b.last));
                }
                false
            } else {
                let wait = ready_at - now;
                next = Some(next.map_or(wait, |n| n.min(wait)));
                true
            }
        });
        (out, next)
    }

    /// Put a folder back (its reload had to wait, e.g. a rename is being
    /// typed); it is due again after [`QUIET`].
    pub fn defer(&mut self, shown: &Path, last: Instant, now: Instant) {
        if let Some((c, _)) = self.watched.iter().find(|(_, s)| s.as_path() == shown) {
            let c = c.clone();
            self.pending.insert(
                c,
                Burst {
                    first: now,
                    last: last.max(now),
                },
            );
        }
    }

    /// Some folder has events waiting for their reload.
    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Test seam: an event for `path` (canonical) at `now`.
    #[cfg(test)]
    pub fn inject(&mut self, path: &Path, now: Instant) {
        self.note(path, now);
    }

    /// Test seam: pretend `folder` is watched (no watcher needed).
    #[cfg(test)]
    pub fn fake_watch(&mut self, folder: &Path) {
        self.watched
            .insert(folder.to_path_buf(), folder.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lr() -> LiveRefresh {
        let mut l = LiveRefresh::new(&egui::Context::default(), false);
        l.fake_watch(Path::new("/w"));
        l
    }

    #[test]
    fn a_burst_is_reloaded_once_when_quiet() {
        let mut l = lr();
        let t0 = Instant::now();
        for i in 0..50 {
            l.inject(Path::new("/w"), t0 + Duration::from_millis(i));
        }
        l.inject(Path::new("/elsewhere"), t0);
        let (due, next) = l.due(t0 + Duration::from_millis(100));
        assert!(due.is_empty());
        assert_eq!(
            next,
            Some(QUIET + Duration::from_millis(49) - Duration::from_millis(100))
        );
        let (due, next) = l.due(t0 + Duration::from_millis(49) + QUIET);
        assert_eq!(due.len(), 1, "one reload for 50 events");
        assert_eq!(due[0].0, PathBuf::from("/w"));
        assert_eq!(next, None);
        assert!(
            l.due(t0 + Duration::from_secs(5)).0.is_empty(),
            "nothing left"
        );
    }

    #[test]
    fn a_steady_stream_reloads_about_once_a_second() {
        let mut l = lr();
        let t0 = Instant::now();
        let mut reloads = 0;
        // An event every 50 ms for 5 s (a long copy into the folder).
        for i in 0..100u64 {
            let now = t0 + Duration::from_millis(i * 50);
            l.inject(Path::new("/w"), now);
            reloads += l.due(now).0.len();
        }
        assert!((4..=6).contains(&reloads), "{reloads} reloads in 5 s");
    }

    #[test]
    fn deferred_folders_come_back() {
        let mut l = lr();
        let t0 = Instant::now();
        l.inject(Path::new("/w"), t0);
        let (due, _) = l.due(t0 + QUIET);
        assert_eq!(due.len(), 1);
        l.defer(Path::new("/w"), due[0].1, t0 + QUIET);
        assert!(l.due(t0 + QUIET).0.is_empty());
        assert_eq!(l.due(t0 + QUIET * 2).0.len(), 1);
    }

    #[test]
    fn disabled_watches_nothing() {
        let mut l = LiveRefresh::new(&egui::Context::default(), false);
        assert!(!l.enabled());
        l.sync([PathBuf::from("/tmp")].into_iter().collect());
        l.poll(Instant::now());
        assert!(l.due(Instant::now()).0.is_empty());
    }
}
