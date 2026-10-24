//! SMB and WebDAV in the app: the connect dialog's tabs (share picker,
//! Nextcloud preset), an SMB share mounted through a fake mounter (location
//! chip, favourite with the password in the Keychain, Servers row, unmount
//! on disconnect), a WebDAV pane (mock backend) and Compare & Sync labels.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use eframe::egui::Theme;
use manta_core::{
    Auth, ConnectionConfig, Entry, EntryKind, Protocol, SharedVfs, SmbOptions, Vfs, VfsError,
    VfsPath, VfsResult, WebDavOptions,
};
use manta_vfs::smb::{MountRequest, Mounter, Share};

use super::{app, fixture, harness, harness_with, save, settle, wgpu_available, WIDE};
use crate::app::{Action, Side};
use crate::dialogs::{ConnectForm, Dialog, ShareList};
use crate::favorites::Favorite;
use crate::i18n::{self, Locale};
use crate::worker::ConnectRequest;

/// "Mounts" shares as folders of a temp dir; records unmounts.
#[derive(Default)]
struct FakeMounter {
    shares: HashMap<String, PathBuf>,
    mounted: Mutex<Vec<PathBuf>>,
    unmounted: Mutex<Vec<PathBuf>>,
}

impl Mounter for FakeMounter {
    fn find_mount(&self, _server: &str, share: &str) -> Option<PathBuf> {
        let p = self.shares.get(share)?;
        self.mounted.lock().unwrap().contains(p).then(|| p.clone())
    }
    fn mount(&self, req: &MountRequest) -> VfsResult<PathBuf> {
        let p = self
            .shares
            .get(&req.share)
            .cloned()
            .ok_or_else(|| VfsError::NotFound(format!("{}: no such share", req.url())))?;
        self.mounted.lock().unwrap().push(p.clone());
        Ok(p)
    }
    fn unmount(&self, mount_point: &Path) -> VfsResult<()> {
        self.mounted.lock().unwrap().retain(|p| p != mount_point);
        self.unmounted
            .lock()
            .unwrap()
            .push(mount_point.to_path_buf());
        Ok(())
    }
    fn list_shares(&self, _req: &MountRequest) -> VfsResult<Vec<Share>> {
        Ok(self
            .shares
            .keys()
            .map(|k| Share {
                name: k.clone(),
                comment: String::new(),
            })
            .collect())
    }
}

fn smb_config(password: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: String::new(),
        protocol: Protocol::Smb,
        host: "nas.local".into(),
        port: 445,
        username: "anna".into(),
        auth: Auth::Password(password.into()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: None,
        smb: Some(Box::new(SmbOptions {
            share: "Team".into(),
        })),
    }
}

fn dav_config() -> ConnectionConfig {
    ConnectionConfig {
        name: String::new(),
        protocol: Protocol::WebDavs,
        host: "cloud.example.com".into(),
        port: 443,
        username: "anna".into(),
        auth: Auth::Password(String::new()),
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
        webdav: Some(Box::new(WebDavOptions {
            root: "/remote.php/dav/files/anna".into(),
            preset: "nextcloud".into(),
        })),
        smb: None,
    }
}

/// The SMB tab, optionally with the server's shares listed.
pub(super) fn smb_form(with_shares: bool) -> ConnectForm {
    let mut f = ConnectForm::from_config(&smb_config(""), Side::Left, None);
    if with_shares {
        let share = |name: &str, comment: &str| Share {
            name: name.into(),
            comment: comment.into(),
        };
        f.smb.set_found(ShareList {
            loading: false,
            server: "nas.local".into(),
            result: Some(Ok(vec![
                share("Media", "Movies and music"),
                share("Public", ""),
                share("Team", "Project files"),
            ])),
        });
    }
    f
}

fn dav_form() -> ConnectForm {
    ConnectForm::from_config(&dav_config(), Side::Left, None)
}

/// A read-only WebDAV stand-in (Nextcloud files).
struct MockDav;

fn entry(name: &str, dir: bool, size: u64) -> Entry {
    Entry {
        name: name.into(),
        path: VfsPath::root().join(name),
        kind: if dir {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        size,
        modified: Some(Utc::now()),
        permissions: None,
        link_target: None,
        hidden: false,
    }
}

impl Vfs for MockDav {
    fn label(&self) -> String {
        "anna@cloud.example.com (WebDAVS)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::WebDavs
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }
    fn list(&self, _: &VfsPath) -> VfsResult<Vec<Entry>> {
        Ok(vec![
            entry("Documents", true, 0),
            entry("Photos", true, 0),
            entry("Nextcloud Manual.pdf", false, 4_869_380),
            entry("Readme.md", false, 1_142),
        ])
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        if path.is_root() {
            return Ok(entry("/", true, 0));
        }
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
        Ok(Box::new(std::io::Cursor::new(b"# Readme".to_vec())))
    }
    fn open_write(&self, _: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        Err(VfsError::Unsupported("mock".into()))
    }
}

fn connect(h: &mut super::H, side: Side, cfg: ConnectionConfig, fs: SharedVfs, fav: bool) {
    let a = app(h);
    let action = a.actions.next_id();
    let request = ConnectRequest {
        config: cfg.clone(),
        favorite: fav.then_some(cfg),
        side,
        action,
        keychain: false,
    };
    a.on_connected(request, Ok((fs, VfsPath::root())));
    settle(h);
}

/// A share with a few folders, mounted through the fake mounter.
fn share_dir(root: &Path) -> PathBuf {
    let dir = root.join("smb-mount").join("Team");
    for d in ["Design", "Projects", "Reports 2026"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    std::fs::write(dir.join("Budget.xlsx"), vec![0u8; 48_211]).unwrap();
    dir
}

fn mounter(dir: &Path) -> Arc<FakeMounter> {
    let mut m = FakeMounter::default();
    m.shares.insert("Team".into(), dir.to_path_buf());
    Arc::new(m)
}

#[test]
fn smb_share_opens_with_its_chip_and_unmounts_on_disconnect() {
    use crate::secrets::SecretStore as _;
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let store = crate::secrets::MemorySecrets::new();
    app(&mut h).secrets = store.clone();
    let dir = share_dir(&fx.root);
    let m = mounter(&dir);
    let cfg = smb_config("pw");
    let fs: SharedVfs = Arc::new(manta_vfs::SmbFs::connect_with(&cfg, m.clone()).expect("mount"));
    connect(&mut h, Side::Left, cfg, fs, true);

    i18n::with_locale(Locale::De, || {
        let a = app(&mut h);
        let loc = a.pane(Side::Left).tab().loc.clone();
        let chip = a.place_chip(&loc);
        assert_eq!(chip.label, "SMB · nas.local/Team");
        assert!(!chip.local);
        let names: Vec<String> = a
            .pane(Side::Left)
            .tab()
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert!(names.contains(&"Reports 2026".to_string()), "{names:?}");
    });

    // The favorite keeps the password in the Keychain, shared by every
    // share of this account on the server.
    assert_eq!(
        store.get("smb://anna@nas.local:445").unwrap().as_deref(),
        Some("pw")
    );
    let fav = app(&mut h).favorites.items.last().cloned().unwrap();
    let Favorite::Connection { config, keychain } = &fav else {
        panic!("{fav:?}")
    };
    assert!(*keychain);
    assert_eq!(config.smb.as_ref().unwrap().share, "Team");
    assert_eq!(fav.display_name(), "nas.local/Team");
    assert_eq!(
        crate::sidebar::favorite_hover(config),
        "SMB anna@nas.local/Team"
    );

    // Disconnect: back home, and the share Manta mounted is ejected.
    let id = app(&mut h).connections[0].id;
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, Action::Disconnect(id));
    settle(&mut h);
    let start = std::time::Instant::now();
    while m.unmounted.lock().unwrap().is_empty() {
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(m.unmounted.lock().unwrap()[0], dir);
    assert!(app(&mut h).pane(Side::Left).tab().loc.conn.is_none());
}

#[test]
fn webdav_pane_and_compare_labels() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    connect(&mut h, Side::Left, dav_config(), Arc::new(MockDav), false);
    let dir = share_dir(&fx.root);
    let cfg = smb_config("");
    let fs: SharedVfs = Arc::new(manta_vfs::SmbFs::connect_with(&cfg, mounter(&dir)).unwrap());
    connect(&mut h, Side::Right, cfg, fs, false);
    i18n::with_locale(Locale::En, || {
        let a = app(&mut h);
        let loc = a.pane(Side::Left).tab().loc.clone();
        assert_eq!(a.place_chip(&loc).label, "WebDAV · cloud.example.com");
        a.active = Side::Left;
    });
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, Action::ShowSync);
    let a = app(&mut h);
    assert_eq!(a.sync.left_label, "WebDAV · cloud.example.com /");
    assert_eq!(a.sync.right_label, "SMB · nas.local/Team /");
}

#[test]
fn render_smb_webdav() {
    if !wgpu_available() {
        return;
    }
    let fx = fixture();
    let dir = share_dir(&fx.root);
    for (locale, lname) in [(Locale::De, "de"), (Locale::En, "en")] {
        for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            let tag = format!("{lname}_{tname}");
            let mut h = harness(&fx, WIDE, theme);
            i18n::set_locale(locale);
            h.run_steps(3);
            app(&mut h).dialog = Some(Dialog::Connect(smb_form(true)));
            save(&mut h, &format!("smb_connect_{tag}"));
            app(&mut h).dialog = Some(Dialog::Connect(dav_form()));
            save(&mut h, &format!("webdav_connect_{tag}"));
            app(&mut h).dialog = None;

            connect(&mut h, Side::Left, dav_config(), Arc::new(MockDav), true);
            let cfg = smb_config("");
            let fs: SharedVfs =
                Arc::new(manta_vfs::SmbFs::connect_with(&cfg, mounter(&dir)).unwrap());
            connect(&mut h, Side::Right, cfg, fs, true);
            save(&mut h, &format!("smb_webdav_panes_{tag}"));
            i18n::set_locale(Locale::En);
        }
    }
}
