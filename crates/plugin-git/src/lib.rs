//! Built-in Git plugin. PUBLIC API CONTRACT (used by the app): `GitPlugin::new()`.
//!
//! - **File list:** status badges for the entries of every shown folder that
//!   lies in a git work tree (folders aggregate their changed descendants).
//! - **Path bar:** a branch chip (ahead/behind, change count, state tone).
//! - **Git panel** (right sidebar): changes with stage/unstage, diff preview,
//!   commit box and recent commits for the active pane's repository.
//!
//! Threads: all plugin trait methods only send messages and return. A
//! coordinator thread ([`engine`]) detects repositories and owns one worker
//! thread per repository ([`worker`]) with its file watcher.

pub mod engine;
pub mod git;
pub mod i18n;
mod kit;
pub mod model;
pub mod panel;
pub mod shared;
pub mod status;
pub mod worker;

#[cfg(test)]
mod render_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{
    CommandContext, CommandSpec, HostRequest, PaneId, PaneState, PanelPlacement, PanelSpec, Plugin,
    PluginEvent, PluginHandle, PluginInfo,
};

use engine::{Engine, EngineMsg};
use i18n::tr;
use model::{REFRESH_CMD, TOGGLE_PANEL_CMD};
use panel::{PanelAction, PanelState};
use shared::{lock, Shared, SharedRef};
use worker::{diff_slot, RepoAction};

pub const PANEL_ID: &str = "git";

pub struct GitPlugin {
    handle: Option<PluginHandle>,
    engine: Option<Engine>,
    shared: SharedRef,
    panel: PanelState,
    /// Last folder per pane that was sent to the engine.
    announced: HashMap<PaneId, VfsPath>,
}

impl GitPlugin {
    pub fn new() -> Self {
        GitPlugin {
            handle: None,
            engine: None,
            shared: Arc::new(Mutex::new(Shared::default())),
            panel: PanelState::default(),
            announced: HashMap::new(),
        }
    }

    fn announce(&mut self, state: &PaneState, refresh: bool) {
        self.announced.insert(state.pane, state.path.clone());
        self.send(EngineMsg::Pane {
            state: state.clone(),
            refresh,
        });
    }

    fn send(&self, m: EngineMsg) {
        if let Some(e) = &self.engine {
            e.send(m);
        }
    }

    /// Run a panel action (what a click in the panel does).
    #[doc(hidden)]
    pub fn apply(&mut self, a: PanelAction) {
        match a {
            PanelAction::Select(sel) => {
                let Some(sel) = sel else {
                    let old = lock(&self.shared).diff.take();
                    if let Some(old) = old {
                        self.send(EngineMsg::Diff {
                            root: old.root,
                            req: None,
                        });
                    }
                    return;
                };
                let req = sel.request();
                {
                    let mut sh = lock(&self.shared);
                    let keep = sh
                        .diff
                        .as_ref()
                        .is_some_and(|d| d.root == sel.root && d.req == req);
                    if !keep {
                        sh.diff = Some(diff_slot(sel.root.clone(), req.clone()));
                    }
                }
                self.send(EngineMsg::Diff {
                    root: sel.root,
                    req: Some(req),
                });
            }
            PanelAction::Stage(root, paths) => self.send(EngineMsg::Action {
                root,
                action: RepoAction::Stage(paths),
            }),
            PanelAction::Unstage(root, paths) => self.send(EngineMsg::Action {
                root,
                action: RepoAction::Unstage(paths),
            }),
            PanelAction::StageAll(root) => self.send(EngineMsg::Action {
                root,
                action: RepoAction::StageAll,
            }),
            PanelAction::Commit(root, msg) => self.send(EngineMsg::Action {
                root,
                action: RepoAction::Commit(msg),
            }),
            PanelAction::Refresh => self.send(EngineMsg::RefreshAll),
        }
    }

    /// Shared state (for tests and diagnostics).
    #[doc(hidden)]
    pub fn shared(&self) -> SharedRef {
        self.shared.clone()
    }

    #[doc(hidden)]
    pub fn panel_state(&self) -> &PanelState {
        &self.panel
    }

    #[doc(hidden)]
    pub fn panel_state_mut(&mut self) -> &mut PanelState {
        &mut self.panel
    }

    /// Select a file in the panel as if clicked (for tests and renders).
    #[doc(hidden)]
    pub fn select_for_test(&mut self, sel: Option<panel::Selection>) {
        self.panel.selection = sel.clone();
        self.apply(PanelAction::Select(sel));
    }
}

impl Default for GitPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for GitPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "manta.git".into(),
            name: "Git".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: tr!("git.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        if self.engine.is_some() {
            self.disable();
        }
        self.engine = Some(Engine::start(handle.clone(), self.shared.clone()));
        self.handle = Some(handle);
    }

    fn disable(&mut self) {
        if let Some(mut e) = self.engine.take() {
            e.send(EngineMsg::Shutdown);
            // The engine stops its workers and joins them. Wait briefly so
            // threads are gone in the common case, but never block the UI
            // on a slow git command: it finishes and exits on its own.
            if let Some(j) = e.join.take() {
                let deadline = Instant::now() + Duration::from_millis(150);
                while !j.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                }
                if j.is_finished() {
                    let _ = j.join();
                }
            }
        }
        *lock(&self.shared) = Shared::default();
        self.panel = PanelState::default();
        self.announced.clear();
        self.handle = None;
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s) => self.announce(s, false),
            PluginEvent::ListingRefreshed(s) => self.announce(s, true),
            PluginEvent::LocaleChanged(_) => {
                // Panel text is drawn per frame; decorations and chips were
                // built by the workers and are sent again translated.
                self.send(EngineMsg::Relocalize);
                if let Some(h) = &self.handle {
                    h.request_repaint();
                }
            }
            PluginEvent::ActivePaneChanged(_) => {
                if let Some(h) = &self.handle {
                    h.request_repaint();
                }
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                id: TOGGLE_PANEL_CMD.into(),
                title: tr!("git.cmd.toggle_panel").into(),
                shortcut: Some("⇧⌘B".into()),
                context: CommandContext::Palette,
            },
            CommandSpec {
                id: REFRESH_CMD.into(),
                title: tr!("git.cmd.refresh").into(),
                shortcut: None,
                context: CommandContext::Palette,
            },
        ]
    }

    fn run_command(&mut self, id: &str, _pane: &PaneState) {
        match id {
            TOGGLE_PANEL_CMD => {
                if let Some(h) = &self.handle {
                    h.send(HostRequest::TogglePanel {
                        panel: PANEL_ID.into(),
                        pane: None,
                    });
                }
            }
            REFRESH_CMD => self.send(EngineMsg::RefreshAll),
            _ => {}
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![PanelSpec {
            id: PANEL_ID.into(),
            title: tr!("git.panel.title").into(),
            placement: PanelPlacement::RightSidebar,
            default_open: false,
        }]
    }

    fn panel_ui(&mut self, panel: &str, pane: &PaneState, ui: &mut egui::Ui) {
        if panel != PANEL_ID {
            return;
        }
        // The panel may show a pane we were not told about (yet).
        if self.engine.is_some() && self.announced.get(&pane.pane) != Some(&pane.path) {
            self.announce(pane, false);
        }
        let actions = panel::panel_ui(&mut self.panel, &self.shared, pane, ui);
        for a in actions {
            self.apply(a);
        }
    }
}
