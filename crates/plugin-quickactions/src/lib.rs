//! Built-in Quick Actions plugin. PUBLIC API CONTRACT (used by the app):
//! `QuickActionsPlugin::new()`.
//!
//! Your own commands for the selected files, configured in `actions.json`
//! in the plugin's data folder (created with commented defaults on first
//! run; JSON with comments and trailing commas). Every action that applies
//! to the active pane's selection is offered in the file list's context
//! menu, in the command palette and in the "Quick Actions" side panel.
//! A port of the former Python plugin (`quick-actions`), with the same
//! file format, placeholders, quoting rules and behaviour.
//!
//! Threads: trait methods only update the shared state and post messages.
//! A worker thread loads actions.json (and reloads it when its mtime
//! changes), checks which actions apply to the selection (that needs
//! `stat`) and starts actions; jobs run on their own threads ([`jobs`]).

pub mod core;
pub mod i18n;
pub mod jobs;
mod panel;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use manta_plugin::i18n::Lang;
use manta_plugin::protocol::Block;
use manta_plugin::{
    CommandContext, CommandSpec, HostRequest, PaneId, PaneState, PanelPlacement, PanelSpec, Plugin,
    PluginEvent, PluginHandle, PluginInfo, ToastLevel,
};

use crate::core::{Action, Mode};
use crate::i18n::tr;
use crate::jobs::{Job, Status};

pub const PLUGIN_ID: &str = "manta.quickactions";
/// The side panel.
pub const PANEL: &str = "actions";
/// The confirmation sheet.
pub const CONFIRM: &str = "confirm";
pub const CONFIG_FILE: &str = "actions.json";
/// Items checked when filtering the menu.
pub const SELECTION_SCAN_LIMIT: usize = 1000;
/// How often actions.json is checked for changes.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// Which actions apply to the active pane's selection (computed on the
/// worker thread, read by the menu and the panel).
#[derive(Debug, Clone, Default)]
pub struct Eval {
    pub pane: Option<PaneId>,
    /// The selection this was computed for.
    pub selection: Vec<String>,
    /// Ids of the applicable actions, in actions.json order.
    pub ok: Vec<String>,
}

/// An action waiting for the confirmation sheet.
pub struct Pending {
    pub action: Action,
    pub state: PaneState,
    pub blocks: Arc<Vec<Block>>,
}

#[derive(Default)]
pub struct Inner {
    pub data_dir: PathBuf,
    pub panes: std::collections::BTreeMap<PaneId, PaneState>,
    pub active: Option<PaneId>,
    /// Panes, most recently active first ({dest} is the first one that
    /// isn't the active pane, like the app's copy target).
    pub mru: Vec<PaneId>,
    pub actions: Vec<Action>,
    /// Problems in actions.json.
    pub errors: Vec<String>,
    pub mtime: Option<SystemTime>,
    pub eval: Eval,
    /// Newest first.
    pub jobs: Vec<Job>,
    pub next_id: u64,
    /// The job whose output the panel shows.
    pub shown_job: Option<u64>,
    pub pending: Option<Pending>,
    /// The command list last offered (to notice changes).
    registered: Vec<(String, String, Option<String>)>,
}

/// State shared by the UI thread, the worker and the job threads.
pub struct SharedState {
    inner: Mutex<Inner>,
    handle: Mutex<Option<PluginHandle>>,
    /// The language of the UI thread, for texts made on other threads.
    lang: AtomicU8,
}

pub type Shared = SharedState;

/// Resets the thread's language when dropped.
pub struct LangGuard(Option<Lang>);

impl Drop for LangGuard {
    fn drop(&mut self) {
        manta_plugin::i18n::set_thread_lang(self.0);
    }
}

impl SharedState {
    fn new() -> Self {
        SharedState {
            inner: Mutex::new(Inner::default()),
            handle: Mutex::new(None),
            lang: AtomicU8::new(0),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run `f` on a job, if it still exists.
    pub fn with_job<R>(&self, id: u64, f: impl FnOnce(&mut Job) -> R) -> Option<R> {
        let mut st = self.lock();
        st.jobs.iter_mut().find(|j| j.id == id).map(f)
    }

    fn set_lang(&self, l: Lang) {
        self.lang.store(l as u8, Ordering::Relaxed);
    }

    pub fn lang(&self) -> Lang {
        Lang::from_index(self.lang.load(Ordering::Relaxed))
    }

    /// Use the UI's language on this (background) thread until the guard
    /// is dropped.
    pub fn ui_lang(&self) -> LangGuard {
        let prev = manta_plugin::i18n::thread_lang();
        manta_plugin::i18n::set_thread_lang(Some(self.lang()));
        LangGuard(prev)
    }

    pub fn send(&self, req: HostRequest) {
        if let Some(h) = self
            .handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.send(req);
        }
    }

    pub fn toast(&self, level: ToastLevel, text: String) {
        self.send(HostRequest::Toast { level, text });
    }

    pub fn repaint(&self) {
        if let Some(h) = self
            .handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.request_repaint();
        }
    }

    fn repaint_after(&self, d: Duration) {
        if let Some(h) = self
            .handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.egui_ctx().request_repaint_after(d);
        }
    }
}

/// Messages to the worker thread.
enum Msg {
    /// (Re)read actions.json; `announce` = toast the result.
    Load { announce: bool },
    /// Check which actions apply to the active pane's selection.
    Evaluate,
    /// A command or panel click: confirm first if the action asks for it.
    Request { id: String, state: PaneState },
    /// Confirmed: run it.
    Start {
        action: Box<Action>,
        state: PaneState,
    },
    /// Open actions.json in the text editor.
    Edit,
}

pub struct QuickActionsPlugin {
    shared: Arc<SharedState>,
    tx: Option<Sender<Msg>>,
}

impl Default for QuickActionsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickActionsPlugin {
    pub fn new() -> Self {
        QuickActionsPlugin {
            shared: Arc::new(SharedState::new()),
            tx: None,
        }
    }

    /// The shared state (tests and renders).
    #[doc(hidden)]
    pub fn shared(&self) -> Arc<SharedState> {
        self.shared.clone()
    }

    fn post(&self, m: Msg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(m);
        }
    }

    fn remember(&self, state: &PaneState) {
        let mut st = self.shared.lock();
        st.panes.insert(state.pane, state.clone());
        if !st.mru.contains(&state.pane) {
            st.mru.push(state.pane);
        }
        if state.active {
            st.active = Some(state.pane);
            touch(&mut st.mru, state.pane);
        }
    }
}

/// Move `p` to the front of the most-recently-active list.
fn touch(mru: &mut Vec<PaneId>, p: PaneId) {
    mru.retain(|x| *x != p);
    mru.insert(0, p);
}

/// The pane {dest} refers to: the most recently active pane other than
/// `p` (with two panes simply the other one).
fn other(st: &Inner, p: PaneId) -> Option<PaneId> {
    st.mru
        .iter()
        .copied()
        .find(|x| *x != p && st.panes.contains_key(x))
}

impl Plugin for QuickActionsPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: tr!("plugin.name").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: tr!("plugin.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        self.shared.set_lang(manta_plugin::i18n::lang());
        {
            let mut st = self.shared.lock();
            st.data_dir = handle.data_dir().clone();
            st.registered.clear();
        }
        *self.shared.handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
        let (tx, rx) = mpsc::channel();
        let shared = self.shared.clone();
        let _ = std::thread::Builder::new()
            .name("qa-worker".into())
            .spawn(move || worker(&shared, &rx));
        let _ = tx.send(Msg::Load { announce: false });
        self.tx = Some(tx);
    }

    fn disable(&mut self) {
        self.tx = None; // the worker ends
        let st = self.shared.lock();
        for j in st.jobs.iter().filter(|j| j.status == Status::Running) {
            j.cancel();
        }
        drop(st);
        *self.shared.handle.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s)
            | PluginEvent::ListingRefreshed(s)
            | PluginEvent::SelectionChanged(s) => {
                self.remember(s);
                self.post(Msg::Evaluate);
            }
            PluginEvent::ActivePaneChanged(p) => {
                let mut st = self.shared.lock();
                st.active = Some(*p);
                touch(&mut st.mru, *p);
                for s in st.panes.values_mut() {
                    s.active = s.pane == *p;
                }
                drop(st);
                self.post(Msg::Evaluate);
            }
            PluginEvent::PaneClosed(p) => {
                let mut st = self.shared.lock();
                st.panes.remove(p);
                st.mru.retain(|x| x != p);
                drop(st);
                self.post(Msg::Evaluate);
            }
            PluginEvent::LocaleChanged(code) => {
                self.shared.set_lang(Lang::from_code(code));
                // Send the (translated) commands again.
                self.shared.lock().registered.clear();
                // Problems in actions.json are reported in the new language.
                self.post(Msg::Load { announce: false });
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let mut v = vec![
            CommandSpec {
                id: "edit".into(),
                title: tr!("cmd.edit").into(),
                shortcut: None,
                context: CommandContext::Palette,
            },
            CommandSpec {
                id: "reload".into(),
                title: tr!("cmd.reload").into(),
                shortcut: None,
                context: CommandContext::Palette,
            },
            CommandSpec {
                id: "panel".into(),
                title: tr!("cmd.panel").into(),
                shortcut: None,
                context: CommandContext::Palette,
            },
        ];
        let st = self.shared.lock();
        for a in st.actions.iter().filter(|a| st.eval.ok.contains(&a.id)) {
            v.push(CommandSpec {
                id: format!("run.{}", a.id),
                title: core::title(a),
                shortcut: a.shortcut.clone(),
                context: CommandContext::FileContextMenu,
            });
        }
        v
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        self.remember(pane);
        match id {
            "edit" => self.post(Msg::Edit),
            "reload" => self.post(Msg::Load { announce: true }),
            "panel" => self.shared.send(HostRequest::SetPanelOpen {
                panel: PANEL.into(),
                pane: None,
                open: true,
            }),
            _ => {
                if let Some(a) = id.strip_prefix("run.") {
                    self.post(Msg::Request {
                        id: a.to_string(),
                        state: pane.clone(),
                    });
                } else if let Some(n) = id.strip_prefix("cancel-").and_then(|n| n.parse().ok()) {
                    jobs::cancel_job(&self.shared, n);
                }
            }
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![
            PanelSpec {
                id: PANEL.into(),
                title: tr!("panel.actions").into(),
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
        (panel == CONFIRM).then_some([420.0, 210.0])
    }

    fn panel_blocks(&self, panel: &str, _pane: Option<PaneId>) -> Option<Arc<Vec<Block>>> {
        match panel {
            PANEL => {
                let st = self.shared.lock();
                let blocks = panel::panel_blocks(&st);
                let busy = st.jobs.iter().any(|j| j.status.busy());
                drop(st);
                if busy {
                    // Keep "running · 3 s" fresh.
                    self.shared.repaint_after(Duration::from_millis(500));
                }
                Some(Arc::new(blocks))
            }
            CONFIRM => Some(
                self.shared
                    .lock()
                    .pending
                    .as_ref()
                    .map(|p| p.blocks.clone())
                    .unwrap_or_default(),
            ),
            _ => None,
        }
    }

    fn panel_action(&mut self, panel: &str, action: &str, pane: &PaneState) {
        if panel == CONFIRM {
            let pending = self.shared.lock().pending.take();
            self.shared.send(HostRequest::SetPanelOpen {
                panel: CONFIRM.into(),
                pane: None,
                open: false,
            });
            if let (Some(p), "confirm:run") = (pending, action) {
                self.post(Msg::Start {
                    action: Box::new(p.action),
                    state: p.state,
                });
            }
            return;
        }
        if let Some(id) = action.strip_prefix("run:") {
            self.post(Msg::Request {
                id: id.to_string(),
                state: pane.clone(),
            });
        } else if let Some(n) = action.strip_prefix("cancel:").and_then(|n| n.parse().ok()) {
            jobs::cancel_job(&self.shared, n);
        } else if let Some(n) = action.strip_prefix("show:").and_then(|n| n.parse().ok()) {
            self.shared.lock().shown_job = Some(n);
            self.shared.repaint();
        } else if action == "edit" {
            self.post(Msg::Edit);
        } else if action == "reload" {
            self.post(Msg::Load { announce: true });
        }
    }
}

// --------------------------------------------------------------- worker

fn worker(shared: &Arc<SharedState>, rx: &Receiver<Msg>) {
    loop {
        let first = match rx.recv_timeout(WATCH_EVERY) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let _lang = shared.ui_lang();
        let Some(first) = first else {
            watch_config(shared);
            continue;
        };
        // Handle what queued up; selection changes coalesce into one check.
        let mut msgs = vec![first];
        msgs.extend(rx.try_iter());
        let mut evaluate = false;
        for m in msgs {
            match m {
                Msg::Evaluate => evaluate = true,
                Msg::Load { announce } => {
                    load_config(shared, announce);
                    evaluate = true;
                }
                other => {
                    if evaluate {
                        evaluate_selection(shared);
                        evaluate = false;
                    }
                    match other {
                        Msg::Request { id, state } => request_action(shared, &id, &state),
                        Msg::Start { action, state } => start_action(shared, &action, &state),
                        Msg::Edit => edit_config(shared),
                        _ => {}
                    }
                }
            }
        }
        if evaluate {
            evaluate_selection(shared);
        }
    }
}

fn config_path(shared: &SharedState) -> PathBuf {
    shared.lock().data_dir.join(CONFIG_FILE)
}

/// Write the default actions.json if there is none.
fn ensure_config(path: &Path) {
    if !path.exists() {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::write(path, core::DEFAULT_ACTIONS) {
            Ok(()) => log::info!("created {} with the default actions", path.display()),
            Err(e) => log::warn!("can't create {}: {e}", path.display()),
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// (Re)read actions.json; keeps the old actions if the file is broken.
fn load_config(shared: &SharedState, announce: bool) {
    let path = config_path(shared);
    ensure_config(&path);
    // The time before reading: a write during the read is noticed later.
    let stamp = mtime(&path);
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| core::ConfigError(e.to_string()))
        .and_then(|t| core::parse_actions(&t));
    match parsed {
        Err(e) => {
            log::warn!("actions.json: {e}");
            {
                let mut st = shared.lock();
                st.errors = vec![e.0.clone()];
                st.mtime = stamp;
            }
            if announce {
                shared.toast(ToastLevel::Error, tr!("load.error", error = e));
            }
        }
        Ok((actions, errors)) => {
            for e in &errors {
                log::warn!("actions.json: {e}");
            }
            let n = actions.len();
            {
                let mut st = shared.lock();
                st.actions = actions;
                st.errors = errors.clone();
                st.mtime = stamp;
            }
            if announce {
                if let Some(first) = errors.first() {
                    shared.toast(
                        ToastLevel::Warning,
                        tr!(
                            "load.skipped",
                            count = n,
                            skipped = errors.len(),
                            error = first
                        ),
                    );
                } else {
                    shared.toast(ToastLevel::Success, tr!("load.ok", count = n));
                }
            }
        }
    }
    shared.repaint();
}

/// Reload actions.json when it changed (checked every second).
fn watch_config(shared: &SharedState) {
    let path = config_path(shared);
    if shared.lock().data_dir.as_os_str().is_empty() {
        return;
    }
    let now = mtime(&path);
    if now != shared.lock().mtime {
        log::info!("actions.json changed, reloading");
        load_config(shared, true);
        evaluate_selection(shared);
    }
}

/// [(path, is_dir)] of a pane's selection (local panes only).
fn pane_items(state: &PaneState, limit: Option<usize>) -> Vec<(String, bool)> {
    let n = limit.unwrap_or(usize::MAX);
    state
        .selection
        .iter()
        .take(n)
        .map(|p| (p.as_str().to_string(), Path::new(p.as_str()).is_dir()))
        .collect()
}

/// The other pane's local folder ({dest}).
fn dest_for(st: &Inner, pane: PaneId) -> Option<String> {
    st.panes
        .get(&other(st, pane)?)?
        .local_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Which actions apply to the active pane's selection; tells the host when
/// the menu changes.
fn evaluate_selection(shared: &SharedState) {
    let (state, actions, dest) = {
        let st = shared.lock();
        let state = st.active.and_then(|a| st.panes.get(&a).cloned());
        let dest = state.as_ref().and_then(|s| dest_for(&st, s.pane));
        (state, st.actions.clone(), dest)
    };
    let mut eval = Eval::default();
    if let Some(s) = state.as_ref().filter(|s| s.local_path.is_some()) {
        let items = pane_items(s, Some(SELECTION_SCAN_LIMIT));
        eval.pane = Some(s.pane);
        eval.selection = s.selection.iter().map(|p| p.as_str().to_string()).collect();
        eval.ok = actions
            .iter()
            .filter(|a| core::applies(a, &items, dest.as_deref()).is_ok())
            .map(|a| a.id.clone())
            .collect();
    }
    let changed = {
        let mut st = shared.lock();
        st.eval = eval;
        let reg: Vec<(String, String, Option<String>)> = st
            .actions
            .iter()
            .filter(|a| st.eval.ok.contains(&a.id))
            .map(|a| (a.id.clone(), a.title.clone(), a.shortcut.clone()))
            .collect();
        let changed = reg != st.registered;
        st.registered = reg;
        changed
    };
    if changed {
        shared.send(HostRequest::ContributionsChanged);
    }
    shared.repaint();
}

fn request_action(shared: &Arc<SharedState>, id: &str, state: &PaneState) {
    let action = shared.lock().actions.iter().find(|a| a.id == id).cloned();
    let Some(action) = action else {
        shared.toast(ToastLevel::Warning, tr!("toast.gone").to_string());
        return;
    };
    if action.confirm.is_some() && state.local_path.is_some() {
        let items = pane_items(state, None);
        let dest = dest_for(&shared.lock(), state.pane);
        if let Err(why) = core::applies(&action, &items, dest.as_deref()) {
            shared.toast(
                ToastLevel::Warning,
                tr!(
                    "toast.not_available",
                    title = core::title(&action),
                    why = why
                ),
            );
            return;
        }
        let blocks = Arc::new(panel::confirm_blocks(&action, &items));
        shared.lock().pending = Some(Pending {
            action,
            state: state.clone(),
            blocks,
        });
        shared.send(HostRequest::SetPanelOpen {
            panel: CONFIRM.into(),
            pane: None,
            open: true,
        });
        shared.repaint();
        return;
    }
    start_action(shared, &action, state);
}

/// Run `action` on a pane's current selection (after the checks).
fn start_action(shared: &Arc<SharedState>, action: &Action, state: &PaneState) {
    let pane = state.pane;
    let Some(folder) = state
        .local_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
    else {
        shared.toast(ToastLevel::Warning, tr!("toast.local_only").to_string());
        return;
    };
    let items = pane_items(state, None);
    let dest = dest_for(&shared.lock(), pane);
    if let Err(why) = core::applies(action, &items, dest.as_deref()) {
        shared.toast(
            ToastLevel::Warning,
            tr!(
                "toast.not_available",
                title = core::title(action),
                why = why
            ),
        );
        return;
    }
    let commands = match core::plan(action, &items, &folder, dest.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            shared.toast(
                ToastLevel::Error,
                tr!("toast.plan_error", title = core::title(action), error = e),
            );
            return;
        }
    };
    let id = {
        let mut st = shared.lock();
        st.next_id += 1;
        st.next_id
    };
    if action.mode == Mode::Terminal {
        let line = commands
            .iter()
            .map(|(c, _)| c.as_str())
            .collect::<Vec<_>>()
            .join(" && ");
        shared.send(HostRequest::RunInTerminal {
            pane: Some(pane),
            command: line.clone(),
            cwd: Some(manta_core::VfsPath::new(&folder)),
        });
        let mut job = Job::new(id, action.clone(), pane, vec![(line, folder)], items.len());
        job.status = Status::Terminal;
        job.ended = Some(job.started);
        jobs::add_job(shared, job);
        shared.repaint();
        return;
    }
    let mut job = Job::new(id, action.clone(), pane, commands, items.len());
    job.refresh_panes = vec![pane];
    if core::uses(&action.command, &["dest"]) {
        if let Some(dest) = other(&shared.lock(), pane) {
            job.refresh_panes.push(dest);
        }
    }
    jobs::add_job(shared, job);
    jobs::schedule(shared);
    shared.repaint();
    jobs::progress_toast_later(shared.clone(), id);
}

fn edit_config(shared: &SharedState) {
    let path = config_path(shared);
    ensure_config(&path);
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg("-t").arg(&path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&path);
        c
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            let _ = std::thread::Builder::new()
                .name("qa-open".into())
                .spawn(move || child.wait());
        }
        Err(e) => shared.toast(
            ToastLevel::Error,
            tr!("toast.open_error", path = path.display(), error = e),
        ),
    }
}
