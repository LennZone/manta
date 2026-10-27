//! Built-in Folder Sizes plugin. PUBLIC API CONTRACT (used by the app):
//! `FolderSizePlugin::new()`.
//!
//! - **File list:** real sizes for the folders of local panes, sent with
//!   [`HostRequest::SetSizes`]. Measured in the background, shown while
//!   counting ("≥ 1.2 GB…"), cached per folder. See [`walker`] for what
//!   is counted (logical size; size on disk in the tooltip).
//! - **Disk Usage panel** (right sidebar, ⌥⌘U): a squarified treemap of the
//!   active pane's folder with zoom, breadcrumb and a list of the largest
//!   items.
//! - **Settings:** measure automatically, or only on demand with the
//!   "Calculate Size" context-menu command.
//!
//! Threads: trait methods only send messages to the coordinator thread
//! ([`engine`]), which owns a small worker pool.

pub mod engine;
pub mod i18n;
pub mod panel;
pub mod settings;
pub mod store;
pub mod treemap;
pub mod walker;

#[cfg(test)]
mod render_tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{
    CommandContext, CommandSpec, FileActionKind, HostRequest, PaneId, PaneState, PanelPlacement,
    PanelSpec, Plugin, PluginEvent, PluginHandle, PluginInfo, ToastLevel,
};

use engine::{Consumer, Engine, Msg, PanelData, PanelRef};
use i18n::tr;
use panel::{PanelAction, PanelUi};
use settings::Settings;

pub const PANEL_ID: &str = "space";
pub const TOGGLE_PANEL_CMD: &str = "show-disk-usage";
pub const CALCULATE_CMD: &str = "calculate-size";

/// The panel counts as hidden when it wasn't drawn for this long.
const PANEL_HIDDEN_AFTER: Duration = Duration::from_millis(1500);

pub struct FolderSizePlugin {
    handle: Option<PluginHandle>,
    engine: Option<Engine>,
    settings: Settings,
    panel_data: PanelRef,
    panel: PanelUi,
    /// The folder last sent to the engine for the panel.
    panel_dir: Option<PathBuf>,
    panel_drawn: Option<Instant>,
}

impl FolderSizePlugin {
    pub fn new() -> Self {
        FolderSizePlugin {
            handle: None,
            engine: None,
            settings: Settings::default(),
            panel_data: Arc::new(Mutex::new(PanelData::default())),
            panel: PanelUi::default(),
            panel_dir: None,
            panel_drawn: None,
        }
    }

    fn send(&self, m: Msg) {
        if let Some(e) = &self.engine {
            e.send(m);
        }
    }

    /// Shared panel data (tests and renders).
    #[doc(hidden)]
    pub fn panel_data(&self) -> PanelRef {
        self.panel_data.clone()
    }

    #[doc(hidden)]
    pub fn panel_ui_state(&mut self) -> &mut PanelUi {
        &mut self.panel
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn set_settings(&mut self, s: Settings) {
        if s == self.settings {
            return;
        }
        let auto_changed = s.auto != self.settings.auto;
        self.settings = s;
        if let Some(h) = &self.handle {
            s.save(h.data_dir());
            if auto_changed {
                h.send(HostRequest::ContributionsChanged);
            }
        }
        self.send(Msg::SetAuto(s.auto));
    }

    fn show_pane(&mut self, s: &PaneState, refresh: bool) {
        self.send(Msg::Show {
            consumer: Consumer::Pane(s.pane),
            dir: s.local_path.clone(),
            vfs_dir: Some(s.path.clone()),
            refresh,
        });
        // A hidden panel stops measuring.
        if self.panel_dir.is_some()
            && self
                .panel_drawn
                .is_none_or(|t| t.elapsed() > PANEL_HIDDEN_AFTER)
        {
            self.panel_dir = None;
            self.send(Msg::Show {
                consumer: Consumer::Panel,
                dir: None,
                vfs_dir: None,
                refresh: false,
            });
        } else if refresh {
            if let (Some(pd), Some(lp)) = (&self.panel_dir, &s.local_path) {
                if pd.starts_with(lp) || lp.starts_with(pd) {
                    let dir = Some(pd.clone());
                    self.send(Msg::Show {
                        consumer: Consumer::Panel,
                        dir,
                        vfs_dir: None,
                        refresh: true,
                    });
                }
            }
        }
    }

    fn apply(&mut self, a: PanelAction, pane: &PaneState) {
        match a {
            PanelAction::Show(dir) => {
                if self.engine.is_some() && self.panel_dir != dir {
                    self.panel_dir = dir.clone();
                    self.send(Msg::Show {
                        consumer: Consumer::Panel,
                        dir,
                        vfs_dir: None,
                        refresh: false,
                    });
                }
            }
            PanelAction::Navigate(path) => {
                if let Some(h) = &self.handle {
                    h.send(HostRequest::Navigate {
                        pane: pane.pane,
                        path: VfsPath::new(&path.to_string_lossy()),
                    });
                }
            }
        }
    }
}

impl Default for FolderSizePlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Local paths a finished file action changed.
fn touched_paths(a: &manta_plugin::FileAction) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let p = |x: &VfsPath| PathBuf::from(x.as_str());
    if a.src_local {
        for s in &a.sources {
            v.push(p(s));
        }
    }
    if let Some(d) = &a.destination {
        if a.dst_local {
            v.push(p(d));
        }
    }
    v
}

impl Plugin for FolderSizePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "manta.foldersize".into(),
            name: tr!("plugin.name").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: tr!("plugin.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        if self.engine.is_some() {
            self.disable();
        }
        self.settings = Settings::load(handle.data_dir());
        self.engine = Some(Engine::start(
            handle.clone(),
            self.panel_data.clone(),
            self.settings.auto,
            Some(store::location(handle.data_dir(), handle.cache_dir())),
        ));
        self.handle = Some(handle);
    }

    fn disable(&mut self) {
        if let Some(mut e) = self.engine.take() {
            e.send(Msg::Shutdown);
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
        *engine::lock(&self.panel_data) = PanelData::default();
        self.panel = PanelUi::default();
        self.panel_dir = None;
        self.handle = None;
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s) => self.show_pane(s, false),
            PluginEvent::ListingRefreshed(s) => self.show_pane(s, true),
            PluginEvent::FileAction(a) if a.is_finished() => {
                if matches!(
                    a.kind,
                    FileActionKind::Open | FileActionKind::Connect | FileActionKind::Disconnect
                ) {
                    return;
                }
                let paths = touched_paths(a);
                if !paths.is_empty() {
                    self.send(Msg::Invalidate(paths));
                }
            }
            PluginEvent::LocaleChanged(_) => {
                // Texts are looked up while drawing; the host asks for
                // commands and panels again.
                if let Some(h) = &self.handle {
                    h.request_repaint();
                }
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let mut v = vec![CommandSpec {
            id: TOGGLE_PANEL_CMD.into(),
            title: tr!("cmd.show").into(),
            shortcut: Some("⌥⌘U".into()),
            context: CommandContext::Palette,
        }];
        if !self.settings.auto {
            v.push(CommandSpec {
                id: CALCULATE_CMD.into(),
                title: tr!("cmd.calculate").into(),
                shortcut: None,
                context: CommandContext::FileContextMenu,
            });
        }
        v
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        match id {
            TOGGLE_PANEL_CMD => {
                if let Some(h) = &self.handle {
                    h.send(HostRequest::TogglePanel {
                        panel: PANEL_ID.into(),
                        pane: None,
                    });
                }
            }
            CALCULATE_CMD => {
                if pane.local_path.is_none() {
                    if let Some(h) = &self.handle {
                        h.send(HostRequest::Toast {
                            level: ToastLevel::Info,
                            text: tr!("toast.local_only").into(),
                        });
                    }
                    return;
                }
                let paths = pane
                    .selection
                    .iter()
                    .map(|p| PathBuf::from(p.as_str()))
                    .collect();
                self.send(Msg::Measure {
                    pane: pane.pane,
                    paths,
                });
            }
            _ => {}
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![PanelSpec {
            id: PANEL_ID.into(),
            title: tr!("panel.title").into(),
            placement: PanelPlacement::RightSidebar,
            default_open: false,
        }]
    }

    fn panel_ui(&mut self, panel: &str, pane: &PaneState, ui: &mut egui::Ui) {
        if panel != PANEL_ID {
            return;
        }
        self.panel_drawn = Some(Instant::now());
        let actions = panel::panel_ui(&mut self.panel, &self.panel_data, pane, ui);
        for a in actions {
            self.apply(a, pane);
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut s = self.settings;
        ui.radio_value(&mut s.auto, true, tr!("settings.auto"))
            .on_hover_text(tr!("settings.auto.tip"));
        ui.radio_value(&mut s.auto, false, tr!("settings.manual"))
            .on_hover_text(tr!("settings.manual.tip"));
        self.set_settings(s);
        true
    }
}

/// The pane a test or render fakes.
#[doc(hidden)]
pub fn pane_state(pane: PaneId, dir: Option<&std::path::Path>) -> PaneState {
    PaneState {
        remote: None,
        pane,
        path: dir.map_or_else(
            || VfsPath::new("/var/www"),
            |d| VfsPath::new(&d.to_string_lossy()),
        ),
        local_path: dir.map(|d| d.to_path_buf()),
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
