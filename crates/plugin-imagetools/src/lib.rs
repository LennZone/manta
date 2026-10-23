//! Built-in Image Tools plugin. PUBLIC API CONTRACT (used by the app):
//! `ImageToolsPlugin::new()`.
//!
//! - **Badges** in the file list of local folders: "lo" (longest edge under
//!   800 px; icons excepted) and "XL" (a web image over 2 MB), and a
//!   "1920×1080 · sRGB · 340 KB" tooltip on every image. At most 2000
//!   images per folder, read in the background with an mtime cache.
//! - **Image panel** (right sidebar, "Show Image Panel"): preview, size,
//!   aspect ratio, format, colour profile, DPI, alpha, animation, the
//!   camera's EXIF and the GPS location (Open in Maps, Strip Metadata…);
//!   a summary for several images.
//! - **Context commands** (an "Image Tools" submenu while images are
//!   selected): resize, convert, rotate/flip, strip metadata, optimize for
//!   web, @2x/@1x sets (each through the "Image Tools" sheet) and Copy as
//!   Data URI.
//!
//! Header parsing is in [`headers`] (container walk + `kamadak-exif`),
//! metadata and formatting in [`meta`], naming/commands/stripping in
//! [`ops`], threads in [`engine`]. Operations run `sips` (and `cwebp`,
//! `exiftool` when installed) as child processes with plain argv; the
//! metadata stripper is built in and lossless.
//!
//! Replaces the former Python plugin `image-tools`; the host copies its
//! `options.json` and preview cache over on first start
//! (`RETIRED_EXTERNAL`).

pub mod engine;
pub mod headers;
pub mod i18n;
pub mod kit;
pub mod meta;
pub mod ops;
pub mod panel;
pub mod preview;

#[cfg(test)]
mod render_tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use manta_core::VfsPath;
use manta_plugin::{
    CommandContext, CommandSpec, HostRequest, PaneId, PaneState, PanelPlacement, PanelSpec, Plugin,
    PluginEvent, PluginHandle, PluginInfo, ToastLevel,
};

use engine::{lock, Msg, Shared, SharedRef, MODAL};
use i18n::tr;
pub use panel::{PanelAction, SheetAction};

pub const ID: &str = "manta.imagetools";
/// The id of the Python plugin this one replaces.
pub const OLD_ID: &str = "image-tools";
pub const PANEL: &str = "image";
pub use engine::MODAL as SHEET;

pub struct ImageToolsPlugin {
    handle: Option<PluginHandle>,
    shared: SharedRef,
    tx: Option<mpsc::Sender<Msg>>,
    engine: Option<JoinHandle<()>>,
    panes: std::collections::BTreeMap<PaneId, PaneState>,
    active: PaneId,
    /// Scan generation per pane (a newer scan makes older ones stop).
    scan_gen: Mutex<std::collections::BTreeMap<PaneId, Arc<AtomicU64>>>,
    /// Clicks queued from outside the UI (tests), applied on the next draw.
    queued: Arc<Mutex<Vec<Click>>>,
}

/// A click in the panel or the sheet (see [`ImageToolsPlugin::click_queue`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    Panel(PanelAction),
    Sheet(SheetAction),
}

impl Default for ImageToolsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageToolsPlugin {
    pub fn new() -> Self {
        ImageToolsPlugin {
            handle: None,
            shared: Arc::new(Mutex::new(Shared::new(
                std::env::temp_dir().join("manta-imagetools"),
            ))),
            tx: None,
            engine: None,
            panes: Default::default(),
            active: PaneId::Left,
            scan_gen: Default::default(),
            queued: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queue clicks; they are applied when a panel is drawn next (tests
    /// drive the plugin through the host this way).
    #[doc(hidden)]
    pub fn click_queue(&self) -> Arc<Mutex<Vec<Click>>> {
        self.queued.clone()
    }

    fn panel_action(&mut self, a: PanelAction, state: &PaneState) {
        match a {
            PanelAction::Op(op) => self.open_modal(&op, state, None),
            PanelAction::DataUri => self.copy_data_uri(state),
            PanelAction::Maps => self.open_in_maps(state),
            PanelAction::Cancel => self.cancel_job(),
        }
    }

    fn sheet_action(&mut self, a: SheetAction) {
        match a {
            SheetAction::Run => self.run_modal(),
            SheetAction::Close => self.close_modal(),
        }
    }

    /// Shared state (tests and renders).
    #[doc(hidden)]
    pub fn shared(&self) -> SharedRef {
        self.shared.clone()
    }

    fn active_state(&self) -> Option<PaneState> {
        self.panes.get(&self.active).cloned()
    }

    fn post_panel(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Panel {
                state: self.active_state(),
            });
        }
    }

    /// Decorate the images of the folder `s` shows (in the background).
    fn start_scan(&self, s: &PaneState) {
        let gen_ref = self
            .scan_gen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(s.pane)
            .or_default()
            .clone();
        let gen = gen_ref.fetch_add(1, Ordering::Relaxed) + 1;
        let (Some(h), Some(folder)) = (self.handle.clone(), s.local_path.clone()) else {
            return;
        };
        let (shared, pane, dir) = (self.shared.clone(), s.pane, s.path.clone());
        let lang = manta_plugin::i18n::lang();
        let _ = std::thread::Builder::new()
            .name("it-scan".into())
            .spawn(move || {
                manta_plugin::i18n::set_thread_lang(Some(lang));
                wait_for_tools(&shared);
                engine::scan_folder(&h, &shared, pane, dir, folder, gen, &gen_ref);
            });
    }

    /// Open the sheet for `op` over the selected images (checked in the
    /// background; a toast says why not).
    fn open_modal(&self, op: &str, state: &PaneState, transform: Option<&str>) {
        let Some(h) = self.handle.clone() else { return };
        let (shared, state, op, transform) = (
            self.shared.clone(),
            state.clone(),
            op.to_string(),
            transform.map(String::from),
        );
        let lang = manta_plugin::i18n::lang();
        std::thread::spawn(move || {
            manta_plugin::i18n::set_thread_lang(Some(lang));
            wait_for_tools(&shared);
            let toast = |level, text: &str| {
                h.send(HostRequest::Toast {
                    level,
                    text: text.into(),
                })
            };
            if state.local_path.is_none() {
                return toast(ToastLevel::Warning, tr!("toast.local_only"));
            }
            let images = engine::selected_images(Some(&state), engine::SCAN_LIMIT);
            if images.is_empty() {
                return toast(ToastLevel::Warning, tr!("toast.select_first"));
            }
            let mut opts = {
                let st = lock(&shared);
                if !st.tools.has("sips") && op != "strip" {
                    return toast(ToastLevel::Error, tr!("toast.needs_sips"));
                }
                if st.busy() {
                    return toast(ToastLevel::Warning, tr!("toast.busy"));
                }
                st.remembered(&op)
            };
            if let Some(t) = transform {
                opts.transform = t;
            }
            let modal = engine::prepare_modal(&shared, &op, images, opts);
            lock(&shared).modal = Some(modal);
            h.send(HostRequest::SetPanelOpen {
                panel: MODAL.into(),
                pane: None,
                open: true,
            });
        });
    }

    fn close_modal(&self) {
        lock(&self.shared).modal = None;
        if let Some(h) = &self.handle {
            h.send(HostRequest::SetPanelOpen {
                panel: MODAL.into(),
                pane: None,
                open: false,
            });
        }
    }

    /// The sheet's primary button: confirm overwriting first, then run.
    fn run_modal(&self) {
        let Some(h) = self.handle.clone() else { return };
        let mut st = lock(&self.shared);
        let Some(m) = st.modal.as_mut() else { return };
        if m.opts.output == "overwrite" && !m.confirm && m.op != "retina" {
            m.confirm = true;
            return;
        }
        let m = st.modal.take().expect("modal");
        let mut opts = m.opts.clone();
        opts.op = m.op.clone();
        st.options.insert(m.op.clone(), opts);
        drop(st);
        self.close_modal();
        let shared = self.shared.clone();
        let lang = manta_plugin::i18n::lang();
        let _ = std::thread::Builder::new()
            .name("it-job".into())
            .spawn(move || {
                engine::save_options(&shared);
                engine::run_job(&h, &shared, m, lang);
                h.request_repaint();
            });
        self.post_panel();
    }

    fn cancel_job(&self) {
        if let Some(j) = &lock(&self.shared).job {
            j.cancelled.store(true, Ordering::Relaxed);
        }
    }

    fn copy_data_uri(&self, state: &PaneState) {
        let Some(h) = self.handle.clone() else { return };
        let (shared, state) = (self.shared.clone(), state.clone());
        let lang = manta_plugin::i18n::lang();
        std::thread::spawn(move || {
            manta_plugin::i18n::set_thread_lang(Some(lang));
            wait_for_tools(&shared);
            let images = engine::selected_images(Some(&state), engine::SCAN_LIMIT);
            if images.len() != 1 {
                return h.send(HostRequest::Toast {
                    level: ToastLevel::Warning,
                    text: tr!("toast.select_one").into(),
                });
            }
            let tools = lock(&shared).tools.clone();
            engine::copy_data_uri(&h, &tools, &images[0]);
        });
    }

    fn open_in_maps(&self, state: &PaneState) {
        let Some(h) = self.handle.clone() else { return };
        let (shared, state) = (self.shared.clone(), state.clone());
        let lang = manta_plugin::i18n::lang();
        std::thread::spawn(move || {
            manta_plugin::i18n::set_thread_lang(Some(lang));
            let images = engine::selected_images(Some(&state), 2);
            let [image] = images.as_slice() else { return };
            match engine::details(&shared, image).and_then(|d| d.exif.gps) {
                Some((lat, lon)) => {
                    let tools = lock(&shared).tools.clone();
                    engine::run(
                        &tools,
                        &["open".into(), meta::maps_url(lat, lon)],
                        Duration::from_secs(10),
                        None,
                    );
                }
                None => h.send(HostRequest::Toast {
                    level: ToastLevel::Info,
                    text: tr!("toast.no_gps").into(),
                }),
            }
        });
    }

    fn remember(&mut self, s: &PaneState) {
        if s.active {
            self.active = s.pane;
        }
        self.panes.insert(s.pane, s.clone());
    }
}

/// Wait (briefly) until the tools are known, so HEIC/TIFF files aren't
/// cached as unreadable before `sips` is found.
fn wait_for_tools(shared: &SharedRef) {
    for _ in 0..250 {
        if lock(shared).tools_ready {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Plugin for ImageToolsPlugin {
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
        if self.tx.is_some() {
            self.disable();
        }
        *lock(&self.shared) = Shared::new(handle.data_dir().clone());
        let (tx, rx) = mpsc::channel();
        let (h, shared) = (handle.clone(), self.shared.clone());
        self.engine = std::thread::Builder::new()
            .name("it-engine".into())
            .spawn(move || engine::engine_loop(h, shared, rx))
            .ok();
        self.tx = Some(tx);
        self.handle = Some(handle);
    }

    fn disable(&mut self) {
        self.cancel_job();
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Msg::Shutdown);
        }
        // The engine thread finishes its current rebuild by itself.
        self.engine = None;
        for g in self
            .scan_gen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            g.fetch_add(1, Ordering::Relaxed);
        }
        self.handle = None;
        self.panes.clear();
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s) | PluginEvent::ListingRefreshed(s) => {
                self.remember(s);
                self.start_scan(s);
                self.post_panel();
            }
            PluginEvent::SelectionChanged(s) => {
                self.remember(s);
                self.post_panel();
            }
            PluginEvent::ActivePaneChanged(p) => {
                self.active = *p;
                for s in self.panes.values_mut() {
                    s.active = s.pane == *p;
                }
                self.post_panel();
            }
            PluginEvent::LocaleChanged(_) => {
                // Decoration tooltips are text: decorate again.
                for s in self.panes.clone().values() {
                    self.start_scan(s);
                }
                self.post_panel();
            }
            PluginEvent::PaneClosed(p) => {
                self.panes.remove(p);
                // A scan still running for it stops.
                if let Some(g) = self
                    .scan_gen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(p)
                {
                    g.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let cmd = |id: &str, title: &str, context| CommandSpec {
            id: id.into(),
            title: title.into(),
            shortcut: None,
            context,
        };
        let mut v = vec![cmd("panel", tr!("cmd.panel"), CommandContext::Palette)];
        let st = lock(&self.shared);
        if !st.images.is_empty() {
            let ctx = CommandContext::FileContextMenu;
            v.extend([
                cmd("resize", tr!("cmd.resize"), ctx),
                cmd("convert", tr!("cmd.convert"), ctx),
                cmd("rotate-left", tr!("cmd.rotate_left"), ctx),
                cmd("rotate-right", tr!("cmd.rotate_right"), ctx),
                cmd("flip", tr!("cmd.flip_h"), ctx),
                cmd("strip", tr!("cmd.strip"), ctx),
                cmd("optimize", tr!("cmd.optimize"), ctx),
                cmd("retina", tr!("cmd.retina"), ctx),
            ]);
            if st.datauri {
                v.push(cmd("datauri", tr!("cmd.datauri"), ctx));
            }
        }
        v
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        self.remember(pane);
        match id {
            "panel" => {
                if let Some(h) = &self.handle {
                    h.send(HostRequest::SetPanelOpen {
                        panel: PANEL.into(),
                        pane: None,
                        open: true,
                    });
                }
            }
            "resize" | "convert" | "strip" | "optimize" | "retina" => {
                self.open_modal(id, pane, None)
            }
            "rotate-left" => self.open_modal("rotate", pane, Some("left")),
            "rotate-right" => self.open_modal("rotate", pane, Some("right")),
            "flip" => self.open_modal("rotate", pane, Some("flip-h")),
            "datauri" => self.copy_data_uri(pane),
            "cancel" => self.cancel_job(),
            _ => {}
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![
            PanelSpec {
                id: PANEL.into(),
                title: tr!("panel.image").into(),
                placement: PanelPlacement::RightSidebar,
                default_open: false,
            },
            PanelSpec {
                id: MODAL.into(),
                title: tr!("panel.tools").into(),
                placement: PanelPlacement::Modal,
                default_open: false,
            },
        ]
    }

    fn panel_size(&self, panel: &str) -> Option<[f32; 2]> {
        (panel == MODAL).then_some([460.0, 500.0])
    }

    fn panel_ui(&mut self, panel: &str, pane: &PaneState, ui: &mut egui::Ui) {
        let state = self.active_state().unwrap_or_else(|| pane.clone());
        let queued: Vec<Click> =
            std::mem::take(&mut *self.queued.lock().unwrap_or_else(|e| e.into_inner()));
        for c in queued {
            match c {
                Click::Panel(a) => self.panel_action(a, &state),
                Click::Sheet(a) => self.sheet_action(a),
            }
        }
        match panel {
            PANEL => {
                for a in panel::side_panel(ui, &self.shared) {
                    self.panel_action(a, &state);
                }
            }
            MODAL => {
                for a in panel::sheet(ui, &self.shared) {
                    self.sheet_action(a);
                }
            }
            _ => {}
        }
    }
}

/// The pane a test or render fakes.
#[doc(hidden)]
pub fn pane_state(pane: PaneId, dir: Option<&Path>, selection: &[PathBuf]) -> PaneState {
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
        selection: selection
            .iter()
            .map(|p| VfsPath::new(&p.to_string_lossy()))
            .collect(),
        active: true,
    }
}
