//! SMB shares through macOS: the share is mounted with NetFS
//! (`NetFSMountURLSync`, the same stack Finder uses: SMB 2/3, Kerberos,
//! signing, the Keychain) without any UI, and then browsed like a local
//! volume. Manta's `/` is the share's root.
//!
//! - A share that is already mounted (by Finder or another app) is reused
//!   and left mounted on disconnect; a share Manta mounted is unmounted
//!   when the last user of the connection goes away.
//! - Shares of a server are listed with `smbutil view`.
//! - Everything system-specific goes through [`Mounter`], so tests use a
//!   fake and never mount anything.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use manta_core::{
    Auth, ConnectionConfig, Entry, Protocol, ResumeCaps, Vfs, VfsError, VfsPath, VfsResult,
};

use crate::local::LocalFs;
use crate::volumes;

/// What to mount.
#[derive(Clone, PartialEq, Eq)]
pub struct MountRequest {
    pub server: String,
    /// 0 or 445: the default port.
    pub port: u16,
    pub share: String,
    /// Empty: guest / Kerberos / the Keychain.
    pub user: String,
    pub password: Option<String>,
}

impl std::fmt::Debug for MountRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MountRequest")
            .field("server", &self.server)
            .field("port", &self.port)
            .field("share", &self.share)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl MountRequest {
    pub fn from_config(cfg: &ConnectionConfig) -> MountRequest {
        MountRequest {
            server: cfg.host.trim().to_string(),
            port: cfg.port,
            share: cfg
                .smb
                .as_ref()
                .map(|s| s.share.trim().trim_matches('/').to_string())
                .unwrap_or_default(),
            user: cfg.username.trim().to_string(),
            password: match &cfg.auth {
                Auth::Password(p) if !p.is_empty() => Some(p.clone()),
                _ => None,
            },
        }
    }

    /// `smb://server[:port]/share` (no credentials).
    pub fn url(&self) -> String {
        let host = if self.server.contains(':') && !self.server.starts_with('[') {
            format!("[{}]", self.server)
        } else {
            self.server.clone()
        };
        let port = if self.port == 0 || self.port == 445 {
            String::new()
        } else {
            format!(":{}", self.port)
        };
        let share = crate::webdav::encode_path(&self.share);
        format!("smb://{host}{port}/{share}")
    }
}

/// A share on a server (for the share picker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub name: String,
    pub comment: String,
}

/// Mounting, unmounting and listing shares.
pub trait Mounter: Send + Sync {
    /// The mount point of `server/share` if it is mounted already.
    fn find_mount(&self, server: &str, share: &str) -> Option<PathBuf>;
    /// Mount the share without UI; returns the mount point.
    fn mount(&self, req: &MountRequest) -> VfsResult<PathBuf>;
    /// Unmount a share this app mounted.
    fn unmount(&self, mount_point: &Path) -> VfsResult<()>;
    /// The disk shares of `req.server` (`req.share` is ignored).
    fn list_shares(&self, req: &MountRequest) -> VfsResult<Vec<Share>>;
}

/// The system's mounter (NetFS, `diskutil`, `smbutil`).
pub fn system_mounter() -> Arc<dyn Mounter> {
    Arc::new(NetFsMounter)
}

/// The macOS implementation of [`Mounter`].
pub struct NetFsMounter;

#[cfg(target_os = "macos")]
mod netfs {
    use std::ffi::c_void;

    #[link(name = "NetFS", kind = "framework")]
    extern "C" {
        pub fn NetFSMountURLSync(
            url: *const c_void,
            mountpath: *const c_void,
            user: *const c_void,
            passwd: *const c_void,
            open_options: *const c_void,
            mount_options: *const c_void,
            mountpoints: *mut *const c_void,
        ) -> i32;
    }
}

impl Mounter for NetFsMounter {
    fn find_mount(&self, server: &str, share: &str) -> Option<PathBuf> {
        volumes::mounted()
            .into_iter()
            .filter(|v| v.fs_type == "smbfs")
            .find(|v| {
                v.server_share().is_some_and(|(h, s)| {
                    h.eq_ignore_ascii_case(server) && s.eq_ignore_ascii_case(share)
                })
            })
            .map(|v| v.path)
    }

    #[cfg(target_os = "macos")]
    fn mount(&self, req: &MountRequest) -> VfsResult<PathBuf> {
        use objc2::rc::Retained;
        use objc2_foundation::{ns_string, NSArray, NSMutableDictionary, NSString, NSURL};
        use std::ffi::c_void;

        let url = NSURL::URLWithString(&NSString::from_str(&req.url()))
            .ok_or_else(|| VfsError::Other(format!("invalid SMB address {}", req.url())))?;
        let user = (!req.user.is_empty()).then(|| NSString::from_str(&req.user));
        let password = req.password.as_deref().map(NSString::from_str);
        // No authentication dialog, no "connect as" UI: errors come back.
        let open = NSMutableDictionary::<NSString, NSString>::from_slices(
            &[ns_string!("UIOption")],
            &[ns_string!("NoUI")],
        );
        let ptr = |s: &Option<Retained<NSString>>| {
            s.as_ref()
                .map_or(std::ptr::null(), |s| Retained::as_ptr(s) as *const c_void)
        };
        let mut points: *const c_void = std::ptr::null();
        // SAFETY: all arguments are valid CF/NS objects (toll-free bridged)
        // or NULL as the API allows; `points` receives a +1 CFArray.
        let rc = unsafe {
            netfs::NetFSMountURLSync(
                Retained::as_ptr(&url) as *const c_void,
                std::ptr::null(),
                ptr(&user),
                ptr(&password),
                Retained::as_ptr(&open) as *const c_void,
                std::ptr::null(),
                &mut points,
            )
        };
        // SAFETY: NetFS returns a retained (+1) CFArrayRef of CFStringRef,
        // or leaves it NULL.
        let arr: Option<Retained<NSArray<NSString>>> =
            unsafe { Retained::from_raw(points as *mut NSArray<NSString>) };
        let mounted = arr
            .and_then(|a| a.firstObject())
            .map(|s| PathBuf::from(s.to_string()));
        match rc {
            0 => mounted.ok_or_else(|| {
                VfsError::Other(format!("{}: mounted, but no mount point", req.url()))
            }),
            // Already mounted (by Finder): use that mount.
            17 => self
                .find_mount(&req.server, &req.share)
                .or(mounted)
                .ok_or_else(|| VfsError::AlreadyExists(req.url())),
            code => Err(mount_error(code, req)),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn mount(&self, req: &MountRequest) -> VfsResult<PathBuf> {
        Err(VfsError::Unsupported(format!("SMB: {}", req.url())))
    }

    fn unmount(&self, mount_point: &Path) -> VfsResult<()> {
        volumes::eject(mount_point)
    }

    fn list_shares(&self, req: &MountRequest) -> VfsResult<Vec<Share>> {
        let run = |target: String| {
            std::process::Command::new("/usr/bin/smbutil")
                .args(["view", "-N", &target])
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| VfsError::Other(format!("smbutil: {e}")))
        };
        let enc = crate::webdav::encode_segment;
        let user = if req.user.is_empty() {
            String::new()
        } else {
            format!("{}@", enc(&req.user))
        };
        // Without a password only (guest, Kerberos, the Keychain): smbutil
        // would take a password in the address, where it shows up in `ps`.
        // If the server refuses, the share name is typed in by hand.
        let out = run(format!("//{user}{}", req.server))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if out.status.success() {
            return Ok(parse_smbutil_view(&stdout));
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        };
        let l = msg.to_ascii_lowercase();
        Err(if l.contains("authentication") || l.contains("auth") {
            VfsError::Auth(format!("{}: {msg}", req.server))
        } else if l.contains("broken pipe")
            || l.contains("timed out")
            || l.contains("refused")
            || l.contains("unreachable")
            || l.contains("unable to connect")
        {
            VfsError::Connection(format!("{}: {msg}", req.server))
        } else {
            VfsError::Other(format!("{}: {msg}", req.server))
        })
    }
}

/// NetFS / errno result codes of a failed mount.
fn mount_error(code: i32, req: &MountRequest) -> VfsError {
    let what = req.url();
    match code {
        // EPERM, EACCES, EAUTH, ENEEDAUTH; NetAuth "authentication failed".
        1 | 13 | 80 | 81 | -6600 | -5045 => VfsError::Auth(format!(
            "{what}: authentication failed (user name or password refused)"
        )),
        2 => VfsError::NotFound(format!("{what}: no such share")),
        // ETIMEDOUT, ECONNREFUSED, EHOSTDOWN, EHOSTUNREACH, ENETDOWN, ENETUNREACH.
        60 => VfsError::Connection(format!("{what}: timed out")),
        61 => VfsError::Connection(format!("{what}: connection refused")),
        50 | 51 | 64 | 65 => VfsError::Connection(format!("{what}: host is unreachable")),
        // ECANCELED: a UI would have been needed (we asked for none).
        89 => VfsError::Auth(format!("{what}: the server needs a user name and password")),
        c => VfsError::Other(format!("{what}: mount failed (error {c})")),
    }
}

/// Disk shares from `smbutil view` output. Administrative shares (`IPC$`,
/// `ADMIN$`, `C$`) are left out.
pub fn parse_smbutil_view(out: &str) -> Vec<Share> {
    let mut lines = out.lines();
    // Column of "Type" in the header.
    let mut type_col = None;
    for l in lines.by_ref() {
        if l.trim_start().starts_with("Share") {
            type_col = l.find("Type");
        }
        if l.trim_start().starts_with("---") {
            break;
        }
    }
    let mut shares = Vec::new();
    for l in lines {
        let t = l.trim_end();
        if t.is_empty() || t.ends_with("shares listed") || t.ends_with("share listed") {
            continue;
        }
        // Name, then the type word (Disk / Pipe / Printer / Device), then
        // the comment. Names may contain spaces and can be wider than the
        // header's column: take the type word nearest to that column.
        let mut split: Option<(usize, &str)> = None;
        for ty in ["Disk", "Pipe", "Printer", "Device"] {
            let mut start = 0;
            while let Some(i) = t[start..].find(ty) {
                let at = start + i;
                let end = at + ty.len();
                let bounded = at > 0
                    && t.as_bytes()[at - 1] == b' '
                    && (end == t.len() || t.as_bytes()[end] == b' ');
                if bounded {
                    let better = match (split, type_col) {
                        (None, _) => true,
                        (Some((b, _)), Some(c)) => at.abs_diff(c) < b.abs_diff(c),
                        (Some((b, _)), None) => at < b,
                    };
                    if better {
                        split = Some((at, ty));
                    }
                }
                start = at + 1;
            }
        }
        let Some((at, ty)) = split else {
            continue;
        };
        if ty != "Disk" {
            continue;
        }
        let name = t[..at].trim().to_string();
        let comment = t[at + ty.len()..].trim().to_string();
        if name.is_empty() || name.ends_with('$') {
            continue;
        }
        shares.push(Share { name, comment });
    }
    shares
}

/// The mount behind a connection; unmounts on drop if Manta mounted it.
struct Mount {
    mounter: Arc<dyn Mounter>,
    req: MountRequest,
    point: Mutex<PathBuf>,
    ours: AtomicBool,
}

impl Drop for Mount {
    fn drop(&mut self) {
        if !self.ours.load(Ordering::SeqCst) {
            return;
        }
        let point = self.point.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mounter = self.mounter.clone();
        // Unmounting can take a moment (or hang on a dead server): never on
        // the thread that dropped the connection (often the UI thread).
        let _ = std::thread::Builder::new()
            .name("smb-unmount".into())
            .spawn(move || {
                if let Err(e) = mounter.unmount(&point) {
                    log::warn!("SMB: unmounting {}: {e}", point.display());
                }
            });
    }
}

/// An SMB share mounted through macOS, browsed like a local folder.
pub struct SmbFs {
    mount: Arc<Mount>,
    label: String,
}

impl SmbFs {
    /// Mount (or reuse) the share of `cfg` with the system's NetFS.
    pub fn connect(cfg: &ConnectionConfig) -> VfsResult<SmbFs> {
        SmbFs::connect_with(cfg, system_mounter())
    }

    /// [`SmbFs::connect`] with another [`Mounter`] (tests).
    pub fn connect_with(cfg: &ConnectionConfig, mounter: Arc<dyn Mounter>) -> VfsResult<SmbFs> {
        if cfg.protocol != Protocol::Smb {
            return Err(VfsError::Other(format!(
                "not an SMB connection: {}",
                cfg.protocol
            )));
        }
        let req = MountRequest::from_config(cfg);
        if req.server.is_empty() {
            return Err(VfsError::Other("no server given".into()));
        }
        if req.share.is_empty() {
            return Err(VfsError::Other(format!("{}: no share given", req.server)));
        }
        let (point, ours) = match mounter.find_mount(&req.server, &req.share) {
            Some(p) => (p, false),
            None => (mounter.mount(&req)?, true),
        };
        let mount = Arc::new(Mount {
            mounter,
            req: req.clone(),
            point: Mutex::new(point.clone()),
            ours: AtomicBool::new(ours),
        });
        if !point.is_dir() {
            return Err(VfsError::NotFound(format!(
                "{}: the mount point {} is missing",
                req.url(),
                point.display()
            )));
        }
        let who = if req.user.is_empty() {
            String::new()
        } else {
            format!("{}@", req.user)
        };
        Ok(SmbFs {
            label: format!("{who}{}/{} (SMB)", req.server, req.share),
            mount,
        })
    }

    /// Where the share is mounted.
    pub fn mount_point(&self) -> PathBuf {
        self.mount
            .point
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Manta mounted the share (and unmounts it on disconnect).
    pub fn mounted_by_manta(&self) -> bool {
        self.mount.ours.load(Ordering::SeqCst)
    }

    /// Unmount now (app exit), if Manta mounted the share. Blocking.
    pub fn unmount_now(&self) -> VfsResult<()> {
        if self.mount.ours.swap(false, Ordering::SeqCst) {
            self.mount.mounter.unmount(&self.mount_point())
        } else {
            Ok(())
        }
    }

    /// The local path of a share path.
    fn os(&self, p: &VfsPath) -> VfsPath {
        let base = self.mount_point();
        let rel = p.as_str().trim_start_matches('/');
        let full = if rel.is_empty() { base } else { base.join(rel) };
        VfsPath::new(&full.to_string_lossy())
    }

    /// The share path of a local path below the mount point.
    fn rel(&self, local: &VfsPath) -> VfsPath {
        let base = VfsPath::new(&self.mount_point().to_string_lossy());
        match local.strip_prefix(&base) {
            Some(r) => VfsPath::new(&r),
            None => local.clone(),
        }
    }

    fn map_err(&self, e: VfsError) -> VfsError {
        // Messages name the share path, not /Volumes/...
        let base = self.mount_point().to_string_lossy().into_owned();
        let fix = |m: String| m.replacen(&base, "", 1);
        match e {
            VfsError::NotFound(m) => VfsError::NotFound(fix(m)),
            VfsError::AlreadyExists(m) => VfsError::AlreadyExists(fix(m)),
            VfsError::PermissionDenied(m) => VfsError::PermissionDenied(fix(m)),
            VfsError::NotADirectory(m) => VfsError::NotADirectory(fix(m)),
            VfsError::DirectoryNotEmpty(m) => VfsError::DirectoryNotEmpty(fix(m)),
            other => other,
        }
    }

    fn entry(&self, mut e: Entry) -> Entry {
        e.path = self.rel(&e.path);
        e
    }
}

impl Vfs for SmbFs {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn protocol(&self) -> Protocol {
        Protocol::Smb
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        Ok(LocalFs
            .list(&self.os(dir))
            .map_err(|e| self.map_err(e))?
            .into_iter()
            .map(|e| self.entry(e))
            .collect())
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let mut e = LocalFs.stat(&self.os(path)).map_err(|e| self.map_err(e))?;
        if path.is_root() {
            e.name = self.mount.req.share.clone();
        }
        Ok(self.entry(e))
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        LocalFs
            .create_dir(&self.os(path))
            .map_err(|e| self.map_err(e))
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        LocalFs
            .remove_file(&self.os(path))
            .map_err(|e| self.map_err(e))
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        if path.is_root() {
            return Err(VfsError::PermissionDenied(
                "/: the share itself can't be removed".into(),
            ));
        }
        LocalFs
            .remove_dir(&self.os(path))
            .map_err(|e| self.map_err(e))
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        LocalFs
            .rename(&self.os(from), &self.os(to))
            .map_err(|e| self.map_err(e))
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        LocalFs
            .open_read(&self.os(path))
            .map_err(|e| self.map_err(e))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        LocalFs
            .open_write(&self.os(path))
            .map_err(|e| self.map_err(e))
    }

    fn set_permissions(&self, path: &VfsPath, mode: u32) -> VfsResult<()> {
        LocalFs
            .set_permissions(&self.os(path), mode)
            .map_err(|e| self.map_err(e))
    }

    fn set_modified(&self, path: &VfsPath, mtime: DateTime<Utc>) -> VfsResult<()> {
        LocalFs
            .set_modified(&self.os(path), mtime)
            .map_err(|e| self.map_err(e))
    }

    fn local_path(&self, path: &VfsPath) -> Option<PathBuf> {
        Some(PathBuf::from(self.os(path).as_str()))
    }

    /// The mounted share reports the server's free space.
    fn space(&self, path: &VfsPath) -> VfsResult<Option<manta_core::Space>> {
        Ok(crate::space::disk_space(std::path::Path::new(
            self.os(path).as_str(),
        )))
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        LocalFs
            .open_read_at(&self.os(path), offset)
            .map_err(|e| self.map_err(e))
    }

    fn open_append(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        LocalFs
            .open_append(&self.os(path))
            .map_err(|e| self.map_err(e))
    }

    fn supports_resume(&self) -> ResumeCaps {
        LocalFs.supports_resume()
    }

    /// The share went away (network drop, sleep, ejected in Finder):
    /// mount it again.
    fn reconnect(&self) -> VfsResult<()> {
        let m = &self.mount;
        let point = self.mount_point();
        let alive = m
            .mounter
            .find_mount(&m.req.server, &m.req.share)
            .filter(|p| p.is_dir());
        let new = match alive {
            Some(p) => p,
            None => {
                let p = m.mounter.mount(&m.req)?;
                m.ours.store(true, Ordering::SeqCst);
                p
            }
        };
        if new != point {
            log::info!("SMB: {} is now mounted at {}", m.req.url(), new.display());
            *m.point.lock().unwrap_or_else(|e| e.into_inner()) = new;
        }
        Ok(())
    }
}

/// List the disk shares of the server in `cfg` (the share picker).
pub fn list_shares(cfg: &ConnectionConfig) -> VfsResult<Vec<Share>> {
    list_shares_with(cfg, &*system_mounter())
}

pub fn list_shares_with(cfg: &ConnectionConfig, mounter: &dyn Mounter) -> VfsResult<Vec<Share>> {
    let req = MountRequest::from_config(cfg);
    if req.server.is_empty() {
        return Err(VfsError::Other("no server given".into()));
    }
    let mut shares = mounter.list_shares(&req)?;
    shares.sort_by_key(|s| s.name.to_lowercase());
    Ok(shares)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manta_core::SmbOptions;
    use std::collections::HashMap;

    /// A mounter that "mounts" shares as temp folders.
    #[derive(Default)]
    pub(crate) struct FakeMounter {
        pub dirs: Mutex<HashMap<String, PathBuf>>,
        pub mounted: Mutex<Vec<(MountRequest, PathBuf)>>,
        pub unmounted: Mutex<Vec<PathBuf>>,
        pub refuse: Option<i32>,
        pub existing: Mutex<HashMap<String, PathBuf>>,
    }

    impl Mounter for FakeMounter {
        fn find_mount(&self, server: &str, share: &str) -> Option<PathBuf> {
            self.existing
                .lock()
                .unwrap()
                .get(&format!("{server}/{share}").to_lowercase())
                .cloned()
        }
        fn mount(&self, req: &MountRequest) -> VfsResult<PathBuf> {
            if let Some(code) = self.refuse {
                return Err(mount_error(code, req));
            }
            let p = self
                .dirs
                .lock()
                .unwrap()
                .get(&req.share)
                .cloned()
                .ok_or_else(|| mount_error(2, req))?;
            self.mounted.lock().unwrap().push((req.clone(), p.clone()));
            self.existing.lock().unwrap().insert(
                format!("{}/{}", req.server, req.share).to_lowercase(),
                p.clone(),
            );
            Ok(p)
        }
        fn unmount(&self, p: &Path) -> VfsResult<()> {
            self.unmounted.lock().unwrap().push(p.to_path_buf());
            self.existing.lock().unwrap().retain(|_, v| v != p);
            Ok(())
        }
        fn list_shares(&self, _req: &MountRequest) -> VfsResult<Vec<Share>> {
            Ok(self
                .dirs
                .lock()
                .unwrap()
                .keys()
                .map(|k| Share {
                    name: k.clone(),
                    comment: String::new(),
                })
                .collect())
        }
    }

    fn cfg(share: &str, password: &str) -> ConnectionConfig {
        ConnectionConfig {
            name: String::new(),
            protocol: Protocol::Smb,
            host: "nas.local".into(),
            port: 0,
            username: "alice".into(),
            auth: Auth::Password(password.into()),
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
            webdav: None,
            smb: Some(Box::new(SmbOptions {
                share: share.into(),
            })),
        }
    }

    fn wait_for(f: impl Fn() -> bool) {
        let start = std::time::Instant::now();
        while !f() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn mounts_browses_and_unmounts() {
        let td = tempfile::tempdir().unwrap();
        let share_dir = td.path().join("Team Share");
        std::fs::create_dir_all(share_dir.join("Docs")).unwrap();
        std::fs::write(share_dir.join("Docs/a.txt"), b"abc").unwrap();
        let m = Arc::new(FakeMounter::default());
        m.dirs
            .lock()
            .unwrap()
            .insert("Team Share".into(), share_dir.clone());

        let fs = SmbFs::connect_with(&cfg("/Team Share/", "pw"), m.clone()).unwrap();
        assert!(fs.mounted_by_manta());
        assert_eq!(fs.label(), "alice@nas.local/Team Share (SMB)");
        assert_eq!(fs.protocol(), Protocol::Smb);
        let (req, _) = m.mounted.lock().unwrap()[0].clone();
        assert_eq!(req.url(), "smb://nas.local/Team%20Share");
        assert_eq!(req.password.as_deref(), Some("pw"));
        assert!(!format!("{req:?}").contains("pw\""), "password hidden");

        let root = fs.list(&VfsPath::root()).unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].path, VfsPath::new("/Docs"));
        assert_eq!(fs.stat(&VfsPath::root()).unwrap().name, "Team Share");
        let mut s = String::new();
        fs.open_read(&VfsPath::new("/Docs/a.txt"))
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "abc");
        let mut w = fs.open_write(&VfsPath::new("/Docs/b.txt")).unwrap();
        w.write_all(b"x").unwrap();
        drop(w);
        assert!(share_dir.join("Docs/b.txt").exists());
        fs.rename(&VfsPath::new("/Docs/b.txt"), &VfsPath::new("/b.txt"))
            .unwrap();
        assert!(share_dir.join("b.txt").exists());
        let err = fs.stat(&VfsPath::new("/nope")).unwrap_err();
        match err {
            VfsError::NotFound(m) => assert_eq!(m, "/nope"),
            e => panic!("{e:?}"),
        }
        assert_eq!(
            fs.local_path(&VfsPath::new("/Docs")),
            Some(share_dir.join("Docs"))
        );
        assert!(fs.supports_resume().append);

        // A second connection to the same share reuses the mount.
        let fs2 = SmbFs::connect_with(&cfg("Team Share", ""), m.clone()).unwrap();
        assert!(!fs2.mounted_by_manta());
        assert_eq!(m.mounted.lock().unwrap().len(), 1);
        drop(fs2);

        // Disconnect: Manta's mount goes away.
        drop(fs);
        wait_for(|| m.unmounted.lock().unwrap().len() == 1);
        assert_eq!(m.unmounted.lock().unwrap()[0], share_dir);
    }

    #[test]
    fn existing_mounts_stay_and_reconnect_remounts() {
        let td = tempfile::tempdir().unwrap();
        let m = Arc::new(FakeMounter::default());
        m.dirs
            .lock()
            .unwrap()
            .insert("pub".into(), td.path().to_path_buf());
        m.existing
            .lock()
            .unwrap()
            .insert("nas.local/pub".into(), td.path().to_path_buf());
        let fs = SmbFs::connect_with(&cfg("pub", ""), m.clone()).unwrap();
        assert!(!fs.mounted_by_manta());
        // Ejected in Finder: reconnect mounts it again (now ours).
        m.existing.lock().unwrap().clear();
        fs.reconnect().unwrap();
        assert!(fs.mounted_by_manta());
        assert_eq!(m.mounted.lock().unwrap().len(), 1);
        drop(fs);
        wait_for(|| m.unmounted.lock().unwrap().len() == 1);
    }

    #[test]
    fn mount_errors() {
        let m = Arc::new(FakeMounter {
            refuse: Some(80),
            ..Default::default()
        });
        let e = SmbFs::connect_with(&cfg("x", "bad"), m).err().unwrap();
        assert!(matches!(e, VfsError::Auth(_)), "{e:?}");
        let m = Arc::new(FakeMounter::default());
        let e = SmbFs::connect_with(&cfg("missing", ""), m.clone())
            .err()
            .unwrap();
        assert!(matches!(e, VfsError::NotFound(_)), "{e:?}");
        let e = SmbFs::connect_with(&cfg("", ""), m.clone()).err().unwrap();
        assert!(e.to_string().contains("no share"), "{e}");
        assert!(mount_error(61, &MountRequest::from_config(&cfg("x", ""))).is_transient());
        assert!(matches!(
            mount_error(-6003, &MountRequest::from_config(&cfg("x", ""))),
            VfsError::Other(_)
        ));
    }

    #[test]
    fn share_listing() {
        let out = "Share                                           Type    Comments\n\
-------------------------------\n\
IPC$                                            Pipe    IPC Service (Samba)\n\
Public                                          Disk    Everyone\n\
Team Share                                      Disk    \n\
print$                                          Disk    Printer Drivers\n\
Laser                                           Printer HP\n\
A very long share name that is wider than the column Disk    wide\n\
\n\
6 shares listed\n";
        let s = parse_smbutil_view(out);
        assert_eq!(
            s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            [
                "Public",
                "Team Share",
                "A very long share name that is wider than the column"
            ]
        );
        assert_eq!(s[0].comment, "Everyone");

        let td = tempfile::tempdir().unwrap();
        let m = FakeMounter::default();
        m.dirs
            .lock()
            .unwrap()
            .insert("b".into(), td.path().to_path_buf());
        m.dirs
            .lock()
            .unwrap()
            .insert("A".into(), td.path().to_path_buf());
        let names: Vec<String> = list_shares_with(&cfg("", ""), &m)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["A", "b"]);
    }

    #[test]
    fn urls() {
        let mut r = MountRequest::from_config(&cfg("Media/Photos", ""));
        assert_eq!(r.url(), "smb://nas.local/Media/Photos");
        r.port = 1445;
        r.server = "fe80::1".into();
        assert_eq!(r.url(), "smb://[fe80::1]:1445/Media/Photos");
        assert_eq!(r.password, None);
    }
}
