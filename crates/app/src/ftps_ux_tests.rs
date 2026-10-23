//! FTPS in the app: the connect dialog's data channel option and the notice
//! when a connection continues with clear data connections (mock backend),
//! German and English, light and dark.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{Duration as Age, Utc};
use eframe::egui::Theme;
use manta_core::{
    Auth, ConnectionConfig, ConnectionNotice, Entry, EntryKind, FtpDataChannel, FtpOptions,
    Protocol, SharedVfs, Vfs, VfsError, VfsPath, VfsResult,
};

use super::{app, fixture, harness, harness_with, save, settle, wgpu_available, WIDE};
use crate::app::Side;
use crate::dialogs::{ConnectForm, Dialog};
use crate::i18n::{self, Locale};
use crate::worker::ConnectRequest;

/// The release web space, as a FTPS server whose data connections had to
/// fall back to clear text once `clear` is set.
struct MockFtps {
    clear: Arc<AtomicBool>,
}

fn file(name: &str, size: u64, days: i64) -> Entry {
    Entry {
        name: name.into(),
        path: VfsPath::root().join(name),
        kind: EntryKind::File,
        size,
        modified: Some(Utc::now() - Age::hours(days * 24 + 5)),
        permissions: Some(0o644),
        link_target: None,
        hidden: false,
    }
}

impl Vfs for MockFtps {
    fn label(&self) -> String {
        "deploy@ftp.example.com (FTPS)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Ftps
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }
    fn list(&self, _: &VfsPath) -> VfsResult<Vec<Entry>> {
        Ok(vec![
            file("Manta-0.1.0.zip", 28_920_071, 0),
            file("update.json", 1_914, 0),
            file("update.json.sig", 89, 0),
        ])
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        self.list(&VfsPath::root())?
            .into_iter()
            .find(|e| e.name == path.file_name())
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }
    fn create_dir(&self, _: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported("mock".into()))
    }
    fn remove_file(&self, _: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported("mock".into()))
    }
    fn remove_dir(&self, _: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported("mock".into()))
    }
    fn rename(&self, _: &VfsPath, _: &VfsPath) -> VfsResult<()> {
        Err(VfsError::Unsupported("mock".into()))
    }
    fn open_read(&self, _: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        Ok(Box::new(std::io::Cursor::new(b"{}".to_vec())))
    }
    fn open_write(&self, _: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        Err(VfsError::Unsupported("mock".into()))
    }
    fn notice(&self) -> Option<ConnectionNotice> {
        self.clear
            .load(Ordering::SeqCst)
            .then_some(ConnectionNotice::ClearDataChannel)
    }
}

fn ftps_config(policy: FtpDataChannel) -> ConnectionConfig {
    ConnectionConfig {
        name: "Manta updates".into(),
        protocol: Protocol::Ftps,
        host: "ftp.example.com".into(),
        port: 21,
        username: "deploy".into(),
        auth: Auth::Password("secret".into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: Some(Box::new(FtpOptions {
            data_channel: policy,
        })),
    }
}

/// Connect the left pane to the mock server, as a finished connect would.
fn connect_mock(h: &mut super::H, clear: Arc<AtomicBool>) {
    let fs: SharedVfs = Arc::new(MockFtps { clear });
    let a = app(h);
    let action = a.actions.next_id();
    let request = ConnectRequest {
        config: ftps_config(FtpDataChannel::Auto),
        favorite: None,
        side: Side::Left,
        action,
        keychain: false,
    };
    a.on_connected(request, Ok((fs, VfsPath::root())));
    settle(h);
}

pub(super) fn ftps_form(policy: FtpDataChannel) -> ConnectForm {
    let mut form = ConnectForm::from_config(&ftps_config(policy), Side::Left, None);
    form.password = String::new();
    form
}

#[test]
fn data_channel_round_trips_through_the_form() {
    for policy in [
        FtpDataChannel::Auto,
        FtpDataChannel::Require,
        FtpDataChannel::Off,
    ] {
        let form = ftps_form(policy);
        assert_eq!(form.ftp_data, policy);
        let cfg = form.to_config().expect("config");
        assert_eq!(cfg.ftp.as_ref().map(|o| o.data_channel), Some(policy));
    }
    // Old favorites without the option: Automatic.
    let mut cfg = ftps_config(FtpDataChannel::Off);
    cfg.ftp = None;
    assert_eq!(
        ConnectForm::from_config(&cfg, Side::Left, None).ftp_data,
        FtpDataChannel::Auto
    );
    // Only FTPS carries it.
    let mut form = ftps_form(FtpDataChannel::Require);
    form.protocol = Protocol::Sftp;
    form.port = "22".into();
    assert_eq!(form.to_config().expect("config").ftp, None);
}

#[test]
fn clear_data_notice_is_shown_once() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let clear = Arc::new(AtomicBool::new(false));
    connect_mock(&mut h, clear.clone());
    let title = crate::i18n::tr("status.ftps_clear_data");
    assert!(!app(&mut h).toasts.titles().iter().any(|t| t == title));
    clear.store(true, Ordering::SeqCst);
    h.run_steps(3);
    let count = |h: &mut super::H| {
        app(h)
            .toasts
            .titles()
            .iter()
            .filter(|t| *t == title)
            .count()
    };
    assert_eq!(count(&mut h), 1);
    h.run_steps(10);
    assert_eq!(count(&mut h), 1, "once per connection");
}

#[test]
fn require_error_is_translated() {
    let msg = format!("connection error: {}", manta_vfs::ftp::DATA_TLS_BLOCKED);
    let de = i18n::with_locale(Locale::De, || crate::i18n::localize_error(&msg));
    assert!(
        de.starts_with(
            "Verbindungsfehler: Das Netzwerk blockiert verschlüsselte FTP-Datenverbindungen"
        ),
        "{de}"
    );
    let en = i18n::with_locale(Locale::En, || crate::i18n::localize_error(&msg));
    assert_eq!(en, msg);
}

#[test]
fn render_ftps() {
    if !wgpu_available() {
        return;
    }
    let fx = fixture();
    for (locale, lname) in [(Locale::De, "de"), (Locale::En, "en")] {
        for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            let tag = format!("{lname}_{tname}");
            let mut h = harness(&fx, WIDE, theme);
            i18n::set_locale(locale);
            h.run_steps(3);

            app(&mut h).dialog = Some(Dialog::Connect(ftps_form(FtpDataChannel::Auto)));
            save(&mut h, &format!("ftps_connect_auto_{tag}"));
            app(&mut h).dialog = Some(Dialog::Connect(ftps_form(FtpDataChannel::Require)));
            save(&mut h, &format!("ftps_connect_require_{tag}"));
            app(&mut h).dialog = None;

            // A pane on the server after the fallback: chip + toast.
            connect_mock(&mut h, Arc::new(AtomicBool::new(true)));
            save(&mut h, &format!("ftps_pane_clear_data_{tag}"));
            i18n::set_locale(Locale::En);
        }
    }
}
