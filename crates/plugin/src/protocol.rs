//! The JSON-lines protocol between Manta and an external plugin process.
//!
//! Every message is one JSON object on one line, tagged by `"type"`.
//! The host writes [`HostMessage`]s to the plugin's stdin and reads
//! [`PluginMessage`]s from its stdout. stderr is the plugin's log.
//!
//! Versioning: the host sends [`PROTOCOL_VERSION`] in `init`; a plugin
//! declares the version it was written for as `api_version` in its
//! `plugin.toml`. New message types and new optional fields may be added
//! within one version, so both sides ignore what they don't know: the host
//! logs and skips unknown message types and unknown fields.
//!
//! See `docs/plugins.md` for the reference with JSON examples.

use std::collections::BTreeMap;
use std::path::PathBuf;

use manta_core::VfsPath;
use serde::{Deserialize, Serialize};

use crate::{
    Chip, CommandContext, CommandSpec, Decoration, FileAction, ListedEntry, PaneId, PaneState,
    PanelPlacement, PanelSpec, RemoteEndpoint, SizeInfo, StatusItem, ToastLevel, Tone,
};

/// Protocol version spoken by this host.
pub const PROTOCOL_VERSION: u32 = 1;

/// Lines longer than this (bytes) are dropped with a log line.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// At most this many decorations are kept from one `set_decorations`.
pub const MAX_DECORATIONS: usize = 20_000;
/// At most this many chips per pane and plugin.
pub const MAX_CHIPS: usize = 6;
/// At most this many status bar items per plugin.
pub const MAX_STATUS_ITEMS: usize = 6;
/// At most this many blocks per panel.
pub const MAX_BLOCKS: usize = 400;
/// At most this many rows in one list / key-value block.
pub const MAX_ROWS: usize = 2_000;
/// Texts longer than this (chars) are cut.
pub const MAX_TEXT: usize = 4_000;

// ------------------------------------------------------------------ host → plugin

/// Messages the host writes to the plugin's stdin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    /// Always the first message after the process starts.
    Init {
        api_version: u32,
        plugin_id: String,
        /// Per-plugin folder for settings and state (exists).
        data_dir: PathBuf,
        /// The panes as far as known (usually both).
        panes: Vec<PaneState>,
        /// The app's UI language, a BCP 47 code like `"en"` or `"de"`.
        #[serde(default = "default_locale")]
        locale: String,
    },
    /// The app's UI language changed.
    LocaleChanged {
        locale: String,
    },
    DirectoryChanged {
        pane_state: PaneState,
    },
    ListingRefreshed {
        pane_state: PaneState,
    },
    SelectionChanged {
        pane_state: PaneState,
    },
    ActivePaneChanged {
        pane: PaneId,
    },
    /// A pane appeared (the user split a pane). Sent before its first
    /// `directory_changed`.
    PaneOpened {
        pane: PaneId,
    },
    /// A pane was closed: forget what you keep for it. Its id may be reused
    /// by a later `pane_opened`.
    PaneClosed {
        pane: PaneId,
    },
    /// A file action done in the GUI started or finished (sent twice per
    /// action with the same `action.id`). Informational only.
    FileAction {
        action: FileAction,
    },
    /// The user ran one of the plugin's commands; `pane` is the active pane.
    RunCommand {
        id: String,
        pane: PaneState,
    },
    /// The user clicked a row or button with an `action` in a panel.
    PanelAction {
        panel: String,
        action: String,
        pane: PaneState,
    },
    /// The plugin is being switched off or the app quits. Exit within
    /// 500 ms; after that the process is killed.
    Shutdown,
    /// Answer to the plugin's `rename_many`: `ok` holds `[old, new]` pairs
    /// that were renamed, `failed` holds `[old, error]` pairs.
    RenameResult {
        pane: PaneId,
        dir: VfsPath,
        #[serde(default)]
        ok: Vec<(String, String)>,
        #[serde(default)]
        failed: Vec<(String, String)>,
    },
    /// Answer to the plugin's `list_dir`.
    DirListing {
        pane: PaneId,
        dir: VfsPath,
        #[serde(default)]
        entries: Vec<ListedEntry>,
        #[serde(default)]
        error: Option<String>,
    },
    /// Answer to the plugin's `read_file`: the file's first `max_bytes`
    /// bytes, base64-encoded (standard alphabet, with padding) in `data`.
    FileContent {
        pane: PaneId,
        path: VfsPath,
        #[serde(default)]
        data: String,
        #[serde(default)]
        size: Option<u64>,
        #[serde(default)]
        truncated: bool,
        #[serde(default)]
        modified: Option<i64>,
        #[serde(default)]
        error: Option<String>,
    },
}

fn default_locale() -> String {
    "en".to_string()
}

/// Default `max_bytes` of a `read_file` without one (1 MiB).
pub const DEFAULT_READ_BYTES: u64 = 1024 * 1024;

fn default_read_bytes() -> u64 {
    DEFAULT_READ_BYTES
}

impl HostMessage {
    /// One JSON line (with trailing newline).
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_else(|_| "{}".into());
        s.push('\n');
        s
    }
}

// ------------------------------------------------------------------ plugin → host

/// Messages a plugin writes to stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMessage {
    /// Decorations for entries of `dir` (file name → decoration). Replaces
    /// this plugin's earlier decorations for the pane. Ignored if the pane
    /// no longer shows `dir`.
    SetDecorations {
        pane: PaneId,
        dir: VfsPath,
        #[serde(default)]
        entries: BTreeMap<String, Decoration>,
    },
    /// Sizes (usually of folders) for entries of `dir`, by file name.
    /// Merged into this plugin's earlier sizes for the pane's folder.
    /// Ignored if the pane no longer shows `dir`.
    SetSizes {
        pane: PaneId,
        dir: VfsPath,
        #[serde(default)]
        sizes: BTreeMap<String, SizeInfo>,
    },
    /// Path-bar chips for `pane` while it shows `dir`.
    SetChips {
        pane: PaneId,
        dir: VfsPath,
        #[serde(default)]
        chips: Vec<Chip>,
    },
    Toast {
        #[serde(default = "default_toast_level")]
        level: ToastLevel,
        text: String,
        /// Optional button label, e.g. "Undo". Needs `command`.
        #[serde(default)]
        button: Option<String>,
        /// The plugin command the button runs (sent back as `run_command`).
        #[serde(default)]
        command: Option<String>,
    },
    Navigate {
        pane: PaneId,
        path: VfsPath,
        /// Set for a pane showing a server/container: the host navigates
        /// only while the pane still shows exactly this backend.
        #[serde(default)]
        remote: Option<RemoteEndpoint>,
    },
    /// Replaces the commands declared in `plugin.toml`.
    RegisterCommands { commands: Vec<CommandDecl> },
    /// Replaces the panels declared in `plugin.toml`.
    RegisterPanels { panels: Vec<PanelDecl> },
    /// Contents of a panel. `pane` selects the instance of a `pane_bottom`
    /// panel; omit it to set both panes (and for `right_sidebar` panels).
    SetPanelContent {
        panel: String,
        #[serde(default)]
        pane: Option<PaneId>,
        #[serde(default)]
        blocks: Vec<Block>,
    },
    SetPanelOpen {
        panel: String,
        #[serde(default)]
        pane: Option<PaneId>,
        open: bool,
    },
    /// Open the panel if it is closed, close it if it is open (`pane`
    /// selects the instance of a `pane_bottom` panel; omit it for the
    /// active pane). Handy for a chip that shows and hides a panel.
    TogglePanel {
        panel: String,
        #[serde(default)]
        pane: Option<PaneId>,
    },
    /// Rename entries of `dir` in `pane` (`[old, new]` name pairs). The
    /// host plans a safe order and answers with `rename_result`.
    RenameMany {
        pane: PaneId,
        dir: VfsPath,
        renames: Vec<(String, String)>,
    },
    /// List `dir` as `pane` shows it; answered with `dir_listing`.
    ListDir { pane: PaneId, dir: VfsPath },
    /// Read up to `max_bytes` (default 1 MiB, at most 16 MiB) of the file
    /// `path` in `pane`'s backend, local or remote; answered with
    /// `file_content`.
    ReadFile {
        pane: PaneId,
        path: VfsPath,
        #[serde(default = "default_read_bytes")]
        max_bytes: u64,
    },
    /// Run `command` visibly in the built-in Terminal below `pane` (omit
    /// for the active pane), after `cd`-ing to `cwd` (optional). The
    /// terminal panel opens if it is closed; a busy shell runs it once idle.
    RunInTerminal {
        #[serde(default)]
        pane: Option<PaneId>,
        command: String,
        #[serde(default)]
        cwd: Option<VfsPath>,
    },
    /// List the folder of `pane` again (omit for both panes).
    RefreshPane {
        #[serde(default)]
        pane: Option<PaneId>,
    },
    /// Select (and reveal) `paths` in `pane` (omit for the active pane),
    /// navigating to the folder of the first path if necessary.
    Select {
        #[serde(default)]
        pane: Option<PaneId>,
        paths: Vec<VfsPath>,
    },
    /// Items in the window's status bar (replaces the ones sent before;
    /// `[]` removes them). See [`StatusItem`].
    SetStatusItems {
        #[serde(default)]
        items: Vec<StatusItem>,
    },
    /// A line for the plugin's log (shown in Settings → Plugins).
    Log {
        #[serde(default)]
        level: LogLevel,
        message: String,
    },
}

fn default_toast_level() -> ToastLevel {
    ToastLevel::Info
}

/// Every `type` a plugin may send (for telling unknown types apart from
/// malformed known ones).
pub const PLUGIN_MESSAGE_TYPES: &[&str] = &[
    "set_decorations",
    "set_sizes",
    "set_chips",
    "toast",
    "navigate",
    "register_commands",
    "register_panels",
    "set_panel_content",
    "set_panel_open",
    "toggle_panel",
    "rename_many",
    "list_dir",
    "read_file",
    "run_in_terminal",
    "refresh_pane",
    "select",
    "set_status_items",
    "log",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

/// A command as declared in `plugin.toml` or `register_commands`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub shortcut: Option<String>,
    /// `"palette"` (default) or `"file_context_menu"`.
    #[serde(default = "default_context")]
    pub context: CommandContext,
}

fn default_context() -> CommandContext {
    CommandContext::Palette
}

impl From<CommandDecl> for CommandSpec {
    fn from(d: CommandDecl) -> Self {
        CommandSpec {
            id: d.id,
            title: d.title,
            shortcut: d.shortcut,
            context: d.context,
        }
    }
}

/// A panel as declared in `plugin.toml` or `register_panels`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelDecl {
    pub id: String,
    pub title: String,
    /// `"pane_bottom"` (default), `"right_sidebar"` or `"modal"`.
    #[serde(default = "default_placement")]
    pub placement: PanelPlacement,
    #[serde(default)]
    pub default_open: bool,
    /// Preferred `[width, height]` of a `modal` panel, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
}

fn default_placement() -> PanelPlacement {
    PanelPlacement::PaneBottom
}

impl From<PanelDecl> for PanelSpec {
    fn from(d: PanelDecl) -> Self {
        PanelSpec {
            id: d.id,
            title: d.title,
            placement: d.placement,
            default_open: d.default_open,
        }
    }
}

// ------------------------------------------------------------------ panel blocks

/// One block of a declarative panel. The host lays blocks out top to
/// bottom in its own style (light and dark).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading {
        text: String,
    },
    Text {
        text: String,
        #[serde(default)]
        tone: Tone,
    },
    /// Monospace text, e.g. command output. Keeps line breaks.
    Code {
        text: String,
    },
    /// Two-column rows, e.g. file properties.
    KeyValue {
        rows: Vec<KeyValueRow>,
    },
    /// Rows with optional icon, detail and click action.
    List {
        rows: Vec<ListRow>,
    },
    /// A row of buttons.
    Buttons {
        buttons: Vec<ButtonSpec>,
    },
    Separator,
    /// A small caption that starts a group of blocks (e.g. above a list),
    /// with an optional right-aligned `detail`.
    Section {
        text: String,
        #[serde(default)]
        detail: Option<String>,
    },
    /// A row of small coloured tags, e.g. the kinds of a project.
    Tags {
        tags: Vec<Tag>,
    },
    /// A local image file (PNG, JPEG, GIF, WebP, BMP), drawn aspect-fit
    /// with rounded corners. `path` must be an absolute path on this Mac
    /// (see [`image_block_path`]); anything else is shown as a note. The
    /// host decodes it in the background at a capped size and reloads it
    /// when the file's modification time changes.
    Image {
        path: String,
        /// Tallest the image is drawn, in points (default 200, at most 800).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_height: Option<f32>,
        /// A small caption below the image.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
}

/// Longest `path` an [`Block::Image`] may have (bytes).
pub const MAX_IMAGE_PATH: usize = 4096;

/// The local file an [`Block::Image`] may show, or `None` if `path` is not
/// a plain absolute local path: URLs (`file://`, `http://`, `data:`),
/// relative paths, `..` components, NUL bytes and overlong paths are
/// rejected.
pub fn image_block_path(path: &str) -> Option<PathBuf> {
    if path.is_empty()
        || path.len() > MAX_IMAGE_PATH
        || path.contains('\0')
        || !path.starts_with('/')
    {
        return None;
    }
    let p = std::path::Path::new(path);
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(p.to_path_buf())
}

/// One tag of a [`Block::Tags`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub text: String,
    #[serde(default)]
    pub tone: Tone,
    /// Same names as list row icons.
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValueRow {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRow {
    /// Same names as chip icons: "branch", "terminal", "info", "warning",
    /// "check", "cloud", "plug", plus "file", "folder".
    #[serde(default)]
    pub icon: Option<String>,
    pub text: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub tone: Tone,
    /// Sent back as `panel_action` when the row is clicked.
    #[serde(default)]
    pub action: Option<String>,
    /// Shown when the pointer rests on the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonSpec {
    pub label: String,
    pub action: String,
    #[serde(default)]
    pub primary: bool,
}

// ------------------------------------------------------------------ parsing

/// Result of reading one line from a plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Message(Box<PluginMessage>),
    /// A well-formed object with a `type` this host doesn't know.
    Unknown(String),
    /// Not JSON, no `type`, or a known type with bad fields.
    Invalid(String),
}

/// Parse one line from a plugin. Blank lines are `Invalid("")`.
pub fn parse_line(line: &str) -> Parsed {
    let line = line.trim();
    if line.is_empty() {
        return Parsed::Invalid(String::new());
    }
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Parsed::Invalid(format!("not valid JSON: {e}")),
    };
    let Some(ty) = value.get("type").and_then(|t| t.as_str()) else {
        return Parsed::Invalid("message has no \"type\" field".into());
    };
    if !PLUGIN_MESSAGE_TYPES.contains(&ty) {
        return Parsed::Unknown(ty.to_string());
    }
    let ty = ty.to_string();
    match serde_json::from_value::<PluginMessage>(value) {
        Ok(m) => Parsed::Message(Box::new(sanitize(m))),
        Err(e) => Parsed::Invalid(format!("bad \"{ty}\" message: {e}")),
    }
}

fn cut(s: &mut String, max: usize) {
    if let Some((i, _)) = s.char_indices().nth(max) {
        s.truncate(i);
        s.push('…');
    }
}

fn cut_block(b: &mut Block) {
    match b {
        Block::Heading { text } | Block::Text { text, .. } => cut(text, MAX_TEXT),
        Block::Code { text } => cut(text, MAX_TEXT * 16),
        Block::KeyValue { rows } => {
            rows.truncate(MAX_ROWS);
            for r in rows {
                cut(&mut r.key, 200);
                cut(&mut r.value, MAX_TEXT);
            }
        }
        Block::List { rows } => {
            rows.truncate(MAX_ROWS);
            for r in rows {
                cut(&mut r.text, MAX_TEXT);
                if let Some(d) = &mut r.detail {
                    cut(d, MAX_TEXT);
                }
                if let Some(t) = &mut r.tooltip {
                    cut(t, MAX_TEXT);
                }
            }
        }
        Block::Buttons { buttons } => {
            buttons.truncate(12);
            for b in buttons {
                cut(&mut b.label, 60);
            }
        }
        Block::Separator => {}
        Block::Section { text, detail } => {
            cut(text, 200);
            if let Some(d) = detail {
                cut(d, 200);
            }
        }
        Block::Tags { tags } => {
            tags.truncate(24);
            for t in tags {
                cut(&mut t.text, 60);
            }
        }
        Block::Image {
            path,
            max_height,
            caption,
        } => {
            if path.len() > MAX_IMAGE_PATH {
                path.clear(); // shown as "not a local image"
            }
            if let Some(h) = max_height {
                *h = if h.is_finite() {
                    h.clamp(24.0, 800.0)
                } else {
                    200.0
                };
            }
            if let Some(c) = caption {
                cut(c, 300);
            }
        }
    }
}

/// Clamp sizes so a buggy or hostile plugin can't flood the UI.
pub fn sanitize(mut m: PluginMessage) -> PluginMessage {
    match &mut m {
        PluginMessage::SetDecorations { entries, .. } => {
            if entries.len() > MAX_DECORATIONS {
                let keep: BTreeMap<String, Decoration> = std::mem::take(entries)
                    .into_iter()
                    .take(MAX_DECORATIONS)
                    .collect();
                *entries = keep;
            }
            for d in entries.values_mut() {
                if let Some(b) = &mut d.badge {
                    // 1–3 characters.
                    b.text = b.text.chars().take(3).collect();
                }
                if let Some(t) = &mut d.tooltip {
                    cut(t, 500);
                }
            }
            entries.retain(|_, d| d.badge.as_ref().is_none_or(|b| !b.text.is_empty()));
        }
        PluginMessage::SetSizes { sizes, .. } => {
            if sizes.len() > MAX_DECORATIONS {
                let keep: BTreeMap<String, SizeInfo> = std::mem::take(sizes)
                    .into_iter()
                    .take(MAX_DECORATIONS)
                    .collect();
                *sizes = keep;
            }
        }
        PluginMessage::SetChips { chips, .. } => {
            chips.truncate(MAX_CHIPS);
            for c in chips {
                cut(&mut c.text, 40);
                if let Some(t) = &mut c.tooltip {
                    cut(t, 500);
                }
            }
        }
        PluginMessage::Toast { text, button, .. } => {
            cut(text, 500);
            if let Some(b) = button {
                cut(b, 24);
            }
        }
        PluginMessage::RenameMany { renames, .. } => renames.truncate(MAX_DECORATIONS),
        PluginMessage::Log { message, .. } => cut(message, MAX_TEXT),
        PluginMessage::RunInTerminal { command, .. } => cut(command, MAX_TEXT * 4),
        PluginMessage::RegisterCommands { commands } => {
            commands.truncate(100);
            for c in commands {
                cut(&mut c.title, 80);
            }
        }
        PluginMessage::RegisterPanels { panels } => {
            panels.truncate(10);
            for p in panels {
                cut(&mut p.title, 60);
            }
        }
        PluginMessage::SetPanelContent { blocks, .. } => {
            blocks.truncate(MAX_BLOCKS);
            blocks.iter_mut().for_each(cut_block);
        }
        PluginMessage::Navigate { .. }
        | PluginMessage::SetPanelOpen { .. }
        | PluginMessage::TogglePanel { .. }
        | PluginMessage::ListDir { .. }
        | PluginMessage::ReadFile { .. }
        | PluginMessage::RefreshPane { .. } => {}
        PluginMessage::Select { paths, .. } => paths.truncate(MAX_ROWS),
        PluginMessage::SetStatusItems { items } => sanitize_status_items(items),
    }
    m
}

/// Clamp status bar items: at most [`MAX_STATUS_ITEMS`], short texts, a
/// meter within 0–1, invalid actions dropped.
pub fn sanitize_status_items(items: &mut Vec<StatusItem>) {
    items.truncate(MAX_STATUS_ITEMS);
    for it in items {
        cut(&mut it.id, 60);
        cut(&mut it.text, 40);
        if let Some(c) = &mut it.compact {
            cut(c, 20);
        }
        if let Some(t) = &mut it.tooltip {
            cut(t, 500);
        }
        it.meter = it
            .meter
            .filter(|m| m.is_finite())
            .map(|m| m.clamp(0.0, 1.0));
        if it.action.as_ref().is_some_and(|a| !a.is_valid()) {
            it.action = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Badge;
    use manta_core::Protocol;

    fn pane() -> PaneState {
        PaneState {
            remote: None,
            pane: PaneId::Left,
            path: VfsPath::new("/Users/anna/Projects"),
            local_path: Some(PathBuf::from("/Users/anna/Projects")),
            protocol: Protocol::Local,
            backend_label: "Local".into(),
            selection: vec![VfsPath::new("/Users/anna/Projects/README.md")],
            active: true,
        }
    }

    #[test]
    fn host_messages_round_trip() {
        let msgs = vec![
            HostMessage::Init {
                api_version: PROTOCOL_VERSION,
                plugin_id: "hello".into(),
                data_dir: PathBuf::from("/tmp/x"),
                panes: vec![pane()],
                locale: "de".into(),
            },
            HostMessage::LocaleChanged {
                locale: "en".into(),
            },
            HostMessage::DirectoryChanged { pane_state: pane() },
            HostMessage::ListingRefreshed { pane_state: pane() },
            HostMessage::SelectionChanged { pane_state: pane() },
            HostMessage::ActivePaneChanged {
                pane: PaneId::Right,
            },
            HostMessage::RunCommand {
                id: "hello".into(),
                pane: pane(),
            },
            HostMessage::PanelAction {
                panel: "info".into(),
                action: "open:1".into(),
                pane: pane(),
            },
            HostMessage::Shutdown,
            HostMessage::RenameResult {
                pane: PaneId::Left,
                dir: VfsPath::new("/a"),
                ok: vec![("a".into(), "b".into())],
                failed: vec![("c".into(), "Permission denied".into())],
            },
            HostMessage::DirListing {
                pane: PaneId::Left,
                dir: VfsPath::new("/a"),
                entries: vec![ListedEntry {
                    name: "a.txt".into(),
                    is_dir: false,
                    size: 3,
                    modified: Some(1_700_000_000),
                    shown: true,
                }],
                error: None,
            },
            HostMessage::FileContent {
                pane: PaneId::Right,
                path: VfsPath::new("/srv/README.md"),
                data: "IyBIaQo=".into(),
                size: Some(5),
                truncated: false,
                modified: Some(1_700_000_000),
                error: None,
            },
        ];
        for m in msgs {
            let line = m.to_line();
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            let back: HostMessage = serde_json::from_str(&line).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn file_action_wire_format() {
        use crate::{FileActionKind, FileActionPhase};
        let mut a = crate::FileAction::new(
            7,
            PaneId::Left,
            FileActionKind::Copy,
            vec![VfsPath::new("/Users/anna/a.txt")],
        );
        a.destination = Some(VfsPath::new("/srv/www"));
        a.dst_local = false;
        a.dst_label = Some("anna@example.org (SFTP)".into());
        a.dst_remote = Some(RemoteEndpoint {
            protocol: Protocol::Sftp,
            user: "anna".into(),
            host: "example.org".into(),
            port: 22,
            context: String::new(),
        });
        let m = HostMessage::FileAction { action: a.clone() };
        let v: serde_json::Value = serde_json::from_str(&m.to_line()).unwrap();
        assert_eq!(v["type"], "file_action");
        assert_eq!(v["action"]["id"], 7);
        assert_eq!(v["action"]["kind"], "copy");
        assert_eq!(v["action"]["phase"], "started");
        assert_eq!(v["action"]["sources"][0], "/Users/anna/a.txt");
        assert_eq!(serde_json::from_value::<HostMessage>(v).unwrap(), m);

        a.kind = FileActionKind::NewFolder;
        a.phase = FileActionPhase::Finished {
            ok: false,
            files: 2,
            bytes: 10,
            errors: vec!["denied".into()],
            cancelled: false,
            elapsed_ms: 400,
        };
        let m = HostMessage::FileAction { action: a };
        let v: serde_json::Value = serde_json::from_str(&m.to_line()).unwrap();
        assert_eq!(v["action"]["kind"], "new_folder");
        assert_eq!(v["action"]["phase"]["finished"]["ok"], false);
        assert_eq!(v["action"]["phase"]["finished"]["elapsed_ms"], 400);
        assert_eq!(serde_json::from_value::<HostMessage>(v).unwrap(), m);

        // Minimal JSON (optional fields left out) parses too.
        let min = r#"{"type":"file_action","action":{"id":1,"pane":"right","kind":"delete",
            "src_local":true,"dst_local":true,"src_label":"Local","phase":"started"}}"#;
        let HostMessage::FileAction { action } = serde_json::from_str(min).unwrap() else {
            panic!()
        };
        assert!(action.sources.is_empty() && action.sync.is_none());

        // The examples in docs/plugins.md.
        let docs = include_str!("../../../docs/plugins.md");
        let examples: Vec<&str> = docs
            .lines()
            .filter(|l| l.starts_with(r#"{"type":"file_action""#))
            .collect();
        assert_eq!(examples.len(), 3);
        for l in examples {
            assert!(
                matches!(
                    serde_json::from_str(l).unwrap(),
                    HostMessage::FileAction { .. }
                ),
                "{l}"
            );
        }
    }

    #[test]
    fn archive_file_action_wire_format() {
        use crate::{ArchiveSpec, FileActionKind};
        let mut a = crate::FileAction::new(
            9,
            PaneId::Left,
            FileActionKind::Extract,
            vec![VfsPath::new("/docs/a.txt")],
        );
        a.destination = Some(VfsPath::new("/out"));
        a.archive = Some(Box::new(ArchiveSpec {
            path: VfsPath::new("/dl/backup.zip"),
            format: "zip".into(),
            local: true,
            remote: None,
            level: None,
            exclude_junk: false,
        }));
        let m = HostMessage::FileAction { action: a };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["action"]["kind"], "extract");
        assert_eq!(v["action"]["archive"]["path"], "/dl/backup.zip");
        assert_eq!(serde_json::from_value::<HostMessage>(v).unwrap(), m);
        // Without an archive the field is left out entirely.
        let plain = crate::FileAction::new(1, PaneId::Left, FileActionKind::Copy, Vec::new());
        let v = serde_json::to_value(HostMessage::FileAction { action: plain }).unwrap();
        assert!(v["action"].get("archive").is_none());
    }

    #[test]
    fn host_message_wire_format() {
        let v: serde_json::Value =
            serde_json::from_str(&HostMessage::DirectoryChanged { pane_state: pane() }.to_line())
                .unwrap();
        assert_eq!(v["type"], "directory_changed");
        assert_eq!(v["pane_state"]["pane"], "left");
        assert_eq!(v["pane_state"]["path"], "/Users/anna/Projects");
        assert_eq!(v["pane_state"]["protocol"], "Local");
        assert_eq!(v["pane_state"]["active"], true);
        let v: serde_json::Value = serde_json::from_str(&HostMessage::Shutdown.to_line()).unwrap();
        assert_eq!(v, serde_json::json!({"type": "shutdown"}));
    }

    #[test]
    fn plugin_messages_round_trip() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "big.iso".to_string(),
            Decoration {
                badge: Some(Badge {
                    text: "L".into(),
                    tone: Tone::Warning,
                }),
                name_tone: None,
                dimmed: false,
                tooltip: Some("Larger than 10 MB".into()),
            },
        );
        let msgs = vec![
            PluginMessage::SetDecorations {
                pane: PaneId::Left,
                dir: VfsPath::new("/a"),
                entries,
            },
            PluginMessage::SetSizes {
                pane: PaneId::Left,
                dir: VfsPath::new("/a"),
                sizes: BTreeMap::from([(
                    "node_modules".to_string(),
                    SizeInfo {
                        bytes: 1_200_000,
                        complete: false,
                        files: 4_000,
                        on_disk: Some(1_500_000),
                        unreadable: 0,
                    },
                )]),
            },
            PluginMessage::SetChips {
                pane: PaneId::Right,
                dir: VfsPath::new("/a"),
                chips: vec![Chip {
                    text: "12 items".into(),
                    tone: Tone::Neutral,
                    icon: Some("info".into()),
                    tooltip: None,
                    command: None,
                }],
            },
            PluginMessage::Toast {
                level: ToastLevel::Success,
                text: "Hi".into(),
                button: None,
                command: None,
            },
            PluginMessage::Toast {
                level: ToastLevel::Success,
                text: "Renamed 3 items".into(),
                button: Some("Undo".into()),
                command: Some("undo".into()),
            },
            PluginMessage::RenameMany {
                pane: PaneId::Left,
                dir: VfsPath::new("/a"),
                renames: vec![("a.txt".into(), "b.txt".into())],
            },
            PluginMessage::ListDir {
                pane: PaneId::Right,
                dir: VfsPath::new("/srv"),
            },
            PluginMessage::ReadFile {
                pane: PaneId::Right,
                path: VfsPath::new("/srv/a.md"),
                max_bytes: 4096,
            },
            PluginMessage::Navigate {
                pane: PaneId::Left,
                path: VfsPath::new("/tmp"),
                remote: None,
            },
            PluginMessage::RegisterCommands {
                commands: vec![CommandDecl {
                    id: "hello".into(),
                    title: "Say hello".into(),
                    shortcut: Some("⌃⌥H".into()),
                    context: CommandContext::Palette,
                }],
            },
            PluginMessage::RegisterPanels {
                panels: vec![PanelDecl {
                    id: "info".into(),
                    title: "File Info".into(),
                    placement: PanelPlacement::RightSidebar,
                    default_open: false,
                    size: None,
                }],
            },
            PluginMessage::SetPanelContent {
                panel: "info".into(),
                pane: None,
                blocks: vec![
                    Block::Heading {
                        text: "README.md".into(),
                    },
                    Block::Text {
                        text: "hi".into(),
                        tone: Tone::Muted,
                    },
                    Block::Code {
                        text: "ls -la".into(),
                    },
                    Block::KeyValue {
                        rows: vec![KeyValueRow {
                            key: "Size".into(),
                            value: "1 KB".into(),
                            tone: Tone::Neutral,
                        }],
                    },
                    Block::List {
                        rows: vec![ListRow {
                            icon: Some("file".into()),
                            text: "a".into(),
                            detail: None,
                            tone: Tone::Accent,
                            action: Some("open:a".into()),
                            tooltip: Some("Open a".into()),
                        }],
                    },
                    Block::Buttons {
                        buttons: vec![ButtonSpec {
                            label: "Refresh".into(),
                            action: "refresh".into(),
                            primary: true,
                        }],
                    },
                    Block::Separator,
                    Block::Section {
                        text: "Scripts".into(),
                        detail: Some("pnpm".into()),
                    },
                    Block::Tags {
                        tags: vec![Tag {
                            text: "Laravel 11".into(),
                            tone: Tone::Danger,
                            icon: Some("rocket".into()),
                        }],
                    },
                    Block::Image {
                        path: "/Users/anna/hero.png".into(),
                        max_height: Some(220.0),
                        caption: Some("1920×1080".into()),
                    },
                    Block::Image {
                        path: "/Users/anna/a.png".into(),
                        max_height: None,
                        caption: None,
                    },
                ],
            },
            PluginMessage::SetPanelOpen {
                panel: "info".into(),
                pane: Some(PaneId::Left),
                open: true,
            },
            PluginMessage::TogglePanel {
                panel: "info".into(),
                pane: None,
            },
            PluginMessage::Log {
                level: LogLevel::Warn,
                message: "careful".into(),
            },
            PluginMessage::RunInTerminal {
                pane: Some(PaneId::Right),
                command: "rsync -a --progress 'a b.txt' /tmp/".into(),
                cwd: Some(VfsPath::new("/Users/anna")),
            },
            PluginMessage::RefreshPane { pane: None },
            PluginMessage::Select {
                pane: Some(PaneId::Left),
                paths: vec![VfsPath::new("/a/b.txt"), VfsPath::new("/a/c d.txt")],
            },
            PluginMessage::Select {
                pane: None,
                paths: vec![],
            },
        ];
        for m in msgs {
            let line = serde_json::to_string(&m).unwrap();
            match parse_line(&line) {
                Parsed::Message(back) => assert_eq!(*back, m),
                other => panic!("{line}: {other:?}"),
            }
        }
    }

    #[test]
    fn rename_and_listing_examples_in_docs_parse() {
        let docs = include_str!("../../../docs/plugins.md");
        let mut n = 0;
        for l in docs.lines().filter(|l| l.starts_with("{\"type\":")) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else {
                continue;
            }; // multi-line example
            let ty = v["type"].as_str().unwrap().to_string();
            match ty.as_str() {
                "rename_many" | "list_dir" | "read_file" | "toast" | "register_panels"
                | "run_in_terminal" | "refresh_pane" | "set_panel_content" | "select"
                | "set_status_items" => {
                    assert!(matches!(parse_line(l), Parsed::Message(_)), "{l}");
                    n += 1;
                }
                "rename_result" | "dir_listing" | "file_content" => {
                    serde_json::from_str::<HostMessage>(l).unwrap_or_else(|e| panic!("{l}: {e}"));
                    n += 1;
                }
                _ => {}
            }
        }
        assert!(n >= 10, "{n}");
        let Parsed::Message(m) = parse_line(r#"{"type":"run_in_terminal","command":"ls"}"#) else {
            panic!()
        };
        assert_eq!(
            *m,
            PluginMessage::RunInTerminal {
                pane: None,
                command: "ls".into(),
                cwd: None
            }
        );
        let Parsed::Message(m) = parse_line(
            r#"{"type":"register_panels","panels":[{"id":"w","title":"W","placement":"modal","size":[640,480]}]}"#,
        ) else {
            panic!()
        };
        let PluginMessage::RegisterPanels { panels } = *m else {
            panic!()
        };
        assert_eq!(panels[0].placement, PanelPlacement::Modal);
        assert_eq!(panels[0].size, Some([640.0, 480.0]));
        // The image block example.
        let image = docs
            .lines()
            .find(|l| l.starts_with("{\"type\":\"set_panel_content\"") && l.contains("\"image\""))
            .expect("an image block example in docs/plugins.md");
        let Parsed::Message(m) = parse_line(image) else {
            panic!("{image}")
        };
        let PluginMessage::SetPanelContent { blocks, .. } = *m else {
            panic!()
        };
        assert!(
            matches!(&blocks[0], Block::Image { path, max_height: Some(h), caption: Some(_) } if path.starts_with('/') && *h == 240.0),
            "{blocks:?}"
        );
    }

    #[test]
    fn image_blocks_are_local_and_capped() {
        assert_eq!(
            image_block_path("/Users/anna/a b.png"),
            Some(PathBuf::from("/Users/anna/a b.png"))
        );
        for bad in [
            "",
            "a.png",
            "./a.png",
            "~/a.png",
            "file:///a.png",
            "https://x.org/a.png",
            "data:image/png;base64,AA",
            "/Users/anna/../../etc/passwd",
            "/a\0b.png",
        ] {
            assert_eq!(image_block_path(bad), None, "{bad}");
        }
        assert_eq!(
            image_block_path(&format!("/{}", "a".repeat(MAX_IMAGE_PATH))),
            None
        );
        let Parsed::Message(m) = parse_line(
            r#"{"type":"set_panel_content","panel":"p","blocks":[{"type":"image","path":"/a.png","max_height":99999},{"type":"image","path":"/b.png"}]}"#,
        ) else {
            panic!()
        };
        let PluginMessage::SetPanelContent { blocks, .. } = *m else {
            panic!()
        };
        assert_eq!(
            blocks[0],
            Block::Image {
                path: "/a.png".into(),
                max_height: Some(800.0),
                caption: None
            }
        );
        assert_eq!(
            blocks[1],
            Block::Image {
                path: "/b.png".into(),
                max_height: None,
                caption: None
            }
        );
    }

    #[test]
    fn minimal_messages_use_defaults() {
        let Parsed::Message(m) = parse_line(r#"{"type":"toast","text":"Hello"}"#) else {
            panic!()
        };
        assert_eq!(
            *m,
            PluginMessage::Toast {
                level: ToastLevel::Info,
                text: "Hello".into(),
                button: None,
                command: None,
            }
        );
        let Parsed::Message(m) = parse_line(
            r#"{"type":"register_commands","commands":[{"id":"a","title":"A","extra":1}]}"#,
        ) else {
            panic!()
        };
        let PluginMessage::RegisterCommands { commands } = *m else {
            panic!()
        };
        assert_eq!(commands[0].context, CommandContext::Palette);
        let Parsed::Message(m) = parse_line(
            r#"{"type":"set_decorations","pane":"left","dir":"/a","entries":{"x":{"dimmed":true}}}"#,
        ) else {
            panic!()
        };
        let PluginMessage::SetDecorations { entries, .. } = *m else {
            panic!()
        };
        assert!(entries["x"].dimmed);
        // set_sizes: only bytes and complete are required.
        let Parsed::Message(m) = parse_line(
            r#"{"type":"set_sizes","pane":"right","dir":"/a","sizes":{"src":{"bytes":10,"complete":true}}}"#,
        ) else {
            panic!()
        };
        let PluginMessage::SetSizes { sizes, .. } = *m else {
            panic!()
        };
        assert_eq!(
            sizes["src"],
            SizeInfo {
                bytes: 10,
                complete: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn unknown_and_invalid_lines() {
        assert_eq!(
            parse_line(r#"{"type":"teleport","to":"mars"}"#),
            Parsed::Unknown("teleport".into())
        );
        assert!(matches!(parse_line("hello"), Parsed::Invalid(_)));
        assert!(matches!(parse_line(r#"{"text":"x"}"#), Parsed::Invalid(_)));
        assert!(matches!(
            parse_line(r#"{"type":"toast"}"#),
            Parsed::Invalid(e) if e.contains("toast")
        ));
        assert_eq!(parse_line("   "), Parsed::Invalid(String::new()));
    }

    #[test]
    fn floods_are_capped() {
        let mut entries = serde_json::Map::new();
        for i in 0..(MAX_DECORATIONS + 50) {
            entries.insert(
                format!("f{i}"),
                serde_json::json!({"badge": {"text": "LONG", "tone": "danger"}}),
            );
        }
        let line = serde_json::json!({
            "type": "set_decorations", "pane": "left", "dir": "/", "entries": entries
        })
        .to_string();
        let Parsed::Message(m) = parse_line(&line) else {
            panic!()
        };
        let PluginMessage::SetDecorations { entries, .. } = *m else {
            panic!()
        };
        assert_eq!(entries.len(), MAX_DECORATIONS);
        assert!(entries
            .values()
            .all(|d| d.badge.as_ref().unwrap().text == "LON"));

        let chips: Vec<_> = (0..20)
            .map(|i| serde_json::json!({"text": format!("c{i}")}))
            .collect();
        let line = serde_json::json!({"type":"set_chips","pane":"right","dir":"/","chips":chips})
            .to_string();
        let Parsed::Message(m) = parse_line(&line) else {
            panic!()
        };
        let PluginMessage::SetChips { chips, .. } = *m else {
            panic!()
        };
        assert_eq!(chips.len(), MAX_CHIPS);
    }

    #[test]
    fn status_items_wire_format() {
        use crate::{StatusAction, StatusLevel};
        // Only id and text are required.
        let Parsed::Message(m) =
            parse_line(r#"{"type":"set_status_items","items":[{"id":"a","text":"Hi"}]}"#)
        else {
            panic!()
        };
        assert_eq!(
            *m,
            PluginMessage::SetStatusItems {
                items: vec![StatusItem::new("a", "Hi")]
            }
        );
        // Every field, as in docs/plugins.md.
        let line = r#"{"type":"set_status_items","items":[{"id":"cpu","text":"CPU 91 %","compact":"","tooltip":"10 cores","icon":"cpu","level":"warn","meter":0.91,"priority":30,"action":{"kind":"open_app","bundle_id":"com.apple.ActivityMonitor"}},{"id":"vpn","text":"VPN","action":{"kind":"command","command":"toggle-vpn"}},{"id":"docs","text":"Docs","level":"error","action":{"kind":"open_url","url":"https://example.org"}}]}"#;
        let Parsed::Message(m) = parse_line(line) else {
            panic!()
        };
        let PluginMessage::SetStatusItems { items } = *m.clone() else {
            panic!()
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].compact.as_deref(), Some(""));
        assert_eq!(items[0].level, StatusLevel::Warn);
        assert_eq!(items[0].meter, Some(0.91));
        assert_eq!(items[0].priority, 30);
        assert_eq!(
            items[0].action,
            Some(StatusAction::OpenApp {
                bundle_id: "com.apple.ActivityMonitor".into()
            })
        );
        assert_eq!(
            items[1].action,
            Some(StatusAction::Command {
                command: "toggle-vpn".into()
            })
        );
        assert_eq!(items[2].level, StatusLevel::Error);
        assert_eq!(
            items[2].action,
            Some(StatusAction::OpenUrl {
                url: "https://example.org".into()
            })
        );
        // Round trip; defaults are left out.
        let out = serde_json::to_string(&*m).unwrap();
        assert!(out.contains(r#""type":"set_status_items""#), "{out}");
        assert!(!out.contains(r#""compact":null"#), "{out}");
        let Parsed::Message(back) = parse_line(&out) else {
            panic!("{out}")
        };
        assert_eq!(back, m);
        // Clamped: too many items, long text, meter out of range, bad actions.
        let items: Vec<_> = (0..20)
            .map(|i| {
                serde_json::json!({"id": format!("i{i}"), "text": "x".repeat(100), "meter": 7.5,
                    "action": {"kind": "open_app", "bundle_id": "-n"}})
            })
            .collect();
        let line = serde_json::json!({"type": "set_status_items", "items": items}).to_string();
        let Parsed::Message(m) = parse_line(&line) else {
            panic!()
        };
        let PluginMessage::SetStatusItems { items } = *m else {
            panic!()
        };
        assert_eq!(items.len(), MAX_STATUS_ITEMS);
        assert_eq!(items[0].text.chars().count(), 41);
        assert_eq!(items[0].meter, Some(1.0));
        assert_eq!(items[0].action, None);
        // Unknown action kinds make the message invalid, not a crash.
        assert!(matches!(
            parse_line(
                r#"{"type":"set_status_items","items":[{"id":"a","text":"b","action":{"kind":"teleport"}}]}"#
            ),
            Parsed::Invalid(_)
        ));
    }

    #[test]
    fn status_actions_are_checked() {
        use crate::StatusAction::*;
        assert!(OpenUrl {
            url: "https://example.org".into()
        }
        .is_valid());
        assert!(OpenUrl {
            url: "x-apple.systempreferences:com.apple.preference.security".into()
        }
        .is_valid());
        for bad in ["", "example.org", "-a:b", ":x", "1http://x"] {
            assert!(!OpenUrl { url: bad.into() }.is_valid(), "{bad}");
        }
        assert!(OpenApp {
            bundle_id: "com.apple.ActivityMonitor".into()
        }
        .is_valid());
        for bad in ["", "-b", ".x", "com.apple/../x", "a b"] {
            assert!(
                !OpenApp {
                    bundle_id: bad.into()
                }
                .is_valid(),
                "{bad}"
            );
        }
        assert!(Command {
            command: "go".into()
        }
        .is_valid());
        assert!(!Command {
            command: String::new()
        }
        .is_valid());
    }

    #[test]
    fn read_file_defaults_to_one_mib() {
        let m: PluginMessage =
            serde_json::from_str(r#"{"type":"read_file","pane":"left","path":"/a/README.md"}"#)
                .unwrap();
        assert_eq!(
            m,
            PluginMessage::ReadFile {
                pane: PaneId::Left,
                path: VfsPath::new("/a/README.md"),
                max_bytes: DEFAULT_READ_BYTES
            }
        );
    }

    #[test]
    fn pane_ids_left_right_and_numbers() {
        // Out: 0 and 1 keep their old names, more panes are numbers.
        assert_eq!(serde_json::to_string(&PaneId::LEFT).unwrap(), r#""left""#);
        assert_eq!(serde_json::to_string(&PaneId::RIGHT).unwrap(), r#""right""#);
        assert_eq!(serde_json::to_string(&PaneId(3)).unwrap(), "3");
        let m = HostMessage::PaneClosed { pane: PaneId(2) };
        assert_eq!(m.to_line().trim(), r#"{"type":"pane_closed","pane":2}"#);
        let m = HostMessage::PaneOpened {
            pane: PaneId::RIGHT,
        };
        assert_eq!(
            m.to_line().trim(),
            r#"{"type":"pane_opened","pane":"right"}"#
        );
        // In: names, numbers and numeric strings.
        for (json, id) in [
            (r#""left""#, PaneId::LEFT),
            (r#""right""#, PaneId::RIGHT),
            ("0", PaneId::LEFT),
            ("1", PaneId::RIGHT),
            ("2", PaneId(2)),
            (r#""7""#, PaneId(7)),
        ] {
            assert_eq!(serde_json::from_str::<PaneId>(json).unwrap(), id, "{json}");
        }
        for bad in [r#""middle""#, "-1", "70000", "1.5", "null"] {
            assert!(serde_json::from_str::<PaneId>(bad).is_err(), "{bad}");
        }
        // Requests from plugins address any pane, in either form.
        let m: PluginMessage =
            serde_json::from_str(r#"{"type":"read_file","pane":2,"path":"/a/x"}"#).unwrap();
        assert!(matches!(
            m,
            PluginMessage::ReadFile {
                pane: PaneId(2),
                ..
            }
        ));
        let m: PluginMessage =
            serde_json::from_str(r#"{"type":"refresh_pane","pane":"right"}"#).unwrap();
        assert!(
            matches!(
                m,
                PluginMessage::RefreshPane {
                    pane: Some(PaneId::RIGHT)
                }
            ),
            "{m:?}"
        );
        let m: PluginMessage = serde_json::from_str(r#"{"type":"refresh_pane","pane":4}"#).unwrap();
        assert!(
            matches!(
                m,
                PluginMessage::RefreshPane {
                    pane: Some(PaneId(4))
                }
            ),
            "{m:?}"
        );
        // A pane state for pane 3 round-trips.
        let st: crate::PaneState = serde_json::from_value(serde_json::json!({
            "pane": 3, "path": "/x", "local_path": "/x", "protocol": "Local",
            "backend_label": "Local", "selection": [], "active": false
        }))
        .unwrap();
        assert_eq!(st.pane, PaneId(3));
        assert_eq!(serde_json::to_value(&st).unwrap()["pane"], 3);
    }
}
