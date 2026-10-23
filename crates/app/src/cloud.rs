//! Cloud folders that macOS already syncs: iCloud Drive and the File
//! Provider folders of the providers' own apps under `~/Library/CloudStorage`
//! (Google Drive for desktop, OneDrive, Dropbox, Box, ...).
//!
//! Manta never signs in to a cloud provider itself (it is no OAuth client of
//! Google, Microsoft or Dropbox). These folders are ordinary local folders;
//! this module adds
//! - the sidebar's "Cloud" section (one row per provider / account) and the
//!   dismissible "Connect Google Drive …" hint when no Google Drive folder
//!   exists,
//! - sync badges for items that are only in the cloud (read from the
//!   `SF_DATALESS` stat flag and `NSURLUbiquitousItem…` resource values; no
//!   network, nothing is downloaded), refreshed in the background,
//! - the upload state after a copy into a cloud folder: the provider app
//!   uploads in its own time, so "Uploading" (pulsing arrow-up badge, also
//!   while the item waits for the provider) and "Upload error" (red, the
//!   provider's message in the tooltip) come from
//!   `NSURLUbiquitousItemIsUploading/IsUploaded/UploadingErrorKey`, which
//!   iCloud Drive and File Provider domains (Google Drive, OneDrive, ...)
//!   report; iCloud also reports a percentage (tooltip),
//! - "Download Now" / "Remove Download" in the context menu.
//!
//! How the two actions work (see `manta_vfs::cloud_local`):
//! - **Download Now**: `NSFileManager startDownloadingUbiquitousItemAtURL`,
//!   which iCloud Drive and File Provider domains both accept (it returns at
//!   once; the badge turns into "downloading" and disappears when done).
//!   Where a provider refuses, Manta reads the first byte of the dataless
//!   placeholder, which makes macOS materialise the whole file (that's what
//!   any app opening the file does). `brctl download` is iCloud-only and an
//!   undocumented tool, so it isn't used. Folders: everything inside.
//! - **Remove Download**: `NSFileManager evictUbiquitousItemAtURL`. iCloud
//!   Drive supports it; for other File Provider domains the provider
//!   decides. `fileproviderctl` has no evict command on current macOS, so
//!   there is no second route: if the provider refuses, the error says to
//!   use Finder's "Remove Download". Nothing is ever deleted from the cloud.
//!
//! The file system calls go through [`CloudProbe`], so tests use a fixture
//! with simulated stat flags ([`FakeProbe`]) and never touch real cloud files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use manta_vfs::cloud_local::{self, CloudLocation, CloudProvider, SyncInfo, SyncState};

use crate::app::Action;
use crate::icons;
use crate::sidebar::{row, Row};
use crate::theme::{self, pal, space, text};
use crate::widgets;

/// Where to get Google Drive for desktop.
pub const GOOGLE_DRIVE_DOWNLOAD: &str = "https://www.google.com/drive/download/";

/// How long a folder's sync states are shown before they're read again
/// (downloads and uploads change them without a file system event).
pub const STATES_TTL: Duration = Duration::from_secs(3);

/// The sync states (with upload details) of a folder's items, by name.
pub type FolderSync = HashMap<String, SyncInfo>;

/// The file system side: sync states, downloads, evictions. Blocking; the
/// app calls it on background threads.
pub trait CloudProbe: Send + Sync {
    /// The state of every item in `dir`, by name.
    fn folder(&self, dir: &Path) -> FolderSync;
    /// "Download Now" for one item (folders: everything inside).
    fn download(&self, path: &Path) -> Result<(), String>;
    /// "Remove Download" for one item.
    fn evict(&self, path: &Path) -> Result<(), String>;
}

/// The real thing (`manta_vfs::cloud_local`).
#[cfg_attr(test, allow(dead_code))]
pub struct SystemProbe;

impl CloudProbe for SystemProbe {
    fn folder(&self, dir: &Path) -> FolderSync {
        cloud_local::folder_info(dir)
            .map(|v| v.into_iter().collect())
            .unwrap_or_default()
    }
    fn download(&self, path: &Path) -> Result<(), String> {
        cloud_local::start_download(path).map_err(|e| e.to_string())
    }
    fn evict(&self, path: &Path) -> Result<(), String> {
        cloud_local::evict(path).map_err(|e| e.to_string())
    }
}

/// Test probe: real files in a temp fixture, simulated `st_flags`
/// (`SF_DATALESS` = cloud-only) and simulated upload states. "Downloading"
/// clears the flag, "removing the download" sets it; every call is
/// recorded.
#[cfg(test)]
#[derive(Default)]
pub struct FakeProbe {
    pub flags: std::sync::Mutex<HashMap<PathBuf, u32>>,
    /// Upload states (uploading / waiting / error) that win over the flags.
    pub uploads: std::sync::Mutex<HashMap<PathBuf, SyncInfo>>,
    pub calls: std::sync::Mutex<Vec<(&'static str, PathBuf)>>,
}

#[cfg(test)]
impl FakeProbe {
    pub fn set_dataless(&self, path: &Path, on: bool) {
        let flags = if on { cloud_local::SF_DATALESS } else { 0 };
        self.flags.lock().unwrap().insert(path.to_path_buf(), flags);
    }

    /// Simulate the provider's upload state of `path` (`None`: uploaded).
    pub fn set_upload(&self, path: &Path, info: Option<SyncInfo>) {
        let mut u = self.uploads.lock().unwrap();
        match info {
            Some(i) => u.insert(path.to_path_buf(), i),
            None => u.remove(path),
        };
    }
}

#[cfg(test)]
impl CloudProbe for FakeProbe {
    fn folder(&self, dir: &Path) -> FolderSync {
        let flags = self.flags.lock().unwrap();
        let uploads = self.uploads.lock().unwrap();
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(u) = uploads.get(&e.path()) {
                    return (name, u.clone());
                }
                let f = flags.get(&e.path()).copied().unwrap_or(0);
                // A folder of a provider that isn't a placeholder: synced.
                let st = match cloud_local::state_from_flags(f) {
                    SyncState::Local => SyncState::Current,
                    s => s,
                };
                (name, SyncInfo::new(st))
            })
            .collect()
    }
    fn download(&self, path: &Path) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(("download", path.to_path_buf()));
        self.set_dataless(path, false);
        Ok(())
    }
    fn evict(&self, path: &Path) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(("evict", path.to_path_buf()));
        self.set_dataless(path, true);
        Ok(())
    }
}

/// "Download Now" or "Remove Download".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudOp {
    Download,
    Evict,
}

/// A finished [`CloudOp`] (for a toast).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpDone {
    pub op: CloudOp,
    /// Items asked for.
    pub count: usize,
    /// "name: error" of the items that failed.
    pub errors: Vec<String>,
}

enum Msg {
    States(PathBuf, FolderSync),
    Done(OpDone, Vec<PathBuf>),
}

struct FolderStates {
    states: Arc<FolderSync>,
    at: Instant,
}

/// Discovered cloud folders and the sync states of the visible ones.
pub struct Cloud {
    pub locations: Vec<CloudLocation>,
    discover: Box<dyn Fn() -> Vec<CloudLocation>>,
    probe: Arc<dyn CloudProbe>,
    folders: HashMap<PathBuf, FolderStates>,
    inflight: HashSet<PathBuf>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Run probe calls right away on the UI thread (tests: deterministic).
    inline: bool,
}

impl Cloud {
    /// The user's cloud folders and the real file system.
    #[cfg_attr(test, allow(dead_code))]
    pub fn system() -> Cloud {
        Cloud::new(
            Box::new(cloud_local::discover_cloud_locations),
            Arc::new(SystemProbe),
            false,
        )
    }

    pub fn new(
        discover: Box<dyn Fn() -> Vec<CloudLocation>>,
        probe: Arc<dyn CloudProbe>,
        inline: bool,
    ) -> Cloud {
        let (tx, rx) = channel();
        let locations = discover();
        Cloud {
            locations,
            discover,
            probe,
            folders: HashMap::new(),
            inflight: HashSet::new(),
            tx,
            rx,
            inline,
        }
    }

    /// Fixed locations and the given probe, run inline (tests).
    #[cfg(test)]
    pub fn fixed(locations: Vec<CloudLocation>, probe: Arc<dyn CloudProbe>) -> Cloud {
        Cloud::new(Box::new(move || locations.clone()), probe, true)
    }

    /// Look for cloud folders again (sidebar refresh, `~/Library/CloudStorage`
    /// changed). Returns whether anything changed.
    pub fn rediscover(&mut self) -> bool {
        let found = (self.discover)();
        if found == self.locations {
            return false;
        }
        log::info!("cloud: {} location(s)", found.len());
        self.locations = found;
        self.folders
            .retain(|d, _| cloud_local::location_of(d, &self.locations).is_some());
        true
    }

    /// A Google Drive for desktop folder exists.
    pub fn has_google_drive(&self) -> bool {
        self.locations
            .iter()
            .any(|l| l.provider == CloudProvider::GoogleDrive)
    }

    /// The cloud folder `path` is in.
    pub fn location_of(&self, path: &Path) -> Option<&CloudLocation> {
        cloud_local::location_of(path, &self.locations)
    }

    /// The last known states of `dir`'s items (`None`: not a cloud folder,
    /// or not read yet). Never blocks.
    pub fn states(&self, dir: &Path) -> Option<Arc<FolderSync>> {
        self.location_of(dir)?;
        self.folders.get(dir).map(|f| f.states.clone())
    }

    /// Once per frame: take finished background work, and (re)read the
    /// states of the visible cloud folders that are missing or older than
    /// [`STATES_TTL`]. Returns the actions that finished.
    pub fn frame(&mut self, ctx: &egui::Context, visible: &[PathBuf]) -> Vec<OpDone> {
        let mut done = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::States(dir, states) => {
                    self.inflight.remove(&dir);
                    self.folders.insert(
                        dir,
                        FolderStates {
                            states: Arc::new(states),
                            at: Instant::now(),
                        },
                    );
                }
                Msg::Done(d, dirs) => {
                    for dir in dirs {
                        self.folders.remove(&dir);
                    }
                    done.push(d);
                }
            }
        }
        let now = Instant::now();
        let mut any = false;
        for dir in visible {
            if self.location_of(dir).is_none() {
                continue;
            }
            any = true;
            let fresh = self
                .folders
                .get(dir)
                .is_some_and(|f| now.duration_since(f.at) < STATES_TTL);
            if fresh || self.inflight.contains(dir) {
                continue;
            }
            self.refresh(ctx, dir.clone());
        }
        if any && !self.inline {
            ctx.request_repaint_after(STATES_TTL);
        }
        done
    }

    /// Forget `dir`'s states (its listing changed): read again next frame.
    pub fn invalidate(&mut self, dir: &Path) {
        self.folders.remove(dir);
    }

    fn refresh(&mut self, ctx: &egui::Context, dir: PathBuf) {
        if self.inline {
            let states = self.probe.folder(&dir);
            self.folders.insert(
                dir,
                FolderStates {
                    states: Arc::new(states),
                    at: Instant::now(),
                },
            );
            return;
        }
        self.inflight.insert(dir.clone());
        let (probe, tx, ctx) = (self.probe.clone(), self.tx.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("cloud-states".into())
            .spawn(move || {
                let states = probe.folder(&dir);
                let _ = tx.send(Msg::States(dir, states));
                ctx.request_repaint();
            })
            .ok();
    }

    /// Start "Download Now" / "Remove Download" for `paths` in the
    /// background; the result comes back from [`Cloud::frame`].
    pub fn run(&mut self, ctx: &egui::Context, op: CloudOp, paths: Vec<PathBuf>) {
        let (probe, tx, ctx) = (self.probe.clone(), self.tx.clone(), ctx.clone());
        let work = move || {
            let mut errors = Vec::new();
            let mut dirs: Vec<PathBuf> = Vec::new();
            for p in &paths {
                let r = match op {
                    CloudOp::Download => probe.download(p),
                    CloudOp::Evict => probe.evict(p),
                };
                if let Err(e) = r {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    errors.push(format!("{name}: {e}"));
                }
                if let Some(d) = p.parent().filter(|d| !dirs.iter().any(|x| x == d)) {
                    dirs.push(d.to_path_buf());
                }
            }
            let _ = tx.send(Msg::Done(
                OpDone {
                    op,
                    count: paths.len(),
                    errors,
                },
                dirs,
            ));
            ctx.request_repaint();
        };
        if self.inline {
            work();
        } else {
            std::thread::Builder::new()
                .name("cloud-op".into())
                .spawn(work)
                .ok();
        }
    }
}

/// Sidebar icon of a provider (Phosphor; no brand colours).
pub fn provider_icon(p: CloudProvider) -> &'static str {
    match p {
        CloudProvider::GoogleDrive => icons::GOOGLE_DRIVE_LOGO,
        CloudProvider::Dropbox => icons::DROPBOX_LOGO,
        CloudProvider::Box => icons::PACKAGE,
        CloudProvider::ICloud | CloudProvider::OneDrive | CloudProvider::Other => icons::CLOUD,
    }
}

/// "Google Drive (a@b.de)" -> ("Google Drive", Some("a@b.de")),
/// "OneDrive – Firma" -> ("OneDrive", Some("Firma")), "Dropbox" -> ("Dropbox", None).
pub fn split_display_name(name: &str) -> (&str, Option<&str>) {
    if let Some((base, rest)) = name.split_once(" – ") {
        return (base, Some(rest));
    }
    if let Some((base, rest)) = name.split_once(" (") {
        if let Some(acc) = rest.strip_suffix(')') {
            return (base, Some(acc));
        }
    }
    (name, None)
}

/// One row of the Cloud section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarEntry {
    pub icon: &'static str,
    /// The provider ("Google Drive").
    pub label: String,
    /// The account / organisation, shown dimmed after the label when the
    /// provider appears more than once.
    pub detail: Option<String>,
    pub tooltip: String,
    pub path: PathBuf,
}

/// The rows of the Cloud section.
pub fn sidebar_entries(locations: &[CloudLocation]) -> Vec<SidebarEntry> {
    let bases: Vec<&str> = locations
        .iter()
        .map(|l| split_display_name(&l.display_name).0)
        .collect();
    locations
        .iter()
        .zip(&bases)
        .map(|(l, base)| {
            let (_, account) = split_display_name(&l.display_name);
            let account = account.map(|a| match a {
                "Personal" | "Persönlich" => t!("cloud.personal").to_string(),
                "Shared Libraries" => t!("cloud.shared_libraries").to_string(),
                a => a.to_string(),
            });
            let several = bases.iter().filter(|b| *b == base).count() > 1;
            let mut tooltip = base.to_string();
            if let Some(a) = &account {
                tooltip.push_str(&format!(" · {a}"));
            }
            tooltip.push('\n');
            tooltip.push_str(&l.path.display().to_string());
            SidebarEntry {
                icon: provider_icon(l.provider),
                label: base.to_string(),
                detail: account.filter(|_| several),
                tooltip,
                path: l.path.clone(),
            }
        })
        .collect()
}

pub(crate) fn hint_open_id() -> egui::Id {
    egui::Id::unique("manta-cloud-gdrive-hint-open")
}

/// The sidebar's "Cloud" section (nothing when no cloud folder exists).
/// `hint`: show the "Connect Google Drive …" row.
pub fn section_ui(
    ui: &mut egui::Ui,
    locations: &[CloudLocation],
    hint: bool,
    is_current: &dyn Fn(&Path) -> bool,
    actions: &mut Vec<Action>,
) {
    if locations.is_empty() {
        return;
    }
    if crate::sidebar::section(
        ui,
        t!("sidebar.cloud"),
        Some((icons::ARROW_CLOCKWISE, t!("sidebar.rescan_cloud"), "")),
    ) {
        actions.push(Action::RescanVolumes);
    }
    for e in sidebar_entries(locations) {
        let (resp, _) = row(
            ui,
            Row {
                icon: e.icon,
                label: &e.label,
                detail: e.detail.as_deref(),
                selected: is_current(&e.path),
                ..Row::default()
            },
        );
        // Screen readers (and tests) see the provider and the account.
        let a11y = match &e.detail {
            Some(d) => format!("{} {d}", e.label),
            None => e.label.clone(),
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &a11y));
        if resp.on_hover_text(&e.tooltip).clicked() {
            actions.push(Action::OpenLocal(e.path.clone()));
        }
    }
    if hint {
        google_drive_hint(ui, actions);
    }
}

/// "Connect Google Drive …": a dimmed row that unfolds a short explanation
/// with the download link and "Don't Show Again".
fn google_drive_hint(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let p = pal(ui);
    let open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(hint_open_id()).unwrap_or(false));
    let (resp, _) = row(
        ui,
        Row {
            icon: icons::GOOGLE_DRIVE_LOGO,
            label: t!("cloud.hint.row"),
            dimmed: true,
            ..Row::default()
        },
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t!("cloud.hint.row")));
    if resp.on_hover_text(t!("cloud.hint.tip")).clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(hint_open_id(), !open));
    }
    if !open {
        return;
    }
    ui.add_space(space::XXS);
    egui::Frame::new()
        .fill(p.content)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(egui::CornerRadius::same(theme::radius::CONTROL))
        .inner_margin(egui::Margin::same(space::SM as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XS;
            ui.add(
                egui::Label::new(
                    RichText::new(t!("cloud.hint.body"))
                        .font(theme::regular(text::CAPTION))
                        .color(p.text_secondary),
                )
                .wrap(),
            );
            let link = ui
                .add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{}  {}",
                            icons::ARROW_SQUARE_OUT,
                            t!("cloud.hint.download")
                        ))
                        .font(theme::medium(text::CAPTION))
                        .color(p.accent_text),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(GOOGLE_DRIVE_DOWNLOAD);
            if link.clicked() {
                actions.push(Action::OpenWebLink(GOOGLE_DRIVE_DOWNLOAD.to_string()));
            }
            let dismiss = ui
                .add(
                    egui::Label::new(
                        RichText::new(t!("cloud.hint.dismiss"))
                            .font(theme::regular(text::CAPTION))
                            .color(p.text_tertiary),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if dismiss.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(hint_open_id(), false));
                actions.push(Action::DismissDriveHint);
            }
        });
}

/// Badge glyph and tooltip of a state (`None`: no badge).
pub fn badge(state: SyncState) -> Option<(&'static str, &'static str)> {
    match state {
        SyncState::Local | SyncState::Current => None,
        SyncState::CloudOnly => Some((icons::CLOUD_ARROW_DOWN, t!("cloud.state.cloud_only"))),
        SyncState::Outdated => Some((icons::CLOUD_WARNING, t!("cloud.state.outdated"))),
        SyncState::Downloading => Some((icons::ARROW_CIRCLE_DOWN, t!("cloud.state.downloading"))),
        SyncState::Uploading => Some((icons::CLOUD_ARROW_UP, t!("cloud.state.uploading"))),
        SyncState::UploadError => Some((icons::WARNING_CIRCLE, t!("cloud.state.upload_error"))),
    }
}

/// The badge's tooltip with the details: "Uploading … 42 %", "Waiting to
/// upload", "Upload error: <provider's message>" (`None`: no badge).
pub fn tooltip(info: &SyncInfo) -> Option<String> {
    let (_, tip) = badge(info.state)?;
    Some(match info.state {
        SyncState::Uploading if info.upload_waiting => t!("cloud.state.upload_waiting").to_string(),
        SyncState::Uploading => match info.upload_percent {
            Some(pct) => t!(
                "cloud.state.uploading_percent",
                percent = format!("{pct:.0}")
            ),
            None => tip.to_string(),
        },
        SyncState::UploadError => match info
            .upload_error
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty())
        {
            Some(e) => t!("cloud.state.upload_error_detail", error = e),
            None => tip.to_string(),
        },
        _ => tip.to_string(),
    })
}

/// One pulse of the "uploading" badge.
const PULSE: f64 = 1.6;

/// Opacity of the "uploading" badge at `time` (seconds): a slow, subtle
/// pulse between 45 % and 100 %.
pub fn upload_pulse(time: f64) -> f32 {
    let phase = (time % PULSE) / PULSE * std::f64::consts::TAU;
    (0.725 + 0.275 * phase.cos()) as f32
}

/// Paint the badge of `state` centred in `rect`; returns whether it drew.
/// Upload errors are red; "uploading" pulses (and keeps repainting while
/// it's on screen, at a low rate).
pub fn paint_badge(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: SyncState,
    size: f32,
    color: egui::Color32,
) -> bool {
    let Some((glyph, _)) = badge(state) else {
        return false;
    };
    let color = match state {
        SyncState::UploadError => pal(ui).danger,
        SyncState::Uploading => {
            ui.ctx().request_repaint_after(Duration::from_millis(80));
            // Accent (unlike the grey "only in the cloud"), except on a
            // selected row's accent background.
            let p = pal(ui);
            let base = if color == p.text_on_accent {
                color
            } else {
                p.accent_text
            };
            base.gamma_multiply(upload_pulse(ui.input(|i| i.time)))
        }
        _ => color,
    };
    widgets::paint_icon(ui, rect, glyph, size, color, false);
    true
}

/// What the context menu offers for the selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MenuOffer {
    pub download: bool,
    pub evict: bool,
}

/// "Download Now" when something selected is only in the cloud (or is a
/// folder, whose contents may be), "Remove Download" when something is
/// downloaded. `states`: the folder's states (`None`: not a cloud folder).
pub fn menu_offer(states: Option<&FolderSync>, selected: &[(String, bool)]) -> MenuOffer {
    let Some(states) = states else {
        return MenuOffer::default();
    };
    let mut o = MenuOffer::default();
    for (name, is_dir) in selected {
        let st = states.get(name).map_or(SyncState::Local, |i| i.state);
        o.download |= *is_dir || st.can_download();
        o.evict |= *is_dir || st.can_evict();
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(name: &str) -> CloudLocation {
        let p = cloud_local::parse_cloud_storage_name(name);
        CloudLocation {
            provider: p.provider,
            provider_id: p.provider_id,
            display_name: p.display_name,
            account: p.account,
            path: PathBuf::from("/cs").join(name),
            icon: p.provider.sf_symbol(),
            file_provider: true,
        }
    }

    #[test]
    fn display_names_split() {
        assert_eq!(
            split_display_name("Google Drive (a@b.de)"),
            ("Google Drive", Some("a@b.de"))
        );
        assert_eq!(
            split_display_name("OneDrive – Firma GmbH"),
            ("OneDrive", Some("Firma GmbH"))
        );
        assert_eq!(split_display_name("Dropbox"), ("Dropbox", None));
    }

    #[test]
    fn accounts_shown_only_when_a_provider_repeats() {
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            let one = sidebar_entries(&[loc("GoogleDrive-a@gmail.com"), loc("Dropbox")]);
            assert_eq!(one[0].label, "Google Drive");
            assert_eq!(one[0].detail, None);
            assert!(one[0].tooltip.contains("a@gmail.com"));
            assert_eq!(one[0].icon, icons::GOOGLE_DRIVE_LOGO);
            assert_eq!(one[1].icon, icons::DROPBOX_LOGO);
            let two = sidebar_entries(&[
                loc("GoogleDrive-a@gmail.com"),
                loc("GoogleDrive-b@firma.de"),
                loc("OneDrive-SharedLibraries-OneDriveCloudTemp"),
                loc("OneDrive-Personal"),
            ]);
            assert_eq!(two[0].detail.as_deref(), Some("a@gmail.com"));
            assert_eq!(two[1].detail.as_deref(), Some("b@firma.de"));
            assert_eq!(two[2].detail.as_deref(), Some("Geteilte Bibliotheken"));
            assert_eq!(two[3].detail.as_deref(), Some("Persönlich"));
        });
    }

    #[test]
    fn menu_offers_follow_states() {
        let states: FolderSync = [
            ("a".to_string(), SyncState::CloudOnly.into()),
            ("b".to_string(), SyncState::Current.into()),
        ]
        .into();
        let sel = |v: &[(&str, bool)]| {
            v.iter()
                .map(|(n, d)| (n.to_string(), *d))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            menu_offer(None, &sel(&[("a", false)])),
            MenuOffer::default()
        );
        assert_eq!(
            menu_offer(Some(&states), &sel(&[("a", false)])),
            MenuOffer {
                download: true,
                evict: false
            }
        );
        assert_eq!(
            menu_offer(Some(&states), &sel(&[("b", false)])),
            MenuOffer {
                download: false,
                evict: true
            }
        );
        assert_eq!(
            menu_offer(Some(&states), &sel(&[("x", true)])),
            MenuOffer {
                download: true,
                evict: true
            }
        );
    }

    #[test]
    fn fake_probe_states_and_actions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir
            .path()
            .join("Library/CloudStorage/GoogleDrive-me@example.com");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("cloud.pdf"), b"").unwrap();
        std::fs::write(root.join("here.txt"), b"x").unwrap();
        let probe = Arc::new(FakeProbe::default());
        probe.set_dataless(&root.join("cloud.pdf"), true);
        let locs = cloud_local::discover_in(dir.path());
        let mut c = Cloud::fixed(locs, probe.clone());
        let ctx = egui::Context::default();
        assert!(c.states(&root).is_none());
        c.frame(&ctx, std::slice::from_ref(&root));
        let s = c.states(&root).unwrap();
        assert_eq!(
            s.get("cloud.pdf").map(|i| i.state),
            Some(SyncState::CloudOnly)
        );
        assert_eq!(s.get("here.txt").map(|i| i.state), Some(SyncState::Current));
        // Not a cloud folder: never probed.
        c.frame(&ctx, &[dir.path().to_path_buf()]);
        assert!(c.states(dir.path()).is_none());

        c.run(&ctx, CloudOp::Download, vec![root.join("cloud.pdf")]);
        let done = c.frame(&ctx, std::slice::from_ref(&root));
        assert_eq!(
            done,
            vec![OpDone {
                op: CloudOp::Download,
                count: 1,
                errors: vec![]
            }]
        );
        assert_eq!(
            c.states(&root).unwrap().get("cloud.pdf").map(|i| i.state),
            Some(SyncState::Current)
        );
        assert_eq!(
            probe.calls.lock().unwrap().as_slice(),
            &[("download", root.join("cloud.pdf"))]
        );
    }

    #[test]
    fn upload_tooltips() {
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            let up = SyncInfo::new(SyncState::Uploading);
            assert_eq!(tooltip(&up).as_deref(), Some("Wird hochgeladen …"));
            let pct = SyncInfo {
                upload_percent: Some(41.6),
                ..up.clone()
            };
            assert_eq!(tooltip(&pct).as_deref(), Some("Wird hochgeladen … 42 %"));
            let wait = SyncInfo {
                upload_waiting: true,
                ..up
            };
            assert_eq!(tooltip(&wait).as_deref(), Some("Wartet auf Upload"));
            let err = SyncInfo {
                state: SyncState::UploadError,
                upload_error: Some("Speicher voll".into()),
                ..SyncInfo::default()
            };
            assert_eq!(
                tooltip(&err).as_deref(),
                Some("Upload-Fehler: Speicher voll")
            );
            assert_eq!(
                tooltip(&SyncInfo::new(SyncState::UploadError)).as_deref(),
                Some("Upload-Fehler")
            );
            assert_eq!(tooltip(&SyncInfo::new(SyncState::Current)), None);
        });
        crate::i18n::with_locale(crate::i18n::Locale::En, || {
            let pct = SyncInfo {
                upload_percent: Some(7.0),
                ..SyncInfo::new(SyncState::Uploading)
            };
            assert_eq!(tooltip(&pct).as_deref(), Some("Uploading… 7%"));
        });
        // The pulse stays subtle and within range.
        for i in 0..40 {
            let a = upload_pulse(i as f64 * 0.1);
            assert!((0.44..=1.0).contains(&a), "{a}");
        }
        assert!((upload_pulse(0.0) - 1.0).abs() < 1e-6);
    }
}
