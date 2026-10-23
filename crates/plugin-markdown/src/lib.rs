//! Built-in Markdown Preview plugin. PUBLIC API CONTRACT (used by the
//! app): `MarkdownPlugin::new()`.
//!
//! - **Panel** (right sidebar, "Markdown", ⌥⌘M): renders the selected
//!   `.md` / `.markdown` / `.mdx` file of the active pane, or the folder's
//!   README when nothing is selected. GitHub-flavoured Markdown with
//!   highlighted code, tables, task lists, alerts, footnotes and images.
//! - **Live:** local files are watched (mtime polling, debounced) and
//!   re-rendered in place, keeping the scroll position. Remote files are
//!   read through the pane's connection with `HostRequest::ReadFile` and
//!   re-read when the pane refreshes (and every few seconds while shown).
//! - **Links:** http(s) opens in the browser, relative `.md` links open in
//!   the preview (with Back), `#anchors` scroll to the heading.
//! - **Privacy:** remote (http) images load only if switched on in the
//!   plugin's settings.
//!
//! Threads: parsing, highlighting, file reads, watching and image loads run
//! on the [`loader`] thread; trait methods only exchange messages.

pub mod doc;
pub mod highlight;
pub mod i18n;
pub mod links;
pub mod loader;
pub mod panel;
pub mod render;
pub mod style;

#[cfg(test)]
mod render_tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::{
    CommandContext, CommandSpec, FileContent, HostRequest, PaneId, PaneState, PanelPlacement,
    PanelSpec, Plugin, PluginEvent, PluginHandle, PluginInfo, ToastLevel,
};
use serde::{Deserialize, Serialize};

use links::ImageSource;
use loader::{Cmd, Content, Loaded, Loader, LocalTarget, Out};
use render::{Heights, ImageState};

pub const PLUGIN_ID: &str = "manta.markdown";
pub const PANEL_ID: &str = "markdown";
pub const TOGGLE_CMD: &str = "toggle-preview";

/// The panel counts as hidden when it wasn't drawn for this long.
const PANEL_HIDDEN_AFTER: Duration = Duration::from_millis(1500);
/// Remote documents are re-read this often while shown.
const REMOTE_POLL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Load `http(s)://` images (off: privacy, no requests to third parties).
    #[serde(default)]
    pub web_images: bool,
}

impl Settings {
    fn path(dir: &std::path::Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &std::path::Path) -> Settings {
        std::fs::read_to_string(Self::path(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &std::path::Path) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(dir), s);
        }
    }
}

/// What the active pane asks the preview to show.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Want {
    /// Nothing selected: the README of this folder, if any.
    Readme {
        pane: PaneId,
        dir: VfsPath,
        local: bool,
    },
    /// A Markdown file.
    File {
        pane: PaneId,
        path: VfsPath,
        local: bool,
    },
    /// The selection isn't Markdown.
    NotMarkdown { name: String },
}

impl Want {
    fn path(&self) -> Option<&VfsPath> {
        match self {
            Want::Readme { dir, .. } => Some(dir),
            Want::File { path, .. } => Some(path),
            Want::NotMarkdown { .. } => None,
        }
    }
}

/// The preview target for a pane: the last selected Markdown file, else
/// the folder's README if nothing is selected.
pub fn want_for(s: &PaneState) -> Want {
    let local = s.local_path.is_some();
    if let Some(p) = s
        .selection
        .iter()
        .rev()
        .find(|p| links::is_markdown(p.file_name()))
    {
        return Want::File {
            pane: s.pane,
            path: p.clone(),
            local,
        };
    }
    match s.selection.last() {
        Some(p) => Want::NotMarkdown {
            name: p.file_name().to_string(),
        },
        None => Want::Readme {
            pane: s.pane,
            dir: s.path.clone(),
            local,
        },
    }
}

/// UI and loading state of the panel.
#[derive(Default)]
pub struct View {
    pub gen: u64,
    pub want: Option<Want>,
    /// Markdown files opened by following links (Back pops).
    pub history: Vec<Want>,
    requested: Option<(Want, bool)>,
    pub loaded: Option<Loaded>,
    /// Scroll to this anchor once it is drawn.
    pub anchor: Option<String>,
    pub images: HashMap<ImageSource, ImageState>,
    pub heights: Heights,
    /// Files the user chose to see in part although too large.
    pub partial_ok: HashSet<VfsPath>,
    /// Remote document read in flight: (gen, path, is README).
    doc_req: Option<(u64, VfsPath, bool)>,
    /// Remote folder listed to find its README: (gen, folder).
    listing_req: Option<(u64, VfsPath)>,
    /// Remote image reads in flight.
    image_reqs: HashMap<VfsPath, ImageSource>,
    last_remote_read: Option<Instant>,
    /// Hash of the remote bytes last parsed (skip re-parsing the same).
    remote_hash: Option<u64>,
    pub toc_open: bool,
    /// Heading / footnote positions from the last frame.
    pub anchor_y: HashMap<String, f32>,
    pub scroll_y: f32,
    image_seq: u64,
}

impl View {
    /// What the panel should show now: the last followed link, else the
    /// active pane's target.
    pub fn current(&self) -> Option<&Want> {
        self.history.last().or(self.want.as_ref())
    }

    /// The loaded document if it belongs to what is shown now (an older
    /// version of the same file counts while the new one loads).
    pub fn shown(&self) -> Option<&Loaded> {
        let l = self.loaded.as_ref()?;
        if l.gen == self.gen {
            return Some(l);
        }
        let cur = self.current()?;
        let same = match cur {
            Want::File { path, .. } => &l.path == path,
            Want::Readme { dir, .. } => {
                l.readme && (l.path.parent().as_ref() == Some(dir) || &l.path == dir)
            }
            Want::NotMarkdown { .. } => false,
        };
        same.then_some(l)
    }
}

pub struct MarkdownPlugin {
    handle: Option<PluginHandle>,
    loader: Option<Loader>,
    rx: Option<Receiver<Out>>,
    settings: Settings,
    panes: std::collections::BTreeMap<PaneId, PaneState>,
    active: PaneId,
    pub view: View,
    panel_drawn: Option<Instant>,
}

impl MarkdownPlugin {
    pub fn new() -> Self {
        MarkdownPlugin {
            handle: None,
            loader: None,
            rx: None,
            settings: Settings::default(),
            panes: Default::default(),
            active: PaneId::Left,
            view: View::default(),
            panel_drawn: None,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    #[doc(hidden)]
    pub fn set_web_images(&mut self, on: bool) {
        self.settings.web_images = on;
    }

    fn send(&self, r: HostRequest) {
        if let Some(h) = &self.handle {
            h.send(r);
        }
    }

    fn cmd(&self, c: Cmd) {
        if let Some(l) = &self.loader {
            l.send(c);
        }
    }

    fn active_state(&self) -> Option<&PaneState> {
        self.panes.get(&self.active)
    }

    fn panel_visible(&self) -> bool {
        self.panel_drawn
            .is_some_and(|t| t.elapsed() < PANEL_HIDDEN_AFTER)
    }

    /// Recompute the target from the active pane; a new target drops the
    /// link history.
    fn update_want(&mut self) {
        let want = self.active_state().map(want_for);
        if want != self.view.want {
            self.view.want = want;
            self.view.history.clear();
            self.view.anchor = None;
            self.view.toc_open = false;
        }
        if self.panel_visible() {
            self.sync();
        } else if self.view.requested.is_some() {
            // Hidden: stop watching, load again when shown.
            self.view.requested = None;
            self.cmd(Cmd::Idle);
        }
    }

    /// Start loading what should be shown, if not requested yet.
    fn sync(&mut self) {
        let Some(cur) = self.view.current().cloned() else {
            return;
        };
        let partial = cur.path().is_some_and(|p| self.view.partial_ok.contains(p));
        let key = (cur.clone(), partial);
        if self.view.requested.as_ref() == Some(&key) {
            return;
        }
        self.view.requested = Some(key);
        self.view.gen += 1;
        self.view.doc_req = None;
        self.view.listing_req = None;
        self.view.remote_hash = None;
        let gen = self.view.gen;
        match cur {
            Want::File {
                path, local: true, ..
            } => {
                let target = LocalTarget::File(PathBuf::from(path.as_str()));
                self.cmd(Cmd::Local {
                    gen,
                    target,
                    allow_partial: partial,
                });
            }
            Want::Readme {
                dir, local: true, ..
            } => {
                let target = LocalTarget::ReadmeIn(PathBuf::from(dir.as_str()));
                self.cmd(Cmd::Local {
                    gen,
                    target,
                    allow_partial: partial,
                });
            }
            Want::File {
                pane,
                path,
                local: false,
            } => {
                self.cmd(Cmd::Idle);
                self.read_remote_doc(pane, path, false);
            }
            Want::Readme {
                pane,
                dir,
                local: false,
            } => {
                self.cmd(Cmd::Idle);
                self.view.listing_req = Some((gen, dir.clone()));
                self.send(HostRequest::ListDir { pane, dir });
            }
            Want::NotMarkdown { .. } => {
                self.cmd(Cmd::Idle);
                self.view.loaded = None;
            }
        }
    }

    fn read_remote_doc(&mut self, pane: PaneId, path: VfsPath, readme: bool) {
        self.view.doc_req = Some((self.view.gen, path.clone(), readme));
        self.view.last_remote_read = Some(Instant::now());
        self.send(HostRequest::ReadFile {
            pane,
            path,
            max_bytes: loader::MAX_DOC_BYTES + 1,
        });
    }

    /// Re-read a remote document (pane refreshed, or timer).
    fn reread_remote(&mut self) {
        if let Some((_, path, readme)) = self.view.doc_req.clone() {
            if let Some(
                Want::File {
                    pane, local: false, ..
                }
                | Want::Readme {
                    pane, local: false, ..
                },
            ) = self.view.current().cloned()
            {
                self.read_remote_doc(pane, path, readme);
            }
        }
    }

    fn on_file_content(&mut self, c: &FileContent) {
        if let Some((gen, path, readme)) = self.view.doc_req.clone() {
            if path == c.path && gen == self.view.gen {
                if let Some(e) = &c.error {
                    self.view.loaded = Some(Loaded {
                        gen,
                        path,
                        readme,
                        content: Content::Unreadable(e.clone()),
                        version: 0,
                    });
                    return;
                }
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    c.bytes.hash(&mut h);
                    h.finish()
                };
                if self.view.remote_hash == Some(hash) {
                    return;
                }
                self.view.remote_hash = Some(hash);
                let allow_partial = self.view.partial_ok.contains(&path);
                let size = c.size.unwrap_or(c.bytes.len() as u64);
                self.cmd(Cmd::Parse {
                    gen,
                    path,
                    readme,
                    bytes: c.bytes.clone(),
                    size,
                    allow_partial,
                });
                return;
            }
        }
        if let Some(src) = self.view.image_reqs.remove(&c.path) {
            let st = match &c.error {
                Some(e) => ImageState::Failed(e.clone()),
                None if c.truncated => ImageState::Failed(i18n::msg_key("image.too_large")),
                None => self.ready_image(&src, Arc::from(c.bytes.as_slice())),
            };
            self.view.images.insert(src, st);
        }
    }

    fn ready_image(&mut self, src: &ImageSource, bytes: Arc<[u8]>) -> ImageState {
        self.view.image_seq += 1;
        let name = match src {
            ImageSource::File(p) => p.file_name().to_string(),
            ImageSource::Web(u) => u
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string(),
            ImageSource::Unsupported(_) => String::new(),
        };
        ImageState::Ready {
            uri: format!("bytes://manta-markdown/{}/{name}", self.view.image_seq),
            bytes,
        }
    }

    /// Results from the loader thread.
    fn drain(&mut self) {
        let Some(rx) = &self.rx else { return };
        let outs: Vec<Out> = rx.try_iter().collect();
        for o in outs {
            match o {
                Out::Loaded(l) => {
                    if l.gen == self.view.gen {
                        self.view.heights = Heights::default();
                        self.view.loaded = Some(l);
                    }
                }
                Out::Image(src, r) => {
                    let st = match r {
                        Ok(b) => self.ready_image(&src, b),
                        Err(e) => ImageState::Failed(e),
                    };
                    self.view.images.insert(src, st);
                }
            }
        }
    }

    fn load_image(&mut self, src: ImageSource) {
        if self.view.images.contains_key(&src) {
            return;
        }
        match &src {
            ImageSource::File(p) => match self.view.current() {
                Some(
                    Want::File {
                        local: false, pane, ..
                    }
                    | Want::Readme {
                        local: false, pane, ..
                    },
                ) => {
                    let pane = *pane;
                    self.view.image_reqs.insert(p.clone(), src.clone());
                    self.send(HostRequest::ReadFile {
                        pane,
                        path: p.clone(),
                        max_bytes: loader::MAX_IMAGE_BYTES,
                    });
                }
                _ => self.cmd(Cmd::Image(src.clone(), PathBuf::from(p.as_str()))),
            },
            ImageSource::Web(u) => {
                if !self.settings.web_images {
                    return;
                }
                self.cmd(Cmd::WebImage(src.clone(), u.clone()));
            }
            ImageSource::Unsupported(_) => return,
        }
        self.view.images.insert(src, ImageState::Loading);
    }

    /// Follow a link to another Markdown file.
    pub fn open_markdown(&mut self, path: VfsPath, anchor: Option<String>) {
        let (pane, local) = match self.view.current() {
            Some(Want::File { pane, local, .. } | Want::Readme { pane, local, .. }) => {
                (*pane, *local)
            }
            _ => (
                self.active,
                self.active_state().is_some_and(|s| s.local_path.is_some()),
            ),
        };
        let want = Want::File { pane, path, local };
        if self.view.current() == Some(&want) {
            self.view.anchor = anchor;
            return;
        }
        self.view.history.push(want);
        self.view.anchor = anchor;
        self.view.toc_open = false;
        self.sync();
    }

    pub fn go_back(&mut self) {
        if self.view.history.pop().is_some() {
            self.view.anchor = None;
            self.view.toc_open = false;
            self.sync();
        }
    }

    fn apply(&mut self, a: panel::UiAction) {
        use panel::UiAction as A;
        match a {
            A::Render(render::Action::OpenUrl(u)) => open_external(&u),
            A::Render(render::Action::Anchor(a)) => self.view.anchor = Some(a),
            A::Render(render::Action::Markdown { path, anchor }) => {
                self.open_markdown(path, anchor)
            }
            A::Render(render::Action::LoadImage(src)) => self.load_image(src),
            A::Render(render::Action::OpenFile(p)) => self.open_file(p),
            A::Back => self.go_back(),
            A::ShowPartial(p) => {
                self.view.partial_ok.insert(p);
                self.sync();
            }
            A::OpenInEditor(p) => spawn(&["open", "-t", p.as_str()]),
            A::Reveal(p) => {
                let pane = match self.view.current() {
                    Some(Want::File { pane, .. } | Want::Readme { pane, .. }) => *pane,
                    _ => self.active,
                };
                if let Some(dir) = p.parent() {
                    self.send(HostRequest::Navigate { pane, path: dir });
                }
            }
        }
    }

    /// A link to some other file: folders open in the pane, local files
    /// with their default app.
    fn open_file(&mut self, p: VfsPath) {
        let (pane, local) = match self.view.current() {
            Some(Want::File { pane, local, .. } | Want::Readme { pane, local, .. }) => {
                (*pane, *local)
            }
            _ => (self.active, false),
        };
        if !local {
            if let Some(dir) = p.parent() {
                self.send(HostRequest::Navigate { pane, path: dir });
            }
            return;
        }
        let Some(h) = self.handle.clone() else { return };
        std::thread::spawn(move || match std::fs::metadata(p.as_str()) {
            Ok(m) if m.is_dir() => h.send(HostRequest::Navigate { pane, path: p }),
            Ok(_) => spawn(&["open", p.as_str()]),
            Err(_) => h.send(HostRequest::Toast {
                level: ToastLevel::Warning,
                text: i18n::tr!("toast.missing", name = p.file_name()),
            }),
        });
    }
}

impl Default for MarkdownPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a command without waiting for it (reaped on a thread).
fn spawn(args: &[&str]) {
    let Some((prog, rest)) = args.split_first() else {
        return;
    };
    match std::process::Command::new(prog)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("markdown: can't run {prog}: {e}"),
    }
}

fn open_external(url: &str) {
    let lower = url.to_ascii_lowercase();
    if ["http://", "https://", "mailto:"]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        spawn(&["open", url]);
    } else {
        log::info!("markdown: not opening link {url}");
    }
}

impl Plugin for MarkdownPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: i18n::tr!("plugin.name").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: i18n::tr!("plugin.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        if self.loader.is_some() {
            self.disable();
        }
        self.settings = Settings::load(handle.data_dir());
        let (tx, rx) = channel();
        let ctx = handle.egui_ctx().clone();
        self.loader = Some(Loader::start(tx, Arc::new(move || ctx.request_repaint())));
        self.rx = Some(rx);
        self.handle = Some(handle);
        // Anything requested before has gone nowhere.
        self.view.requested = None;
    }

    fn disable(&mut self) {
        if let Some(mut l) = self.loader.take() {
            l.send(Cmd::Shutdown);
            // Detach: the thread exits on its own within one poll tick.
            l.join.take();
        }
        self.rx = None;
        self.handle = None;
        self.view = View::default();
        self.panes.clear();
    }

    fn on_event(&mut self, event: &PluginEvent) {
        match event {
            PluginEvent::DirectoryChanged(s) | PluginEvent::SelectionChanged(s) => {
                if s.active {
                    self.active = s.pane;
                }
                self.panes.insert(s.pane, s.clone());
                self.update_want();
            }
            PluginEvent::ListingRefreshed(s) => {
                self.panes.insert(s.pane, s.clone());
                self.update_want();
                // Remote files can't be watched: read again on refresh.
                if s.local_path.is_none() && self.panel_visible() {
                    if let Some(Want::Readme { dir, .. }) = self.view.current() {
                        if *dir == s.path {
                            self.view.requested = None;
                            self.sync();
                            return;
                        }
                    }
                    self.reread_remote();
                }
            }
            PluginEvent::ActivePaneChanged(p) => {
                self.active = *p;
                for s in self.panes.values_mut() {
                    s.active = s.pane == *p;
                }
                self.update_want();
            }
            PluginEvent::DirListing(l) => {
                let Some((gen, dir)) = self.view.listing_req.clone() else {
                    return;
                };
                if gen != self.view.gen || dir != l.dir {
                    return;
                }
                self.view.listing_req = None;
                let names = l
                    .entries
                    .iter()
                    .filter(|e| !e.is_dir)
                    .map(|e| e.name.as_str());
                match loader::pick_readme(names) {
                    Some(name) => {
                        let path = dir.join(name);
                        self.read_remote_doc(l.pane, path, true);
                    }
                    None => {
                        self.view.loaded = Some(Loaded {
                            gen,
                            path: dir,
                            readme: true,
                            content: Content::NoReadme,
                            version: 0,
                        })
                    }
                }
            }
            PluginEvent::FileContent(c) => self.on_file_content(c),
            PluginEvent::PaneClosed(p) => {
                self.panes.remove(p);
            }
            PluginEvent::LocaleChanged(_) => {
                // Texts are looked up while drawing.
                if let Some(h) = &self.handle {
                    h.request_repaint();
                }
            }
            _ => {}
        }
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec {
            id: TOGGLE_CMD.into(),
            title: i18n::tr!("cmd.toggle").into(),
            shortcut: Some("⌥⌘M".into()),
            context: CommandContext::Palette,
        }]
    }

    fn run_command(&mut self, id: &str, _pane: &PaneState) {
        if id == TOGGLE_CMD {
            self.send(HostRequest::TogglePanel {
                panel: PANEL_ID.into(),
                pane: None,
            });
        }
    }

    fn panels(&self) -> Vec<PanelSpec> {
        vec![PanelSpec {
            id: PANEL_ID.into(),
            title: i18n::tr!("panel.title").into(),
            placement: PanelPlacement::RightSidebar,
            default_open: false,
        }]
    }

    fn panel_ui(&mut self, panel: &str, pane: &PaneState, ui: &mut egui::Ui) {
        if panel != PANEL_ID {
            return;
        }
        let was_visible = self.panel_visible();
        self.panel_drawn = Some(Instant::now());
        // The host passes the active pane; keep our copy in step.
        if self.active != pane.pane || !self.panes.contains_key(&pane.pane) {
            self.active = pane.pane;
            self.panes.insert(pane.pane, pane.clone());
            self.update_want();
        }
        if !was_visible {
            self.view.requested = None;
        }
        self.drain();
        self.sync();
        if let (Some(t), Some(Want::File { local: false, .. })) =
            (self.view.last_remote_read, self.view.current())
        {
            if t.elapsed() > REMOTE_POLL {
                self.reread_remote();
            }
        }
        if matches!(
            self.view.current(),
            Some(Want::File { local: false, .. } | Want::Readme { local: false, .. })
        ) {
            ui.ctx().request_repaint_after(REMOTE_POLL);
        }
        let actions = panel::panel_ui(&mut self.view, self.settings.web_images, ui);
        for a in actions {
            self.apply(a);
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut s = self.settings;
        ui.checkbox(&mut s.web_images, i18n::tr!("settings.web_images"))
            .on_hover_text(i18n::tr!("settings.web_images.tip"));
        if s != self.settings {
            self.settings = s;
            if let Some(h) = &self.handle {
                s.save(h.data_dir());
            }
            // Retry web images that were blocked.
            self.view
                .images
                .retain(|k, _| !matches!(k, ImageSource::Web(_)));
        }
        true
    }
}

/// A pane for tests and renders.
#[doc(hidden)]
pub fn pane_state(pane: PaneId, dir: &std::path::Path, selection: &[&str]) -> PaneState {
    let path = VfsPath::new(&dir.to_string_lossy());
    PaneState {
        remote: None,
        pane,
        selection: selection.iter().map(|n| path.join(n)).collect(),
        path,
        local_path: Some(dir.to_path_buf()),
        protocol: manta_core::Protocol::Local,
        backend_label: "Local".into(),
        active: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn want_from_selection() {
        let dir = std::path::Path::new("/p");
        assert_eq!(
            want_for(&pane_state(PaneId::Left, dir, &[])),
            Want::Readme {
                pane: PaneId::Left,
                dir: VfsPath::new("/p"),
                local: true
            }
        );
        assert_eq!(
            want_for(&pane_state(
                PaneId::Left,
                dir,
                &["a.md", "b.txt", "c.MARKDOWN"]
            )),
            Want::File {
                pane: PaneId::Left,
                path: VfsPath::new("/p/c.MARKDOWN"),
                local: true
            }
        );
        assert_eq!(
            want_for(&pane_state(PaneId::Left, dir, &["main.rs"])),
            Want::NotMarkdown {
                name: "main.rs".into()
            }
        );
        let mut remote = pane_state(PaneId::Right, dir, &["x.mdx"]);
        remote.local_path = None;
        assert_eq!(
            want_for(&remote),
            Want::File {
                pane: PaneId::Right,
                path: VfsPath::new("/p/x.mdx"),
                local: false
            }
        );
    }
}
