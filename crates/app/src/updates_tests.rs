//! The update controller against a fake engine (no network, no bundles).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{TimeZone, Utc};
use manta_updater::{
    Channel, DownloadedUpdate, LocationProblem, Notes, Release, UpdateError, UpdateState,
    UpdateStatus, Version,
};

use super::*;
use crate::i18n::{with_locale, Locale};

/// What the controller asked the engine to do, and what it answers.
#[derive(Default)]
pub(crate) struct FakeState {
    pub calls: Vec<String>,
    pub queue: Vec<UpdateStatus>,
    pub last_error: Option<UpdateError>,
    pub zip: Option<(Release, PathBuf)>,
    pub blocker: Option<LocationProblem>,
}

#[derive(Clone, Default)]
pub(crate) struct Fake(pub Rc<RefCell<FakeState>>);

impl Fake {
    pub fn push(&self, s: UpdateStatus) {
        self.0.borrow_mut().queue.push(s);
    }
    pub fn fail(&self, e: UpdateError) {
        self.0.borrow_mut().last_error = Some(e.clone());
        self.push(UpdateStatus::Failed(e.to_string()));
    }
    pub fn calls(&self) -> Vec<String> {
        self.0.borrow().calls.clone()
    }
    fn call(&self, c: String) -> bool {
        self.0.borrow_mut().calls.push(c);
        true
    }
}

impl Backend for Fake {
    fn poll(&self) -> Vec<UpdateStatus> {
        std::mem::take(&mut self.0.borrow_mut().queue)
    }
    fn check(&self, skipped: Option<Version>) -> bool {
        self.call(format!(
            "check({})",
            skipped.map(|v| v.to_string()).unwrap_or_default()
        ))
    }
    fn download_release(&self, release: Release) -> bool {
        self.call(format!("download({})", release.version))
    }
    fn restore_download(&self, release: Release, _zip: PathBuf) -> bool {
        self.call(format!("restore({})", release.version))
    }
    fn cancel(&self) {
        self.call("cancel".into());
    }
    fn install_and_relaunch(&self) -> bool {
        self.call("install".into())
    }
    fn location_problem(&self) -> Option<LocationProblem> {
        self.0.borrow().blocker.clone()
    }
    fn last_error(&self) -> Option<UpdateError> {
        self.0.borrow().last_error.clone()
    }
    fn downloaded_zip(&self) -> Option<(Release, PathBuf)> {
        self.0.borrow().zip.clone()
    }
    fn set_channel(&self, channel: Channel) {
        self.call(format!("channel({})", channel.as_str()));
    }
}

pub(crate) fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

pub(crate) const NOTES_EN: &str = "## New\n\n- **Software updates**: Manta checks for new versions at launch and installs them after you confirm.\n- Faster folder listings on SFTP.\n\n## Fixed\n\n- Copying into an archive no longer hangs.\n- Quick Look follows the cursor again. Details in the [changelog](https://manta-filemanager.com/updates/).\n";
pub(crate) const NOTES_DE: &str = "## Neu\n\n- **Softwareupdates**: Manta sucht beim Start nach neuen Versionen und installiert sie nach deiner Bestätigung.\n- Schnellere Ordnerlisten über SFTP.\n\n## Behoben\n\n- Kopieren in ein Archiv hängt nicht mehr.\n- Quick Look folgt wieder dem Cursor. Details im [Änderungsprotokoll](https://manta-filemanager.com/updates/).\n";

pub(crate) fn release(version: &str) -> Release {
    Release {
        version: v(version),
        channel: None,
        published: chrono::DateTime::parse_from_rfc3339("2026-10-21T10:00:00+00:00").unwrap(),
        min_macos: "11.0".into(),
        notes: Notes {
            en: NOTES_EN.into(),
            de: Some(NOTES_DE.into()),
        },
        url: format!("Manta-{version}.zip"),
        size: 21_734_521,
        sha256: "0".repeat(64),
        signature: String::new(),
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap()
}

fn with_fake(state: UpdateState) -> (Updates, Fake) {
    let fake = Fake::default();
    let u = Updates::new(Some(Box::new(fake.clone())), None, state, v("0.1.0"));
    (u, fake)
}

#[test]
fn launch_check_runs_once_per_start_and_failures_stay_silent() {
    let (mut u, fake) = with_fake(UpdateState::default());
    assert!(u.tick(now(), false).is_empty());
    assert_eq!(fake.calls(), ["check()"], "checked at launch");
    fake.push(UpdateStatus::Checking);
    fake.fail(UpdateError::Network(
        "cannot resolve manta-filemanager.com (offline?)".into(),
    ));
    let events = u.tick(now(), false);
    assert!(events.is_empty(), "no toast, no dialog: {events:?}");
    assert!(!u.open, "the window stays closed");
    assert!(
        !matches!(u.view, View::Failed(_)),
        "nothing to show later either"
    );
    assert!(u.notice.is_none());
    for _ in 0..5 {
        u.tick(now(), false);
    }
    assert_eq!(fake.calls(), ["check()"], "only once per start");
    // Also a missing feed (404) is silent.
    let (mut u, fake) = with_fake(UpdateState::default());
    u.tick(now(), false);
    fake.fail(UpdateError::Http {
        status: 404,
        url: "https://manta-filemanager.com/updates/update.json".into(),
    });
    assert!(u.tick(now(), false).is_empty());
    assert!(!u.open);
}

#[test]
fn launch_check_finds_an_update_and_only_hints_at_it() {
    let (mut u, fake) = with_fake(UpdateState::default());
    u.tick(now(), false);
    fake.push(UpdateStatus::Available(release("0.2.0")));
    let events = u.tick(now(), false);
    assert_eq!(events, [Event::Notice(v("0.2.0"))], "a toast, not a modal");
    assert!(!u.open, "no window over the user's work");
    assert_eq!(
        u.notice,
        Some(Notice {
            version: v("0.2.0"),
            ready: false
        }),
        "status bar hint"
    );
    assert_eq!(u.state.last_check, Some(now()));
    // The toast's "Show" opens the window on the update.
    u.show();
    assert!(u.open);
    assert_eq!(u.view, View::Available);
}

#[test]
fn launch_check_respects_the_switch_and_the_skipped_version() {
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.tick(now(), false);
    assert!(fake.calls().is_empty(), "switched off: no launch check");

    let (mut u, fake) = with_fake(UpdateState {
        skipped_version: Some(v("0.2.0")),
        ..Default::default()
    });
    u.tick(now(), false);
    assert_eq!(
        fake.calls(),
        ["check(0.2.0)"],
        "the engine leaves out skipped versions"
    );
    // Every launch, even right after the last check.
    let (mut u, fake) = with_fake(UpdateState {
        last_check: Some(now()),
        ..Default::default()
    });
    u.tick(now(), false);
    assert_eq!(fake.calls(), ["check()"]);
}

#[test]
fn manual_check_shows_up_to_date() {
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.tick(now(), false);
    assert!(u.check_now());
    assert!(u.open);
    assert_eq!(u.view, View::Checking);
    assert!(u.checking(), "Settings shows a spinner");
    fake.push(UpdateStatus::Checking);
    fake.push(UpdateStatus::UpToDate);
    u.tick(now(), false);
    assert_eq!(u.view, View::UpToDate);
    assert!(u.open);
    assert_eq!(u.state.last_check, Some(now()));
    u.later();
    assert!(!u.open);
}

#[test]
fn manual_check_shows_the_error_with_retry() {
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.tick(now(), false);
    u.check_now();
    fake.fail(UpdateError::Network(
        "cannot resolve manta-filemanager.com (offline?)".into(),
    ));
    u.tick(now(), false);
    let View::Failed(f) = &u.view else {
        panic!("{:?}", u.view)
    };
    assert_eq!(f.during, Op::Check);
    assert_eq!(f.kind, FailKind::Offline);
    let (title, body) = with_locale(Locale::De, || crate::update_window::failure_text(f));
    assert_eq!(title, "Suche nach Updates fehlgeschlagen");
    assert!(body.contains("Internetverbindung"), "{body}");
    let (title, _) = with_locale(Locale::En, || crate::update_window::failure_text(f));
    assert_eq!(title, "Couldn't check for updates");
    u.retry();
    assert_eq!(fake.calls(), ["check()", "check()"]);
    assert_eq!(u.view, View::Checking);
}

#[test]
fn manual_check_while_the_launch_check_runs_shows_its_result() {
    let (mut u, fake) = with_fake(UpdateState::default());
    u.tick(now(), false);
    u.check_now();
    fake.push(UpdateStatus::Available(release("0.2.0")));
    let events = u.tick(now(), false);
    assert!(
        events.is_empty(),
        "the window shows it, no toast: {events:?}"
    );
    assert!(u.open);
    assert_eq!(u.view, View::Available);
}

#[test]
fn install_downloads_installs_and_quits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.set_state_path(dir.path().join("update-state.json"));
    u.tick(now(), false);
    u.check_now();
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.install();
    assert_eq!(fake.calls(), ["check()", "download(0.2.0)"]);
    assert_eq!(
        u.view,
        View::Downloading {
            done: 0,
            total: 21_734_521
        }
    );
    fake.push(UpdateStatus::Downloading {
        done: 12_300_000,
        total: 21_734_521,
    });
    u.tick(now(), false);
    assert_eq!(
        u.view,
        View::Downloading {
            done: 12_300_000,
            total: 21_734_521
        }
    );
    let zip = dir.path().join("Manta-0.2.0.zip");
    std::fs::write(&zip, b"zip").unwrap();
    fake.0.borrow_mut().zip = Some((release("0.2.0"), zip.clone()));
    fake.push(UpdateStatus::ReadyToInstall(release("0.2.0")));
    u.tick(now(), false);
    assert_eq!(
        fake.calls().last().unwrap(),
        "install",
        "confirmed once: installs when ready"
    );
    assert_eq!(
        load_state(&dir.path().join("update-state.json"))
            .downloaded
            .map(|d| d.zip),
        Some(zip)
    );
    fake.push(UpdateStatus::Installing);
    fake.push(UpdateStatus::Installed { relaunch: true });
    let events = u.tick(now(), false);
    assert_eq!(events, [Event::Quit]);
    assert_eq!(u.view, View::Installed);
    assert!(load_state(&dir.path().join("update-state.json"))
        .downloaded
        .is_none());
    // The window can't be closed while it relaunches.
    u.close();
    assert!(u.open);
}

#[test]
fn install_waits_for_running_transfers() {
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.tick(now(), false);
    u.check_now();
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.install();
    fake.push(UpdateStatus::ReadyToInstall(release("0.2.0")));
    u.tick(now(), true);
    assert_eq!(u.view, View::Ready);
    assert!(u.waiting_for_transfers);
    assert!(!fake.calls().contains(&"install".to_string()));
    u.tick(now(), false);
    assert!(
        fake.calls().contains(&"install".to_string()),
        "installs once the transfers are done"
    );
}

#[test]
fn later_closes_and_offers_it_again_next_launch() {
    let (mut u, fake) = with_fake(UpdateState::default());
    u.tick(now(), false);
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.show();
    u.later();
    assert!(!u.open);
    assert!(u.notice.is_none());
    assert!(u.state.skipped_version.is_none(), "not skipped");
    assert_eq!(fake.calls(), ["check()"], "nothing downloaded");
}

#[test]
fn skip_version_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-state.json");
    let (mut u, fake) = with_fake(UpdateState::default());
    u.set_state_path(path.clone());
    u.tick(now(), false);
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.show();
    u.skip();
    assert!(!u.open);
    assert!(u.notice.is_none());
    assert_eq!(load_state(&path).skipped_version, Some(v("0.2.0")));
    assert_eq!(u.settings_info().skipped, Some(v("0.2.0")));
    // Next launch: the engine is told to leave it out.
    let (mut next, fake) = with_fake(load_state(&path));
    next.tick(now(), false);
    assert_eq!(fake.calls(), ["check(0.2.0)"]);
    next.apply_settings(SettingsAction::ResetSkipped);
    assert_eq!(next.state.skipped_version, None);
}

#[test]
fn cancel_download_goes_back_to_the_offer() {
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.tick(now(), false);
    u.check_now();
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.install();
    fake.push(UpdateStatus::Downloading {
        done: 1_000_000,
        total: 21_734_521,
    });
    u.tick(now(), false);
    u.cancel_download();
    assert_eq!(fake.calls().last().unwrap(), "cancel");
    fake.push(UpdateStatus::Idle);
    u.tick(now(), false);
    assert_eq!(u.view, View::Available);
    // Ready later (another download) does not install by itself any more.
    fake.push(UpdateStatus::ReadyToInstall(release("0.2.0")));
    u.tick(now(), false);
    assert!(!fake.calls().contains(&"install".to_string()));
}

#[test]
fn failed_download_shows_retry() {
    let (mut u, fake) = with_fake(UpdateState {
        auto_check: false,
        ..Default::default()
    });
    u.tick(now(), false);
    u.check_now();
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.install();
    fake.fail(UpdateError::Integrity("SHA-256 mismatch".into()));
    u.tick(now(), false);
    let View::Failed(f) = &u.view else {
        panic!("{:?}", u.view)
    };
    assert_eq!((f.during, &f.kind), (Op::Download, &FailKind::Damaged));
    u.retry();
    assert_eq!(fake.calls().last().unwrap(), "download(0.2.0)");
}

#[test]
fn blocker_keeps_install_off_and_says_what_to_do() {
    let fake = Fake::default();
    fake.0.borrow_mut().blocker = Some(LocationProblem::ReadOnlyVolume);
    let mut u = Updates::new(
        Some(Box::new(fake.clone())),
        None,
        UpdateState::default(),
        v("0.1.0"),
    );
    assert_eq!(u.blocker, Some(LocationProblem::ReadOnlyVolume));
    u.tick(now(), false);
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    u.show();
    assert!(!u.can_install());
    u.install();
    assert_eq!(fake.calls(), ["check()"], "no download from a disk image");
    let (title, fix) = with_locale(Locale::De, || {
        crate::update_window::blocker_text(&LocationProblem::ReadOnlyVolume)
    });
    assert_eq!(title, "Manta läuft von einem Disk-Image");
    assert!(
        fix.starts_with("Ziehe Manta zuerst in den Programme-Ordner"),
        "{fix}"
    );
    let (_, fix) = with_locale(Locale::En, || {
        crate::update_window::blocker_text(&LocationProblem::Translocated)
    });
    assert!(fix.contains("Applications folder"), "{fix}");
}

#[test]
fn disabled_build_never_checks() {
    let mut u = Updates::new(
        None,
        Some(Disabled::NoKey),
        UpdateState::default(),
        v("0.1.0"),
    );
    assert!(!u.enabled());
    assert!(u.tick(now(), false).is_empty());
    assert!(!u.check_now(), "the app opens Settings instead");
    assert!(!u.open);
    let info = u.settings_info();
    assert_eq!(info.disabled, Some(Disabled::NoKey));
    let reason = with_locale(Locale::De, || Disabled::NoKey.reason());
    assert!(reason.contains("Signaturschlüssel"), "{reason}");
    // Changing settings still works (and doesn't crash without an engine).
    u.apply_settings(SettingsAction::Channel(Channel::Beta));
    assert_eq!(u.state.channel, Channel::Beta);
}

#[test]
fn settings_are_saved_and_reach_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Manta/update-state.json");
    let (mut u, fake) = with_fake(UpdateState::default());
    u.set_state_path(path.clone());
    u.apply_settings(SettingsAction::AutoCheck(false));
    u.apply_settings(SettingsAction::Channel(Channel::Beta));
    let saved = load_state(&path);
    assert!(!saved.auto_check);
    assert_eq!(saved.channel, Channel::Beta);
    assert_eq!(saved.interval_hours, 0, "every launch");
    assert_eq!(fake.calls(), ["channel(beta)"]);
    u.apply_settings(SettingsAction::CheckNow);
    assert!(u.open);
    assert_eq!(fake.calls().last().unwrap(), "check()");
    // A broken file gives the defaults.
    std::fs::write(&path, "{not json").unwrap();
    assert_eq!(load_state(&path), UpdateState::default());
}

#[test]
fn an_earlier_download_is_verified_again_instead_of_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("Manta-0.2.0.zip");
    std::fs::write(&zip, b"zip").unwrap();
    let state = UpdateState {
        downloaded: Some(DownloadedUpdate {
            release: release("0.2.0"),
            zip: zip.clone(),
        }),
        ..Default::default()
    };
    let (mut u, fake) = with_fake(state);
    u.tick(now(), false);
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    assert_eq!(fake.calls(), ["check()", "restore(0.2.0)"]);
    fake.push(UpdateStatus::ReadyToInstall(release("0.2.0")));
    u.tick(now(), false);
    assert_eq!(
        u.notice,
        Some(Notice {
            version: v("0.2.0"),
            ready: true
        })
    );
    assert_eq!(u.view, View::Ready);

    // A ZIP that no longer verifies is forgotten silently.
    let state = UpdateState {
        downloaded: Some(DownloadedUpdate {
            release: release("0.2.0"),
            zip,
        }),
        ..Default::default()
    };
    let (mut u, fake) = with_fake(state);
    u.tick(now(), false);
    fake.push(UpdateStatus::Available(release("0.2.0")));
    u.tick(now(), false);
    fake.fail(UpdateError::Integrity("size mismatch".into()));
    u.tick(now(), false);
    assert_eq!(u.view, View::Available);
    assert!(u.state.downloaded.is_none());
    assert!(!u.open);
}
