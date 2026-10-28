//! Cloud folders that macOS already syncs: iCloud Drive and the File
//! Provider domains under `~/Library/CloudStorage` (Google Drive for
//! desktop, OneDrive, Dropbox, Box, and others). They are ordinary local
//! folders for Manta ([`crate::LocalFs`]); this module finds them, reports
//! the download/sync state of their items and starts downloads / evictions.
//!
//! - [`discover_cloud_locations`]: fast (a couple of `read_dir`s, no network
//!   and no file content access), fine for a background thread at start-up.
//! - [`item_status`] / [`folder_status`] / [`SyncStatusCache`]: per-item
//!   state from `NSURL` resource values (`NSURLUbiquitousItem…` keys) plus
//!   the `SF_DATALESS` flag of File Provider placeholders. Never touches file
//!   contents, so it doesn't trigger downloads.
//! - [`start_download`] / [`evict`]: "Download now" / "Remove download".
//!   Blocking; call them on a background thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use manta_core::VfsResult;

/// Known providers (everything else is [`CloudProvider::Other`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudProvider {
    ICloud,
    GoogleDrive,
    OneDrive,
    Dropbox,
    Box,
    Other,
}

impl CloudProvider {
    /// Stable id: `icloud`, `google-drive`, `onedrive`, `dropbox`, `box`, `other`.
    pub fn id(self) -> &'static str {
        match self {
            CloudProvider::ICloud => "icloud",
            CloudProvider::GoogleDrive => "google-drive",
            CloudProvider::OneDrive => "onedrive",
            CloudProvider::Dropbox => "dropbox",
            CloudProvider::Box => "box",
            CloudProvider::Other => "other",
        }
    }

    /// SF Symbol name that fits the provider.
    pub fn sf_symbol(self) -> &'static str {
        match self {
            CloudProvider::ICloud => "icloud",
            CloudProvider::GoogleDrive => "externaldrive.connected.to.line.below",
            CloudProvider::OneDrive => "cloud",
            CloudProvider::Dropbox => "shippingbox",
            CloudProvider::Box => "archivebox",
            CloudProvider::Other => "externaldrive.badge.icloud",
        }
    }

    /// Phosphor icon name (the app's icon font): `cloud`,
    /// `google-drive-logo`, `dropbox-logo`, `package`, ...
    pub fn phosphor(self) -> &'static str {
        match self {
            CloudProvider::ICloud | CloudProvider::OneDrive | CloudProvider::Other => "cloud",
            CloudProvider::GoogleDrive => "google-drive-logo",
            CloudProvider::Dropbox => "dropbox-logo",
            CloudProvider::Box => "package",
        }
    }
}

/// A synced cloud folder on this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudLocation {
    pub provider: CloudProvider,
    /// `icloud`, `google-drive`, ... ([`CloudProvider::id`]); for
    /// [`CloudProvider::Other`] the app part of the folder name, lower-case.
    pub provider_id: String,
    /// "iCloud Drive", "Google Drive (name@gmail.com)", "OneDrive – Firma".
    pub display_name: String,
    /// Account / organisation part of the folder name, if any.
    pub account: Option<String>,
    pub path: PathBuf,
    /// SF Symbol name ([`CloudProvider::sf_symbol`]).
    pub icon: &'static str,
    /// Managed by a File Provider extension (`~/Library/CloudStorage`)
    /// rather than iCloud's own sync.
    pub file_provider: bool,
}

/// Environment variable that replaces the home directory for
/// [`discover_cloud_locations`] (tests, demos): `MANTA_CLOUD_HOME=/tmp/fake`.
pub const HOME_OVERRIDE_ENV: &str = "MANTA_CLOUD_HOME";

/// The home directory cloud folders are looked for in
/// ([`HOME_OVERRIDE_ENV`], else the user's home).
pub fn cloud_home() -> Option<PathBuf> {
    std::env::var_os(HOME_OVERRIDE_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

/// `~/Library/CloudStorage` (watched for new / removed providers).
pub fn cloud_storage_dir() -> Option<PathBuf> {
    cloud_home().map(|h| h.join("Library/CloudStorage"))
}

/// Find iCloud Drive and the File Provider folders of the current user.
pub fn discover_cloud_locations() -> Vec<CloudLocation> {
    match cloud_home() {
        Some(home) => discover_in(&home),
        None => Vec::new(),
    }
}

/// [`discover_cloud_locations`] for another home directory (tests).
pub fn discover_in(home: &Path) -> Vec<CloudLocation> {
    let mut out = Vec::new();
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud.is_dir() {
        out.push(CloudLocation {
            provider: CloudProvider::ICloud,
            provider_id: CloudProvider::ICloud.id().into(),
            display_name: "iCloud Drive".into(),
            account: None,
            path: icloud,
            icon: CloudProvider::ICloud.sf_symbol(),
            file_provider: false,
        });
    }
    let storage = home.join("Library/CloudStorage");
    let mut found: Vec<CloudLocation> = std::fs::read_dir(&storage)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !e.path().is_dir() {
                return None;
            }
            let p = parse_cloud_storage_name(&name);
            if p.provider == CloudProvider::ICloud
                && out.iter().any(|l| l.provider == CloudProvider::ICloud)
            {
                return None;
            }
            Some(CloudLocation {
                provider: p.provider,
                provider_id: p.provider_id,
                display_name: p.display_name,
                account: p.account,
                path: e.path(),
                icon: p.provider.sf_symbol(),
                file_provider: true,
            })
        })
        .collect();
    // Dropbox's classic (non-File-Provider) folder.
    let legacy_dropbox = home.join("Dropbox");
    if !found.iter().any(|l| l.provider == CloudProvider::Dropbox)
        && legacy_dropbox.join(".dropbox").exists()
        && !std::fs::symlink_metadata(&legacy_dropbox)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
    {
        found.push(CloudLocation {
            provider: CloudProvider::Dropbox,
            provider_id: CloudProvider::Dropbox.id().into(),
            display_name: "Dropbox".into(),
            account: None,
            path: legacy_dropbox,
            icon: CloudProvider::Dropbox.sf_symbol(),
            file_provider: false,
        });
    }
    found.sort_by_key(|a| a.display_name.to_lowercase());
    out.extend(found);
    out
}

/// The cloud location `path` lies in, if any. The apps' folders in
/// iCloud Drive (`Mobile Documents/<app>/Documents`) belong to iCloud Drive.
pub fn location_of<'a>(path: &Path, locations: &'a [CloudLocation]) -> Option<&'a CloudLocation> {
    locations
        .iter()
        .filter(|l| {
            path.starts_with(&l.path)
                || (l.provider == CloudProvider::ICloud
                    && l.path.parent().is_some_and(|mobile| {
                        crate::icloud::app_library_in(mobile, path).is_some()
                    }))
        })
        .max_by_key(|l| l.path.as_os_str().len())
}

/// What a `~/Library/CloudStorage` folder name says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    pub provider: CloudProvider,
    pub provider_id: String,
    pub display_name: String,
    pub account: Option<String>,
}

/// `GoogleDrive-a@b.com` -> Google Drive (a@b.com), `OneDrive-FirmaGmbH` ->
/// OneDrive – Firma GmbH, `Box-Box` -> Box, `Dropbox` -> Dropbox,
/// `ProtonDrive-a@b.c-folder` -> Proton Drive (a@b.c-folder), ...
pub fn parse_cloud_storage_name(name: &str) -> ParsedName {
    let (app, account) = match name.split_once('-') {
        Some((a, rest)) if !a.is_empty() && !rest.is_empty() => (a, Some(rest.to_string())),
        _ => (name, None),
    };
    let (provider, app_name) = match app.to_ascii_lowercase().as_str() {
        "googledrive" => (CloudProvider::GoogleDrive, "Google Drive".to_string()),
        "onedrive" => (CloudProvider::OneDrive, "OneDrive".to_string()),
        "dropbox" => (CloudProvider::Dropbox, "Dropbox".to_string()),
        "box" => (CloudProvider::Box, "Box".to_string()),
        "iclouddrive" | "icloud" => (CloudProvider::ICloud, "iCloud Drive".to_string()),
        "protondrive" => (CloudProvider::Other, "Proton Drive".to_string()),
        "synologydrive" => (CloudProvider::Other, "Synology Drive".to_string()),
        "pclouddrive" => (CloudProvider::Other, "pCloud Drive".to_string()),
        _ => (CloudProvider::Other, app.to_string()),
    };
    let provider_id = match provider {
        CloudProvider::Other => app.to_ascii_lowercase(),
        p => p.id().to_string(),
    };
    // "Box-Box", "iCloudDrive-iCloudDrive": the account repeats the app.
    let account = account.filter(|a| !a.eq_ignore_ascii_case(app));
    let display_name = match (&account, provider) {
        (None, _) => app_name,
        (Some(a), CloudProvider::OneDrive) => match a.strip_prefix("SharedLibraries-") {
            Some(_) => format!("{app_name} – Shared Libraries"),
            None if a == "Personal" || a == "Persönlich" => format!("{app_name} – {a}"),
            None => format!("{app_name} – {}", split_camel(a)),
        },
        (Some(a), _) if a.contains('@') => format!("{app_name} ({a})"),
        (Some(a), _) => format!("{app_name} – {a}"),
    };
    ParsedName {
        provider,
        provider_id,
        display_name,
        account,
    }
}

/// OneDrive drops the spaces of organisation names: `ContosoTravelGmbH`
/// -> `Contoso Travel GmbH`. Splits before an upper-case letter that
/// follows a lower-case one and is followed by a lower-case one.
fn split_camel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0
            && c.is_uppercase()
            && chars[i - 1].is_lowercase()
            && chars.get(i + 1).is_some_and(|n| n.is_lowercase())
        {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Download / sync state of one item in a cloud folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SyncState {
    /// Not managed by a cloud provider (or the state is unknown).
    #[default]
    Local,
    /// Only in the cloud: a placeholder (iCloud "not downloaded", File
    /// Provider dataless file).
    CloudOnly,
    /// Downloaded, but a newer version exists in the cloud.
    Outdated,
    /// Downloaded and up to date.
    Current,
    /// Being downloaded.
    Downloading,
    /// Local changes being uploaded, or waiting to be uploaded (see
    /// [`SyncInfo::upload_waiting`]).
    Uploading,
    /// The provider couldn't upload the item (see [`SyncInfo::upload_error`]).
    UploadError,
}

/// One item's sync state plus the details a tooltip shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncInfo {
    pub state: SyncState,
    /// [`SyncState::Uploading`] but not started yet: the provider says "not
    /// uploaded" (`NSURLUbiquitousItemIsUploadedKey`) and isn't uploading.
    pub upload_waiting: bool,
    /// The provider's upload error (`NSURLUbiquitousItemUploadingErrorKey`).
    pub upload_error: Option<String>,
    /// Upload progress 0–100 where the system reports it (iCloud Drive,
    /// `NSMetadataUbiquitousItemPercentUploadedKey`); `None` elsewhere.
    pub upload_percent: Option<f32>,
}

impl SyncInfo {
    pub fn new(state: SyncState) -> SyncInfo {
        SyncInfo {
            state,
            ..SyncInfo::default()
        }
    }
}

impl From<SyncState> for SyncInfo {
    fn from(state: SyncState) -> SyncInfo {
        SyncInfo::new(state)
    }
}

/// Files that providers never upload (Finder's `.DS_Store`, Office lock
/// files, ...). They report "not uploaded" forever, so that alone doesn't
/// mean an upload is pending.
pub fn never_uploaded(name: &str) -> bool {
    matches!(
        name,
        ".DS_Store" | "Icon\r" | ".localized" | "desktop.ini" | "Thumbs.db"
    ) || name.starts_with("~$")
        || name.starts_with(".~lock.")
        || name.ends_with(".tmp")
}

impl SyncState {
    /// SF Symbol for a badge (`None`: no badge).
    pub fn sf_symbol(self) -> Option<&'static str> {
        match self {
            SyncState::Local | SyncState::Current => None,
            SyncState::CloudOnly => Some("icloud.and.arrow.down"),
            SyncState::Outdated => Some("exclamationmark.icloud"),
            SyncState::Downloading => Some("arrow.down.circle"),
            SyncState::Uploading => Some("arrow.up.circle"),
            SyncState::UploadError => Some("exclamationmark.icloud.fill"),
        }
    }

    /// Phosphor icon name for a badge (`None`: no badge).
    pub fn phosphor(self) -> Option<&'static str> {
        match self {
            SyncState::Local | SyncState::Current => None,
            SyncState::CloudOnly => Some("cloud-arrow-down"),
            SyncState::Outdated => Some("cloud-warning"),
            SyncState::Downloading => Some("arrow-circle-down"),
            SyncState::Uploading => Some("cloud-arrow-up"),
            SyncState::UploadError => Some("warning-circle"),
        }
    }

    /// Can "Download now" do something?
    pub fn can_download(self) -> bool {
        matches!(self, SyncState::CloudOnly | SyncState::Outdated)
    }

    /// Can "Remove download" do something?
    pub fn can_evict(self) -> bool {
        matches!(self, SyncState::Current | SyncState::Outdated)
    }
}

/// `SF_DATALESS` (sys/stat.h): a File Provider placeholder without content.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Is `path` a dataless placeholder? Uses `lstat`, which never materialises.
pub fn is_dataless(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        std::fs::symlink_metadata(path)
            .map(|m| m.st_flags() & SF_DATALESS != 0)
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// The state that the `st_flags` of an item alone say (no resource values):
/// `SF_DATALESS` = only in the cloud. For fakes and fixtures.
pub fn state_from_flags(st_flags: u32) -> SyncState {
    combine(st_flags & SF_DATALESS != 0, None)
}

/// Combine the raw facts into a state (pure; unit-tested).
pub(crate) fn combine(dataless: bool, info: Option<platform::UbiquityInfo>) -> SyncState {
    combine_info("", dataless, info).state
}

/// Combine the raw facts about the item `name` into a state with details
/// (pure; unit-tested).
pub(crate) fn combine_info(
    name: &str,
    dataless: bool,
    info: Option<platform::UbiquityInfo>,
) -> SyncInfo {
    let Some(i) = info.filter(|i| i.ubiquitous) else {
        return SyncInfo::new(if dataless {
            SyncState::CloudOnly
        } else {
            SyncState::Local
        });
    };
    if i.downloading {
        return SyncInfo::new(SyncState::Downloading);
    }
    if let Some(e) = i.upload_error {
        return SyncInfo {
            state: SyncState::UploadError,
            upload_error: Some(e),
            ..SyncInfo::default()
        };
    }
    if i.uploading {
        return SyncInfo::new(SyncState::Uploading);
    }
    if dataless {
        return SyncInfo::new(SyncState::CloudOnly);
    }
    let not_downloaded = i.status == Some(platform::DownloadStatus::NotDownloaded);
    if i.uploaded == Some(false) && !not_downloaded && !never_uploaded(name) {
        return SyncInfo {
            state: SyncState::Uploading,
            upload_waiting: true,
            ..SyncInfo::default()
        };
    }
    SyncInfo::new(match i.status {
        Some(platform::DownloadStatus::NotDownloaded) => SyncState::CloudOnly,
        Some(platform::DownloadStatus::Downloaded) => SyncState::Outdated,
        Some(platform::DownloadStatus::Current) | None => SyncState::Current,
    })
}

/// The sync state of one item. Cheap-ish (one `lstat` plus one resource
/// value query); doesn't download anything.
pub fn item_status(path: &Path) -> SyncState {
    item_info(path).state
}

/// [`item_status`] with the upload details (error text; the upload
/// percentage where the system reports one). Read-only, never downloads.
pub fn item_info(path: &Path) -> SyncInfo {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let mut info = combine_info(&name, is_dataless(path), platform::ubiquity_info(path));
    if info.state == SyncState::Uploading && !info.upload_waiting {
        info.upload_percent = platform::upload_percent(path);
    }
    info
}

/// The sync state of every item in `dir` (name, state), sorted by name.
pub fn folder_status(dir: &Path) -> VfsResult<Vec<(String, SyncState)>> {
    Ok(folder_info(dir)?
        .into_iter()
        .map(|(n, i)| (n, i.state))
        .collect())
}

/// [`folder_status`] with the upload details of each item.
pub fn folder_info(dir: &Path) -> VfsResult<Vec<(String, SyncInfo)>> {
    let mut out: Vec<(String, SyncInfo)> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                item_info(&e.path()),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Per-folder cache of [`folder_status`], valid while the folder's mtime is
/// unchanged and for at most `ttl` (downloads and uploads change the state
/// without touching the folder's mtime).
pub struct SyncStatusCache {
    ttl: Duration,
    map: Mutex<HashMap<PathBuf, CacheEntry>>,
}

struct CacheEntry {
    mtime: Option<SystemTime>,
    at: Instant,
    states: Arc<HashMap<String, SyncState>>,
}

impl Default for SyncStatusCache {
    fn default() -> Self {
        SyncStatusCache::new(Duration::from_secs(3))
    }
}

impl SyncStatusCache {
    pub fn new(ttl: Duration) -> SyncStatusCache {
        SyncStatusCache {
            ttl,
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Cached states if still valid; otherwise recomputes them (blocking, so
    /// call this from a background thread).
    pub fn folder(&self, dir: &Path) -> VfsResult<Arc<HashMap<String, SyncState>>> {
        let mtime = std::fs::metadata(dir).and_then(|m| m.modified()).ok();
        if let Some(hit) = self.cached(dir, mtime) {
            return Ok(hit);
        }
        let states: Arc<HashMap<String, SyncState>> =
            Arc::new(folder_status(dir)?.into_iter().collect());
        self.map.lock().unwrap().insert(
            dir.to_path_buf(),
            CacheEntry {
                mtime,
                at: Instant::now(),
                states: states.clone(),
            },
        );
        Ok(states)
    }

    /// Only the cached value (never blocks on the file system).
    pub fn peek(&self, dir: &Path) -> Option<Arc<HashMap<String, SyncState>>> {
        self.map.lock().unwrap().get(dir).map(|e| e.states.clone())
    }

    fn cached(
        &self,
        dir: &Path,
        mtime: Option<SystemTime>,
    ) -> Option<Arc<HashMap<String, SyncState>>> {
        let map = self.map.lock().unwrap();
        let e = map.get(dir)?;
        (e.mtime == mtime && e.at.elapsed() < self.ttl).then(|| e.states.clone())
    }

    /// Drop a folder (after an action on one of its items).
    pub fn invalidate(&self, dir: &Path) {
        self.map.lock().unwrap().remove(dir);
    }
}

/// "Download now". iCloud: `NSFileManager startDownloadingUbiquitousItemAtURL`
/// (asynchronous; watch the state). Other File Provider domains: the same
/// call where the provider supports it, otherwise reading the first byte,
/// which makes macOS materialise the whole file (blocking until done).
/// Folders: every item below them. Blocking; call it off the UI thread.
pub fn start_download(path: &Path) -> VfsResult<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        let mut first_err = None;
        for e in std::fs::read_dir(path)?.flatten() {
            if let Err(err) = start_download(&e.path()) {
                first_err.get_or_insert(err);
            }
        }
        return first_err.map_or(Ok(()), Err);
    }
    match platform::start_downloading(path) {
        Ok(()) => Ok(()),
        Err(e) if is_dataless(path) => {
            log::debug!(
                "startDownloadingUbiquitousItem {}: {e}; reading instead",
                path.display()
            );
            materialise_by_reading(path)
        }
        Err(e) => Err(e),
    }
}

fn materialise_by_reading(path: &Path) -> VfsResult<()> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut b = [0u8; 1];
    let _ = f.read(&mut b)?;
    Ok(())
}

/// "Remove download": `NSFileManager evictUbiquitousItemAtURL`. Works for
/// iCloud Drive; for other File Provider domains only if the provider
/// accepts it, otherwise `VfsError::Unsupported` with a hint to use Finder's
/// "Remove Download". Never deletes anything from the cloud.
pub fn evict(path: &Path) -> VfsResult<()> {
    platform::evict(path)
}

#[cfg(target_os = "macos")]
pub(crate) mod platform {
    use std::path::Path;

    use manta_core::{VfsError, VfsResult};
    use objc2::rc::autoreleasepool;
    use objc2::AllocAnyThread;
    use objc2_foundation::{
        NSArray, NSError, NSFileManager, NSMetadataItem,
        NSMetadataUbiquitousItemPercentUploadedKey, NSNumber, NSString, NSURLIsUbiquitousItemKey,
        NSURLUbiquitousItemDownloadingStatusCurrent,
        NSURLUbiquitousItemDownloadingStatusDownloaded, NSURLUbiquitousItemDownloadingStatusKey,
        NSURLUbiquitousItemDownloadingStatusNotDownloaded, NSURLUbiquitousItemIsDownloadingKey,
        NSURLUbiquitousItemIsUploadedKey, NSURLUbiquitousItemIsUploadingKey,
        NSURLUbiquitousItemUploadingErrorKey, NSURL,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DownloadStatus {
        NotDownloaded,
        Downloaded,
        Current,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct UbiquityInfo {
        pub ubiquitous: bool,
        pub status: Option<DownloadStatus>,
        pub downloading: bool,
        pub uploading: bool,
        /// `NSURLUbiquitousItemIsUploadedKey` (`None`: not reported).
        pub uploaded: Option<bool>,
        /// `NSURLUbiquitousItemUploadingErrorKey`'s description.
        pub upload_error: Option<String>,
    }

    pub fn ubiquity_info(path: &Path) -> Option<UbiquityInfo> {
        autoreleasepool(|_| {
            let url = NSURL::from_file_path(path)?;
            // SAFETY: Foundation's constant keys, valid for the process lifetime.
            let (k_ubiq, k_status, k_down, k_up, k_uploaded, k_err) = unsafe {
                (
                    NSURLIsUbiquitousItemKey,
                    NSURLUbiquitousItemDownloadingStatusKey,
                    NSURLUbiquitousItemIsDownloadingKey,
                    NSURLUbiquitousItemIsUploadingKey,
                    NSURLUbiquitousItemIsUploadedKey,
                    NSURLUbiquitousItemUploadingErrorKey,
                )
            };
            let keys = NSArray::from_slice(&[k_ubiq, k_status, k_down, k_up, k_uploaded, k_err]);
            let dict = url.resourceValuesForKeys_error(&keys).ok()?;
            let flag = |k: &NSString| {
                dict.objectForKey(k)
                    .and_then(|o| o.downcast::<NSNumber>().ok())
                    .is_some_and(|n| n.boolValue())
            };
            let status = dict
                .objectForKey(k_status)
                .and_then(|o| o.downcast::<NSString>().ok())
                .and_then(|s| {
                    // SAFETY: as above.
                    let (nd, d, c) = unsafe {
                        (
                            NSURLUbiquitousItemDownloadingStatusNotDownloaded,
                            NSURLUbiquitousItemDownloadingStatusDownloaded,
                            NSURLUbiquitousItemDownloadingStatusCurrent,
                        )
                    };
                    if &*s == nd {
                        Some(DownloadStatus::NotDownloaded)
                    } else if &*s == d {
                        Some(DownloadStatus::Downloaded)
                    } else if &*s == c {
                        Some(DownloadStatus::Current)
                    } else {
                        None
                    }
                });
            let uploaded = dict
                .objectForKey(k_uploaded)
                .and_then(|o| o.downcast::<NSNumber>().ok())
                .map(|n| n.boolValue());
            let upload_error = dict
                .objectForKey(k_err)
                .and_then(|o| o.downcast::<NSError>().ok())
                .map(|e| e.localizedDescription().to_string());
            Some(UbiquityInfo {
                ubiquitous: flag(k_ubiq),
                status,
                downloading: flag(k_down),
                uploading: flag(k_up),
                uploaded,
                upload_error,
            })
        })
    }

    /// `NSMetadataUbiquitousItemPercentUploadedKey` of the item's metadata
    /// (reported for iCloud Drive while uploading; `None` elsewhere).
    /// Read-only and quick (no query, no run loop).
    pub fn upload_percent(path: &Path) -> Option<f32> {
        autoreleasepool(|_| {
            let url = NSURL::from_file_path(path)?;
            let item = NSMetadataItem::initWithURL(NSMetadataItem::alloc(), &url)?;
            // SAFETY: Foundation's constant key, valid for the process lifetime.
            let key = unsafe { NSMetadataUbiquitousItemPercentUploadedKey };
            let n = item.valueForAttribute(key)?.downcast::<NSNumber>().ok()?;
            let v = n.doubleValue() as f32;
            (0.0..=100.0).contains(&v).then_some(v)
        })
    }

    fn url(path: &Path) -> VfsResult<objc2::rc::Retained<NSURL>> {
        NSURL::from_file_path(path)
            .ok_or_else(|| VfsError::Other(format!("{}: invalid path", path.display())))
    }

    pub fn start_downloading(path: &Path) -> VfsResult<()> {
        autoreleasepool(|_| {
            let url = url(path)?;
            NSFileManager::defaultManager()
                .startDownloadingUbiquitousItemAtURL_error(&url)
                .map_err(|e| {
                    VfsError::Other(format!("{}: {}", path.display(), e.localizedDescription()))
                })
        })
    }

    pub fn evict(path: &Path) -> VfsResult<()> {
        autoreleasepool(|_| {
            let url = url(path)?;
            NSFileManager::defaultManager().evictUbiquitousItemAtURL_error(&url).map_err(|e| {
                VfsError::Unsupported(format!(
                    "{}: this cloud provider doesn't let Manta remove the download ({}); use “Remove Download” in Finder",
                    path.display(),
                    e.localizedDescription()
                ))
            })
        })
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) mod platform {
    use std::path::Path;

    use manta_core::{VfsError, VfsResult};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DownloadStatus {
        NotDownloaded,
        Downloaded,
        Current,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct UbiquityInfo {
        pub ubiquitous: bool,
        pub status: Option<DownloadStatus>,
        pub downloading: bool,
        pub uploading: bool,
        pub uploaded: Option<bool>,
        pub upload_error: Option<String>,
    }

    pub fn ubiquity_info(_path: &Path) -> Option<UbiquityInfo> {
        None
    }
    pub fn upload_percent(_path: &Path) -> Option<f32> {
        None
    }
    pub fn start_downloading(_path: &Path) -> VfsResult<()> {
        Err(VfsError::Unsupported("cloud downloads need macOS".into()))
    }
    pub fn evict(_path: &Path) -> VfsResult<()> {
        Err(VfsError::Unsupported("cloud downloads need macOS".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::platform::{DownloadStatus, UbiquityInfo};
    use super::*;

    #[test]
    fn parses_folder_names() {
        let p = parse_cloud_storage_name("GoogleDrive-name@gmail.com");
        assert_eq!(p.provider, CloudProvider::GoogleDrive);
        assert_eq!(p.display_name, "Google Drive (name@gmail.com)");
        assert_eq!(p.account.as_deref(), Some("name@gmail.com"));
        // Emails with dashes stay whole.
        assert_eq!(
            parse_cloud_storage_name("GoogleDrive-a-b@x-y.de").display_name,
            "Google Drive (a-b@x-y.de)"
        );
        assert_eq!(
            parse_cloud_storage_name("OneDrive-Firma").display_name,
            "OneDrive – Firma"
        );
        assert_eq!(
            parse_cloud_storage_name("OneDrive-ContosoTravelGmbH").display_name,
            "OneDrive – Contoso Travel GmbH"
        );
        assert_eq!(
            parse_cloud_storage_name("OneDrive-Personal").display_name,
            "OneDrive – Personal"
        );
        assert_eq!(
            parse_cloud_storage_name("OneDrive-SharedLibraries-OneDriveCloudTemp").display_name,
            "OneDrive – Shared Libraries"
        );
        let b = parse_cloud_storage_name("Box-Box");
        assert_eq!(
            (b.provider, b.display_name.as_str(), b.account),
            (CloudProvider::Box, "Box", None)
        );
        let d = parse_cloud_storage_name("Dropbox");
        assert_eq!(
            (d.provider, d.display_name.as_str()),
            (CloudProvider::Dropbox, "Dropbox")
        );
        assert_eq!(
            parse_cloud_storage_name("Dropbox-Team").display_name,
            "Dropbox – Team"
        );
        assert_eq!(
            parse_cloud_storage_name("iCloudDrive-iCloudDrive").provider,
            CloudProvider::ICloud
        );
        let o = parse_cloud_storage_name("ProtonDrive-me@proton.me-folder");
        assert_eq!(
            (o.provider, o.provider_id.as_str()),
            (CloudProvider::Other, "protondrive")
        );
        assert_eq!(o.display_name, "Proton Drive (me@proton.me-folder)");
        let n = parse_cloud_storage_name("Nextcloud");
        assert_eq!(
            (n.provider_id.as_str(), n.display_name.as_str()),
            ("nextcloud", "Nextcloud")
        );
    }

    #[test]
    fn discovers_fixture_folders() {
        let home = tempfile::tempdir().unwrap();
        let cs = home.path().join("Library/CloudStorage");
        for d in [
            "GoogleDrive-name@gmail.com",
            "OneDrive-Firma",
            "Dropbox",
            "Box-Box",
            ".hidden",
            "SynologyDrive-nas",
        ] {
            std::fs::create_dir_all(cs.join(d)).unwrap();
        }
        std::fs::write(cs.join("stray-file"), b"").unwrap();
        std::fs::create_dir_all(
            home.path()
                .join("Library/Mobile Documents/com~apple~CloudDocs"),
        )
        .unwrap();
        let found = discover_in(home.path());
        let names: Vec<&str> = found.iter().map(|l| l.display_name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "iCloud Drive",
                "Box",
                "Dropbox",
                "Google Drive (name@gmail.com)",
                "OneDrive – Firma",
                "Synology Drive – nas"
            ]
        );
        assert!(!found[0].file_provider && found[1].file_provider);
        assert_eq!(found[3].path, cs.join("GoogleDrive-name@gmail.com"));
        assert_eq!(found[3].icon, CloudProvider::GoogleDrive.sf_symbol());
        let inner = cs.join("OneDrive-Firma/Docs/x.txt");
        assert_eq!(
            location_of(&inner, &found).map(|l| l.provider),
            Some(CloudProvider::OneDrive)
        );
        assert!(location_of(Path::new("/tmp"), &found).is_none());
        // An app's folder in iCloud Drive belongs to iCloud Drive.
        let mobile = home.path().join("Library/Mobile Documents");
        assert_eq!(
            location_of(&mobile.join("iCloud~md~obsidian/Documents/a.md"), &found)
                .map(|l| l.provider),
            Some(CloudProvider::ICloud)
        );
        assert!(location_of(&mobile.join("iCloud~md~obsidian"), &found).is_none());

        // Nothing there: nothing found (and no panic).
        assert!(discover_in(&home.path().join("nope")).is_empty());
    }

    #[test]
    fn home_override_env() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(
            home.path()
                .join("Library/CloudStorage/GoogleDrive-me@example.com"),
        )
        .unwrap();
        // Only this test sets the variable (other tests use `discover_in`).
        std::env::set_var(HOME_OVERRIDE_ENV, home.path());
        let found = discover_cloud_locations();
        let dir = cloud_storage_dir();
        std::env::remove_var(HOME_OVERRIDE_ENV);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].account.as_deref(), Some("me@example.com"));
        assert_eq!(dir, Some(home.path().join("Library/CloudStorage")));
        assert_eq!(state_from_flags(SF_DATALESS | 0x20), SyncState::CloudOnly);
        assert_eq!(state_from_flags(0), SyncState::Local);
    }

    #[test]
    fn legacy_dropbox_folder() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Dropbox/.dropbox")).unwrap();
        let found = discover_in(home.path());
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].provider, found[0].file_provider),
            (CloudProvider::Dropbox, false)
        );
    }

    #[test]
    fn state_mapping() {
        let info = |status, downloading, uploading| {
            Some(UbiquityInfo {
                ubiquitous: true,
                status,
                downloading,
                uploading,
                ..UbiquityInfo::default()
            })
        };
        assert_eq!(combine(false, None), SyncState::Local);
        assert_eq!(combine(true, None), SyncState::CloudOnly);
        assert_eq!(
            combine(
                false,
                info(Some(DownloadStatus::NotDownloaded), false, false)
            ),
            SyncState::CloudOnly
        );
        assert_eq!(
            combine(false, info(Some(DownloadStatus::Downloaded), false, false)),
            SyncState::Outdated
        );
        assert_eq!(
            combine(false, info(Some(DownloadStatus::Current), false, false)),
            SyncState::Current
        );
        assert_eq!(
            combine(true, info(Some(DownloadStatus::Current), false, false)),
            SyncState::CloudOnly
        );
        assert_eq!(
            combine(true, info(None, true, false)),
            SyncState::Downloading
        );
        assert_eq!(
            combine(false, info(Some(DownloadStatus::Current), false, true)),
            SyncState::Uploading
        );
        assert!(SyncState::CloudOnly.can_download() && !SyncState::CloudOnly.can_evict());
        assert!(SyncState::Current.can_evict() && SyncState::Current.sf_symbol().is_none());
    }

    #[test]
    fn upload_states() {
        let base = UbiquityInfo {
            ubiquitous: true,
            status: Some(DownloadStatus::Current),
            uploaded: Some(true),
            ..UbiquityInfo::default()
        };
        assert_eq!(
            combine_info("a.pdf", false, Some(base.clone())),
            SyncInfo::new(SyncState::Current)
        );
        // Uploading right now.
        let up = UbiquityInfo {
            uploading: true,
            uploaded: Some(false),
            ..base.clone()
        };
        assert_eq!(
            combine_info("a.pdf", false, Some(up)),
            SyncInfo::new(SyncState::Uploading)
        );
        // Not uploaded yet and not uploading: waiting for the provider.
        let pending = UbiquityInfo {
            uploaded: Some(false),
            ..base.clone()
        };
        let i = combine_info("a.pdf", false, Some(pending.clone()));
        assert_eq!((i.state, i.upload_waiting), (SyncState::Uploading, true));
        // ... except files providers never upload (OneDrive's .DS_Store).
        assert_eq!(
            combine_info(".DS_Store", false, Some(pending.clone())).state,
            SyncState::Current
        );
        assert_eq!(
            combine_info("~$Brief.docx", false, Some(pending)).state,
            SyncState::Current
        );
        // An upload error wins over "uploading".
        let err = UbiquityInfo {
            uploading: true,
            upload_error: Some("Quota exceeded".into()),
            ..base.clone()
        };
        let i = combine_info("a.pdf", false, Some(err));
        assert_eq!(
            (i.state, i.upload_error.as_deref()),
            (SyncState::UploadError, Some("Quota exceeded"))
        );
        // Not a cloud item (iCloud reports an error for its .DS_Store): local.
        let local = UbiquityInfo {
            ubiquitous: false,
            upload_error: Some("x".into()),
            ..base
        };
        assert_eq!(
            combine_info(".DS_Store", false, Some(local)).state,
            SyncState::Local
        );
        assert!(
            !SyncState::UploadError.can_download() && SyncState::UploadError.phosphor().is_some()
        );
    }

    #[test]
    fn local_files_are_local_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert_eq!(item_status(&dir.path().join("a.txt")), SyncState::Local);
        assert!(!is_dataless(&dir.path().join("a.txt")));
        let cache = SyncStatusCache::new(Duration::from_secs(60));
        assert!(cache.peek(dir.path()).is_none());
        let s = cache.folder(dir.path()).unwrap();
        assert_eq!(s.get("a.txt"), Some(&SyncState::Local));
        assert_eq!(s.len(), 2);
        assert!(
            Arc::ptr_eq(&s, &cache.folder(dir.path()).unwrap()),
            "cached"
        );
        // A change of the folder (new entry -> new mtime) refreshes.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.path().join("b.txt"), b"b").unwrap();
        let s2 = cache.folder(dir.path()).unwrap();
        assert_eq!(s2.len(), 3);
        cache.invalidate(dir.path());
        assert!(cache.peek(dir.path()).is_none());
    }

    /// Read-only look at the real cloud folders of this Mac:
    /// `cargo test -p manta-vfs cloud_local -- --ignored --nocapture`.
    /// Never downloads or evicts anything.
    #[test]
    #[ignore = "reads the real ~/Library/CloudStorage (manual)"]
    fn real_cloud_folders_read_only() {
        for loc in discover_cloud_locations() {
            println!(
                "{:<45} {:<13} {}",
                loc.display_name,
                loc.provider_id,
                loc.path.display()
            );
            match folder_info(&loc.path) {
                Ok(items) => {
                    for (name, st) in items.iter().take(8) {
                        println!("    {st:?}\t{name}");
                    }
                }
                Err(e) => println!("    (cannot list: {e})"),
            }
            let my_drive = loc.path.join("My Drive");
            if let Ok(items) = folder_status(&my_drive) {
                println!("  My Drive:");
                for (name, st) in items.iter().take(8) {
                    println!("    {st:?}\t{name}");
                }
            }
        }
    }
}
