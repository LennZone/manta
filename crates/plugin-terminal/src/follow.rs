//! The live-follow state machine, kept free of IO so it can be unit tested.
//!
//! Paths come in pairs: `raw` is what we type / send to the host (it keeps
//! the user's spelling, e.g. `/tmp`), `canon` is the canonical form used for
//! every comparison (e.g. `/private/tmp`).

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dir {
    pub raw: PathBuf,
    pub canon: PathBuf,
}

impl Dir {
    pub fn new(raw: impl Into<PathBuf>, canon: impl Into<PathBuf>) -> Self {
        Dir {
            raw: raw.into(),
            canon: canon.into(),
        }
    }

    /// For tests and already-canonical paths.
    pub fn same(p: impl Into<PathBuf>) -> Self {
        let p = p.into();
        Dir {
            raw: p.clone(),
            canon: p,
        }
    }
}

#[derive(Debug, Default)]
pub struct Follow {
    /// Where the shell is (last OSC 7, or assumed after a cd we typed into a
    /// shell without integration).
    pub shell_cwd: Option<Dir>,
    /// What the pane above shows; `None` for remote panes.
    pub pane_dir: Option<Dir>,
    /// Latest directory the shell should move to once it is idle.
    pub pending: Option<Dir>,
    /// A cd we typed and whose OSC 7 echo we have not seen yet.
    pub in_flight: Option<Dir>,
    seen_cwd: bool,
}

impl Follow {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pane above now shows `dir` (`None`: a remote folder).
    pub fn pane_changed(&mut self, dir: Option<Dir>) {
        self.pane_dir = dir.clone();
        let Some(dir) = dir else {
            self.pending = None;
            return;
        };
        let target = &dir.canon;
        let shell_there = self.in_flight.as_ref().map(|d| &d.canon) == Some(target)
            || (self.in_flight.is_none()
                && self.shell_cwd.as_ref().map(|d| &d.canon) == Some(target));
        self.pending = if shell_there { None } else { Some(dir) };
    }

    /// If the shell is idle and a target is waiting, hand it out for typing.
    pub fn take_cd(&mut self, idle: bool) -> Option<Dir> {
        if !idle || self.in_flight.is_some() {
            return None;
        }
        let dir = self.pending.take()?;
        self.in_flight = Some(dir.clone());
        Some(dir)
    }

    /// A cd we typed into a shell that will never report back: assume it
    /// worked.
    pub fn assume_arrived(&mut self) {
        if let Some(d) = self.in_flight.take() {
            self.shell_cwd = Some(d);
        }
        self.seen_cwd = true;
    }

    /// Give up waiting for the echo of our cd (e.g. the command was eaten).
    pub fn clear_in_flight(&mut self) {
        self.in_flight = None;
    }

    /// The shell reported its directory (OSC 7). Returns a directory the pane
    /// should navigate to (reverse follow), if any.
    pub fn cwd_reported(&mut self, dir: Dir, reverse: bool) -> Option<Dir> {
        let first = !self.seen_cwd;
        self.seen_cwd = true;
        self.shell_cwd = Some(dir.clone());

        if self.in_flight.take().is_some() {
            // Our own cd's echo (or its failure): never bounce it back.
            if self.pending.as_ref().map(|p| &p.canon) == Some(&dir.canon) {
                self.pending = None;
            }
            return None;
        }
        if self.pending.as_ref().map(|p| &p.canon) == Some(&dir.canon) {
            self.pending = None;
            return None;
        }
        let pane = self.pane_dir.as_ref()?;
        if pane.canon == dir.canon {
            return None;
        }
        if first {
            // The rc files moved the shell somewhere else: the pane wins.
            self.pending = Some(pane.clone());
            return None;
        }
        if self.pending.is_some() || !reverse {
            // The pane changed while a program ran; that newer intent wins
            // and is applied once the shell is idle.
            return None;
        }
        self.pane_dir = Some(dir.clone());
        Some(dir)
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(p: &str) -> Dir {
        Dir::same(p)
    }

    /// Shell started in the pane's folder and reported it.
    fn started(at: &str) -> Follow {
        let mut f = Follow::new();
        f.pane_changed(Some(d(at)));
        assert_eq!(f.cwd_reported(d(at), true), None);
        assert!(!f.has_pending());
        f
    }

    #[test]
    fn idle_follow_and_echo_suppression() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/b")));
        assert!(f.has_pending());
        assert_eq!(f.take_cd(true), Some(d("/b")));
        // While in flight nothing else is typed.
        assert_eq!(f.take_cd(true), None);
        // The echo does not navigate the pane back.
        assert_eq!(f.cwd_reported(d("/b"), true), None);
        assert!(!f.has_pending());
        assert_eq!(f.in_flight, None);
    }

    #[test]
    fn busy_keeps_latest_target_only() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/b")));
        f.pane_changed(Some(d("/c")));
        f.pane_changed(Some(d("/d")));
        assert_eq!(f.take_cd(false), None);
        assert_eq!(f.pending, Some(d("/d")));
        // vim exits; the prompt reports the old cwd: no navigation, the
        // pending target survives.
        assert_eq!(f.cwd_reported(d("/a"), true), None);
        assert_eq!(f.take_cd(true), Some(d("/d")));
    }

    #[test]
    fn going_back_cancels_pending() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/b")));
        f.pane_changed(Some(d("/a")));
        assert!(!f.has_pending());
        assert_eq!(f.take_cd(true), None);
    }

    #[test]
    fn rapid_navigation_while_in_flight() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/b")));
        assert_eq!(f.take_cd(true), Some(d("/b")));
        f.pane_changed(Some(d("/c")));
        assert_eq!(f.take_cd(true), None, "wait for the echo first");
        assert_eq!(f.cwd_reported(d("/b"), true), None);
        assert_eq!(f.take_cd(true), Some(d("/c")));
        assert_eq!(f.cwd_reported(d("/c"), true), None);
        assert!(!f.has_pending());
    }

    #[test]
    fn pane_echo_of_in_flight_is_noop() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/b")));
        f.take_cd(true);
        // Host re-sends the same dir (e.g. tab switch back) before the echo.
        f.pane_changed(Some(d("/b")));
        assert!(!f.has_pending());
    }

    #[test]
    fn user_cd_navigates_pane_once() {
        let mut f = started("/a");
        assert_eq!(f.cwd_reported(d("/x"), true), Some(d("/x")));
        // The host follows and tells us: no cd back.
        f.pane_changed(Some(d("/x")));
        assert!(!f.has_pending());
        assert_eq!(f.take_cd(true), None);
        // Same prompt again (e.g. Enter on an empty line): nothing.
        assert_eq!(f.cwd_reported(d("/x"), true), None);
    }

    #[test]
    fn reverse_follow_can_be_disabled() {
        let mut f = started("/a");
        assert_eq!(f.cwd_reported(d("/x"), false), None);
        assert!(!f.has_pending());
    }

    #[test]
    fn canonical_comparison() {
        let mut f = Follow::new();
        f.pane_changed(Some(Dir::new("/tmp/p", "/private/tmp/p")));
        assert_eq!(
            f.cwd_reported(Dir::new("/private/tmp/p", "/private/tmp/p"), true),
            None
        );
        assert!(!f.has_pending());
    }

    #[test]
    fn rc_files_moving_the_shell_are_overridden() {
        let mut f = Follow::new();
        f.pane_changed(Some(d("/a")));
        // ~/.zshrc does `cd ~/projects`.
        assert_eq!(f.cwd_reported(d("/home/projects"), true), None);
        assert_eq!(f.take_cd(true), Some(d("/a")));
    }

    #[test]
    fn failed_cd_does_not_bounce() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/gone")));
        assert_eq!(f.take_cd(true), Some(d("/gone")));
        // cd failed; shell still in /a.
        assert_eq!(f.cwd_reported(d("/a"), true), None);
        assert!(!f.has_pending());
    }

    #[test]
    fn user_cd_while_pending_loses_to_pane() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/p")));
        // Busy; then the command ends in a different dir the user cd'd to.
        assert_eq!(f.cwd_reported(d("/u"), true), None);
        assert_eq!(f.take_cd(true), Some(d("/p")));
    }

    #[test]
    fn remote_pane() {
        let mut f = started("/a");
        f.pane_changed(Some(d("/b")));
        f.pane_changed(None);
        assert!(!f.has_pending());
        // No reverse navigation into a remote backend.
        assert_eq!(f.cwd_reported(d("/x"), true), None);
    }

    #[test]
    fn shell_without_integration() {
        let mut f = Follow::new();
        f.shell_cwd = Some(d("/a"));
        f.pane_changed(Some(d("/b")));
        assert_eq!(f.take_cd(true), Some(d("/b")));
        f.assume_arrived();
        assert_eq!(f.shell_cwd, Some(d("/b")));
        f.pane_changed(Some(d("/b")));
        assert!(!f.has_pending());
    }
}
