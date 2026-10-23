//! Built-in Batch Rename plugin. PUBLIC API CONTRACT (used by the app):
//! `BatchRenamePlugin::new()`, [`PLUGIN_ID`], [`BATCH_CMD`].
//!
//! - **Batch Rename…** (⇧⌘R, palette and file context menu; the app also
//!   routes F2 / Rename with several selected items here) opens a modal
//!   sheet for the selection, or for every file in the folder when nothing
//!   is selected. Works for local and remote panes.
//! - **Rules** ([`rules`]) run top to bottom: find & replace (text or
//!   regex), insert, remove, case, numbering, date, extension, clean-up.
//! - **Preview** ([`preview`]): old → new with a character diff and inline
//!   conflicts; Rename is disabled while there are any.
//! - The app renames with [`HostRequest::RenameMany`] (safe order, one file
//!   action, refresh) and answers with `PluginEvent::RenameResult`; the
//!   plugin then offers **Undo** in a toast and in the palette.
//! - **Presets** ([`presets`]) are saved in the plugin data dir.
//!
//! Renders: see `render_batch_rename*` in `crates/app/src/ux_tests.rs`
//! (the real host modal).
//!
//! Threads: none. The folder listing comes from the host
//! ([`HostRequest::ListDir`]); preset files are written on a short-lived
//! thread.

pub mod i18n;
pub mod presets;
pub mod preview;
pub mod rules;
pub mod sheet;
pub mod style;

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use chrono::NaiveDateTime;
use manta_core::{Protocol, VfsPath};
use manta_plugin::{
    CommandContext, CommandSpec, DirListing, HostRequest, ListedEntry, PaneId, PaneState,
    PanelPlacement, PanelSpec, Plugin, PluginEvent, PluginHandle, PluginInfo, RenameResult,
    ToastLevel,
};

use i18n::tr;
use presets::Presets;
use preview::{Item, Preview};
use rules::{Rule, RuleType};
use sheet::{Scope, SheetAction, SheetUi, Status, View};

pub const PLUGIN_ID: &str = "manta.rename";
pub const PANEL_ID: &str = "batch-rename";
pub const BATCH_CMD: &str = "batch-rename";
pub const UNDO_CMD: &str = "undo-rename";

/// One open sheet.
pub struct Session {
    pub pane: PaneId,
    pub dir: VfsPath,
    pub remote: Option<String>,
    /// Names the user selected (2 or more), else empty = all files.
    pub selected: Vec<String>,
    /// `None` while the listing is on its way.
    pub listing: Option<Result<Vec<ListedEntry>, String>>,
    pub items: Vec<Item>,
    pub all_names: Vec<String>,
    pub case_insensitive: bool,
    /// "Today" for the date rule, fixed when the sheet opens.
    pub today: NaiveDateTime,
}

impl Session {
    fn dir_name(&self) -> String {
        if self.dir.is_root() {
            "/".into()
        } else {
            self.dir.file_name().to_string()
        }
    }

    fn scope(&self) -> Scope {
        if self.selected.is_empty() {
            Scope::AllFiles(self.items.len())
        } else {
            Scope::Selection(self.selected.len())
        }
    }

    /// Fill `items` from the listing.
    fn apply_listing(&mut self, entries: Vec<ListedEntry>) {
        self.all_names = entries.iter().map(|e| e.name.clone()).collect();
        let item = |e: &ListedEntry| Item {
            name: e.name.clone(),
            is_dir: e.is_dir,
            modified: e.modified,
        };
        self.items = if self.selected.is_empty() {
            entries
                .iter()
                .filter(|e| e.shown && !e.is_dir)
                .map(item)
                .collect()
        } else {
            // In pane order; selected names the listing lacks are dropped.
            entries
                .iter()
                .filter(|e| self.selected.contains(&e.name))
                .map(item)
                .collect()
        };
        self.listing = Some(Ok(entries));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Apply {
        pane: PaneId,
        dir: VfsPath,
        count: usize,
    },
    Undo {
        pane: PaneId,
        dir: VfsPath,
    },
}

/// The last batch, for Undo: (new name, old name) pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoBatch {
    pub pane: PaneId,
    pub dir: VfsPath,
    pub pairs: Vec<(String, String)>,
}

pub struct BatchRenamePlugin {
    handle: Option<PluginHandle>,
    pub session: Option<Session>,
    pub rules: Vec<Rule>,
    pub presets: Presets,
    pub ui: SheetUi,
    preview: Preview,
    preview_key: u64,
    pending: Option<Pending>,
    pub undo: Option<UndoBatch>,
    /// Why the last attempt was refused (shown in the footer).
    banner: Option<String>,
}

impl Default for BatchRenamePlugin {
    fn default() -> Self {
        Self::new()
    }
}

fn default_rules() -> Vec<Rule> {
    vec![RuleType::Replace.new_rule()]
}

impl BatchRenamePlugin {
    pub fn new() -> Self {
        BatchRenamePlugin {
            handle: None,
            session: None,
            rules: default_rules(),
            presets: Presets::default(),
            ui: SheetUi::default(),
            preview: Preview::default(),
            preview_key: 0,
            pending: None,
            undo: None,
            banner: None,
        }
    }

    fn send(&self, req: HostRequest) {
        if let Some(h) = &self.handle {
            h.send(req);
        }
    }

    fn data_dir(&self) -> Option<PathBuf> {
        self.handle.as_ref().map(|h| h.data_dir().clone())
    }

    /// Open the sheet for the pane's selection (2+ items) or all its files.
    pub fn open(&mut self, pane: &PaneState) {
        let selected: Vec<String> = pane
            .selection
            .iter()
            .filter(|p| p.parent().as_ref() == Some(&pane.path))
            .map(|p| p.file_name().to_string())
            .collect();
        let selected = if selected.len() >= 2 {
            selected
        } else {
            Vec::new()
        };
        self.session = Some(Session {
            pane: pane.pane,
            dir: pane.path.clone(),
            remote: (pane.local_path.is_none()).then(|| pane.backend_label.clone()),
            selected,
            listing: None,
            items: Vec::new(),
            all_names: Vec::new(),
            case_insensitive: pane.protocol == Protocol::Local,
            today: chrono::Local::now().naive_local(),
        });
        self.preview_key = 0;
        self.banner = None;
        self.ui = SheetUi::default();
        self.ui.focus_first = true;
        if let Some(dir) = self.data_dir() {
            let last = presets::load_last(&dir);
            if !last.is_empty() {
                self.rules = last;
            }
        }
        self.send(HostRequest::ListDir {
            pane: pane.pane,
            dir: pane.path.clone(),
        });
        self.send(HostRequest::SetPanelOpen {
            panel: PANEL_ID.into(),
            pane: None,
            open: true,
        });
    }

    /// Feed a listing directly (tests and renders).
    #[doc(hidden)]
    pub fn on_listing(&mut self, l: &DirListing) {
        let Some(s) = &mut self.session else { return };
        if s.pane != l.pane || s.dir != l.dir {
            return;
        }
        match &l.error {
            Some(e) => s.listing = Some(Err(e.clone())),
            None => s.apply_listing(l.entries.clone()),
        }
        self.preview_key = 0;
    }

    /// The current preview (recomputed when rules or items changed).
    pub fn preview(&mut self) -> &Preview {
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            serde_json::to_string(&self.rules)
                .unwrap_or_default()
                .hash(&mut h);
            if let Some(s) = &self.session {
                s.items.len().hash(&mut h);
                s.all_names.len().hash(&mut h);
                s.listing.is_some().hash(&mut h);
            }
            h.finish() | 1
        };
        if key != self.preview_key {
            if self.preview_key != 0 {
                self.banner = None;
            }
            self.preview_key = key;
            self.preview = match &self.session {
                Some(s) => preview::compute(
                    &s.items,
                    &s.all_names,
                    &self.rules,
                    s.today,
                    s.case_insensitive,
                ),
                None => Preview::default(),
            };
        }
        &self.preview
    }

    fn apply(&mut self) {
        let renames = self.preview().renames();
        let Some(s) = &self.session else { return };
        if renames.is_empty() || self.pending.is_some() {
            return;
        }
        self.banner = None;
        self.pending = Some(Pending::Apply {
            pane: s.pane,
            dir: s.dir.clone(),
            count: renames.len(),
        });
        let req = HostRequest::RenameMany {
            pane: s.pane,
            dir: s.dir.clone(),
            renames,
        };
        self.send(req);
        // Remember the rules for next time.
        if let Some(dir) = self.data_dir() {
            let rules = self.rules.clone();
            std::thread::spawn(move || {
                if let Err(e) = presets::save_last(&dir, &rules) {
                    log::warn!("batch rename: couldn't save the last rules: {e}");
                }
            });
        }
    }

    fn close_sheet(&mut self) {
        self.session = None;
        self.send(HostRequest::SetPanelOpen {
            panel: PANEL_ID.into(),
            pane: None,
            open: false,
        });
    }

    fn save_presets(&self) {
        if let Some(dir) = self.data_dir() {
            let p = self.presets.clone();
            std::thread::spawn(move || {
                if let Err(e) = p.save(&dir) {
                    log::warn!("batch rename: couldn't save presets: {e}");
                }
            });
        }
    }

    fn undo(&mut self) {
        let Some(u) = &self.undo else {
            self.send(HostRequest::Toast {
                level: ToastLevel::Info,
                text: tr!("toast.nothing_to_undo").into(),
            });
            return;
        };
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(Pending::Undo {
            pane: u.pane,
            dir: u.dir.clone(),
        });
        self.send(HostRequest::RenameMany {
            pane: u.pane,
            dir: u.dir.clone(),
            renames: u.pairs.clone(),
        });
    }

    fn on_result(&mut self, r: &RenameResult) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let (pane, dir) = match &pending {
            Pending::Apply { pane, dir, .. } | Pending::Undo { pane, dir } => (*pane, dir),
        };
        if pane != r.pane || *dir != r.dir {
            return;
        }
        self.pending = None;
        let had_undo = self.undo.is_some();
        match pending {
            Pending::Apply { count, .. } => {
                if r.ok.is_empty() {
                    // Refused or everything failed: keep the sheet open.
                    let e = r
                        .failed
                        .first()
                        .map(|(_, e)| e.clone())
                        .unwrap_or_else(|| tr!("banner.nothing_renamed").into());
                    self.banner = Some(tr!("banner.nothing_renamed_error", error = e));
                    return;
                }
                self.undo = Some(UndoBatch {
                    pane: r.pane,
                    dir: r.dir.clone(),
                    pairs: r.ok.iter().map(|(o, n)| (n.clone(), o.clone())).collect(),
                });
                self.close_sheet();
                let n = r.ok.len();
                let (level, text) = if r.failed.is_empty() {
                    (ToastLevel::Success, tr!("toast.renamed", count = n))
                } else {
                    let (name, e) = &r.failed[0];
                    (
                        ToastLevel::Warning,
                        tr!(
                            "toast.renamed_partial",
                            done = n,
                            total = count,
                            name = name,
                            error = e,
                            more = if r.failed.len() > 1 { " …" } else { "" },
                        ),
                    )
                };
                self.send(HostRequest::ActionToast {
                    level,
                    text,
                    button: tr!("toast.undo").into(),
                    command: UNDO_CMD.into(),
                });
            }
            Pending::Undo { .. } => {
                let n = r.ok.len();
                if r.failed.is_empty() {
                    self.undo = None;
                    self.send(HostRequest::Toast {
                        level: ToastLevel::Success,
                        text: tr!("toast.undid", count = n),
                    });
                } else {
                    // Keep what is left to undo.
                    if let Some(u) = &mut self.undo {
                        let done: Vec<&String> = r.ok.iter().map(|(o, _)| o).collect();
                        u.pairs.retain(|(new, _)| !done.contains(&new));
                    }
                    let (name, e) = &r.failed[0];
                    self.send(HostRequest::Toast {
                        level: ToastLevel::Error,
                        text: tr!("toast.undo_failed", name = name, error = e),
                    });
                }
            }
        }
        if had_undo != self.undo.is_some() {
            self.send(HostRequest::ContributionsChanged);
        }
    }

    fn handle_actions(&mut self, actions: Vec<SheetAction>) {
        for a in actions {
            match a {
                SheetAction::Apply => self.apply(),
                SheetAction::Cancel => self.close_sheet(),
                SheetAction::SavePreset(name) => {
                    self.presets.upsert(&name, self.rules.clone());
                    self.save_presets();
                    self.send(HostRequest::Toast {
                        level: ToastLevel::Success,
                        text: tr!("toast.preset_saved", name = name),
                    });
                }
                SheetAction::LoadPreset(name) => {
                    if let Some(p) = self.presets.get(&name) {
                        self.rules = p.rules.clone();
                        self.banner = None;
                    }
                }
                SheetAction::DeletePreset(name) => {
                    if self.presets.remove(&name) {
                        self.save_presets();
                    }
                }
            }
        }
    }
}

impl Plugin for BatchRenamePlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: tr!("info.name").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: tr!("info.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        self.presets = Presets::load(handle.data_dir());
        self.handle = Some(handle);
    }

    fn disable(&mut self) {
        self.handle = None;
        self.session = None;
        self.pending = None;
        self.undo = None;
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirListing(l) => self.on_listing(l),
            PluginEvent::RenameResult(r) => self.on_result(r),
            PluginEvent::LocaleChanged(_) => {
                // Everything is translated while drawing; the refusal banner
                // was formatted in the old language.
                self.banner = None;
                // Command titles (the host also asks again by itself).
                self.send(HostRequest::ContributionsChanged);
                if let Some(h) = &self.handle {
                    h.request_repaint();
                }
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let mut v = vec![CommandSpec {
            id: BATCH_CMD.into(),
            title: tr!("cmd.batch").into(),
            shortcut: Some("⇧⌘R".into()),
            context: CommandContext::FileContextMenu,
        }];
        if self.undo.is_some() {
            v.push(CommandSpec {
                id: UNDO_CMD.into(),
                title: tr!("cmd.undo").into(),
                shortcut: None,
                context: CommandContext::Palette,
            });
        }
        v
    }

    fn run_command(&mut self, id: &str, pane: &PaneState) {
        match id {
            BATCH_CMD => self.open(pane),
            UNDO_CMD => self.undo(),
            _ => {}
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![PanelSpec {
            id: PANEL_ID.into(),
            title: tr!("panel.title").into(),
            placement: PanelPlacement::Modal,
            default_open: false,
        }]
    }

    fn panel_size(&self, panel: &str) -> Option<[f32; 2]> {
        (panel == PANEL_ID).then_some(sheet::SHEET_SIZE)
    }

    fn panel_ui(&mut self, panel: &str, _pane: &PaneState, ui: &mut egui::Ui) {
        if panel != PANEL_ID {
            return;
        }
        if self.session.is_none() {
            let p = style::pal(ui);
            ui.painter().text(
                ui.max_rect().center(),
                egui::Align2::CENTER_CENTER,
                tr!("panel.idle"),
                style::regular(style::text::BODY),
                p.text2,
            );
            return;
        }
        self.preview();
        let busy = self.pending.is_some();
        let Some(s) = &self.session else { return };
        let status = match &s.listing {
            None => Status::Loading,
            Some(Ok(_)) => Status::Ready,
            Some(Err(e)) => Status::Error(e.as_str()),
        };
        let dir_name = s.dir_name();
        let view = View {
            rules: &mut self.rules,
            preview: &self.preview,
            dir_name: &dir_name,
            remote: s.remote.as_deref(),
            scope: s.scope(),
            status,
            presets: &self.presets,
            busy,
            banner: self.banner.as_deref(),
        };
        let actions = sheet::sheet_ui(ui, &mut self.ui, view);
        self.handle_actions(actions);
    }
}
