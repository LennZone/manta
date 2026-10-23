//! Software updates in the app, on top of the `manta-updater` engine
//! (see docs/updates.md):
//!
//! * a silent check at every launch (failures are only logged),
//! * Manta → "Check for Updates…" and the palette command, which open the
//!   update window and show the result there,
//! * the update window (`update_window.rs`): release notes, then download,
//!   install and relaunch, always only after the user confirmed,
//! * Settings → General → Updates (`update_window::settings_section`).
//!
//! [`Updates`] is the UI-free controller. It talks to the engine through the
//! small [`Backend`] trait, so tests drive it with a fake. Its state
//! ([`UpdateState`]) is stored as `update-state.json` next to `settings.json`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use manta_updater::{
    Channel, DownloadedUpdate, LocationProblem, Release, UpdateError, UpdateState, UpdateStatus,
    Updater, UpdaterConfig, Version,
};

use crate::aux_window::AuxWindow;

/// The engine as the app uses it (implemented by [`Updater`]; tests use a fake).
/// Every method returns immediately.
pub trait Backend {
    fn poll(&self) -> Vec<UpdateStatus>;
    fn check(&self, skipped: Option<Version>) -> bool;
    fn download_release(&self, release: Release) -> bool;
    fn restore_download(&self, release: Release, zip: PathBuf) -> bool;
    fn cancel(&self);
    fn install_and_relaunch(&self) -> bool;
    fn location_problem(&self) -> Option<LocationProblem>;
    fn last_error(&self) -> Option<UpdateError>;
    fn downloaded_zip(&self) -> Option<(Release, PathBuf)>;
    fn set_channel(&self, channel: Channel);
}

impl Backend for Updater {
    fn poll(&self) -> Vec<UpdateStatus> {
        Updater::poll(self)
    }
    fn check(&self, skipped: Option<Version>) -> bool {
        Updater::check(self, skipped)
    }
    fn download_release(&self, release: Release) -> bool {
        Updater::download_release(self, release)
    }
    fn restore_download(&self, release: Release, zip: PathBuf) -> bool {
        Updater::restore_download(self, release, zip)
    }
    fn cancel(&self) {
        Updater::cancel(self)
    }
    fn install_and_relaunch(&self) -> bool {
        Updater::install_and_relaunch(self)
    }
    fn location_problem(&self) -> Option<LocationProblem> {
        Updater::location_problem(self)
    }
    fn last_error(&self) -> Option<UpdateError> {
        Updater::last_error(self)
    }
    fn downloaded_zip(&self) -> Option<(Release, PathBuf)> {
        Updater::downloaded_zip(self)
    }
    fn set_channel(&self, channel: Channel) {
        Updater::set_channel(self, channel)
    }
}

/// Why this build doesn't update itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disabled {
    /// No public signing key compiled in (`packaging/update-public-key.txt`).
    NoKey,
    /// No feed URL configured.
    NoFeed,
    /// The feed URL or key is invalid (the engine's English message).
    Invalid(String),
}

impl Disabled {
    /// Classify the engine's "Updates are disabled: …" error.
    pub fn of(err: &UpdateError) -> Disabled {
        if manta_updater::keys::embedded_public_key().is_err() {
            Disabled::NoKey
        } else if manta_updater::configured_feed_url().is_none() {
            Disabled::NoFeed
        } else {
            Disabled::Invalid(err.to_string())
        }
    }

    /// The reason in plain words (Settings).
    pub fn reason(&self) -> String {
        match self {
            Disabled::NoKey => t!("update.disabled.no_key").into(),
            Disabled::NoFeed => t!("update.disabled.no_feed").into(),
            Disabled::Invalid(e) => t!("update.disabled.invalid", error = e),
        }
    }
}

/// What the update was doing when it failed (for Retry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Check,
    /// Re-verifying a ZIP downloaded in an earlier session.
    Restore,
    Download,
    Install,
}

/// The kind of failure, for a friendly, localised message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailKind {
    Offline,
    Server(u16),
    Feed,
    Signature,
    Damaged,
    Rejected,
    Location(Option<LocationProblem>),
    Install,
    Other,
}

impl FailKind {
    pub fn of(err: Option<&UpdateError>) -> FailKind {
        match err {
            Some(UpdateError::Network(_)) => FailKind::Offline,
            Some(UpdateError::Http { status, .. }) => FailKind::Server(*status),
            Some(
                UpdateError::Manifest(_) | UpdateError::InvalidUrl(_) | UpdateError::InsecureUrl(_),
            ) => FailKind::Feed,
            Some(UpdateError::Signature(_)) => FailKind::Signature,
            Some(UpdateError::Integrity(_)) => FailKind::Damaged,
            Some(UpdateError::Bundle(_)) => FailKind::Rejected,
            Some(UpdateError::Location(_)) => FailKind::Location(None),
            Some(UpdateError::Install(_)) => FailKind::Install,
            _ => FailKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub during: Op,
    pub kind: FailKind,
    /// The engine's (English) message, shown as the technical detail.
    pub detail: String,
}

/// What the update window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Checking,
    UpToDate,
    /// [`Updates::release`] can be installed.
    Available,
    Downloading {
        done: u64,
        total: u64,
    },
    /// Downloaded and verified; "Install and Relaunch".
    Ready,
    Installing,
    /// Installed: the app quits now and the helper opens the new version.
    Installed,
    Failed(Failure),
}

/// The unobtrusive hint in the status bar after a launch check found an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub version: Version,
    /// Already downloaded (only "Install and Relaunch" left).
    pub ready: bool,
}

/// What the app has to do after [`Updates::tick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The launch check found this version: show the toast.
    Notice(Version),
    /// The update is installed: quit now (the helper relaunches).
    Quit,
}

/// `MANTA_UPDATE_SELFTEST=1`: click through the whole flow without a user
/// (check → download → install → relaunch), logging every step.
pub struct SelfTest {
    started: Instant,
    clicked: bool,
}

const SELFTEST_TIMEOUT: Duration = Duration::from_secs(180);

/// The toast button's command (handled by the app, not a plugin).
pub const TOAST_COMMAND: &str = "manta.app:show-update";

/// The app's update controller.
pub struct Updates {
    backend: Option<Box<dyn Backend>>,
    /// Set when this build can't update itself.
    pub disabled: Option<Disabled>,
    pub state: UpdateState,
    state_path: Option<PathBuf>,
    /// The running version.
    pub current: Version,
    /// Where the app runs keeps it from replacing itself (DMG, translocated, `cargo run`).
    pub blocker: Option<LocationProblem>,
    /// The update window is open.
    pub open: bool,
    pub view: View,
    /// The release on offer.
    pub release: Option<Release>,
    /// The running operation was started by the user (show its result).
    manual: bool,
    /// The user chose "Install and Relaunch": install once downloaded.
    confirmed: bool,
    last_op: Op,
    launch_checked: bool,
    pub notice: Option<Notice>,
    /// Installing waits for running transfers to finish.
    pub waiting_for_transfers: bool,
    /// The native window (placement, focus).
    pub(crate) window: AuxWindow,
    /// Parsed release notes: (version, language, document).
    pub(crate) notes: Option<(Version, &'static str, manta_plugin_markdown::doc::Doc)>,
    pub(crate) heights: manta_plugin_markdown::render::Heights,
    selftest: Option<SelfTest>,
}

/// `~/Library/Application Support/Manta/update-state.json`.
pub fn default_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Manta").join("update-state.json"))
}

/// Missing or unreadable files give the defaults.
pub fn load_state(path: &Path) -> UpdateState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_state(state: &UpdateState, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

impl Updates {
    /// The controller for the running app: the real engine (or the reason
    /// it is off), the stored state, a repaint on every status change.
    pub fn for_app(ctx: &eframe::egui::Context) -> Updates {
        let state_path = default_state_path();
        let mut state = state_path.as_deref().map(load_state).unwrap_or_default();
        let current = manta_updater::current_version(env!("CARGO_PKG_VERSION"));
        if state.forget_stale_download(&current) {
            log::info!("updates: forgot the downloaded update (installed or gone)");
            if let Some(p) = &state_path {
                let _ = save_state(&state, p);
            }
        }
        let (backend, disabled): (Option<Box<dyn Backend>>, _) =
            match UpdaterConfig::for_running_app(state.channel, env!("CARGO_PKG_VERSION")) {
                Ok(cfg) => {
                    log::info!(
                        "updates: Manta {} ({}), feed {}, channel {}",
                        cfg.current_version,
                        std::env::current_exe()
                            .map(|e| e.display().to_string())
                            .unwrap_or_default(),
                        cfg.feed_url,
                        cfg.channel.as_str()
                    );
                    let updater = Updater::new(cfg);
                    let ctx = ctx.clone();
                    updater.set_waker(move || ctx.request_repaint());
                    (Some(Box::new(updater)), None)
                }
                Err(e) => {
                    log::info!("updates: Manta {current}: {e}");
                    (None, Some(Disabled::of(&e)))
                }
            };
        let mut u = Updates::new(backend, disabled, state, current);
        u.state_path = state_path;
        if std::env::var_os("MANTA_UPDATE_SELFTEST").is_some() {
            log::info!("update selftest: on (running {})", u.current);
            u.selftest = Some(SelfTest {
                started: Instant::now(),
                clicked: false,
            });
        }
        u
    }

    /// A controller around any backend (tests). Nothing is saved unless
    /// [`Updates::set_state_path`] is called.
    pub fn new(
        backend: Option<Box<dyn Backend>>,
        disabled: Option<Disabled>,
        state: UpdateState,
        current: Version,
    ) -> Updates {
        let blocker = backend.as_ref().and_then(|b| b.location_problem());
        Updates {
            backend,
            disabled,
            state,
            state_path: None,
            current,
            blocker,
            open: false,
            view: View::Checking,
            release: None,
            manual: false,
            confirmed: false,
            last_op: Op::Check,
            launch_checked: false,
            notice: None,
            waiting_for_transfers: false,
            window: AuxWindow::default(),
            notes: None,
            heights: Default::default(),
            selftest: None,
        }
    }

    #[cfg(test)]
    pub fn set_state_path(&mut self, path: PathBuf) {
        self.state_path = Some(path);
    }

    /// Show a given state in the window (render tests).
    #[cfg(test)]
    pub fn show_state(&mut self, view: View, release: Option<Release>) {
        self.open = true;
        self.view = view;
        self.release = release;
    }

    #[cfg(test)]
    pub fn set_blocker(&mut self, blocker: Option<LocationProblem>) {
        self.blocker = blocker;
    }

    pub fn enabled(&self) -> bool {
        self.backend.is_some()
    }

    fn save(&self) {
        if let Some(p) = &self.state_path {
            if let Err(e) = save_state(&self.state, p) {
                log::warn!("updates: can't save {}: {e}", p.display());
            }
        }
    }

    /// The window is busy with something the user should see (not a result).
    fn in_progress(&self) -> bool {
        matches!(
            self.view,
            View::Downloading { .. } | View::Ready | View::Installing | View::Installed
        )
    }

    /// Once per frame: the launch check (first call), engine status changes,
    /// installing once confirmed. `transfers_busy`: file transfers are
    /// running (installing waits for them, because it quits the app).
    pub fn tick(&mut self, now: DateTime<Utc>, transfers_busy: bool) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.launch_checked {
            self.launch_checked = true;
            self.launch_check(now);
        }
        let changes = self.backend.as_ref().map(|b| b.poll()).unwrap_or_default();
        for s in changes {
            self.apply(s, now, &mut events);
        }
        self.waiting_for_transfers = false;
        if self.view == View::Ready && self.confirmed {
            if transfers_busy {
                self.waiting_for_transfers = true;
            } else {
                self.install_now();
            }
        }
        self.selftest_step(&mut events);
        events
    }

    /// The silent check at launch (every launch, when switched on).
    fn launch_check(&mut self, now: DateTime<Utc>) {
        let Some(b) = &self.backend else { return };
        if !self.state.check_due(now) {
            log::info!("updates: automatic check is off");
            return;
        }
        log::info!("updates: checking at launch");
        self.manual = false;
        self.last_op = Op::Check;
        b.check(self.state.skipped_version.clone());
    }

    fn apply(&mut self, s: UpdateStatus, now: DateTime<Utc>, events: &mut Vec<Event>) {
        log::debug!("updates: status {s:?}");
        match s {
            UpdateStatus::Idle => {
                // A cancelled download.
                if matches!(self.view, View::Downloading { .. }) {
                    self.view = if self.release.is_some() {
                        View::Available
                    } else {
                        View::UpToDate
                    };
                }
            }
            UpdateStatus::Checking => {
                if self.manual {
                    self.view = View::Checking;
                }
            }
            UpdateStatus::UpToDate => {
                self.state.last_check = Some(now);
                self.save();
                log::info!("updates: {} is up to date", self.current);
                if self.last_op == Op::Restore {
                    // The remembered ZIP is not newer: forget it.
                    self.state.downloaded = None;
                    self.save();
                }
                if self.manual {
                    self.release = None;
                    self.view = View::UpToDate;
                }
            }
            UpdateStatus::Available(r) => {
                self.state.last_check = Some(now);
                log::info!(
                    "updates: {} is available (running {})",
                    r.version,
                    self.current
                );
                let version = r.version.clone();
                self.release = Some(r.clone());
                self.view = View::Available;
                // Downloaded in an earlier session: verify it again instead of downloading.
                let remembered = self
                    .state
                    .downloaded
                    .clone()
                    .filter(|d| d.release.version == version && d.zip.is_file());
                if let (Some(d), Some(b)) = (remembered, &self.backend) {
                    self.last_op = Op::Restore;
                    b.restore_download(r, d.zip);
                }
                self.save();
                if !self.manual {
                    self.notice = Some(Notice {
                        version: version.clone(),
                        ready: false,
                    });
                    events.push(Event::Notice(version));
                }
            }
            UpdateStatus::Downloading { done, total } => {
                self.view = View::Downloading { done, total }
            }
            UpdateStatus::ReadyToInstall(r) => {
                if let Some((release, zip)) = self.backend.as_ref().and_then(|b| b.downloaded_zip())
                {
                    self.state.downloaded = Some(DownloadedUpdate { release, zip });
                    self.save();
                }
                log::info!("updates: {} downloaded and verified", r.version);
                self.release = Some(r.clone());
                self.view = View::Ready;
                if !self.confirmed && (!self.open || self.notice.is_some()) {
                    self.notice = Some(Notice {
                        version: r.version,
                        ready: true,
                    });
                }
            }
            UpdateStatus::Installing => self.view = View::Installing,
            UpdateStatus::Installed { relaunch } => {
                log::info!("updates: installed, quitting (relaunch: {relaunch})");
                self.view = View::Installed;
                self.state.downloaded = None;
                self.save();
                events.push(Event::Quit);
            }
            UpdateStatus::Failed(msg) => {
                let err = self.backend.as_ref().and_then(|b| b.last_error());
                match self.last_op {
                    Op::Check if !self.manual => {
                        // Silent: offline, feed missing … never bother the user.
                        log::info!("updates: launch check failed (silent): {msg}");
                    }
                    Op::Restore => {
                        log::info!("updates: the earlier download can't be used ({msg}); downloading again when asked");
                        self.state.downloaded = None;
                        self.save();
                        self.view = View::Available;
                        if self.confirmed {
                            self.start_download();
                        }
                    }
                    op => {
                        let mut kind = FailKind::of(err.as_ref());
                        if kind == FailKind::Location(None) {
                            kind = FailKind::Location(self.blocker.clone());
                        }
                        self.confirmed = false;
                        self.view = View::Failed(Failure {
                            during: op,
                            kind,
                            detail: msg,
                        });
                    }
                }
            }
        }
    }

    /// Manta → Check for Updates… (and the palette, Settings): opens the
    /// window and checks. False when updates are off in this build.
    pub fn check_now(&mut self) -> bool {
        let Some(b) = &self.backend else { return false };
        self.open = true;
        self.window.request_focus();
        self.manual = true;
        if self.in_progress() {
            return true;
        }
        self.confirmed = false;
        self.view = View::Checking;
        self.last_op = Op::Check;
        // When the launch check is still running, its result shows here.
        b.check(self.state.skipped_version.clone());
        true
    }

    /// Open the window for the update on offer (toast, status bar).
    pub fn show(&mut self) {
        self.open = true;
        self.manual = true;
        self.window.request_focus();
        if self.release.is_some() && matches!(self.view, View::Checking | View::UpToDate) {
            self.view = View::Available;
        }
    }

    /// Whether "Install and Relaunch" can run here.
    pub fn can_install(&self) -> bool {
        self.blocker.is_none() && self.release.is_some()
    }

    /// "Install and Relaunch": download (if needed), then install and quit.
    pub fn install(&mut self) {
        if !self.can_install() {
            return;
        }
        self.confirmed = true;
        self.notice = None;
        match self.view {
            View::Ready => self.install_now(),
            View::Downloading { .. } | View::Installing | View::Installed => {}
            _ => {
                if self.last_op != Op::Restore || matches!(self.view, View::Failed(_)) {
                    self.start_download();
                }
            }
        }
    }

    fn start_download(&mut self) {
        let (Some(b), Some(r)) = (&self.backend, &self.release) else {
            return;
        };
        log::info!("updates: downloading {}", r.version);
        self.last_op = Op::Download;
        if b.download_release(r.clone()) {
            self.view = View::Downloading {
                done: 0,
                total: r.size,
            };
        }
    }

    fn install_now(&mut self) {
        let Some(b) = &self.backend else { return };
        if self.blocker.is_some() {
            return;
        }
        log::info!("updates: installing");
        self.last_op = Op::Install;
        self.confirmed = false;
        if b.install_and_relaunch() {
            self.view = View::Installing;
        }
    }

    /// Stop the running download (the partial file is resumed next time).
    pub fn cancel_download(&mut self) {
        if let Some(b) = &self.backend {
            b.cancel();
        }
        self.confirmed = false;
    }

    /// "Later": close the window; offered again at the next launch.
    pub fn later(&mut self) {
        self.open = false;
        self.confirmed = false;
        if self.view != View::Ready {
            self.notice = None;
        }
    }

    /// The window's close button (same as "Later", except a download keeps
    /// running and is offered when it is done).
    pub fn close(&mut self) {
        if matches!(self.view, View::Installing | View::Installed) {
            return;
        }
        self.later();
    }

    /// "Skip This Version": never offer it (or older ones) again.
    pub fn skip(&mut self) {
        if let Some(r) = self.release.take() {
            log::info!("updates: skipping {}", r.version);
            self.state.skipped_version = Some(r.version);
            self.save();
        }
        self.cancel_download();
        self.notice = None;
        self.open = false;
        self.view = View::UpToDate;
    }

    /// "Try Again" after a failure.
    pub fn retry(&mut self) {
        let View::Failed(f) = &self.view else { return };
        match f.during {
            Op::Check | Op::Restore => {
                self.check_now();
            }
            Op::Download => {
                self.confirmed = true;
                self.start_download();
            }
            Op::Install => {
                // The prepared app is used up by a failed install: download again.
                self.confirmed = true;
                self.start_download();
            }
        }
    }

    pub fn set_auto_check(&mut self, on: bool) {
        self.state.auto_check = on;
        self.save();
    }

    pub fn set_channel(&mut self, channel: Channel) {
        self.state.channel = channel;
        if let Some(b) = &self.backend {
            b.set_channel(channel);
        }
        self.save();
    }

    pub fn reset_skipped(&mut self) {
        self.state.skipped_version = None;
        self.save();
    }

    /// A check the user started is running (Settings shows a spinner).
    pub fn checking(&self) -> bool {
        self.manual && self.view == View::Checking
    }

    fn selftest_step(&mut self, events: &mut Vec<Event>) {
        let Some(st) = &mut self.selftest else { return };
        let say = |m: &str| {
            log::info!("update selftest: {m}");
            eprintln!("update selftest: {m}");
        };
        if st.started.elapsed() > SELFTEST_TIMEOUT {
            say(&format!("TIMEOUT in state {:?}", self.view));
            self.selftest = None;
            events.push(Event::Quit);
            return;
        }
        if self.backend.is_none() {
            say("FAILED: updates are disabled in this build");
            self.selftest = None;
            events.push(Event::Quit);
            return;
        }
        match &self.view {
            View::Available if !st.clicked && self.release.is_some() => {
                st.clicked = true;
                let v = self
                    .release
                    .as_ref()
                    .map(|r| r.version.to_string())
                    .unwrap_or_default();
                say(&format!("{v} available (running {}), opening the window and clicking Install and Relaunch", self.current));
                self.show();
                self.install();
            }
            View::Failed(f) => {
                say(&format!("FAILED during {:?}: {}", f.during, f.detail));
                self.selftest = None;
                events.push(Event::Quit);
            }
            View::Installed => {
                say("installed, quitting for the relaunch");
                self.selftest = None;
            }
            _ => {}
        }
    }

    /// The selftest is running (it keeps frames coming).
    pub fn selftest_active(&self) -> bool {
        self.selftest.is_some()
    }
}

/// What Settings → General → Updates shows (a snapshot, set every frame).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsInfo {
    pub current: Version,
    pub disabled: Option<Disabled>,
    pub auto_check: bool,
    pub channel: Channel,
    pub last_check: Option<DateTime<Utc>>,
    pub skipped: Option<Version>,
    pub checking: bool,
}

/// What the user changed in the Updates section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsAction {
    AutoCheck(bool),
    Channel(Channel),
    CheckNow,
    ResetSkipped,
}

impl Updates {
    pub fn settings_info(&self) -> SettingsInfo {
        SettingsInfo {
            current: self.current.clone(),
            disabled: self.disabled.clone(),
            auto_check: self.state.auto_check,
            channel: self.state.channel,
            last_check: self.state.last_check,
            skipped: self.state.skipped_version.clone(),
            checking: self.checking(),
        }
    }

    pub fn apply_settings(&mut self, a: SettingsAction) {
        match a {
            SettingsAction::AutoCheck(on) => self.set_auto_check(on),
            SettingsAction::Channel(c) => self.set_channel(c),
            SettingsAction::CheckNow => {
                self.check_now();
            }
            SettingsAction::ResetSkipped => self.reset_skipped(),
        }
    }
}

#[cfg(test)]
#[path = "updates_tests.rs"]
pub(crate) mod tests;
