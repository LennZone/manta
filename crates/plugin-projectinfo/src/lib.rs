//! Built-in Project Info plugin. PUBLIC API CONTRACT (used by the app):
//! `ProjectInfoPlugin::new()`.
//!
//! Detects the project the active pane is in (Laravel, Composer,
//! npm/pnpm/yarn/bun, Docker Compose, Cargo, Python, Make, just, Task; see
//! [`detect`]) and turns its scripts into one-click commands that run
//! visibly in Manta's terminal (`HostRequest::RunInTerminal`, `cd` to
//! the project root first). Path-bar chip, "Project" side panel, a script
//! picker sheet, a confirmation sheet for destructive scripts, recent runs
//! (`recent.json`) and ⌥⌘R to re-run the last one. A port of the former
//! Python plugin (`project-info`).
//!
//! Threads: trait methods only update the shared state and post messages.
//! A worker thread detects projects (file IO), sends the chips, saves
//! `recent.json` and polls the shown project's manifests once a second.

pub mod detect;
pub mod i18n;
mod panel;
pub mod parsers;
pub mod value;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use manta_core::VfsPath;
use manta_plugin::i18n::Lang;
use manta_plugin::protocol::Block;
use manta_plugin::{
    Chip, CommandContext, CommandSpec, HostRequest, PaneId, PaneState, PanelPlacement, PanelSpec,
    Plugin, PluginEvent, PluginHandle, PluginInfo, ToastLevel, Tone,
};
use serde::{Deserialize, Serialize};

use crate::detect::Project;
use crate::i18n::tr;

pub const PLUGIN_ID: &str = "manta.projectinfo";
/// The side panel.
pub const PANEL: &str = "project";
/// "Run Project Script…"
pub const PICKER: &str = "picker";
/// "Run Script" (destructive scripts ask first).
pub const CONFIRM: &str = "confirm";
pub const RECENT_FILE: &str = "recent.json";
/// Recent runs kept per project.
pub const RECENT_MAX: usize = 5;
const POLL: Duration = Duration::from_secs(1);
/// Inside panel action strings (paths may contain ':').
pub const SEP: char = '\t';

/// One run of a script, as kept in `recent.json` (same format as the
/// Python plugin's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub section: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub danger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<f64>,
}

/// What was detected for a pane.
#[derive(Debug, Clone, Default)]
pub struct Det {
    pub folder: Option<String>,
    pub projects: Vec<Project>,
    pub watch: Vec<String>,
    pub sig: Vec<Option<(i128, u64)>>,
    /// A pane that isn't a local folder.
    pub remote: bool,
}

#[derive(Default)]
pub struct Inner {
    pub data_dir: PathBuf,
    pub home: String,
    pub panes: std::collections::BTreeMap<PaneId, PaneState>,
    pub active: Option<PaneId>,
    pub det: BTreeMap<PaneId, Det>,
    /// project root → runs, newest first.
    pub recent: BTreeMap<String, Vec<Run>>,
    /// A destructive run waiting for the confirm sheet.
    pub pending: Option<(Run, PaneId)>,
    /// Exactly one manifest is selected in the active pane.
    pub show_scripts: bool,
    chip_sig: BTreeMap<PaneId, String>,
}

impl Inner {
    fn active_pane(&self) -> PaneId {
        self.active.unwrap_or(PaneId::Left)
    }

    pub fn projects_of(&self, pane: PaneId) -> &[Project] {
        self.det
            .get(&pane)
            .map(|d| d.projects.as_slice())
            .unwrap_or(&[])
    }

    /// The detected project with this root (any pane).
    pub fn find_project(&self, root: &str) -> Option<&Project> {
        self.det
            .values()
            .flat_map(|d| d.projects.iter())
            .find(|p| p.root == root)
    }
}

pub struct SharedState {
    inner: Mutex<Inner>,
    handle: Mutex<Option<PluginHandle>>,
    lang: AtomicU8,
}

/// Resets the thread's language when dropped.
struct LangGuard(Option<Lang>);

impl Drop for LangGuard {
    fn drop(&mut self) {
        manta_plugin::i18n::set_thread_lang(self.0);
    }
}

impl SharedState {
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_lang(&self, l: Lang) {
        self.lang.store(l as u8, Ordering::Relaxed);
    }

    fn ui_lang(&self) -> LangGuard {
        let l = Lang::from_index(self.lang.load(Ordering::Relaxed));
        let prev = manta_plugin::i18n::thread_lang();
        manta_plugin::i18n::set_thread_lang(Some(l));
        LangGuard(prev)
    }

    fn send(&self, req: HostRequest) {
        if let Some(h) = self
            .handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.send(req);
        }
    }

    fn toast(&self, level: ToastLevel, text: String) {
        self.send(HostRequest::Toast { level, text });
    }

    fn repaint(&self) {
        if let Some(h) = self
            .handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.request_repaint();
        }
    }
}

enum Msg {
    LoadRecent,
    /// A pane changed; detect again if its folder changed (or `force`).
    Pane {
        state: PaneState,
        force: bool,
    },
    /// The active pane changed: detect it again.
    Active(PaneId),
    Selection,
    /// The language changed: detection holds translated texts.
    Locale,
    SaveRecent,
}

pub struct ProjectInfoPlugin {
    shared: Arc<SharedState>,
    tx: Option<Sender<Msg>>,
    home: Option<String>,
}

impl Default for ProjectInfoPlugin {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl ProjectInfoPlugin {
    pub fn new() -> Self {
        ProjectInfoPlugin {
            shared: Arc::new(SharedState {
                inner: Mutex::new(Inner::default()),
                handle: Mutex::new(None),
                lang: AtomicU8::new(0),
            }),
            tx: None,
            home: None,
        }
    }

    /// Treat `home` as the home folder (the walk up stops there).
    pub fn with_home(mut self, home: &str) -> Self {
        self.home = Some(home.to_string());
        self
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

    fn remember(&self, state: &PaneState) -> Option<PaneState> {
        let mut st = self.shared.lock();
        let old = st.panes.insert(state.pane, state.clone());
        if state.active {
            st.active = Some(state.pane);
        }
        old
    }

    fn pane_changed(&self, state: &PaneState, force: bool) {
        let old = self.remember(state);
        let changed = old.is_none_or(|o| o.local_path != state.local_path || o.path != state.path);
        if force || changed {
            self.post(Msg::Pane {
                state: state.clone(),
                force: true,
            });
        } else {
            self.post(Msg::Selection);
        }
    }

    fn open(&self, panel: &str, open: bool) {
        self.shared.send(HostRequest::SetPanelOpen {
            panel: panel.into(),
            pane: None,
            open,
        });
    }

    /// Run now, or ask first in the confirm sheet if it is destructive.
    fn request_run(&self, run: Run, pane: PaneId) {
        if run.danger.is_none() {
            self.start(run, pane);
            return;
        }
        self.shared.lock().pending = Some((run, pane));
        self.open(CONFIRM, true);
    }

    fn start(&self, run: Run, pane: PaneId) {
        self.shared.send(HostRequest::RunInTerminal {
            pane: Some(pane),
            command: run.command.clone(),
            cwd: Some(VfsPath::new(&run.cwd)),
        });
        remember_run(&mut self.shared.lock(), run);
        self.post(Msg::SaveRecent);
        self.shared.repaint();
    }

    fn run_key(&self, root: &str, key: &str, pane: PaneId) {
        let run = {
            let st = self.shared.lock();
            st.find_project(root)
                .and_then(|p| p.find_script(key).map(|sc| run_for(p, sc)))
        };
        match run {
            Some(r) => self.request_run(r, pane),
            None => self
                .shared
                .toast(ToastLevel::Warning, tr!("toast.script_gone").into()),
        }
    }

    fn run_recent(&self, root: &str, index: usize, pane: PaneId) {
        let run = self
            .shared
            .lock()
            .recent
            .get(root)
            .and_then(|l| l.get(index))
            .cloned();
        if let Some(mut r) = run {
            r.ts = None;
            self.request_run(r, pane);
        }
    }

    /// The newest run of the pane's project (or a project it is in).
    fn rerun_last(&self, pane: PaneId) {
        let (best, first_name) = {
            let st = self.shared.lock();
            let projects = st.projects_of(pane);
            let mut best: Option<Run> = None;
            for p in projects {
                if let Some(r) = st.recent.get(&p.root).and_then(|l| l.first()) {
                    if best
                        .as_ref()
                        .is_none_or(|b| r.ts.unwrap_or(0.0) > b.ts.unwrap_or(0.0))
                    {
                        best = Some(r.clone());
                    }
                }
            }
            (best, projects.first().map(|p| p.name.clone()))
        };
        match (best, first_name) {
            (Some(mut r), _) => {
                r.ts = None;
                self.request_run(r, pane);
            }
            (None, Some(name)) => self
                .shared
                .toast(ToastLevel::Info, tr!("toast.nothing_run", name = name)),
            (None, None) => self
                .shared
                .toast(ToastLevel::Info, tr!("toast.no_project").into()),
        }
    }
}

fn run_for(p: &Project, sc: &detect::Script) -> Run {
    Run {
        title: sc.name.clone(),
        section: p
            .sections
            .iter()
            .find(|s| s.id == sc.section)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| sc.section.clone()),
        command: sc.command.clone(),
        cwd: p.root.clone(),
        danger: sc.danger.clone(),
        ts: None,
    }
}

fn remember_run(st: &mut Inner, run: Run) {
    let list = st.recent.entry(run.cwd.clone()).or_default();
    list.retain(|r| r.command != run.command);
    list.insert(
        0,
        Run {
            ts: Some(now_ts()),
            ..run
        },
    );
    list.truncate(RECENT_MAX);
    if st.recent.len() > 200 {
        // Forget the projects used longest ago.
        let mut all: Vec<(String, Vec<Run>)> = std::mem::take(&mut st.recent).into_iter().collect();
        let newest = |l: &Vec<Run>| l.iter().filter_map(|r| r.ts).fold(0.0, f64::max);
        all.sort_by(|a, b| newest(&b.1).total_cmp(&newest(&a.1)));
        all.truncate(200);
        st.recent = all.into_iter().collect();
    }
}

impl Plugin for ProjectInfoPlugin {
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
        let home = self
            .home
            .clone()
            .or_else(|| {
                std::env::var("MANTA_PROJECT_INFO_HOME")
                    .ok()
                    .filter(|h| !h.is_empty())
            })
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_default();
        {
            let mut st = self.shared.lock();
            st.data_dir = handle.data_dir().clone();
            st.home = home;
            st.chip_sig.clear();
        }
        *self.shared.handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
        let (tx, rx) = mpsc::channel();
        let shared = self.shared.clone();
        let _ = std::thread::Builder::new()
            .name("pi-worker".into())
            .spawn(move || worker(&shared, &rx));
        let _ = tx.send(Msg::LoadRecent);
        self.tx = Some(tx);
    }

    fn disable(&mut self) {
        self.tx = None;
        *self.shared.handle.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let mut st = self.shared.lock();
        st.det.clear();
        st.pending = None;
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s) => self.pane_changed(s, false),
            PluginEvent::ListingRefreshed(s) => self.pane_changed(s, true),
            PluginEvent::SelectionChanged(s) => {
                self.remember(s);
                self.post(Msg::Selection);
            }
            PluginEvent::ActivePaneChanged(p) => {
                {
                    let mut st = self.shared.lock();
                    st.active = Some(*p);
                    for s in st.panes.values_mut() {
                        s.active = s.pane == *p;
                    }
                }
                self.post(Msg::Active(*p));
            }
            PluginEvent::LocaleChanged(code) => {
                self.shared.set_lang(Lang::from_code(code));
                self.post(Msg::Locale);
            }
            PluginEvent::PaneClosed(p) => {
                let mut st = self.shared.lock();
                st.panes.remove(p);
                st.det.remove(p);
                st.chip_sig.remove(p);
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let mut v = vec![
            CommandSpec {
                id: "run".into(),
                title: tr!("cmd.run").into(),
                shortcut: None,
                context: CommandContext::Palette,
            },
            CommandSpec {
                id: "panel".into(),
                title: tr!("cmd.panel").into(),
                shortcut: None,
                context: CommandContext::Palette,
            },
            CommandSpec {
                id: "rerun".into(),
                title: tr!("cmd.rerun").into(),
                shortcut: Some("⌥⌘R".into()),
                context: CommandContext::Palette,
            },
        ];
        if self.shared.lock().show_scripts {
            v.push(CommandSpec {
                id: "show-scripts".into(),
                title: tr!("cmd.show_scripts").into(),
                shortcut: None,
                context: CommandContext::FileContextMenu,
            });
        }
        v
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        self.pane_changed(pane, false);
        match id {
            "run" => self.open(PICKER, true),
            "panel" | "show-scripts" => self.open(PANEL, true),
            "toggle" => self.shared.send(HostRequest::TogglePanel {
                panel: PANEL.into(),
                pane: None,
            }),
            "rerun" => self.rerun_last(pane.pane),
            _ => {}
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![
            PanelSpec {
                id: PANEL.into(),
                title: tr!("panel.project").into(),
                placement: PanelPlacement::RightSidebar,
                default_open: false,
            },
            PanelSpec {
                id: PICKER.into(),
                title: tr!("panel.picker").into(),
                placement: PanelPlacement::Modal,
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
        match panel {
            PICKER => Some([540.0, 620.0]),
            CONFIRM => Some([440.0, 200.0]),
            _ => None,
        }
    }

    fn panel_blocks(&self, panel: &str, _pane: Option<PaneId>) -> Option<Arc<Vec<Block>>> {
        let st = self.shared.lock();
        let blocks = match panel {
            PANEL => panel::panel_blocks(&st),
            PICKER => panel::picker_blocks(&st),
            CONFIRM => panel::confirm_blocks(&st),
            _ => return None,
        };
        Some(Arc::new(blocks))
    }

    fn panel_action(&mut self, panel: &str, action: &str, state: &PaneState) {
        let pane = state.pane;
        if panel == CONFIRM {
            let pending = self.shared.lock().pending.take();
            self.open(CONFIRM, false);
            if let (Some((run, p)), "confirm") = (pending, action) {
                self.start(run, p);
            }
            return;
        }
        if panel == PICKER {
            self.open(PICKER, false);
            if action == "close" {
                return;
            }
        }
        let parts: Vec<&str> = action.split(SEP).collect();
        match parts.as_slice() {
            ["run", root, key] => self.run_key(root, key, pane),
            ["recent", root, i] => {
                if let Ok(i) = i.parse() {
                    self.run_recent(root, i, pane);
                }
            }
            ["open", kind, root] => open_thing(&self.shared, kind, root),
            ["goto", path] => self.shared.send(HostRequest::Navigate {
                pane,
                path: VfsPath::new(path),
            }),
            _ => {}
        }
    }
}

// --------------------------------------------------------------- worker

fn worker(shared: &Arc<SharedState>, rx: &Receiver<Msg>) {
    loop {
        let msg = match rx.recv_timeout(POLL) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let _lang = shared.ui_lang();
        let mut msgs: Vec<Msg> = msg.into_iter().collect();
        msgs.extend(rx.try_iter());
        if msgs.is_empty() {
            watch(shared);
            continue;
        }
        let mut detect_panes: BTreeMap<PaneId, bool> = BTreeMap::new();
        let mut selection = false;
        let mut save = false;
        for m in msgs {
            match m {
                Msg::LoadRecent => load_recent(shared),
                Msg::Pane { state, force } => *detect_panes.entry(state.pane).or_default() |= force,
                Msg::Active(p) => {
                    detect_panes.insert(p, true);
                }
                Msg::Selection => selection = true,
                Msg::Locale => {
                    let mut st = shared.lock();
                    st.chip_sig.clear();
                    detect_panes = st.panes.keys().map(|p| (*p, true)).collect();
                }
                Msg::SaveRecent => save = true,
            }
        }
        for (pane, _) in detect_panes.iter().filter(|(_, d)| **d) {
            detect(shared, *pane);
        }
        if selection || detect_panes.values().any(|d| *d) {
            update_show_scripts(shared);
        }
        if save {
            save_recent(shared);
        }
        shared.repaint();
    }
}

fn load_recent(shared: &SharedState) {
    let path = shared.lock().data_dir.join(RECENT_FILE);
    let mut recent = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(&text) {
            for (k, v) in m {
                if let serde_json::Value::Array(items) = v {
                    let runs: Vec<Run> = items
                        .into_iter()
                        .filter_map(|i| serde_json::from_value(i).ok())
                        .collect();
                    recent.insert(k, runs);
                }
            }
        }
    }
    shared.lock().recent = recent;
}

fn save_recent(shared: &SharedState) {
    let (dir, json) = {
        let st = shared.lock();
        (
            st.data_dir.clone(),
            serde_json::to_string_pretty(&st.recent),
        )
    };
    let Ok(json) = json else { return };
    let path = dir.join(RECENT_FILE);
    let tmp = dir.join("recent.json.tmp");
    let r = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&tmp, json))
        .and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = r {
        log::warn!("can't save recent.json: {e}");
    }
}

/// (Re)detect the projects of a pane's folder, then its chips.
fn detect(shared: &SharedState, pane: PaneId) {
    let (state, home) = {
        let st = shared.lock();
        (st.panes.get(&pane).cloned(), st.home.clone())
    };
    let folder = state
        .as_ref()
        .and_then(|s| s.local_path.as_ref())
        .map(|p| p.to_string_lossy().into_owned());
    let det = match &folder {
        None => Det {
            remote: state.is_some(),
            ..Det::default()
        },
        Some(f) => {
            let projects = detect::find_projects(f, None, &home);
            let watch = detect::watch_list(f, &projects, None, &home);
            let sig = detect::signature(&watch);
            Det {
                folder: Some(f.clone()),
                projects,
                watch,
                sig,
                remote: false,
            }
        }
    };
    shared.lock().det.insert(pane, det);
    update_chips(shared, pane);
}

/// Detect again when a manifest of the shown project changed.
fn watch(shared: &SharedState) {
    let (pane, watch, sig) = {
        let st = shared.lock();
        let pane = st.active_pane();
        match st.det.get(&pane) {
            Some(d) if d.folder.is_some() => (pane, d.watch.clone(), d.sig.clone()),
            _ => return,
        }
    };
    if detect::signature(&watch) != sig {
        log::info!("project files changed, detecting again");
        detect(shared, pane);
        update_show_scripts(shared);
        shared.repaint();
    }
}

fn update_chips(shared: &SharedState, pane: PaneId) {
    let (state, chips) = {
        let st = shared.lock();
        let Some(state) = st.panes.get(&pane).cloned() else {
            return;
        };
        let mut chips = Vec::new();
        if let Some(p) = st.det.get(&pane).and_then(|d| d.projects.first()) {
            let projects = st.projects_of(pane);
            let kinds = p
                .tags
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" · ");
            let mut tip = format!(
                "{}\n{}\n{kinds}",
                p.name,
                detect::short_path(&p.root, &st.home, 80)
            );
            if projects.len() > 1 {
                let names = projects[1..]
                    .iter()
                    .map(|o| o.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                tip.push('\n');
                tip.push_str(&tr!("chip.inside", names = names));
            }
            tip.push('\n');
            tip.push_str(tr!("chip.click"));
            chips.push(Chip {
                text: detect::chip_text(p),
                tone: Tone::Neutral,
                icon: Some(p.primary_icon.clone()),
                tooltip: Some(tip),
                command: Some("toggle".into()),
            });
        }
        (state, chips)
    };
    let sig = format!("{}\n{chips:?}", state.path);
    {
        let mut st = shared.lock();
        if st.chip_sig.get(&pane).map(String::as_str) == Some(sig.as_str()) {
            return;
        }
        st.chip_sig.insert(pane, sig);
    }
    shared.send(HostRequest::SetChips {
        pane,
        dir: state.path,
        chips,
    });
}

/// "Show Scripts" in the context menu when exactly one manifest is selected.
fn update_show_scripts(shared: &SharedState) {
    let sel = {
        let st = shared.lock();
        st.panes
            .get(&st.active_pane())
            .filter(|s| s.local_path.is_some() && s.selection.len() == 1)
            .map(|s| s.selection[0].as_str().to_string())
    };
    let show = sel.is_some_and(|p| {
        detect::manifests().any(|m| m == detect::basename(&p)) && std::path::Path::new(&p).is_file()
    });
    let changed = {
        let mut st = shared.lock();
        std::mem::replace(&mut st.show_scripts, show) != show
    };
    if changed {
        shared.send(HostRequest::ContributionsChanged);
    }
}

/// VS Code's `code` command, if installed.
fn find_code() -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    for d in path
        .split(':')
        .chain(["/opt/homebrew/bin", "/usr/local/bin"])
    {
        if d.is_empty() {
            continue;
        }
        let c = std::path::Path::new(d).join("code");
        if c.is_file() && is_executable(&c) {
            return Some(c.to_string_lossy().into_owned());
        }
    }
    let app = "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code";
    std::path::Path::new(app).is_file().then(|| app.to_string())
}

fn is_executable(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Start a GUI helper on a thread; a toast if it fails.
fn spawn(shared: &Arc<SharedState>, argv: Vec<String>, what: String) {
    let shared = shared.clone();
    let _ = std::thread::Builder::new()
        .name("pi-open".into())
        .spawn(move || {
            let _lang = shared.ui_lang();
            let out = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output();
            match out {
                Err(e) => shared.toast(
                    ToastLevel::Error,
                    tr!("toast.open_failed", what = what, error = e),
                ),
                Ok(o) if !o.status.success() => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    let text = match err.trim().lines().last() {
                        Some(last) => tr!("toast.open_failed", what = what, error = last),
                        None => tr!("toast.open_failed_short", what = what),
                    };
                    shared.toast(ToastLevel::Error, text);
                }
                Ok(_) => {}
            }
        });
}

fn open_thing(shared: &Arc<SharedState>, kind: &str, root: &str) {
    let mac = cfg!(target_os = "macos");
    let (shared2, kind, root) = (shared.clone(), kind.to_string(), root.to_string());
    // Finding `code` and checking .env read the disk: off the UI thread.
    let _ = std::thread::Builder::new()
        .name("pi-open".into())
        .spawn(move || {
            let _lang = shared2.ui_lang();
            match kind.as_str() {
                "code" => match find_code() {
                    Some(code) => spawn(&shared2, vec![code, root], "VS Code".into()),
                    None if mac => spawn(
                        &shared2,
                        vec![
                            "open".into(),
                            "-b".into(),
                            "com.microsoft.VSCode".into(),
                            root,
                        ],
                        tr!("what.vscode_missing").into(),
                    ),
                    None => shared2.toast(ToastLevel::Error, tr!("toast.no_code").into()),
                },
                "finder" => {
                    let opener = if mac { "open" } else { "xdg-open" };
                    spawn(
                        &shared2,
                        vec![opener.into(), root],
                        tr!("what.folder").into(),
                    );
                }
                "env" => {
                    let env = detect::pj(&root, ".env");
                    if !std::path::Path::new(&env).is_file() {
                        let home = shared2.lock().home.clone();
                        shared2.toast(
                            ToastLevel::Warning,
                            tr!("toast.no_env", path = detect::short_path(&root, &home, 48)),
                        );
                        return;
                    }
                    let argv = if mac {
                        vec!["open".into(), "-t".into(), env]
                    } else {
                        vec!["xdg-open".into(), env]
                    };
                    spawn(&shared2, argv, ".env".into());
                }
                _ => {}
            }
        });
}
