//! Built-in Duplicates plugin. PUBLIC API CONTRACT (used by the app):
//! `DuplicatesPlugin::new()`.
//!
//! Finds identical files in a folder and its subfolders (size, then a quick
//! hash of both ends, then SHA-256; see [`scan`]) and moves the extra copies
//! to the Trash, never deleting anything permanently.
//!
//! - **Commands:** "Find Duplicates in This Folder", "Show Duplicates
//!   Panel", "Edit Duplicates Settings…" (palette); "Find Duplicates Here"
//!   and "Keep Duplicates in This Folder" (context menu, while one local
//!   folder is selected).
//! - **Duplicates panel** (right sidebar): progress with Cancel, a summary,
//!   the groups (biggest savings first) with Trash Extras, Reveal and
//!   Select in Pane, and the options.
//! - **Move to Trash** sheet: confirms before anything moves. The extras
//!   are checked again first (size and date from the scan), then moved with
//!   the `trash` tool or by the Finder via `osascript` (or the
//!   `trash_command` from `settings.json`, which tests use).
//!
//! Threads: the trait methods only touch shared state; the scan, the
//! trashing, the settings file and the Finder run on their own threads.
//! This replaces the former Python plugin `duplicates`; the host copies its
//! `settings.json` over on first start (`RETIRED_EXTERNAL`).

pub mod i18n;
pub mod kit;
pub mod panel;
pub mod scan;

#[cfg(test)]
mod render_tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{
    CommandContext, CommandSpec, HostRequest, PaneId, PaneState, PanelPlacement, PanelSpec, Plugin,
    PluginEvent, PluginHandle, PluginInfo, ToastLevel,
};

use i18n::tr;
use scan::{FileInfo, Group, Phase, Settings, Stats};

pub const ID: &str = "manta.duplicates";
/// The id of the Python plugin this one replaces.
pub const OLD_ID: &str = "duplicates";
pub const PANEL: &str = "duplicates";
pub const CONFIRM: &str = "confirm";
/// Groups shown per "Show More".
pub const PAGE: usize = 30;
pub const MAX_GROUPS_SHOWN: usize = 120;
const TRASH_BATCH: usize = 200;
const TRASH_TIMEOUT: Duration = Duration::from_secs(300);
const REVEAL_MAX: usize = 20;

/// Progress of a running scan, written by the scan threads.
#[derive(Default)]
pub struct ScanProgress {
    phase: AtomicU8,
    pub done: AtomicUsize,
    pub total: AtomicUsize,
}

impl ScanProgress {
    pub fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Relaxed) {
            1 => Phase::Hashing,
            2 => Phase::Verifying,
            _ => Phase::Listing,
        }
    }

    fn set(&self, phase: Phase, done: usize, total: usize) {
        self.phase.store(phase as u8, Ordering::Relaxed);
        self.done.store(done, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }
}

pub struct ScanState {
    pub root: PathBuf,
    cancel: Arc<AtomicBool>,
    pub progress: Arc<ScanProgress>,
    pub started: Instant,
}

pub struct ScanResult {
    pub root: PathBuf,
    /// Sorted by reclaimable bytes.
    pub groups: Vec<Group>,
    pub stats: Stats,
    pub elapsed: Duration,
    /// Group key → the path the user chose to keep.
    pub keep: HashMap<String, PathBuf>,
    /// Options changed since the scan.
    pub stale: bool,
}

impl ScanResult {
    /// (groups, duplicate files, reclaimable bytes).
    pub fn totals(&self) -> (usize, usize, u64) {
        let files = self.groups.iter().map(|g| g.files.len() - 1).sum();
        (
            self.groups.len(),
            files,
            self.groups.iter().map(Group::reclaimable).sum(),
        )
    }
}

/// A note shown instead of (or above) results.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Cancelled,
    Failed(String),
}

/// Which groups an action applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    All,
    Group(usize),
}

/// Everything the panel shows; shared with the worker threads.
pub struct Shared {
    pub settings: Settings,
    pub settings_errors: Vec<String>,
    pub scan: Option<ScanState>,
    pub result: Option<ScanResult>,
    pub message: Option<Message>,
    pub limit: usize,
    /// Groups and their extras waiting for the confirmation sheet.
    pub pending: Option<Vec<(Group, Vec<FileInfo>)>>,
    /// (done, total) while trashing.
    pub trashing: Option<(usize, usize)>,
    /// The active pane's single selected local folder (context commands).
    pub folder_selected: Option<PathBuf>,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            settings: Settings::default(),
            settings_errors: Vec::new(),
            scan: None,
            result: None,
            message: None,
            limit: PAGE,
            pending: None,
            trashing: None,
            folder_selected: None,
        }
    }
}

impl Shared {
    /// Index of the file kept in `g`.
    pub fn kept(&self, g: &Group) -> usize {
        if let Some(manual) = self.result.as_ref().and_then(|r| r.keep.get(&g.key)) {
            if let Some(i) = g.files.iter().position(|f| &f.path == manual) {
                return i;
            }
        }
        scan::keep_index(
            &g.files,
            &self.settings.keep_rule,
            &self.settings.keep_folder,
        )
    }

    pub fn extras(&self, g: &Group) -> Vec<FileInfo> {
        let k = self.kept(g);
        g.files
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != k)
            .map(|(_, f)| f.clone())
            .collect()
    }

    /// [(group, extras)] for `target`.
    pub fn targets(&self, target: Target) -> Vec<(Group, Vec<FileInfo>)> {
        let Some(r) = &self.result else {
            return Vec::new();
        };
        match target {
            Target::All => r
                .groups
                .iter()
                .map(|g| (g.clone(), self.extras(g)))
                .collect(),
            Target::Group(i) => r
                .groups
                .get(i)
                .map(|g| vec![(g.clone(), self.extras(g))])
                .unwrap_or_default(),
        }
    }

    pub fn busy(&self) -> bool {
        self.scan.is_some() || self.trashing.is_some()
    }
}

pub type SharedRef = Arc<Mutex<Shared>>;

pub fn lock(s: &SharedRef) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a click in the panel or the sheet asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Scan,
    Cancel,
    Clear,
    More,
    AskTrash(Target),
    Reveal(Target),
    Select(Target),
    Keep(usize, usize),
    CycleRule,
    CycleMinSize,
    ToggleHidden,
    EditSettings,
    ConfirmTrash,
    CancelTrash,
}

pub struct DuplicatesPlugin {
    handle: Option<PluginHandle>,
    shared: SharedRef,
    panes: std::collections::BTreeMap<PaneId, PaneState>,
    active: PaneId,
    /// Generation of the "is the selection a folder?" check.
    sel_gen: Arc<AtomicU64>,
    /// Clicks queued from outside the UI (tests), applied on the next draw.
    queued: Arc<Mutex<Vec<Action>>>,
}

impl Default for DuplicatesPlugin {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl DuplicatesPlugin {
    pub fn new() -> Self {
        DuplicatesPlugin {
            handle: None,
            shared: Arc::new(Mutex::new(Shared::default())),
            panes: Default::default(),
            active: PaneId::Left,
            sel_gen: Arc::new(AtomicU64::new(0)),
            queued: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queue panel clicks; they are applied when the panel is drawn next
    /// (tests drive the plugin through the host this way).
    #[doc(hidden)]
    pub fn click_queue(&self) -> Arc<Mutex<Vec<Action>>> {
        self.queued.clone()
    }

    /// Shared state (tests and renders).
    #[doc(hidden)]
    pub fn shared(&self) -> SharedRef {
        self.shared.clone()
    }

    fn settings_path(&self) -> Option<PathBuf> {
        self.handle
            .as_ref()
            .map(|h| h.data_dir().join("settings.json"))
    }

    fn send(&self, req: HostRequest) {
        if let Some(h) = &self.handle {
            h.send(req);
        }
    }

    fn toast(&self, level: ToastLevel, text: impl Into<String>) {
        self.send(HostRequest::Toast {
            level,
            text: text.into(),
        });
    }

    fn open_panel(&self) {
        self.send(HostRequest::SetPanelOpen {
            panel: PANEL.into(),
            pane: None,
            open: true,
        });
    }

    fn active_state(&self) -> Option<&PaneState> {
        self.panes.get(&self.active)
    }

    /// Load `settings.json` (and its error texts) in the background.
    fn load_settings(&self) {
        let (Some(path), Some(h)) = (self.settings_path(), self.handle.clone()) else {
            return;
        };
        let shared = self.shared.clone();
        let lang = manta_plugin::i18n::lang();
        std::thread::spawn(move || {
            let (s, errors) = manta_plugin::i18n::with_lang(lang, || scan::load_settings(&path));
            for e in &errors {
                log::warn!("duplicates: {e}");
            }
            let mut st = lock(&shared);
            st.settings = s;
            st.settings_errors = errors;
            drop(st);
            h.request_repaint();
        });
    }

    /// Write `settings.json` in the background.
    fn save_settings(&self) {
        let (Some(path), Some(h)) = (self.settings_path(), self.handle.clone()) else {
            return;
        };
        let s = lock(&self.shared).settings.clone();
        std::thread::spawn(move || {
            if let Err(e) = scan::save_settings(&path, &s) {
                h.send(HostRequest::Toast {
                    level: ToastLevel::Error,
                    text: tr!("toast.save_failed", error = e),
                });
            }
        });
    }

    /// Open `settings.json` in the text editor (written first if missing).
    fn edit_settings(&self) {
        let (Some(path), Some(h)) = (self.settings_path(), self.handle.clone()) else {
            return;
        };
        let s = lock(&self.shared).settings.clone();
        std::thread::spawn(move || {
            if !path.exists() {
                let _ = scan::save_settings(&path, &s);
            }
            let r = std::process::Command::new("open")
                .arg("-t")
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Err(e) = r {
                h.send(HostRequest::Toast {
                    level: ToastLevel::Error,
                    text: tr!("toast.open_failed", path = path.display(), error = e),
                });
            }
        });
    }

    /// Scan `root` in the background (ignored while busy).
    pub fn start_scan(&mut self, root: PathBuf) {
        let Some(h) = self.handle.clone() else { return };
        let mut st = lock(&self.shared);
        if st.busy() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(ScanProgress::default());
        st.scan = Some(ScanState {
            root: root.clone(),
            cancel: cancel.clone(),
            progress: progress.clone(),
            started: Instant::now(),
        });
        st.message = None;
        st.limit = PAGE;
        let settings = st.settings.clone();
        drop(st);
        self.open_panel();
        let shared = self.shared.clone();
        let started = Instant::now();
        let spawned = std::thread::Builder::new()
            .name("dup-scan".into())
            .spawn(move || {
                let report =
                    |phase: Phase, done: usize, total: usize| progress.set(phase, done, total);
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scan::find_duplicates(&root, &settings, &cancel, &report)
                }));
                let mut st = lock(&shared);
                if !st
                    .scan
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(&s.cancel, &cancel))
                {
                    return;
                }
                st.scan = None;
                match out {
                    Ok(Ok((groups, stats))) => {
                        log::info!(
                            "duplicates: scanned {}: {} files, {} groups",
                            root.display(),
                            stats.files,
                            groups.len()
                        );
                        st.result = Some(ScanResult {
                            root,
                            groups,
                            stats,
                            elapsed: started.elapsed(),
                            keep: HashMap::new(),
                            stale: false,
                        });
                    }
                    Ok(Err(scan::Cancelled)) => st.message = Some(Message::Cancelled),
                    Err(e) => {
                        let why = e
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_default();
                        st.message = Some(Message::Failed(why));
                    }
                }
                drop(st);
                h.request_repaint();
            });
        if spawned.is_err() {
            lock(&self.shared).scan = None;
        }
    }

    pub fn cancel_scan(&self) {
        if let Some(s) = &lock(&self.shared).scan {
            s.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Scan the folder a pane shows; remote panes get a calm message.
    fn scan_state(&mut self, state: &PaneState) {
        match &state.local_path {
            Some(local) => self.start_scan(local.clone()),
            None => {
                lock(&self.shared).message = None;
                self.open_panel();
                self.toast(ToastLevel::Info, tr!("toast.local_only"));
            }
        }
    }

    /// "Find Duplicates Here": the selected folder, else the pane's folder.
    fn scan_selected_folder(&mut self, state: &PaneState) {
        let first = state.selection.first().map(|p| PathBuf::from(p.as_str()));
        match first.filter(|_| state.local_path.is_some()) {
            Some(f) if lock(&self.shared).folder_selected.as_ref() == Some(&f) => {
                self.start_scan(f)
            }
            _ => self.scan_state(state),
        }
    }

    /// "Keep Duplicates in This Folder".
    fn keep_here(&mut self, state: &PaneState) {
        let Some(f) = state.selection.first().map(|p| PathBuf::from(p.as_str())) else {
            return;
        };
        if state.local_path.is_none() || lock(&self.shared).folder_selected.as_ref() != Some(&f) {
            return;
        }
        let mut st = lock(&self.shared);
        st.settings.keep_rule = "folder".into();
        st.settings.keep_folder = f.to_string_lossy().into_owned();
        if let Some(r) = &mut st.result {
            r.keep.clear();
        }
        drop(st);
        self.save_settings();
        self.toast(
            ToastLevel::Success,
            tr!(
                "toast.keep_here",
                name = scan::folder_name(&f.to_string_lossy())
            ),
        );
    }

    /// Check in the background whether the active pane's selection is one
    /// local folder (for the context commands).
    fn check_selection(&self) {
        let Some(h) = self.handle.clone() else { return };
        let gen = self.sel_gen.fetch_add(1, Ordering::Relaxed) + 1;
        let candidate = self
            .active_state()
            .filter(|s| s.local_path.is_some() && s.selection.len() == 1)
            .map(|s| PathBuf::from(s.selection[0].as_str()));
        let (shared, sel_gen) = (self.shared.clone(), self.sel_gen.clone());
        let set = move |v: Option<PathBuf>| {
            let mut st = lock(&shared);
            if sel_gen.load(Ordering::Relaxed) != gen || st.folder_selected == v {
                return;
            }
            let changed = st.folder_selected.is_some() != v.is_some();
            st.folder_selected = v;
            drop(st);
            if changed {
                h.send(HostRequest::ContributionsChanged);
            }
        };
        match candidate {
            None => set(None),
            Some(p) => {
                std::thread::spawn(move || {
                    let is_dir = std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir());
                    set(is_dir.then_some(p));
                });
            }
        }
    }

    /// Open the confirmation sheet for `target`'s extras.
    fn ask_trash(&self, target: Target) {
        let mut st = lock(&self.shared);
        if st.busy() {
            return;
        }
        let work = st.targets(target);
        if work.iter().all(|(_, ex)| ex.is_empty()) {
            return;
        }
        st.pending = Some(work);
        drop(st);
        self.send(HostRequest::SetPanelOpen {
            panel: CONFIRM.into(),
            pane: None,
            open: true,
        });
    }

    fn close_confirm(&self) {
        self.send(HostRequest::SetPanelOpen {
            panel: CONFIRM.into(),
            pane: None,
            open: false,
        });
    }

    /// Move the pending extras to the Trash (re-checked first), then report.
    fn confirm_trash(&self) {
        let Some(h) = self.handle.clone() else { return };
        let mut st = lock(&self.shared);
        let Some(work) = st.pending.take() else {
            return;
        };
        if st.trashing.is_some() {
            return;
        }
        st.trashing = Some((0, work.iter().map(|(_, ex)| ex.len()).sum()));
        let settings = st.settings.clone();
        drop(st);
        self.close_confirm();
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("dup-trash".into())
            .spawn(move || run_trash(&h, &shared, work, &settings, scan::which))
            .ok();
    }

    fn reveal(&self, target: Target) {
        let st = lock(&self.shared);
        let Some(r) = &st.result else { return };
        let paths: Vec<PathBuf> = match target {
            Target::All => r
                .groups
                .iter()
                .flat_map(|g| st.extras(g))
                .map(|f| f.path)
                .collect(),
            Target::Group(i) => r
                .groups
                .get(i)
                .map(|g| g.files.iter().map(|f| f.path.clone()).collect())
                .unwrap_or_default(),
        };
        drop(st);
        let Some(h) = self.handle.clone() else { return };
        std::thread::spawn(move || {
            let paths: Vec<PathBuf> = paths
                .into_iter()
                .filter(|p| p.symlink_metadata().is_ok())
                .take(REVEAL_MAX)
                .collect();
            if paths.is_empty() {
                return;
            }
            let r = std::process::Command::new("open")
                .arg("-R")
                .args(&paths)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Err(e) = r {
                h.send(HostRequest::Toast {
                    level: ToastLevel::Error,
                    text: tr!("toast.reveal_failed", error = e),
                });
            }
        });
    }

    /// Select the extras in the pane: those in the folder holding most of
    /// them (for all groups: the scanned folder itself if it has any).
    fn select_in_pane(&self, target: Target) {
        let st = lock(&self.shared);
        let Some(r) = &st.result else { return };
        let files: Vec<FileInfo> = st
            .targets(target)
            .into_iter()
            .flat_map(|(_, ex)| ex)
            .collect();
        let root = r.root.clone();
        drop(st);
        if files.is_empty() {
            return;
        }
        let paths = select_paths(&files, &root, target == Target::All);
        let n = paths.len();
        self.send(HostRequest::Select {
            pane: None,
            paths: paths
                .iter()
                .map(|p| VfsPath::new(&p.to_string_lossy()))
                .collect(),
        });
        if n < files.len() {
            self.toast(
                ToastLevel::Info,
                tr!("toast.selected_some", n = n, total = files.len()),
            );
        }
    }

    fn set_keep(&self, group: usize, file: usize) {
        let mut st = lock(&self.shared);
        let Some(r) = &mut st.result else { return };
        let Some(g) = r.groups.get(group) else { return };
        let Some(f) = g.files.get(file) else { return };
        let (key, path) = (g.key.clone(), f.path.clone());
        r.keep.insert(key, path);
    }

    fn change_option(&self, a: &Action) {
        let mut st = lock(&self.shared);
        match a {
            Action::CycleRule => {
                let i = scan::KEEP_RULES
                    .iter()
                    .position(|r| *r == st.settings.keep_rule)
                    .unwrap_or(0);
                st.settings.keep_rule = scan::KEEP_RULES[(i + 1) % scan::KEEP_RULES.len()].into();
                if let Some(r) = &mut st.result {
                    r.keep.clear();
                }
            }
            Action::CycleMinSize => {
                let cur = st.settings.min_size;
                st.settings.min_size = scan::MIN_SIZES
                    .iter()
                    .copied()
                    .find(|&x| x > cur)
                    .unwrap_or(scan::MIN_SIZES[0]);
                if let Some(r) = &mut st.result {
                    r.stale = true;
                }
            }
            Action::ToggleHidden => {
                st.settings.include_hidden = !st.settings.include_hidden;
                if let Some(r) = &mut st.result {
                    r.stale = true;
                }
            }
            _ => return,
        }
        drop(st);
        self.save_settings();
    }

    /// Carry out a click from the panel or the sheet.
    pub fn apply(&mut self, a: Action, pane: &PaneState) {
        match a {
            Action::Scan => {
                let state = self.active_state().cloned().unwrap_or_else(|| pane.clone());
                self.scan_state(&state);
            }
            Action::Cancel => self.cancel_scan(),
            Action::Clear => {
                let mut st = lock(&self.shared);
                st.result = None;
                st.message = None;
            }
            Action::More => {
                let mut st = lock(&self.shared);
                st.limit = (st.limit + PAGE).min(MAX_GROUPS_SHOWN);
            }
            Action::AskTrash(t) => self.ask_trash(t),
            Action::Reveal(t) => self.reveal(t),
            Action::Select(t) => self.select_in_pane(t),
            Action::Keep(g, f) => self.set_keep(g, f),
            Action::CycleRule | Action::CycleMinSize | Action::ToggleHidden => {
                self.change_option(&a)
            }
            Action::EditSettings => self.edit_settings(),
            Action::ConfirmTrash => self.confirm_trash(),
            Action::CancelTrash => {
                lock(&self.shared).pending = None;
                self.close_confirm();
            }
        }
        if let Some(h) = &self.handle {
            h.request_repaint();
        }
    }
}

/// The extras to select: grouped by folder, the folder with most of them
/// (ties: the first seen); with `prefer_root`, the scanned folder if any
/// extras are directly in it.
pub fn select_paths(files: &[FileInfo], root: &Path, prefer_root: bool) -> Vec<PathBuf> {
    let mut by_dir: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for f in files {
        let dir = f.path.parent().map(Path::to_path_buf).unwrap_or_default();
        match by_dir.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, v)) => v.push(f.path.clone()),
            None => by_dir.push((dir, vec![f.path.clone()])),
        }
    }
    if prefer_root {
        if let Some((_, v)) = by_dir.iter().find(|(d, _)| d == root) {
            return v.clone();
        }
    }
    let mut best: Option<&(PathBuf, Vec<PathBuf>)> = None;
    for e in &by_dir {
        let key = |e: &(PathBuf, Vec<PathBuf>)| (e.1.len(), -(e.0.as_os_str().len() as i64));
        if best.is_none_or(|b| key(e) > key(b)) {
            best = Some(e);
        }
    }
    best.map(|b| b.1.clone()).unwrap_or_default()
}

/// Run `argv`, waiting at most `timeout`; (success, last stderr line).
fn run_tool(argv: &[String], timeout: Duration) -> (bool, Option<String>) {
    use std::io::Read as _;
    let Some((exe, args)) = argv.split_first() else {
        return (false, None);
    };
    let child = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (false, Some(e.to_string())),
    };
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let err = reader.join().unwrap_or_default();
    let last = err.trim().lines().last().map(str::to_string);
    match status {
        Some(st) if st.success() => (true, None),
        Some(_) => (false, last),
        None => (false, Some(last.unwrap_or_else(|| "timed out".into()))),
    }
}

/// The trash thread: re-check, move in batches, update the results, toast.
fn run_trash(
    h: &PluginHandle,
    shared: &SharedRef,
    work: Vec<(Group, Vec<FileInfo>)>,
    settings: &Settings,
    which: impl Fn(&str) -> Option<PathBuf>,
) {
    let mut todo = Vec::new();
    let mut skipped = 0usize;
    for (g, ex) in &work {
        let keep: Vec<&FileInfo> = g.files.iter().filter(|f| !ex.contains(f)).collect();
        if keep.len() != 1 || !scan::unchanged(keep[0]) {
            skipped += ex.len();
            continue;
        }
        for f in ex {
            if scan::unchanged(f) {
                todo.push(f.clone());
            } else {
                skipped += 1;
            }
        }
    }
    let mut moved: Vec<FileInfo> = Vec::new();
    let mut errors = Vec::new();
    for batch in todo.chunks(TRASH_BATCH) {
        let paths: Vec<PathBuf> = batch.iter().map(|f| f.path.clone()).collect();
        let argv = scan::trash_argv(settings, &paths, &which);
        let (ok, err) = run_tool(&argv, TRASH_TIMEOUT);
        if !ok {
            errors.extend(err);
        }
        moved.extend(
            batch
                .iter()
                .filter(|f| f.path.symlink_metadata().is_err())
                .cloned(),
        );
        lock(shared).trashing = Some((moved.len(), todo.len()));
        h.request_repaint();
    }
    let gone: std::collections::HashSet<&PathBuf> = moved.iter().map(|f| &f.path).collect();
    let mut st = lock(shared);
    st.trashing = None;
    if let Some(r) = &mut st.result {
        for g in &mut r.groups {
            g.files.retain(|f| !gone.contains(&f.path));
        }
        r.groups.retain(|g| g.files.len() > 1);
        scan::sort_groups(&mut r.groups);
    }
    drop(st);
    h.send(HostRequest::RefreshPane { pane: None });
    let freed: u64 = moved.iter().map(|f| f.size).sum();
    let failed = todo.len() - moved.len();
    let (level, text) = if !moved.is_empty() && failed == 0 && skipped == 0 {
        (
            ToastLevel::Success,
            tr!(
                "toast.moved",
                files = tr!("files", count = moved.len()),
                size = scan::human_size(freed)
            ),
        )
    } else {
        let mut parts = vec![tr!(
            "toast.moved_part",
            files = tr!("files", count = moved.len()),
            size = scan::human_size(freed)
        )];
        if skipped > 0 {
            parts.push(tr!("toast.changed", files = tr!("files", count = skipped)));
        }
        if failed > 0 {
            let mut s = tr!("toast.failed", count = failed);
            if let Some(e) = errors.first() {
                s.push_str(": ");
                s.push_str(e);
            }
            parts.push(s);
        }
        (
            if moved.is_empty() {
                ToastLevel::Error
            } else {
                ToastLevel::Warning
            },
            parts.join(tr!("note.sep")),
        )
    };
    h.send(HostRequest::Toast { level, text });
    log::info!(
        "duplicates: trashed {}, skipped {skipped}, failed {failed}",
        moved.len()
    );
}

impl Plugin for DuplicatesPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: ID.into(),
            name: tr!("plugin.name").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: tr!("plugin.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        self.handle = Some(handle);
        self.load_settings();
    }

    fn disable(&mut self) {
        self.cancel_scan();
        self.handle = None;
        *lock(&self.shared) = Shared::default();
        self.panes.clear();
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s)
            | PluginEvent::ListingRefreshed(s)
            | PluginEvent::SelectionChanged(s) => {
                if s.active {
                    self.active = s.pane;
                }
                self.panes.insert(s.pane, s.clone());
                if s.pane == self.active {
                    self.check_selection();
                }
            }
            PluginEvent::ActivePaneChanged(p) => {
                self.active = *p;
                for s in self.panes.values_mut() {
                    s.active = s.pane == *p;
                }
                self.check_selection();
            }
            PluginEvent::LocaleChanged(_) => self.load_settings(),
            PluginEvent::PaneClosed(p) => {
                self.panes.remove(p);
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let palette = |id: &str, title: &str| CommandSpec {
            id: id.into(),
            title: title.into(),
            shortcut: None,
            context: CommandContext::Palette,
        };
        let mut v = vec![
            palette("scan", tr!("cmd.scan")),
            palette("panel", tr!("cmd.panel")),
            palette("settings", tr!("cmd.settings")),
        ];
        if lock(&self.shared).folder_selected.is_some() {
            for (id, title) in [
                ("scan-here", tr!("cmd.scan_here")),
                ("keep-here", tr!("cmd.keep_here")),
            ] {
                v.push(CommandSpec {
                    id: id.into(),
                    title: title.into(),
                    shortcut: None,
                    context: CommandContext::FileContextMenu,
                });
            }
        }
        v
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        self.panes.insert(pane.pane, pane.clone());
        if pane.active {
            self.active = pane.pane;
        }
        match id {
            "scan" => self.scan_state(pane),
            "scan-here" => self.scan_selected_folder(pane),
            "keep-here" => self.keep_here(pane),
            "panel" => self.open_panel(),
            "settings" => self.edit_settings(),
            _ => {}
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![
            PanelSpec {
                id: PANEL.into(),
                title: tr!("panel.duplicates").into(),
                placement: PanelPlacement::RightSidebar,
                default_open: false,
            },
            PanelSpec {
                id: CONFIRM.into(),
                title: tr!("panel.confirm").into(),
                placement: PanelPlacement::Modal,
                default_open: false,
            },
        ]
    }

    fn panel_size(&self, panel: &str) -> Option<[f32; 2]> {
        (panel == CONFIRM).then_some([440.0, 340.0])
    }

    fn panel_ui(&mut self, panel: &str, pane: &PaneState, ui: &mut egui::Ui) {
        let active = self.active_state().cloned().unwrap_or_else(|| pane.clone());
        let queued: Vec<Action> =
            std::mem::take(&mut *self.queued.lock().unwrap_or_else(|e| e.into_inner()));
        for a in queued {
            self.apply(a, pane);
        }
        let actions = match panel {
            PANEL => panel::panel_ui(ui, &self.shared, &active, now_secs()),
            CONFIRM => panel::confirm_ui(ui, &self.shared),
            _ => Vec::new(),
        };
        for a in actions {
            self.apply(a, pane);
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) -> bool {
        if let Some(a) = panel::settings_ui(ui, &self.shared) {
            match a {
                Action::EditSettings => self.edit_settings(),
                _ => {
                    if let Some(r) = &mut lock(&self.shared).result {
                        r.stale = true;
                        r.keep.clear();
                    }
                    self.save_settings();
                }
            }
        }
        true
    }
}

/// The pane a test or render fakes.
#[doc(hidden)]
pub fn pane_state(pane: PaneId, dir: Option<&Path>) -> PaneState {
    PaneState {
        remote: None,
        pane,
        path: dir.map_or_else(
            || VfsPath::new("/var/www"),
            |d| VfsPath::new(&d.to_string_lossy()),
        ),
        local_path: dir.map(Path::to_path_buf),
        protocol: if dir.is_some() {
            manta_core::Protocol::Local
        } else {
            manta_core::Protocol::Sftp
        },
        backend_label: if dir.is_some() {
            "Local".into()
        } else {
            "deploy@web-01 (SFTP)".into()
        },
        selection: Vec::new(),
        active: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_prefers_the_root_then_the_fullest_folder() {
        let f = |p: &str| FileInfo::new(p, p.trim_start_matches("/r/"), 5, 1);
        let files = vec![f("/r/a/x"), f("/r/b/y"), f("/r/b/z"), f("/r/w")];
        let root = Path::new("/r");
        assert_eq!(
            select_paths(&files, root, true),
            vec![PathBuf::from("/r/w")]
        );
        assert_eq!(
            select_paths(&files, root, false),
            vec![PathBuf::from("/r/b/y"), PathBuf::from("/r/b/z")]
        );
        let tie = vec![f("/r/long/x"), f("/r/s/y")];
        assert_eq!(
            select_paths(&tie, root, false),
            vec![PathBuf::from("/r/s/y")],
            "ties: the shorter folder"
        );
    }

    #[test]
    fn run_tool_reports_the_last_error_line() {
        let argv: Vec<String> = ["/bin/sh", "-c", "echo one >&2; echo two >&2; exit 3"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            run_tool(&argv, Duration::from_secs(5)),
            (false, Some("two".into()))
        );
        let ok: Vec<String> = ["/usr/bin/true"].map(String::from).to_vec();
        assert_eq!(run_tool(&ok, Duration::from_secs(5)), (true, None));
        let slow: Vec<String> = ["/bin/sleep", "5"].map(String::from).to_vec();
        assert!(!run_tool(&slow, Duration::from_millis(100)).0);
    }
}
