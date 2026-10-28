//! Google Drive backend (Drive API v3 over HTTPS, OAuth 2.0 with PKCE).
//!
//! Path model (names follow [`set_language`]; both languages always resolve):
//! - `/` lists three virtual folders: `/My Drive` („Meine Ablage“, the
//!   user's root folder), `/Shared with me` („Für mich freigegeben“, read-
//!   mostly: nothing can be created at its top level) and `/Shared drives`
//!   („Geteilte Ablagen“, one folder per shared drive).
//! - Below that, paths are mapped to file ids through a per-folder listing
//!   cache. Drive's duplicate names get stable ` (2)` suffixes, see
//!   [`tree`](self::tree) for the rules.
//! - Google Docs/Sheets/Slides/Drawings appear as `.docx`/`.xlsx`/`.pptx`/
//!   `.pdf` files that are exported on read and can't be written (they can
//!   be renamed, moved and trashed).
//! - Deleting moves items to Drive's trash (`trashed=true`); nothing is
//!   deleted permanently.
//!
//! Sign-in: [`sign_in`] (browser, PKCE, loopback) stores the refresh token
//! in the Keychain and returns the [`ConnectionConfig`] to connect with;
//! [`crate::connect`] then opens [`GDriveFs`]. See docs/cloud.md.

pub mod api;
pub mod oauth;
pub mod store;
pub mod tree;
mod writer;

#[cfg(test)]
mod fake_server;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use manta_core::{
    ConnectionConfig, Entry, EntryKind, Protocol, ResumableUpload, ResumeCaps, UploadCheckpoint,
    UploadSession, Vfs, VfsError, VfsPath, VfsResult,
};
use serde_json::json;

use api::{Api, Call, DriveList, FileList, FileMeta, FILE_FIELDS};
pub use oauth::{
    authorize, authorize_with, is_configured, AuthCancel, ClientSource, Endpoints, OAuthClient,
    Tokens, AUTHORIZE_TIMEOUT, DRIVE_SCOPE, NOT_CONFIGURED,
};
pub use store::{KeychainStore, MemoryStore, TokenStore, KEYCHAIN_SERVICE};
pub use tree::{ExportFormat, EXPORT_FORMATS};
use tree::{Node, NodeKind};

/// Upload chunk size (a multiple of 256 KiB, as Drive requires).
pub const UPLOAD_CHUNK: usize = 8 * 1024 * 1024;

/// How long a cached folder listing is used to resolve paths.
const LISTING_TTL: Duration = Duration::from_secs(60);

static GERMAN: AtomicBool = AtomicBool::new(false);

/// Language of the virtual top-level folder names (`"de"` or anything else
/// for English). The UI calls this with its language at start-up; paths in
/// either language resolve regardless.
pub fn set_language(lang: &str) {
    GERMAN.store(
        lang.to_ascii_lowercase().starts_with("de"),
        Ordering::Relaxed,
    );
}

/// The three virtual folders at `/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    MyDrive,
    SharedWithMe,
    SharedDrives,
}

impl Area {
    pub const ALL: [Area; 3] = [Area::MyDrive, Area::SharedWithMe, Area::SharedDrives];

    /// (English, German)
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Area::MyDrive => ("My Drive", "Meine Ablage"),
            Area::SharedWithMe => ("Shared with me", "Für mich freigegeben"),
            Area::SharedDrives => ("Shared drives", "Geteilte Ablagen"),
        }
    }

    /// The name in the current language ([`set_language`]).
    pub fn name(self) -> &'static str {
        let (en, de) = self.names();
        if GERMAN.load(Ordering::Relaxed) {
            de
        } else {
            en
        }
    }

    pub fn from_name(name: &str) -> Option<Area> {
        Area::ALL.into_iter().find(|a| {
            let (en, de) = a.names();
            name.eq_ignore_ascii_case(en) || name == de
        })
    }

    /// `/My Drive` etc. in the current language.
    pub fn path(self) -> VfsPath {
        VfsPath::root().join(self.name())
    }
}

/// Sign in with the browser and remember the account: runs [`authorize`],
/// stores the refresh token in the Keychain and returns the connection to
/// save as a favorite / pass to [`crate::connect`]. Blocking; call it on a
/// background thread. `cancel` aborts the wait for the browser.
pub fn sign_in(open_url: impl Fn(&str), cancel: &AuthCancel) -> VfsResult<ConnectionConfig> {
    let client =
        OAuthClient::configured().ok_or_else(|| VfsError::Unsupported(NOT_CONFIGURED.into()))?;
    let tokens = authorize_with(&client, open_url, cancel, AUTHORIZE_TIMEOUT)?;
    KeychainStore.save(&tokens.email, &tokens.refresh_token)?;
    Ok(ConnectionConfig::google_drive(&tokens.email, None))
}

/// Is a sign-in for `email` stored in the Keychain?
pub fn has_sign_in(email: &str) -> bool {
    matches!(KeychainStore.load(email), Ok(Some(_)))
}

/// Forget the account: revoke the token at Google (best effort) and remove
/// it from the Keychain.
pub fn sign_out(email: &str) -> VfsResult<()> {
    if let (Some(client), Ok(Some(token))) = (OAuthClient::configured(), KeychainStore.load(email))
    {
        if let Err(e) = oauth::revoke(&client, &token) {
            log::info!("Google Drive sign-out: revoking failed: {e}");
        }
    }
    KeychainStore.delete(email)
}

/// Open a connection from a [`ConnectionConfig`] (`username` = email).
pub fn connect(cfg: &ConnectionConfig) -> VfsResult<GDriveFs> {
    let client =
        OAuthClient::configured().ok_or_else(|| VfsError::Unsupported(NOT_CONFIGURED.into()))?;
    GDriveFs::open(&cfg.username, client, Arc::new(KeychainStore))
}

struct Listing {
    fetched: Instant,
    children: Arc<Vec<(String, Node)>>,
}

pub(crate) struct Inner {
    pub api: Api,
    listings: Mutex<HashMap<VfsPath, Listing>>,
    pub chunk_size: usize,
}

impl Inner {
    /// Forget cached listings of `dir` and everything below it.
    pub(crate) fn forget(&self, dir: &VfsPath) {
        self.listings
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(dir));
    }
}

/// Google Drive as a [`Vfs`]. Cheap to share; all methods block on HTTP.
pub struct GDriveFs {
    inner: Arc<Inner>,
}

/// Which collection a files.list query searches.
#[derive(Clone, Copy)]
enum Corpus<'a> {
    User,
    AllDrives,
    Drive(&'a str),
}

/// Where a path points.
enum Resolved {
    Root,
    Area(Area),
    Item(Node),
}

fn dir_entry(path: VfsPath, name: String, modified: Option<DateTime<Utc>>) -> Entry {
    Entry {
        name,
        path,
        kind: EntryKind::Directory,
        size: 0,
        modified,
        permissions: None,
        link_target: None,
        hidden: false,
    }
}

fn node_entry(path: VfsPath, name: String, node: &Node) -> Entry {
    let (kind, permissions) = match node.kind {
        NodeKind::Folder => (EntryKind::Directory, None),
        NodeKind::File => (EntryKind::File, None),
        NodeKind::Native(_) => (EntryKind::File, Some(0o444)),
        NodeKind::Unsupported => (EntryKind::Other, Some(0o444)),
    };
    Entry {
        name,
        path,
        kind,
        size: node.size,
        modified: node.modified,
        permissions,
        link_target: None,
        hidden: false,
    }
}

fn q_escape(id: &str) -> String {
    id.replace('\\', "\\\\").replace('\'', "\\'")
}

impl GDriveFs {
    /// Connect as `email` with the refresh token from `store`; verifies the
    /// sign-in by refreshing the access token.
    pub fn open(
        email: &str,
        client: OAuthClient,
        store: Arc<dyn TokenStore>,
    ) -> VfsResult<GDriveFs> {
        let email = email.trim();
        if email.is_empty() {
            return Err(VfsError::Auth(
                "no Google account given; sign in to Google Drive first".into(),
            ));
        }
        let refresh = store.load(email)?.ok_or_else(|| {
            VfsError::Auth(format!(
                "not signed in to Google Drive as {email}; please sign in again"
            ))
        })?;
        let fs = GDriveFs::with_api(Api::new(client, email.to_string(), refresh, None, store));
        fs.inner.api.ensure_token()?;
        Ok(fs)
    }

    /// Use fresh [`Tokens`] from [`authorize`] (no extra token refresh).
    pub fn from_tokens(
        tokens: &Tokens,
        client: OAuthClient,
        store: Arc<dyn TokenStore>,
    ) -> GDriveFs {
        GDriveFs::with_api(Api::new(
            client,
            tokens.email.clone(),
            tokens.refresh_token.clone(),
            Some((tokens.access_token.clone(), tokens.expires_at)),
            store,
        ))
    }

    fn with_api(api: Api) -> GDriveFs {
        GDriveFs {
            inner: Arc::new(Inner {
                api,
                listings: Mutex::new(HashMap::new()),
                chunk_size: UPLOAD_CHUNK,
            }),
        }
    }

    /// The signed-in account.
    pub fn email(&self) -> &str {
        &self.inner.api.email
    }

    #[cfg(test)]
    pub(crate) fn tune_for_tests(&mut self, chunk: usize) {
        let inner = Arc::get_mut(&mut self.inner).expect("unshared");
        inner.chunk_size = chunk;
        inner.api.retry = api::RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(2),
            max_delay: Duration::from_millis(20),
        };
    }

    fn files_url(&self) -> String {
        format!("{}/drive/v3/files", self.inner.api.base())
    }

    /// Fetch every page of a files.list query.
    fn list_query(&self, q: String, corpus: Corpus<'_>, ctx: &str) -> VfsResult<Vec<Node>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut call = Call::new("GET", self.files_url())
                .query("q", q.clone())
                .query("pageSize", "1000")
                .query("fields", format!("nextPageToken,files({FILE_FIELDS})"))
                .query("supportsAllDrives", "true")
                .query("includeItemsFromAllDrives", "true");
            call = match corpus {
                Corpus::Drive(d) => call.query("corpora", "drive").query("driveId", d),
                Corpus::User => call.query("corpora", "user"),
                Corpus::AllDrives => call.query("corpora", "allDrives"),
            };
            if let Some(p) = &page {
                call = call.query("pageToken", p.clone());
            }
            let list: FileList = self.inner.api.send_json(&call, ctx)?;
            out.extend(list.files.iter().map(Node::from_meta));
            match list.next_page_token.filter(|t| !t.is_empty()) {
                Some(t) => page = Some(t),
                None => return Ok(out),
            }
        }
    }

    fn list_drives(&self, ctx: &str) -> VfsResult<Vec<Node>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut call = Call::new("GET", format!("{}/drive/v3/drives", self.inner.api.base()))
                .query("pageSize", "100")
                .query("fields", "nextPageToken,drives(id,name)");
            if let Some(p) = &page {
                call = call.query("pageToken", p.clone());
            }
            let list: DriveList = self.inner.api.send_json(&call, ctx)?;
            out.extend(
                list.drives
                    .iter()
                    .map(|d| Node::folder(&d.id, &d.name, Some(d.id.clone()))),
            );
            match list.next_page_token.filter(|t| !t.is_empty()) {
                Some(t) => page = Some(t),
                None => return Ok(out),
            }
        }
    }

    /// Fetch the children of a directory from Drive and cache them.
    fn fetch_children(&self, dir: &VfsPath, at: &Resolved) -> VfsResult<Arc<Vec<(String, Node)>>> {
        let ctx = dir.to_string();
        let nodes = match at {
            Resolved::Root => return Err(VfsError::Other("the root has no Drive listing".into())),
            Resolved::Area(Area::MyDrive) => self.list_query(
                "'root' in parents and trashed = false".into(),
                Corpus::User,
                &ctx,
            )?,
            Resolved::Area(Area::SharedWithMe) => self.list_query(
                "sharedWithMe = true and trashed = false".into(),
                Corpus::User,
                &ctx,
            )?,
            Resolved::Area(Area::SharedDrives) => self.list_drives(&ctx)?,
            Resolved::Item(n) if n.is_folder() => self.list_query(
                format!("'{}' in parents and trashed = false", q_escape(&n.id)),
                n.drive_id
                    .as_deref()
                    .map_or(Corpus::AllDrives, Corpus::Drive),
                &ctx,
            )?,
            Resolved::Item(_) => return Err(VfsError::NotADirectory(ctx)),
        };
        let children = Arc::new(tree::disambiguate(nodes));
        self.inner.listings.lock().unwrap().insert(
            dir.clone(),
            Listing {
                fetched: Instant::now(),
                children: children.clone(),
            },
        );
        Ok(children)
    }

    fn cached_children(&self, dir: &VfsPath) -> Option<Arc<Vec<(String, Node)>>> {
        let map = self.inner.listings.lock().unwrap();
        map.get(dir)
            .filter(|l| l.fetched.elapsed() < LISTING_TTL)
            .map(|l| l.children.clone())
    }

    fn invalidate(&self, dir: &VfsPath) {
        self.inner.forget(dir);
    }

    fn resolve(&self, path: &VfsPath) -> VfsResult<Resolved> {
        let comps = path.components();
        let Some(first) = comps.first() else {
            return Ok(Resolved::Root);
        };
        let area = Area::from_name(first).ok_or_else(|| VfsError::NotFound(path.to_string()))?;
        let mut cur_path = VfsPath::root().join(first);
        let mut cur = Resolved::Area(area);
        for (i, name) in comps.iter().enumerate().skip(1) {
            let find = |children: &Arc<Vec<(String, Node)>>| {
                children
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, node)| node.clone())
            };
            let node = match self.cached_children(&cur_path).as_ref().and_then(find) {
                Some(n) => n,
                None => {
                    let fresh = self.fetch_children(&cur_path, &cur)?;
                    find(&fresh).ok_or_else(|| VfsError::NotFound(path.to_string()))?
                }
            };
            if i + 1 < comps.len() && !node.is_folder() {
                return Err(VfsError::NotADirectory(cur_path.join(name).to_string()));
            }
            cur_path = cur_path.join(name);
            cur = Resolved::Item(node);
        }
        Ok(cur)
    }

    /// A folder to create things in: (parent id, drive id).
    fn writable_parent(&self, path: &VfsPath) -> VfsResult<(String, Option<String>)> {
        let parent = path
            .parent()
            .ok_or_else(|| VfsError::PermissionDenied("/".into()))?;
        match self.resolve(&parent)? {
            Resolved::Root => Err(VfsError::PermissionDenied(format!(
                "{path}: only {}, {} and {} live here",
                Area::MyDrive.name(),
                Area::SharedWithMe.name(),
                Area::SharedDrives.name()
            ))),
            Resolved::Area(Area::MyDrive) => Ok(("root".into(), None)),
            Resolved::Area(Area::SharedWithMe) => Err(VfsError::PermissionDenied(format!(
                "{path}: nothing can be created directly in “{}”; open a shared folder first",
                Area::SharedWithMe.name()
            ))),
            Resolved::Area(Area::SharedDrives) => Err(VfsError::Unsupported(format!(
                "{path}: creating shared drives isn't supported; create it at drive.google.com"
            ))),
            Resolved::Item(n) if n.is_folder() => Ok((n.id, n.drive_id)),
            Resolved::Item(_) => Err(VfsError::NotADirectory(parent.to_string())),
        }
    }

    fn item(&self, path: &VfsPath) -> VfsResult<Node> {
        match self.resolve(path)? {
            Resolved::Item(n) => Ok(n),
            _ => Err(VfsError::PermissionDenied(format!(
                "{path}: this is a fixed Google Drive folder"
            ))),
        }
    }

    fn patch(&self, id: &str, body: serde_json::Value, ctx: &str) -> VfsResult<()> {
        let call = Call::new("PATCH", format!("{}/{id}", self.files_url()))
            .query("supportsAllDrives", "true")
            .query("fields", "id")
            .json(&body);
        self.inner.api.send(&call, ctx).map(|_| ())
    }

    fn trash(&self, path: &VfsPath, node: &Node) -> VfsResult<()> {
        let ctx = path.to_string();
        if node.drive_id.as_deref() == Some(node.id.as_str()) {
            return Err(VfsError::Unsupported(format!(
                "{ctx}: shared drives can't be deleted from Manta"
            )));
        }
        self.patch(&node.id, json!({ "trashed": true }), &ctx)?;
        if let Some(p) = path.parent() {
            self.invalidate(&p);
        }
        self.invalidate(path);
        Ok(())
    }

    fn get_meta(&self, id: &str, fields: &str, ctx: &str) -> VfsResult<FileMeta> {
        let call = Call::new("GET", format!("{}/{id}", self.files_url()))
            .query("supportsAllDrives", "true")
            .query("fields", fields);
        self.inner.api.send_json(&call, ctx)
    }

    /// Create a new file or update the existing one at `path`.
    fn write_target(&self, path: &VfsPath) -> VfsResult<writer::Target> {
        let ctx = path.to_string();
        let target = match self.resolve(path) {
            Ok(Resolved::Item(n)) => match n.kind {
                NodeKind::File => writer::Target::Update { id: n.id },
                NodeKind::Folder => return Err(VfsError::Other(format!("{ctx}: is a folder"))),
                NodeKind::Native(_) | NodeKind::Unsupported => {
                    return Err(VfsError::PermissionDenied(format!(
                        "{ctx}: Google Docs, Sheets and Slides are read-only in Manta; edit them at drive.google.com"
                    )))
                }
            },
            Ok(_) => return Err(VfsError::PermissionDenied(format!("{ctx}: this is a fixed Google Drive folder"))),
            Err(VfsError::NotFound(_)) => {
                let (parent_id, _) = self.writable_parent(path)?;
                writer::Target::Create { parent_id, name: tree::drive_component(path.file_name()) }
            }
            Err(e) => return Err(e),
        };
        // The listing changes once the upload completes; forget it now.
        self.invalidate(&path.parent().unwrap_or_else(VfsPath::root));
        Ok(target)
    }
}

impl Vfs for GDriveFs {
    fn label(&self) -> String {
        format!("{} (Google Drive)", self.inner.api.email)
    }

    fn protocol(&self) -> Protocol {
        Protocol::GoogleDrive
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(Area::MyDrive.path())
    }

    /// The account's storage (`about.get storageQuota`). Shared drives
    /// belong to the organisation: unknown there, like an unlimited account.
    fn space(&self, path: &VfsPath) -> VfsResult<Option<manta_core::Space>> {
        if path.starts_with(&Area::SharedDrives.path()) {
            return Ok(None);
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Quota {
            limit: Option<String>,
            usage: Option<String>,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct About {
            storage_quota: Option<Quota>,
        }
        let call = Call::new("GET", format!("{}/drive/v3/about", self.inner.api.base()))
            .query("fields", "storageQuota(limit,usage)");
        let about: About = self.inner.api.send_json(&call, "about")?;
        let Some(q) = about.storage_quota else {
            return Ok(None);
        };
        let num = |v: Option<String>| v.and_then(|v| v.parse::<u64>().ok());
        Ok(match (num(q.limit), num(q.usage)) {
            (Some(limit), Some(usage)) => Some(manta_core::Space {
                available: limit.saturating_sub(usage),
                total: Some(limit),
            }),
            _ => None,
        })
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let at = self.resolve(dir)?;
        if let Resolved::Root = at {
            return Ok(Area::ALL
                .iter()
                .map(|a| dir_entry(a.path(), a.name().to_string(), None))
                .collect());
        }
        if let Resolved::Item(n) = &at {
            if !n.is_folder() {
                return Err(VfsError::NotADirectory(dir.to_string()));
            }
        }
        let children = self.fetch_children(dir, &at)?;
        Ok(children
            .iter()
            .map(|(name, node)| node_entry(dir.join(name), name.clone(), node))
            .collect())
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        Ok(match self.resolve(path)? {
            Resolved::Root => dir_entry(VfsPath::root(), String::new(), None),
            Resolved::Area(a) => dir_entry(path.clone(), a.name().to_string(), None),
            Resolved::Item(n) => node_entry(path.clone(), path.file_name().to_string(), &n),
        })
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let ctx = path.to_string();
        if self.resolve(path).is_ok() {
            return Err(VfsError::AlreadyExists(ctx));
        }
        let (parent_id, _) = self.writable_parent(path)?;
        let body = json!({
            "name": tree::drive_component(path.file_name()),
            "mimeType": tree::FOLDER_MIME,
            "parents": [parent_id],
        });
        let call = Call::new("POST", self.files_url())
            .query("supportsAllDrives", "true")
            .query("fields", "id")
            .json(&body);
        self.inner.api.send(&call, &ctx)?;
        if let Some(p) = path.parent() {
            self.invalidate(&p);
        }
        Ok(())
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        let node = self.item(path)?;
        if node.is_folder() {
            return Err(VfsError::Other(format!("{path}: is a folder")));
        }
        self.trash(path, &node)
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let node = self.item(path)?;
        if !node.is_folder() {
            return Err(VfsError::NotADirectory(path.to_string()));
        }
        if !self
            .fetch_children(path, &Resolved::Item(node.clone()))?
            .is_empty()
        {
            return Err(VfsError::DirectoryNotEmpty(path.to_string()));
        }
        self.trash(path, &node)
    }

    /// Moves the folder to the trash in one call (Drive trashes the content
    /// with it).
    fn remove_tree(&self, path: &VfsPath) -> VfsResult<()> {
        let node = self.item(path)?;
        self.trash(path, &node)
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        let ctx = format!("{from} → {to}");
        let node = self.item(from)?;
        if from == to {
            return Ok(());
        }
        if to.starts_with(from) {
            return Err(VfsError::Other(format!(
                "{ctx}: can't move a folder into itself"
            )));
        }
        // Native semantics like the local backend: an existing target file
        // is replaced (moved to the trash); folders are never replaced.
        if let Ok(Resolved::Item(existing)) = self.resolve(to) {
            if existing.id == node.id {
                // Same item under another name ("a (2)" -> "a"): nothing to do.
                return Ok(());
            }
            if existing.is_folder() || node.is_folder() {
                return Err(VfsError::AlreadyExists(to.to_string()));
            }
            self.trash(to, &existing)?;
        }
        let (new_parent, _) = self.writable_parent(to)?;
        let mut call = Call::new("PATCH", format!("{}/{}", self.files_url(), node.id))
            .query("supportsAllDrives", "true")
            .query("fields", "id")
            .json(&json!({ "name": tree::drive_name_for(&node.kind, to.file_name()) }));
        if from.parent() != to.parent() {
            let meta = self.get_meta(&node.id, "parents", &ctx)?;
            let old = meta.parents.unwrap_or_default().join(",");
            call = call.query("addParents", new_parent);
            if !old.is_empty() {
                call = call.query("removeParents", old);
            }
        }
        self.inner.api.send(&call, &ctx)?;
        for p in [from.parent(), to.parent()].into_iter().flatten() {
            self.invalidate(&p);
        }
        self.invalidate(from);
        Ok(())
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let ctx = path.to_string();
        let node = self.item(path)?;
        let base = format!("{}/{}", self.files_url(), node.id);
        let call = match node.kind {
            NodeKind::Folder => return Err(VfsError::Other(format!("{ctx}: is a folder"))),
            NodeKind::Unsupported => {
                return Err(VfsError::Unsupported(format!(
                    "{ctx}: this kind of Google file (Form, Site, Map, …) can only be opened at drive.google.com"
                )))
            }
            NodeKind::Native(f) => Call::new("GET", format!("{base}/export")).query("mimeType", f.export_mime),
            NodeKind::File => Call::new("GET", base).query("alt", "media").query("supportsAllDrives", "true"),
        };
        let resp = self.inner.api.send(&call, &ctx)?;
        Ok(Box::new(BodyReader(resp.into_body().into_reader())))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn std::io::Write + Send>> {
        let target = self.write_target(path)?;
        Ok(Box::new(writer::DriveWriter::new(
            self.inner.clone(),
            target,
            path.to_string(),
        )))
    }

    /// Ranged download for binary files; Google Docs exports can't seek and
    /// skip the head instead.
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        let node = if offset == 0 {
            None
        } else {
            Some(self.item(path)?)
        };
        let Some(node) = node.filter(|n| n.kind == NodeKind::File) else {
            let mut r = self.open_read(path)?;
            if offset > 0 {
                let skipped = std::io::copy(&mut (&mut r).take(offset), &mut std::io::sink())?;
                if skipped < offset {
                    return Err(VfsError::Other(format!(
                        "{path}: shorter than the resume offset {offset}"
                    )));
                }
            }
            return Ok(r);
        };
        let call = Call::new("GET", format!("{}/{}", self.files_url(), node.id))
            .query("alt", "media")
            .query("supportsAllDrives", "true")
            .header("range", format!("bytes={offset}-"));
        let resp = self.inner.api.send(&call, path.as_str())?;
        let partial = resp.status().as_u16() == 206;
        let mut r = BodyReader(resp.into_body().into_reader());
        if !partial {
            let skipped = std::io::copy(&mut (&mut r).take(offset), &mut std::io::sink())?;
            if skipped < offset {
                return Err(VfsError::Other(format!(
                    "{path}: shorter than the resume offset {offset}"
                )));
            }
        }
        Ok(Box::new(r))
    }

    fn supports_resume(&self) -> ResumeCaps {
        ResumeCaps {
            read_at: true,
            append: false,
            upload_session: true,
        }
    }

    /// `md5Checksum` (binary files) or the `version` counter.
    fn content_tag(&self, path: &VfsPath) -> Option<String> {
        let node = self.item(path).ok()?;
        let call = Call::new("GET", format!("{}/{}", self.files_url(), node.id))
            .query("supportsAllDrives", "true")
            .query("fields", "md5Checksum,version");
        let v: serde_json::Value = self.inner.api.send_json(&call, path.as_str()).ok()?;
        v["md5Checksum"]
            .as_str()
            .map(|m| format!("md5:{m}"))
            .or_else(|| v["version"].as_str().map(|n| format!("v:{n}")))
    }

    /// Drive's resumable upload; a resume asks for the uploaded range
    /// (`Content-Range: bytes */size`) and continues there. An expired
    /// session (404) starts over.
    fn open_upload(
        &self,
        path: &VfsPath,
        size: u64,
        resume: Option<&UploadSession>,
        checkpoint: UploadCheckpoint,
    ) -> VfsResult<ResumableUpload> {
        let ctx = path.to_string();
        let target = self.write_target(path)?;
        let mut w =
            writer::DriveWriter::resumable(self.inner.clone(), target, ctx.clone(), checkpoint);
        let mut offset = 0;
        if let Some(session) = resume.and_then(writer::DriveSession::decode) {
            match writer::DriveWriter::query(&self.inner, &session.uri, size, &ctx) {
                Ok(writer::SessionState::Partial(n)) => {
                    log::info!("{ctx}: continuing the Drive upload at byte {n}");
                    w.continue_at(session.uri, n);
                    offset = n;
                }
                Ok(writer::SessionState::Complete) => {
                    w.completed();
                    offset = size;
                }
                Err(VfsError::NotFound(_)) => {
                    log::info!("{ctx}: the Drive upload session expired; starting over")
                }
                Err(e) => return Err(e),
            }
        }
        Ok(ResumableUpload {
            writer: Box::new(w),
            offset,
        })
    }

    fn abort_upload(&self, path: &VfsPath, session: &UploadSession) -> VfsResult<()> {
        if let Some(s) = writer::DriveSession::decode(session) {
            writer::abort_session(&self.inner, &s.uri, path.as_str());
        }
        Ok(())
    }

    fn set_modified(&self, path: &VfsPath, mtime: DateTime<Utc>) -> VfsResult<()> {
        let node = self.item(path)?;
        let t = mtime.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        self.patch(&node.id, json!({ "modifiedTime": t }), path.as_str())?;
        if let Some(p) = path.parent() {
            self.invalidate(&p);
        }
        Ok(())
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Response body reader (download / export).
struct BodyReader(ureq::BodyReader<'static>);

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}
