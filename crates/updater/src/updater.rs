//! [`Updater`]: the facade the UI uses. Every operation runs on a background
//! thread and reports [`UpdateStatus`] changes through a channel.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ed25519_dalek::VerifyingKey;
use semver::Version;

use crate::bundle;
use crate::check::{self, CheckOptions};
use crate::download::{self, VerifiedDownload};
use crate::error::{Result, UpdateError};
use crate::install::{self, BackupDisposal};
use crate::keys;
use crate::manifest::{Channel, Release, DEFAULT_APP_ID};
use crate::prepare::{self, PrepareOptions, PreparedApp};

/// What the updater is doing. The UI shows the latest value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        done: u64,
        total: u64,
    },
    /// Downloaded, verified and unpacked; [`Updater::install_and_relaunch`] can run.
    ReadyToInstall(Release),
    Installing,
    /// The new version is in place and a helper waits to open it: the app
    /// must quit now (the relaunch happens when this process has exited).
    Installed {
        relaunch: bool,
    },
    Failed(String),
}

/// Configuration of an [`Updater`].
#[derive(Clone, Debug)]
pub struct UpdaterConfig {
    pub feed_url: String,
    pub app_id: String,
    pub current_version: Version,
    pub channel: Channel,
    pub public_key: VerifyingKey,
    /// Where ZIPs are downloaded and unpacked (`~/Library/Caches/Manta/updates`).
    pub download_dir: PathBuf,
    /// The bundle to replace. `None`: the running app's (with location checks).
    pub bundle_path: Option<PathBuf>,
    pub backup: BackupDisposal,
    /// Open the new version after the current process exits.
    pub relaunch: bool,
    /// Override the macOS version used for `min_macos` (tests).
    pub os_version: Option<check::OsVersion>,
}

/// The feed URL for the running app. Precedence:
/// 1. env `MANTA_UPDATE_URL` at run time (testing),
/// 2. `MantaUpdateFeedURL` in the running bundle's Info.plist,
/// 3. `MANTA_UPDATE_URL` at compile time, else `https://manta-filemanager.com/updates/update.json`.
///
/// The pre-rename `RUSTLIFT_UPDATE_URL` is still read in 1. (deprecation alias).
pub fn configured_feed_url() -> Option<String> {
    let nonempty = |s: String| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(u) = std::env::var("MANTA_UPDATE_URL").ok().and_then(nonempty) {
        return Some(u);
    }
    // Deprecation alias from before the rename to Manta.
    if let Some(u) = std::env::var("RUSTLIFT_UPDATE_URL").ok().and_then(nonempty) {
        log::warn!("RUSTLIFT_UPDATE_URL is deprecated, use MANTA_UPDATE_URL");
        return Some(u);
    }
    let exe = std::env::current_exe().ok()?;
    if let Some(b) = install::bundle_of_executable(&exe.canonicalize().unwrap_or(exe)) {
        if let Some(u) = bundle::read_bundle_info(&b).ok().and_then(|i| i.feed_url) {
            return Some(u);
        }
    }
    nonempty(env!("MANTA_UPDATE_URL_EMBEDDED").to_string())
}

/// The running app's version: `CFBundleShortVersionString` of its bundle, else `fallback`
/// (pass `env!("CARGO_PKG_VERSION")`).
pub fn current_version(fallback: &str) -> Version {
    std::env::current_exe()
        .ok()
        .and_then(|e| install::bundle_of_executable(&e.canonicalize().unwrap_or(e)))
        .and_then(|b| bundle::read_bundle_info(&b).ok())
        .and_then(|i| Version::parse(&i.version).ok())
        .or_else(|| Version::parse(fallback).ok())
        .unwrap_or_else(|| Version::new(0, 0, 0))
}

impl UpdaterConfig {
    /// Config for the running app: built-in key, configured feed URL, version
    /// from the bundle. `Err(Disabled)` explains why updates are off (no key,
    /// no feed URL, invalid URL). Location problems (DMG, `cargo run`) are
    /// reported later, at install time, so checking still works there.
    pub fn for_running_app(channel: Channel, fallback_version: &str) -> Result<UpdaterConfig> {
        let public_key = keys::embedded_public_key()?;
        let feed_url = configured_feed_url().ok_or_else(|| {
            UpdateError::Disabled(
                "no update feed URL configured (MANTA_UPDATE_URL at build time)".into(),
            )
        })?;
        check::validate_url(&feed_url).map_err(|e| UpdateError::Disabled(e.to_string()))?;
        let download_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Manta/updates");
        Ok(UpdaterConfig {
            feed_url,
            app_id: DEFAULT_APP_ID.to_string(),
            current_version: current_version(fallback_version),
            channel,
            public_key,
            download_dir,
            bundle_path: None,
            backup: BackupDisposal::Trash,
            relaunch: true,
            os_version: check::macos_version(),
        })
    }
}

type Waker = Box<dyn Fn() + Send + Sync>;

struct Inner {
    config: Mutex<UpdaterConfig>,
    status: Mutex<UpdateStatus>,
    tx: Mutex<Sender<UpdateStatus>>,
    busy: AtomicBool,
    cancel: AtomicBool,
    download: Mutex<Option<VerifiedDownload>>,
    prepared: Mutex<Option<PreparedApp>>,
    waker: Mutex<Option<Waker>>,
    /// The error behind the last `Failed` status (for localised messages).
    last_error: Mutex<Option<UpdateError>>,
}

impl Inner {
    fn set(&self, s: UpdateStatus) {
        *self.status.lock().unwrap() = s.clone();
        let _ = self.tx.lock().unwrap().send(s);
        if let Some(w) = self.waker.lock().unwrap().as_ref() {
            w();
        }
    }
}

/// Background update engine. Cheap to clone (shared state).
///
/// ```no_run
/// # use manta_updater::*;
/// let cfg = UpdaterConfig::for_running_app(Channel::Stable, env!("CARGO_PKG_VERSION")).unwrap();
/// let updater = Updater::new(cfg);
/// updater.set_waker(|| { /* ctx.request_repaint() */ });
/// updater.check(None);
/// // every frame:
/// for status in updater.poll() { /* react, e.g. persist last_check on UpToDate/Available */ }
/// match updater.status() { UpdateStatus::Available(r) => { updater.download(); } _ => {} }
/// ```
#[derive(Clone)]
pub struct Updater {
    inner: Arc<Inner>,
    rx: Arc<Mutex<Receiver<UpdateStatus>>>,
}

impl Updater {
    pub fn new(config: UpdaterConfig) -> Updater {
        let (tx, rx) = channel();
        Updater {
            inner: Arc::new(Inner {
                config: Mutex::new(config),
                status: Mutex::new(UpdateStatus::Idle),
                tx: Mutex::new(tx),
                busy: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
                download: Mutex::new(None),
                prepared: Mutex::new(None),
                waker: Mutex::new(None),
                last_error: Mutex::new(None),
            }),
            rx: Arc::new(Mutex::new(rx)),
        }
    }

    /// Called after every status change (from a background thread), e.g. to
    /// request a repaint.
    pub fn set_waker(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.inner.waker.lock().unwrap() = Some(Box::new(f));
    }

    /// The latest status.
    pub fn status(&self) -> UpdateStatus {
        self.inner.status.lock().unwrap().clone()
    }

    /// All status changes since the last call (never blocks).
    pub fn poll(&self) -> Vec<UpdateStatus> {
        self.rx.lock().unwrap().try_iter().collect()
    }

    /// Whether an operation is running.
    pub fn is_busy(&self) -> bool {
        self.inner.busy.load(Ordering::SeqCst)
    }

    pub fn set_channel(&self, channel: Channel) {
        self.inner.config.lock().unwrap().channel = channel;
    }

    pub fn config(&self) -> UpdaterConfig {
        self.inner.config.lock().unwrap().clone()
    }

    /// Asks a running download to stop (the `.part` file stays for resuming).
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
    }

    /// Runs `job` on a thread unless something is already running.
    fn spawn(&self, job: impl FnOnce(&Inner) -> Result<UpdateStatus> + Send + 'static) -> bool {
        if self.inner.busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.inner.cancel.store(false, Ordering::SeqCst);
        let inner = self.inner.clone();
        std::thread::Builder::new()
            .name("manta-updater".into())
            .spawn(move || {
                let result = job(&inner);
                inner.busy.store(false, Ordering::SeqCst);
                match result {
                    Ok(s) => inner.set(s),
                    Err(UpdateError::Cancelled) => inner.set(UpdateStatus::Idle),
                    Err(e) => {
                        log::warn!("updater: {e}");
                        *inner.last_error.lock().unwrap() = Some(e.clone());
                        inner.set(UpdateStatus::Failed(e.to_string()))
                    }
                }
            })
            .is_ok()
    }

    /// Checks the feed → `Checking`, then `UpToDate`, `Available(release)` or
    /// `Failed`. Returns false if another operation is running.
    pub fn check(&self, skipped: Option<Version>) -> bool {
        self.spawn(move |inner| {
            inner.set(UpdateStatus::Checking);
            let cfg = inner.config.lock().unwrap().clone();
            let opts = CheckOptions {
                key: cfg.public_key,
                app_id: cfg.app_id.clone(),
                os_version: cfg.os_version.clone(),
            };
            match check::check_with(
                &opts,
                &cfg.feed_url,
                &cfg.current_version,
                cfg.channel,
                skipped.as_ref(),
            )? {
                Some(r) => Ok(UpdateStatus::Available(r)),
                None => Ok(UpdateStatus::UpToDate),
            }
        })
    }

    /// Downloads and prepares the release from the last `Available` status →
    /// `Downloading{..}` …, then `ReadyToInstall(release)`.
    pub fn download(&self) -> bool {
        let UpdateStatus::Available(release) = self.status() else {
            return false;
        };
        self.download_release(release)
    }

    /// Downloads and prepares a specific (feed-verified) release.
    pub fn download_release(&self, release: Release) -> bool {
        self.spawn(move |inner| {
            let cfg = inner.config.lock().unwrap().clone();
            inner.set(UpdateStatus::Downloading {
                done: 0,
                total: release.size,
            });
            let mut last = Instant::now() - Duration::from_secs(1);
            let mut progress = |done: u64, total: u64| {
                // At most ~10 updates per second, plus the final one.
                if done == total || last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    inner.set(UpdateStatus::Downloading { done, total });
                }
            };
            let dl = download::download_with(
                &cfg.public_key,
                &cfg.app_id,
                &release,
                &cfg.download_dir,
                &mut progress,
                &inner.cancel,
            )?;
            Self::prepare_inner(inner, &cfg, dl)
        })
    }

    /// Re-verifies a ZIP remembered in [`crate::UpdateState::downloaded`] and
    /// prepares it → `ReadyToInstall` (or `Failed`; then forget it).
    pub fn restore_download(&self, release: Release, zip: PathBuf) -> bool {
        self.spawn(move |inner| {
            let cfg = inner.config.lock().unwrap().clone();
            if release.version <= cfg.current_version {
                return Ok(UpdateStatus::UpToDate);
            }
            let dl = download::verify_file(&cfg.public_key, &cfg.app_id, &release, &zip)?;
            Self::prepare_inner(inner, &cfg, dl)
        })
    }

    fn prepare_inner(
        inner: &Inner,
        cfg: &UpdaterConfig,
        dl: VerifiedDownload,
    ) -> Result<UpdateStatus> {
        let current = match &cfg.bundle_path {
            Some(p) => Some(p.clone()),
            None => install::running_bundle().ok(),
        };
        let opts = PrepareOptions {
            expected_bundle_id: cfg.app_id.clone(),
            current_bundle: current,
            work_dir: None,
        };
        let prepared = prepare::prepare_with(&dl, &opts)?;
        let release = dl.release().clone();
        *inner.download.lock().unwrap() = Some(dl);
        *inner.prepared.lock().unwrap() = Some(prepared);
        Ok(UpdateStatus::ReadyToInstall(release))
    }

    /// The verified ZIP of the last download (to remember in `UpdateState`).
    pub fn downloaded_zip(&self) -> Option<(Release, PathBuf)> {
        self.inner
            .download
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| (d.release().clone(), d.path().to_path_buf()))
    }

    /// Replaces the app (one admin prompt if its folder isn't writable),
    /// deletes the ZIP and, if configured, starts the relaunch helper →
    /// `Installing`, then `Installed{relaunch}`: QUIT THE APP NOW.
    pub fn install_and_relaunch(&self) -> bool {
        if !matches!(self.status(), UpdateStatus::ReadyToInstall(_)) {
            return false;
        }
        self.spawn(|inner| {
            let cfg = inner.config.lock().unwrap().clone();
            let prepared = inner
                .prepared
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| UpdateError::Install("nothing prepared".into()))?;
            inner.set(UpdateStatus::Installing);
            let target = match &cfg.bundle_path {
                Some(p) => p.clone(),
                None => install::running_bundle()?,
            };
            let mut plan = install::install(prepared, &target)?;
            plan.backup = cfg.backup;
            let outcome = plan.apply()?;
            if let Some(d) = inner.download.lock().unwrap().take() {
                let _ = std::fs::remove_file(d.path());
            }
            if cfg.relaunch {
                install::relaunch_after_exit(std::process::id(), &outcome.installed).map_err(
                    |e| {
                        UpdateError::Install(format!(
                            "installed, but could not schedule the relaunch: {e}"
                        ))
                    },
                )?;
            }
            Ok(UpdateStatus::Installed {
                relaunch: cfg.relaunch,
            })
        })
    }

    /// The error behind the latest [`UpdateStatus::Failed`] (the status
    /// only carries its English text; UIs localise by the variant).
    pub fn last_error(&self) -> Option<UpdateError> {
        self.inner.last_error.lock().unwrap().clone()
    }

    /// [`Updater::install_blocker`] as a value (to explain it in the UI's language).
    pub fn location_problem(&self) -> Option<install::LocationProblem> {
        let bundle = match self.config().bundle_path {
            Some(p) => Some(p),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| install::bundle_of_executable(&e.canonicalize().unwrap_or(e))),
        };
        let ro = bundle.as_deref().is_some_and(install::is_read_only);
        install::check_location(bundle.as_deref(), ro).err()
    }

    /// Location problem that would block installing, checked up front so the
    /// UI can explain it before downloading (`None`: fine).
    pub fn install_blocker(&self) -> Option<String> {
        let cfg = self.config();
        match &cfg.bundle_path {
            Some(p) => install::check_location(Some(p), install::is_read_only(p))
                .err()
                .map(|e| e.to_string()),
            None => install::running_bundle().err().map(|e| e.to_string()),
        }
    }
}

/// Convenience: the download folder used for the running app.
pub fn default_download_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Manta/updates")
}
