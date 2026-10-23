//! S3 in the app: the connect dialog's S3 tab (provider presets, AWS
//! profile) and a pane browsing a bucket (mock S3 backend), light and dark.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

use chrono::{Duration as Age, Utc};
use eframe::egui::Theme;
use manta_core::{
    Auth, ConnectionConfig, Entry, EntryKind, Protocol, S3Options, SharedVfs, Vfs, VfsError,
    VfsPath, VfsResult,
};

use super::{app, harness, harness_with, save, select, settle, WIDE};
use crate::app::Side;
use crate::dialogs::{AuthKind, ConnectForm, Dialog};
use crate::favorites::Favorite;
use crate::worker::ConnectRequest;

/// A read-only stand-in for an S3 connection with a few buckets.
struct MockS3 {
    dirs: HashMap<String, Vec<Entry>>,
}

fn entry(dir: &str, name: &str, dir_kind: bool, size: u64, days: i64) -> Entry {
    let path = VfsPath::new(dir).join(name);
    Entry {
        name: name.into(),
        path,
        kind: if dir_kind {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        size,
        modified: (!dir_kind || days > 0).then(|| Utc::now() - Age::hours(days * 24 + 3)),
        permissions: None,
        link_target: None,
        hidden: name.starts_with('.'),
    }
}

impl MockS3 {
    fn new() -> MockS3 {
        let mut dirs = HashMap::new();
        dirs.insert(
            "/".to_string(),
            vec![
                entry("/", "media-assets", true, 0, 410),
                entry("/", "backups-2026", true, 0, 120),
                entry("/", "website-prod", true, 0, 38),
            ],
        );
        let b = "/media-assets";
        dirs.insert(
            b.to_string(),
            vec![
                entry(b, "campaigns", true, 0, 0),
                entry(b, "product-shots", true, 0, 0),
                entry(b, "raw", true, 0, 0),
                entry(b, "brand-guidelines.pdf", false, 4_218_331, 12),
                entry(b, "hero-loop.mp4", false, 148_204_112, 3),
                entry(b, "logo.svg", false, 18_402, 40),
                entry(b, "logo@2x.png", false, 96_117, 40),
                entry(b, "README.md", false, 2_311, 1),
                entry(b, "sitemap.xml", false, 44_870, 0),
            ],
        );
        MockS3 { dirs }
    }
}

impl Vfs for MockS3 {
    fn label(&self) -> String {
        "media-assets (S3)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::S3
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::new("/media-assets"))
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        Ok(self.dirs.get(dir.as_str()).cloned().unwrap_or_default())
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        if self.dirs.contains_key(path.as_str()) {
            return Ok(entry("/", path.file_name(), true, 0, 0));
        }
        let parent = path.parent().unwrap_or_else(VfsPath::root);
        self.list(&parent)?
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
        Ok(Box::new(std::io::Cursor::new(b"# media-assets\n".to_vec())))
    }
    fn open_write(&self, _: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        Err(VfsError::Unsupported("mock".into()))
    }
}

fn s3_config() -> ConnectionConfig {
    ConnectionConfig {
        name: "Media (R2)".into(),
        protocol: Protocol::S3,
        host: "8f2c1e.r2.cloudflarestorage.com".into(),
        port: 443,
        username: "4c1f8e0d2b".into(),
        auth: Auth::Password(String::new()),
        initial_path: None,
        s3: Some(Box::new(S3Options {
            provider: "r2".into(),
            endpoint: "https://8f2c1e.r2.cloudflarestorage.com".into(),
            region: "auto".into(),
            bucket: "media-assets".into(),
            path_style: true,
            profile: None,
            account_id: "8f2c1e".into(),
        })),
        container: None,
        ftp: None,
    }
}

/// Connect the left pane to the mock bucket, as a finished connect would.
fn connect_mock(h: &mut super::H) {
    let fs: SharedVfs = Arc::new(MockS3::new());
    let a = app(h);
    a.favorites.items.push(Favorite::Connection {
        config: s3_config(),
        keychain: false,
    });
    let action = a.actions.next_id();
    let request = ConnectRequest {
        config: s3_config(),
        favorite: None,
        side: Side::Left,
        action,
        keychain: false,
    };
    a.on_connected(request, Ok((fs, VfsPath::new("/media-assets"))));
    settle(h);
}

#[test]
fn s3_pane_is_remote_with_cloud_location() {
    let fx = super::fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    connect_mock(&mut h);
    let a = app(&mut h);
    let tab = a.pane(Side::Left).tab();
    assert_eq!(tab.loc.fs.protocol(), Protocol::S3);
    assert_eq!(tab.loc.path, VfsPath::new("/media-assets"));
    assert!(tab.loc.conn.is_some());
    assert!(tab.entries.iter().any(|e| e.name == "hero-loop.mp4"));
    assert_eq!(
        tab.loc.fs.local_path(&tab.loc.path),
        None,
        "plugins see no local path"
    );
    // The terminal echo gets the S3 endpoint, not an scp-style user@host.
    let ep = a.endpoint(&tab.loc.fs).expect("remote endpoint");
    assert_eq!(
        (ep.protocol, ep.host.as_str()),
        (Protocol::S3, "8f2c1e.r2.cloudflarestorage.com")
    );
}

#[test]
fn render_s3() {
    if !super::wgpu_available() {
        return;
    }
    let fx = super::fixture();
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let mut h = harness(&fx, WIDE, theme);

        // Connect dialog: Cloudflare R2 preset with keys entered.
        let mut form = ConnectForm::new(Side::Left);
        form.protocol = manta_core::Protocol::S3;
        form.auth_kind = AuthKind::Password;
        form.s3.set_provider("r2");
        form.s3.account = "8f2c1e9a4b7d".into();
        form.s3.set_provider("r2");
        form.user = "4c1f8e0d2b9a7e65".into();
        form.password = "secret".into();
        form.s3.bucket = "media-assets".into();
        form.save_favorite = true;
        form.name = "Media (R2)".into();
        form.test_result = Some(Ok("/media-assets".into()));
        app(&mut h).dialog = Some(Dialog::Connect(form));
        save(&mut h, &format!("s3_connect_r2_{tname}"));

        // AWS profile variant.
        let mut form = ConnectForm::new(Side::Left);
        form.protocol = manta_core::Protocol::S3;
        form.auth_kind = AuthKind::AwsProfile;
        form.s3.region = String::new();
        form.s3.refresh_endpoint();
        form.s3
            .set_profiles(vec!["default".into(), "work".into(), "personal".into()]);
        form.s3.profile = "work".into();
        app(&mut h).dialog = Some(Dialog::Connect(form));
        save(&mut h, &format!("s3_connect_profile_{tname}"));
        app(&mut h).dialog = None;

        // A pane browsing a bucket.
        connect_mock(&mut h);
        select(&mut h, Side::Left, &["brand-guidelines.pdf"]);
        save(&mut h, &format!("s3_pane_{tname}"));
    }
}
