//! Manta as the default file manager (macOS).
//!
//! macOS has no switch to replace Finder. File managers like ForkLift and
//! Path Finder do two things, and so do we:
//!
//! 1. Set the global default `NSFileViewer` to their bundle id. "Show in
//!    Finder" in other apps (`NSWorkspace activateFileViewerSelectingURLs`)
//!    then asks that app to reveal the items.
//! 2. Become the LaunchServices handler for `public.folder`, so opening a
//!    folder from another app (`open ~/Downloads`, a folder link) opens it in
//!    the file manager.
//!
//! The Desktop, the Finder icon in the Dock and double-clicks inside Finder
//! stay with Finder; the UI says so.
//!
//! Before switching, the previous `NSFileViewer` and folder handler are saved
//! (in `default-viewer.json` next to `settings.json`) so switching off gives
//! the role back to that app, or to Finder if it is gone.
//!
//! All OS calls go through [`ViewerOs`], so the logic is unit-tested with a
//! fake. The open / reveal Apple Events arrive in [`native`] (macOS), are
//! queued with [`push_request`] and applied by the app on the next frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Finder's bundle id: where the folder role goes when nothing else is left.
pub const FINDER_ID: &str = "com.apple.finder";
/// "Not now" is offered this many times; then the prompt stops asking.
pub const MAX_NOT_NOW: u8 = 3;

// ------------------------------------------------------------------ prefs

/// Whether the first-run prompt may still ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptState {
    /// Ask at launch (until "Not now" was chosen [`MAX_NOT_NOW`] times).
    #[default]
    Ask,
    /// "Don't ask again".
    Never,
    /// The user answered with "Set as default" (or switched it in Settings).
    Answered,
}

/// Persisted state, in `config_dir/Manta/default-viewer.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ViewerPrefs {
    /// `NSFileViewer` before Manta took it (`None`: it was not set).
    #[serde(default)]
    pub prev_file_viewer: Option<String>,
    /// The `public.folder` handler before Manta took it.
    #[serde(default)]
    pub prev_folder_handler: Option<String>,
    #[serde(default)]
    pub prompt_state: PromptState,
    #[serde(default)]
    pub not_now_count: u8,
}

impl ViewerPrefs {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Manta").join("default-viewer.json"))
    }

    /// Missing or unreadable files give the defaults.
    pub fn load(path: &Path) -> ViewerPrefs {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

// ------------------------------------------------------------------ OS seam

/// The operating-system calls (macOS: CFPreferences, LaunchServices,
/// NSWorkspace, NSBundle). Faked in tests.
pub trait ViewerOs: Send + Sync {
    /// Our bundle id; `None` when not running from an app bundle.
    fn bundle_id(&self) -> Option<String>;
    /// Path of our `.app` bundle.
    fn bundle_path(&self) -> Option<PathBuf>;
    fn home_dir(&self) -> Option<PathBuf>;
    /// The global `NSFileViewer` default (`None`: not set, i.e. Finder).
    fn file_viewer(&self) -> Option<String>;
    /// Set (`Some`) or remove (`None`) the global `NSFileViewer`.
    fn set_file_viewer(&self, id: Option<&str>) -> Result<(), String>;
    /// The default app for `public.folder`.
    fn folder_handler(&self) -> Option<String>;
    fn set_folder_handler(&self, id: &str) -> Result<(), String>;
    /// An app with this bundle id is installed.
    fn app_exists(&self, id: &str) -> bool;
    /// Its display name ("ForkLift"), if installed.
    fn app_name(&self, id: &str) -> Option<String>;
}

/// Off macOS (and in tests unless replaced): nothing is available.
pub struct Unsupported;

impl ViewerOs for Unsupported {
    fn bundle_id(&self) -> Option<String> {
        None
    }
    fn bundle_path(&self) -> Option<PathBuf> {
        None
    }
    fn home_dir(&self) -> Option<PathBuf> {
        None
    }
    fn file_viewer(&self) -> Option<String> {
        None
    }
    fn set_file_viewer(&self, _id: Option<&str>) -> Result<(), String> {
        Err("unsupported".into())
    }
    fn folder_handler(&self) -> Option<String> {
        None
    }
    fn set_folder_handler(&self, _id: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    fn app_exists(&self, _id: &str) -> bool {
        false
    }
    fn app_name(&self, _id: &str) -> Option<String> {
        None
    }
}

/// The real OS on this platform.
pub fn platform_os() -> Arc<dyn ViewerOs> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(native::MacOs)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(Unsupported)
    }
}

// ------------------------------------------------------------------ logic

/// Why the feature can't be used right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// Not macOS.
    Unsupported,
    /// Not running from Manta.app (e.g. `cargo run`).
    NotBundled,
    /// The app runs from somewhere else (a DMG, Downloads …).
    NotInApplications(PathBuf),
}

impl Unavailable {
    pub fn reason(&self) -> String {
        match self {
            Unavailable::Unsupported => t!("viewer.unavailable.unsupported").into(),
            Unavailable::NotBundled => t!("viewer.unavailable.not_bundled").into(),
            Unavailable::NotInApplications(p) => {
                t!("viewer.unavailable.not_in_applications", path = p.display())
            }
        }
    }
}

fn same_id(a: &str, b: &str) -> bool {
    // LaunchServices hands out lower-cased bundle ids.
    a.eq_ignore_ascii_case(b)
}

fn is_ours(os: &dyn ViewerOs, id: Option<&str>) -> bool {
    match (os.bundle_id(), id) {
        (Some(me), Some(id)) => same_id(&me, id),
        _ => false,
    }
}

/// `Ok` when Manta is a bundled app in /Applications or ~/Applications.
pub fn available(os: &dyn ViewerOs) -> Result<(), Unavailable> {
    if !cfg!(target_os = "macos") && os.bundle_id().is_none() {
        return Err(Unavailable::Unsupported);
    }
    if os.bundle_id().is_none() {
        return Err(Unavailable::NotBundled);
    }
    let Some(path) = os.bundle_path() else {
        return Err(Unavailable::NotBundled);
    };
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(h) = os.home_dir() {
        roots.push(h.join("Applications"));
    }
    if roots.iter().any(|r| path.starts_with(r)) {
        Ok(())
    } else {
        Err(Unavailable::NotInApplications(path))
    }
}

/// An app holding (or getting back) the file-viewer role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub id: String,
    pub name: String,
}

fn holder(os: &dyn ViewerOs, id: &str) -> Holder {
    let name = if same_id(id, FINDER_ID) {
        "Finder".to_string()
    } else {
        os.app_name(id).unwrap_or_else(|| id.to_string())
    };
    Holder {
        id: id.to_string(),
        name,
    }
}

/// Who handles "Show in Finder" now: `NSFileViewer`, or Finder when unset.
pub fn current_holder(os: &dyn ViewerOs) -> Holder {
    match os.file_viewer() {
        Some(id) if !id.is_empty() => holder(os, &id),
        _ => match os.folder_handler() {
            Some(id) if !same_id(&id, FINDER_ID) => holder(os, &id),
            _ => holder(os, FINDER_ID),
        },
    }
}

/// Manta holds `NSFileViewer` ("Show in Finder"). The folder role is
/// best-effort: recent macOS versions refuse to hand `public.folder` to any
/// app but Finder (paramErr), see [`holds_folders`].
pub fn is_default(os: &dyn ViewerOs) -> bool {
    is_ours(os, os.file_viewer().as_deref())
}

/// Manta is also the handler for `public.folder`.
pub fn holds_folders(os: &dyn ViewerOs) -> bool {
    is_ours(os, os.folder_handler().as_deref())
}

/// What switching off would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    /// New `NSFileViewer`: `Some(Some(id))` set, `Some(None)` remove,
    /// `None` leave alone (it isn't ours any more).
    pub file_viewer: Option<Option<String>>,
    /// New `public.folder` handler; `None`: leave alone.
    pub folder_handler: Option<String>,
}

/// The saved previous holder if it is still installed (and not us).
fn usable_prev(os: &dyn ViewerOs, prev: Option<&str>) -> Option<String> {
    prev.filter(|id| !id.is_empty() && !is_ours(os, Some(id)) && os.app_exists(id))
        .map(str::to_string)
}

pub fn restore_plan(os: &dyn ViewerOs, prefs: &ViewerPrefs) -> RestorePlan {
    let file_viewer = is_ours(os, os.file_viewer().as_deref())
        .then(|| usable_prev(os, prefs.prev_file_viewer.as_deref()));
    let folder_handler = is_ours(os, os.folder_handler().as_deref()).then(|| {
        usable_prev(os, prefs.prev_folder_handler.as_deref())
            .unwrap_or_else(|| FINDER_ID.to_string())
    });
    RestorePlan {
        file_viewer,
        folder_handler,
    }
}

/// Where the role goes when switched off ("Back to ForkLift").
pub fn restore_target(os: &dyn ViewerOs, prefs: &ViewerPrefs) -> Holder {
    match usable_prev(os, prefs.prev_file_viewer.as_deref()) {
        Some(id) => holder(os, &id),
        None => holder(os, FINDER_ID),
    }
}

/// Make Manta the default file manager (`on`) or give the role back.
///
/// Switching on first records the previous holders in `prefs` and calls
/// `save` (so they survive a crash half-way), then sets both. Switching off
/// restores the saved holders if they are still installed, otherwise removes
/// `NSFileViewer` and gives `public.folder` to Finder; values that no longer
/// point at Manta are left alone.
pub fn set_default(
    os: &dyn ViewerOs,
    prefs: &mut ViewerPrefs,
    on: bool,
    save: &mut dyn FnMut(&ViewerPrefs) -> Result<(), String>,
) -> Result<(), String> {
    if on {
        available(os).map_err(|u| u.reason())?;
        let me = os
            .bundle_id()
            .ok_or_else(|| Unavailable::NotBundled.reason())?;
        let fv = os.file_viewer();
        let fh = os.folder_handler();
        if !is_ours(os, fv.as_deref()) {
            prefs.prev_file_viewer = fv.filter(|s| !s.is_empty());
        }
        if !is_ours(os, fh.as_deref()) {
            prefs.prev_folder_handler = fh;
        }
        save(prefs)?;
        log::info!(
            "default viewer: on (saved previous NSFileViewer={:?}, public.folder={:?})",
            prefs.prev_file_viewer,
            prefs.prev_folder_handler
        );
        os.set_file_viewer(Some(&me))?;
        if let Err(e) = os.set_folder_handler(&me) {
            // macOS 26 answers paramErr for public.folder whatever the app;
            // "Show in Finder" works without it, folders stay with Finder.
            log::warn!("default viewer: macOS kept public.folder ({e}); NSFileViewer is set");
        }
        Ok(())
    } else {
        let plan = restore_plan(os, prefs);
        log::info!("default viewer: off, restoring {plan:?}");
        if let Some(fv) = &plan.file_viewer {
            os.set_file_viewer(fv.as_deref())?;
        }
        if let Some(fh) = &plan.folder_handler {
            os.set_folder_handler(fh)?;
        }
        prefs.prev_file_viewer = None;
        prefs.prev_folder_handler = None;
        save(prefs)
    }
}

/// Show the first-run prompt?
pub fn should_prompt(available: bool, is_default: bool, prefs: &ViewerPrefs) -> bool {
    available
        && !is_default
        && prefs.prompt_state == PromptState::Ask
        && prefs.not_now_count < MAX_NOT_NOW
}

/// The buttons of the first-run prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptChoice {
    SetDefault,
    NotNow,
    Never,
}

/// Record the answer (the caller saves and, for SetDefault, switches).
pub fn record_choice(prefs: &mut ViewerPrefs, choice: PromptChoice) {
    match choice {
        PromptChoice::SetDefault => prefs.prompt_state = PromptState::Answered,
        PromptChoice::NotNow => prefs.not_now_count = prefs.not_now_count.saturating_add(1),
        PromptChoice::Never => prefs.prompt_state = PromptState::Never,
    }
}

// ------------------------------------------------------------------ requests

/// What another app asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Open folders (`aevt/odoc`, e.g. `open ~/Downloads`).
    Open,
    /// Reveal items (`misc/mvis`, "Show in Finder").
    Reveal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub kind: RequestKind,
    pub paths: Vec<PathBuf>,
}

/// A Manta plugin file (`.mantaplugin`): opening it installs it instead of
/// showing its folder.
pub fn is_plugin_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mantaplugin"))
        && !p.is_dir()
}

/// One tab to show: `dir` with `select` (names in it) selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPlan {
    pub dir: PathBuf,
    pub select: Vec<String>,
}

fn name_of(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Turn a request into tabs.
///
/// * Open, a folder: that folder (each folder gets its own tab).
/// * A file (or Reveal of anything): its parent with it selected.
/// * Several items: the parent of the first, with every item that lives in
///   that folder selected.
pub fn plan(req: &OpenRequest, is_dir: &dyn Fn(&Path) -> bool) -> Vec<OpenPlan> {
    let paths: Vec<&PathBuf> = req.paths.iter().filter(|p| p.is_absolute()).collect();
    let Some(first) = paths.first() else {
        return Vec::new();
    };
    if req.kind == RequestKind::Open && paths.iter().all(|p| is_dir(p)) {
        return paths
            .iter()
            .map(|p| OpenPlan {
                dir: p.to_path_buf(),
                select: Vec::new(),
            })
            .collect();
    }
    // A single folder that is "/" has no parent to reveal it in.
    let Some(parent) = first.parent() else {
        return vec![OpenPlan {
            dir: first.to_path_buf(),
            select: Vec::new(),
        }];
    };
    let select = paths
        .iter()
        .filter(|p| p.parent() == Some(parent))
        .filter_map(|p| name_of(p))
        .collect();
    vec![OpenPlan {
        dir: parent.to_path_buf(),
        select,
    }]
}

static QUEUE: Mutex<Vec<OpenRequest>> = Mutex::new(Vec::new());
static REPAINT: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();

/// Queue a request (called from the Apple Event handler) and wake the UI.
pub fn push_request(req: OpenRequest) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(req);
    }
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

/// Requests that arrived since the last call (once per frame).
pub fn take_requests() -> Vec<OpenRequest> {
    QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// Let [`push_request`] wake the UI.
pub fn set_repaint_context(ctx: &eframe::egui::Context) {
    let _ = REPAINT.set(ctx.clone());
}

// ------------------------------------------------------------------ controller

/// What Settings and the prompt show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerStatus {
    pub available: Result<(), Unavailable>,
    pub is_default: bool,
    /// Manta also opens folders from other apps (`public.folder`).
    pub folders: bool,
    /// Current holder of the role ("ForkLift").
    pub holder: Holder,
    /// Where switching off gives the role back to.
    pub restore_to: Holder,
    /// A switch is in progress.
    pub busy: bool,
}

/// The app's handle on the feature: prefs, live status, background switch.
pub struct DefaultViewer {
    os: Arc<dyn ViewerOs>,
    pub prefs: ViewerPrefs,
    path: Option<PathBuf>,
    pub status: ViewerStatus,
    busy: Option<Receiver<(ViewerPrefs, Result<(), String>)>>,
    /// The first-run prompt is up.
    pub prompt_open: bool,
    /// The prompt was asked for (palette, menu bar) rather than shown at
    /// launch: "Cancel" instead of "Not Now" / "Don't Ask Again", and
    /// cancelling records nothing.
    pub prompt_manual: bool,
    /// Test seam: keep `status` as given (no OS reads).
    frozen: bool,
}

impl DefaultViewer {
    pub fn new(os: Arc<dyn ViewerOs>, path: Option<PathBuf>) -> Self {
        let prefs = path.as_deref().map(ViewerPrefs::load).unwrap_or_default();
        let status = Self::read_status(&*os, &prefs);
        DefaultViewer {
            os,
            prefs,
            path,
            status,
            busy: None,
            prompt_open: false,
            prompt_manual: false,
            frozen: false,
        }
    }

    /// The real OS and `default-viewer.json` (tests: nothing available).
    pub fn platform() -> Self {
        if cfg!(test) {
            Self::new(Arc::new(Unsupported), None)
        } else {
            Self::new(platform_os(), ViewerPrefs::default_path())
        }
    }

    fn read_status(os: &dyn ViewerOs, prefs: &ViewerPrefs) -> ViewerStatus {
        let available = available(os);
        ViewerStatus {
            is_default: available.is_ok() && is_default(os),
            folders: available.is_ok() && holds_folders(os),
            available,
            holder: current_holder(os),
            restore_to: restore_target(os, prefs),
            busy: false,
        }
    }

    /// Re-read the real state (Settings opened, after a switch).
    pub fn refresh(&mut self) {
        if self.frozen {
            return;
        }
        let busy = self.busy.is_some();
        self.status = Self::read_status(&*self.os, &self.prefs);
        self.status.busy = busy;
    }

    /// The feature can be used at all on this platform (Settings shows it).
    pub fn supported(&self) -> bool {
        self.status.available != Err(Unavailable::Unsupported)
    }

    /// At launch: open the prompt if it should be asked.
    pub fn maybe_prompt(&mut self) {
        if should_prompt(
            self.status.available.is_ok(),
            self.status.is_default,
            &self.prefs,
        ) {
            log::info!(
                "default viewer: asking (holder {})",
                self.status.holder.name
            );
            self.prompt_open = true;
        }
    }

    fn save(&self) {
        if let Some(path) = &self.path {
            if let Err(e) = self.prefs.save(path) {
                log::warn!("default viewer: could not save {}: {e}", path.display());
            }
        }
    }

    /// The prompt was answered.
    pub fn answer(&mut self, choice: PromptChoice) {
        log::info!(
            "default viewer: prompt answered {choice:?}{}",
            if self.prompt_manual {
                " (asked for)"
            } else {
                ""
            }
        );
        self.prompt_open = false;
        let manual = std::mem::take(&mut self.prompt_manual);
        if manual && choice != PromptChoice::SetDefault {
            return;
        }
        record_choice(&mut self.prefs, choice);
        self.save();
        if choice == PromptChoice::SetDefault {
            self.switch(true);
        }
    }

    /// The command (palette, menu bar) can switch right now.
    pub fn can_toggle(&self) -> bool {
        self.status.available.is_ok() && !self.status.busy && !self.prompt_open
    }

    /// "Use Manta as default file manager" from the palette or the menu
    /// bar: on → the same question as at launch (what changes, what stays
    /// with Finder); off → give the role back right away (nothing is lost;
    /// the toast says to whom).
    pub fn toggle(&mut self) {
        if !self.can_toggle() {
            return;
        }
        if self.status.is_default {
            log::info!("default viewer: switching off (command)");
            self.switch(false);
        } else {
            self.prompt_manual = true;
            self.prompt_open = true;
        }
    }

    /// Switch on or off in the background (CFPreferences / LaunchServices
    /// calls can take a moment; never on the UI thread).
    pub fn switch(&mut self, on: bool) {
        if self.busy.is_some() {
            return;
        }
        if on {
            // Settings switched it on: don't ask at launch any more.
            self.prefs.prompt_state = PromptState::Answered;
        }
        let (tx, rx) = mpsc::channel();
        let os = self.os.clone();
        let mut prefs = self.prefs.clone();
        let path = self.path.clone();
        let ctx = REPAINT.get().cloned();
        std::thread::Builder::new()
            .name("manta-default-viewer".into())
            .spawn(move || {
                let mut save = |p: &ViewerPrefs| match &path {
                    Some(path) => p.save(path),
                    None => Ok(()),
                };
                let r = set_default(&*os, &mut prefs, on, &mut save);
                if let Err(e) = &r {
                    log::warn!(
                        "default viewer: switching {} failed: {e}",
                        if on { "on" } else { "off" }
                    );
                }
                let _ = tx.send((prefs, r));
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            })
            .ok();
        self.busy = Some(rx);
        self.status.busy = true;
    }

    /// Wait for a running switch (tests).
    #[cfg(test)]
    pub fn wait(&mut self) -> Option<Result<(), String>> {
        let rx = self.busy.take()?;
        let r = rx.recv_timeout(std::time::Duration::from_secs(15));
        match r {
            Ok((prefs, r)) => {
                self.prefs = prefs;
                self.refresh();
                Some(r)
            }
            Err(_) => Some(Err("timed out".into())),
        }
    }

    /// Once per frame: a finished switch, with its error if any.
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        let rx = self.busy.as_ref()?;
        match rx.try_recv() {
            Ok((prefs, r)) => {
                self.busy = None;
                self.prefs = prefs;
                self.refresh();
                Some(r)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = None;
                self.refresh();
                Some(Err("switch thread ended".into()))
            }
        }
    }

    /// Test seam.
    #[cfg(test)]
    pub fn with_status(status: ViewerStatus) -> Self {
        DefaultViewer {
            os: Arc::new(Unsupported),
            prefs: ViewerPrefs::default(),
            path: None,
            status,
            busy: None,
            prompt_open: false,
            prompt_manual: false,
            frozen: true,
        }
    }
}

// ------------------------------------------------------------------ macOS

#[cfg(target_os = "macos")]
pub mod native;

// ------------------------------------------------------------------ UI

/// One "✓ changes" / "– stays" line of the prompt.
fn effect_row(ui: &mut eframe::egui::Ui, glyph: &str, color: eframe::egui::Color32, label: &str) {
    use crate::theme::{pal, space, text};
    use eframe::egui;
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
        crate::widgets::paint_icon(ui, r, glyph, 14.0, color, false);
        ui.add(
            egui::Label::new(
                egui::RichText::new(label)
                    .font(crate::theme::regular(text::BODY))
                    .color(p.text),
            )
            .wrap(),
        );
    });
}

/// A titled group of effect rows on a soft card.
fn effect_group(
    ui: &mut eframe::egui::Ui,
    title: &str,
    rows: &[(&str, eframe::egui::Color32, &str)],
) {
    use crate::theme::{pal, radius, space, text};
    use eframe::egui;
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.stripe)
        .corner_radius(egui::CornerRadius::same(radius::PANEL))
        .inner_margin(egui::Margin::symmetric(
            space::MD as i8 + 4,
            space::SM as i8 + 2,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XXS;
            ui.label(
                egui::RichText::new(title)
                    .font(crate::theme::semibold(text::SMALL))
                    .color(p.text_secondary),
            );
            ui.add_space(space::XXS);
            for (glyph, color, label) in rows {
                effect_row(ui, glyph, *color, label);
            }
        });
}

/// The first-run question. Returns the button that was pressed (Esc or a
/// click outside counts as "Not now").
/// `manual`: asked for from the palette / menu bar (only "Cancel" and
/// "Set as Default").
pub fn prompt_ui(
    ctx: &eframe::egui::Context,
    status: &ViewerStatus,
    manual: bool,
) -> Option<PromptChoice> {
    use crate::dialogs;
    use crate::icons;
    use crate::theme::{self, space, text};
    use crate::widgets::{self, ButtonKind};
    use eframe::egui;
    let p = theme::pal_ctx(ctx);
    let mut choice = None;
    let modal = dialogs::dialog_modal("default-viewer-prompt", ctx).show(ctx, |ui| {
        ui.set_width(468.0);
        dialogs::dialog_header(
            ui,
            icons::FOLDER_OPEN,
            p.accent_text,
            p.accent_soft,
            t!("viewer.prompt.title"),
            "",
        );
        ui.add_space(space::MD);
        ui.add(
            egui::Label::new(egui::RichText::new(t!("viewer.prompt.body")).color(p.text)).wrap(),
        );
        ui.add_space(space::MD);
        effect_group(
            ui,
            t!("viewer.prompt.changes"),
            &[
                (icons::CHECK, p.success, t!("viewer.effect.reveal")),
                (icons::CHECK, p.success, t!("viewer.effect.folders")),
            ],
        );
        ui.add_space(space::SM);
        effect_group(
            ui,
            t!("viewer.prompt.stays"),
            &[
                (
                    icons::DESKTOP,
                    p.text_secondary,
                    t!("viewer.effect.desktop"),
                ),
                (
                    icons::FOLDER_SIMPLE,
                    p.text_secondary,
                    t!("viewer.effect.finder_windows"),
                ),
            ],
        );
        ui.add_space(space::MD);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.label(
                egui::RichText::new(t!("viewer.currently", name = status.holder.name))
                    .font(theme::medium(text::SMALL))
                    .color(p.text_secondary),
            );
            ui.label(
                egui::RichText::new(format!("·  {}", t!("viewer.prompt.later")))
                    .font(theme::regular(text::SMALL))
                    .color(p.text_tertiary),
            );
        });
        ui.add_space(space::LG);
        ui.horizontal(|ui| {
            if !manual
                && widgets::button(ui, t!("viewer.prompt.never"), ButtonKind::Secondary, true)
                    .clicked()
            {
                choice = Some(PromptChoice::Never);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, t!("viewer.prompt.set"), ButtonKind::Primary, true).clicked()
                {
                    choice = Some(PromptChoice::SetDefault);
                }
                let later = if manual {
                    t!("common.cancel")
                } else {
                    t!("viewer.prompt.not_now")
                };
                if widgets::button(ui, later, ButtonKind::Secondary, true).clicked() {
                    choice = Some(PromptChoice::NotNow);
                }
            });
        });
    });
    if choice.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        choice = Some(PromptChoice::SetDefault);
    }
    if choice.is_none() && modal.should_close() {
        choice = Some(PromptChoice::NotNow);
    }
    choice
}

/// `MANTA_DV_SELFTEST` for the end-to-end check on a real Mac:
/// `status` prints the state and exits; `off` gives the role back and exits;
/// `on` takes the role, then starts the app normally (to receive events).
pub mod selftest {
    use super::*;

    pub fn active() -> bool {
        std::env::var_os("MANTA_DV_SELFTEST").is_some()
    }

    fn report(os: &dyn ViewerOs, prefs: &ViewerPrefs) {
        let line = format!(
            "DV self-test: bundle_id={:?} bundle_path={:?} available={:?} NSFileViewer={:?} public.folder={:?} is_default={} prefs={:?}",
            os.bundle_id(),
            os.bundle_path(),
            available(os),
            os.file_viewer(),
            os.folder_handler(),
            is_default(os),
            prefs
        );
        log::info!("{line}");
        println!("{line}");
    }

    /// `Some(exit code)` to exit before the UI starts.
    pub fn run_before_ui() -> Option<i32> {
        let mode = std::env::var("MANTA_DV_SELFTEST").ok()?;
        let os = platform_os();
        let path = ViewerPrefs::default_path();
        let mut prefs = path.as_deref().map(ViewerPrefs::load).unwrap_or_default();
        report(&*os, &prefs);
        let mut save = |p: &ViewerPrefs| match &path {
            Some(path) => p.save(path),
            None => Ok(()),
        };
        let code = match mode.as_str() {
            "status" => return Some(0),
            "on" | "off" => {
                let on = mode == "on";
                let r = set_default(&*os, &mut prefs, on, &mut save);
                let line = format!("DV self-test: set_default({on}) -> {r:?}");
                log::info!("{line}");
                println!("{line}");
                report(&*os, &prefs);
                match (r, on) {
                    (Ok(()), true) => return None,
                    (Ok(()), false) => 0,
                    (Err(_), _) => 1,
                }
            }
            other => {
                eprintln!("MANTA_DV_SELFTEST: unknown mode {other:?} (status, on, off)");
                2
            }
        };
        Some(code)
    }
}

#[cfg(test)]
pub(crate) mod tests;
