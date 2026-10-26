//! Message types for writing [Manta](https://manta-filemanager.com) plugins
//! in Rust.
//!
//! A Manta plugin is a program that Manta starts and talks to with **JSON
//! lines**: one JSON object per line, Manta → plugin on stdin
//! ([`HostMessage`]), plugin → Manta on stdout ([`PluginMessage`]). stderr
//! is the plugin's log. The full reference with every field is
//! `docs/plugins.md` in the Manta repository.
//!
//! This crate has no dependency on Manta itself (only `serde` and
//! `serde_json`), so plugins can use it straight from git:
//!
//! ```toml
//! [dependencies]
//! manta-plugin-protocol = { git = "https://github.com/web-workers-com/manta" }
//! ```
//!
//! A minimal plugin:
//!
//! ```no_run
//! use manta_plugin_protocol::{run, HostMessage, Out, PluginMessage, ToastLevel};
//!
//! fn main() -> std::io::Result<()> {
//!     run(|msg, out: &mut Out| {
//!         if let HostMessage::RunCommand { id, .. } = msg {
//!             if id == "hello" {
//!                 out.send(&PluginMessage::toast(ToastLevel::Success, "Hello from Rust!"))?;
//!             }
//!         }
//!         Ok(())
//!     })
//! }
//! ```
//!
//! # Compatibility
//!
//! * The crate's major version is the protocol's `api_version` (1).
//! * Manta may add message types and optional fields within one
//!   `api_version`. Unknown host messages parse as [`HostMessage::Unknown`],
//!   unknown fields are ignored, unknown enum values map to an `Other` /
//!   `Unknown` variant where Manta documents that new values may appear.
//! * Manta's own test suite checks this crate against the host's types, so
//!   the two can't drift apart unnoticed.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

/// The protocol version this crate speaks (`api_version` in `plugin.toml`
/// and in [`HostMessage::Init`]).
pub const API_VERSION: u32 = 1;

/// Every `type` a plugin may send, in the order of [`PluginMessage`].
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

/// Every `type` Manta sends with api_version 1, in the order of
/// [`HostMessage`] (without `Unknown`).
pub const HOST_MESSAGE_TYPES: &[&str] = &[
    "init",
    "locale_changed",
    "directory_changed",
    "listing_refreshed",
    "selection_changed",
    "active_pane_changed",
    "pane_opened",
    "pane_closed",
    "file_action",
    "run_command",
    "panel_action",
    "shutdown",
    "rename_result",
    "dir_listing",
    "file_content",
];

// ------------------------------------------------------------------ panes

/// A pane id. JSON: `"left"` (0), `"right"` (1) or a number for every
/// further pane. Ids are identities, not positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Pane(pub u16);

impl Pane {
    pub const LEFT: Pane = Pane(0);
    pub const RIGHT: Pane = Pane(1);
}

impl Serialize for Pane {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            0 => s.serialize_str("left"),
            1 => s.serialize_str("right"),
            n => s.serialize_u16(n),
        }
    }
}

impl<'de> Deserialize<'de> for Pane {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Pane;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("\"left\", \"right\" or a pane number")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Pane, E> {
                match v {
                    "left" => Ok(Pane::LEFT),
                    "right" => Ok(Pane::RIGHT),
                    n => n
                        .parse()
                        .map(Pane)
                        .map_err(|_| E::custom(format!("unknown pane {v:?}"))),
                }
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Pane, E> {
                u16::try_from(v)
                    .map(Pane)
                    .map_err(|_| E::custom("pane number out of range"))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Pane, E> {
                u16::try_from(v)
                    .map(Pane)
                    .map_err(|_| E::custom("pane number out of range"))
            }
        }
        d.deserialize_any(V)
    }
}

/// What a plugin knows about a pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    pub pane: Pane,
    /// The folder inside the pane's backend.
    pub path: String,
    /// The real OS path, or `None` for servers, containers and archives.
    /// Check it before touching files.
    #[serde(default)]
    pub local_path: Option<String>,
    /// `"Local"`, `"Sftp"`, `"Ftp"`, `"Ftps"`, `"S3"`, `"Docker"`,
    /// `"Kubernetes"` … (treat unknown values as remote).
    pub protocol: String,
    /// e.g. `"Local"` or `"deploy@web-01 (SFTP)"`.
    #[serde(default)]
    pub backend_label: String,
    /// Selected paths in display order (the entry under the cursor when
    /// nothing is selected).
    #[serde(default)]
    pub selection: Vec<String>,
    #[serde(default)]
    pub active: bool,
    /// The server, container or bucket the pane shows.
    #[serde(default)]
    pub remote: Option<RemoteEndpoint>,
}

/// A server, container or bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEndpoint {
    pub protocol: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    /// Kubernetes: the kubeconfig context (omitted when it is the current one).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context: String,
}

// ------------------------------------------------------------------ host → plugin

/// Messages Manta writes to the plugin's stdin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    /// Always the first message.
    Init {
        api_version: u32,
        plugin_id: String,
        /// A folder of the plugin's own for settings and caches (exists).
        data_dir: String,
        #[serde(default)]
        panes: Vec<PaneState>,
        /// The app's UI language (`"en"`, `"de"`, `"zh-Hans"` …).
        #[serde(default = "default_locale")]
        locale: String,
    },
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
        pane: Pane,
    },
    PaneOpened {
        pane: Pane,
    },
    PaneClosed {
        pane: Pane,
    },
    /// Informational: a file action the user did in the GUI started or
    /// finished.
    FileAction {
        action: FileAction,
    },
    /// The user ran one of your commands (`pane`: the active pane).
    RunCommand {
        id: String,
        pane: PaneState,
    },
    /// A panel row or button with an `action` was clicked.
    PanelAction {
        panel: String,
        action: String,
        pane: PaneState,
    },
    /// Exit within 500 ms.
    Shutdown,
    /// Answer to [`PluginMessage::RenameMany`].
    RenameResult {
        pane: Pane,
        dir: String,
        #[serde(default)]
        ok: Vec<(String, String)>,
        #[serde(default)]
        failed: Vec<(String, String)>,
    },
    /// Answer to [`PluginMessage::ListDir`].
    DirListing {
        pane: Pane,
        dir: String,
        #[serde(default)]
        entries: Vec<ListedEntry>,
        #[serde(default)]
        error: Option<String>,
    },
    /// Answer to [`PluginMessage::ReadFile`]; `data` is base64 (standard
    /// alphabet, padded).
    FileContent {
        pane: Pane,
        path: String,
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
    /// A message type newer than this crate: ignore it.
    #[serde(other)]
    Unknown,
}

fn default_locale() -> String {
    "en".into()
}

/// One entry of a [`HostMessage::DirListing`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedEntry {
    pub name: String,
    #[serde(default)]
    pub is_dir: bool,
    #[serde(default)]
    pub size: u64,
    /// Unix seconds.
    #[serde(default)]
    pub modified: Option<i64>,
    /// Visible in the pane (not hidden or filtered out).
    #[serde(default)]
    pub shown: bool,
}

/// A file action done in the GUI (see `docs/plugins.md`, "FileAction").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileAction {
    pub id: u64,
    pub pane: Pane,
    pub kind: FileActionKind,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub src_local: bool,
    #[serde(default)]
    pub dst_local: bool,
    #[serde(default)]
    pub src_label: String,
    #[serde(default)]
    pub dst_label: Option<String>,
    pub phase: FileActionPhase,
    #[serde(default)]
    pub src_remote: Option<RemoteEndpoint>,
    #[serde(default)]
    pub dst_remote: Option<RemoteEndpoint>,
    /// `sync` only.
    #[serde(default)]
    pub sync: Option<serde_json::Value>,
    /// Batch renames: every `[old, new]` pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renames: Vec<(String, String)>,
    /// `compress` / `extract` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileActionKind {
    Copy,
    Move,
    Delete,
    Trash,
    Rename,
    NewFolder,
    NewFile,
    Open,
    Connect,
    Disconnect,
    Sync,
    Compress,
    Extract,
    /// A kind newer than this crate.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileActionPhase {
    Started,
    Finished {
        ok: bool,
        files: u64,
        bytes: u64,
        #[serde(default)]
        errors: Vec<String>,
        #[serde(default)]
        cancelled: bool,
        elapsed_ms: u64,
    },
}

// ------------------------------------------------------------------ plugin → host

/// Semantic colour, mapped to the light or dark theme by Manta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Info,
    Muted,
}

/// 1–3 characters next to a file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    pub text: String,
    pub tone: Tone,
}

/// How one entry of a folder is decorated.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Decoration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<Badge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_tone: Option<Tone>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dimmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
}

/// A size in the Size column (usually of a folder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SizeInfo {
    pub bytes: u64,
    /// `false` while still counting.
    pub complete: bool,
    #[serde(default)]
    pub files: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<u64>,
    #[serde(default)]
    pub unreadable: u64,
}

/// A chip in the pane's path bar.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Chip {
    pub text: String,
    #[serde(default)]
    pub tone: Tone,
    /// An icon name, e.g. `"branch"` (see `docs/plugins.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// A command of your plugin, run on click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandContext {
    /// The command palette (⇧⌘P).
    #[default]
    Palette,
    /// The palette and the file list's context menu.
    FileContextMenu,
}

/// A command (as in `plugin.toml` or `register_commands`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDecl {
    /// No `:` or spaces; unique within the plugin.
    pub id: String,
    pub title: String,
    /// e.g. `"⌃⌥H"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub context: CommandContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelPlacement {
    /// One per pane, below the file list.
    #[default]
    PaneBottom,
    /// One, on the right side of the window.
    RightSidebar,
    /// A sheet over the dimmed window.
    Modal,
}

/// A panel (as in `plugin.toml` or `register_panels`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelDecl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub placement: PanelPlacement,
    #[serde(default)]
    pub default_open: bool,
    /// `[width, height]` of a `modal` panel, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
}

/// One block of a declarative panel.
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
    Code {
        text: String,
    },
    KeyValue {
        rows: Vec<KeyValueRow>,
    },
    List {
        rows: Vec<ListRow>,
    },
    Buttons {
        buttons: Vec<ButtonSpec>,
    },
    Separator,
    Section {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Tags {
        tags: Vec<Tag>,
    },
    /// An absolute local image path (PNG, JPEG, GIF, WebP, BMP).
    Image {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_height: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValueRow {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ListRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub tone: Tone,
    /// Sent back as `panel_action` when clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub text: String,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusLevel {
    #[default]
    Normal,
    Warn,
    Error,
}

/// What a click on a status item does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatusAction {
    Command { command: String },
    OpenUrl { url: String },
    OpenApp { bundle_id: String },
}

/// An item in the window's status bar.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StatusItem {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub level: StatusLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meter: Option<f32>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<StatusAction>,
}

/// Messages a plugin writes to stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMessage {
    /// Replaces your decorations for the folder `dir` that `pane` shows.
    SetDecorations {
        pane: Pane,
        dir: String,
        entries: BTreeMap<String, Decoration>,
    },
    /// Merged into your earlier sizes for `dir`.
    SetSizes {
        pane: Pane,
        dir: String,
        sizes: BTreeMap<String, SizeInfo>,
    },
    /// Up to 6 chips in the path bar while `pane` shows `dir`.
    SetChips {
        pane: Pane,
        dir: String,
        chips: Vec<Chip>,
    },
    Toast {
        level: ToastLevel,
        text: String,
        /// A button label; needs `command`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
    Navigate {
        pane: Pane,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<RemoteEndpoint>,
    },
    RegisterCommands {
        commands: Vec<CommandDecl>,
    },
    RegisterPanels {
        panels: Vec<PanelDecl>,
    },
    SetPanelContent {
        panel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<Pane>,
        blocks: Vec<Block>,
    },
    SetPanelOpen {
        panel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<Pane>,
        open: bool,
    },
    TogglePanel {
        panel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<Pane>,
    },
    /// Answered with [`HostMessage::RenameResult`].
    RenameMany {
        pane: Pane,
        dir: String,
        renames: Vec<(String, String)>,
    },
    /// Answered with [`HostMessage::DirListing`].
    ListDir {
        pane: Pane,
        dir: String,
    },
    /// Answered with [`HostMessage::FileContent`]. `max_bytes`: default
    /// 1 MiB, at most 16 MiB.
    ReadFile {
        pane: Pane,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_bytes: Option<u64>,
    },
    RunInTerminal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<Pane>,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    RefreshPane {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<Pane>,
    },
    Select {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane: Option<Pane>,
        paths: Vec<String>,
    },
    /// Replaces your status bar items (`[]` removes them).
    SetStatusItems {
        items: Vec<StatusItem>,
    },
    Log {
        level: LogLevel,
        message: String,
    },
}

impl PluginMessage {
    /// A toast without a button.
    pub fn toast(level: ToastLevel, text: impl Into<String>) -> Self {
        PluginMessage::Toast {
            level,
            text: text.into(),
            button: None,
            command: None,
        }
    }

    /// A line for Settings → Plugins → Show Log.
    pub fn log(level: LogLevel, message: impl Into<String>) -> Self {
        PluginMessage::Log {
            level,
            message: message.into(),
        }
    }

    /// One JSON line, with the trailing newline.
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).expect("plugin messages always serialize");
        s.push('\n');
        s
    }
}

impl HostMessage {
    /// Parse one line from stdin.
    pub fn parse(line: &str) -> serde_json::Result<HostMessage> {
        serde_json::from_str(line.trim())
    }
}

// ------------------------------------------------------------------ stdio loop

/// Where a plugin writes its messages: stdout, flushed after every line.
pub struct Out {
    w: Box<dyn Write>,
}

impl Out {
    /// Stdout.
    pub fn stdout() -> Out {
        Out {
            w: Box::new(io::stdout()),
        }
    }

    /// Any writer (tests).
    pub fn new(w: impl Write + 'static) -> Out {
        Out { w: Box::new(w) }
    }

    /// Write one message and flush (Manta reads line by line).
    pub fn send(&mut self, msg: &PluginMessage) -> io::Result<()> {
        self.w.write_all(msg.to_line().as_bytes())?;
        self.w.flush()
    }
}

/// Read host messages from stdin and call `handle` for each, until
/// `shutdown` or the end of stdin. Lines that don't parse are reported on
/// stderr (your log) and skipped. Returns when the plugin should exit.
pub fn run(handle: impl FnMut(HostMessage, &mut Out) -> io::Result<()>) -> io::Result<()> {
    run_with(io::stdin().lock(), Out::stdout(), handle)
}

/// [`run`] with any reader and [`Out`] (tests).
pub fn run_with(
    input: impl BufRead,
    mut out: Out,
    mut handle: impl FnMut(HostMessage, &mut Out) -> io::Result<()>,
) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match HostMessage::parse(&line) {
            Ok(HostMessage::Shutdown) => {
                handle(HostMessage::Shutdown, &mut out)?;
                break;
            }
            Ok(msg) => handle(msg, &mut out)?,
            Err(e) => eprintln!("manta-plugin-protocol: skipped a line I can't read: {e}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn panes_use_names_and_numbers() {
        assert_eq!(serde_json::to_string(&Pane::LEFT).unwrap(), "\"left\"");
        assert_eq!(serde_json::to_string(&Pane(3)).unwrap(), "3");
        for s in ["\"right\"", "1", "\"1\""] {
            assert_eq!(serde_json::from_str::<Pane>(s).unwrap(), Pane::RIGHT);
        }
    }

    #[test]
    fn unknown_messages_and_kinds_are_tolerated() {
        assert_eq!(
            HostMessage::parse(r#"{"type":"from_the_future","x":1}"#).unwrap(),
            HostMessage::Unknown
        );
        let m = HostMessage::parse(
            r#"{"type":"file_action","action":{"id":1,"pane":"left","kind":"teleport","phase":"started","new_field":true}}"#,
        )
        .unwrap();
        let HostMessage::FileAction { action } = m else {
            panic!()
        };
        assert_eq!(action.kind, FileActionKind::Other);
        let m = HostMessage::parse(
            r#"{"type":"init","api_version":1,"plugin_id":"x","data_dir":"/tmp"}"#,
        )
        .unwrap();
        assert!(matches!(m, HostMessage::Init { locale, .. } if locale == "en"));
    }

    #[test]
    fn messages_serialize_to_the_documented_json() {
        let m = PluginMessage::toast(ToastLevel::Success, "Hi");
        assert_eq!(
            m.to_line(),
            "{\"type\":\"toast\",\"level\":\"success\",\"text\":\"Hi\"}\n"
        );
        let m = PluginMessage::SetDecorations {
            pane: Pane::LEFT,
            dir: "/a".into(),
            entries: [(
                "big.iso".to_string(),
                Decoration {
                    badge: Some(Badge {
                        text: "L".into(),
                        tone: Tone::Warning,
                    }),
                    ..Default::default()
                },
            )]
            .into(),
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"type":"set_decorations","pane":"left","dir":"/a","entries":{"big.iso":{"badge":{"text":"L","tone":"warning"}}}}"#
        );
    }

    #[test]
    fn run_answers_until_shutdown() {
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl Write for Buf {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let input = concat!(
            "{\"type\":\"init\",\"api_version\":1,\"plugin_id\":\"x\",\"data_dir\":\"/tmp\",\"panes\":[],\"locale\":\"de\"}\n",
            "not json\n",
            "{\"type\":\"run_command\",\"id\":\"hello\",\"pane\":{\"pane\":\"left\",\"path\":\"/\",\"local_path\":\"/\",\"protocol\":\"Local\",\"backend_label\":\"Local\",\"selection\":[],\"active\":true}}\n",
            "{\"type\":\"shutdown\"}\n",
            "{\"type\":\"run_command\",\"id\":\"after\",\"pane\":{\"pane\":\"left\",\"path\":\"/\",\"protocol\":\"Local\"}}\n",
        );
        let buf = Buf::default();
        let mut seen = Vec::new();
        run_with(input.as_bytes(), Out::new(buf.clone()), |m, out| {
            if let HostMessage::RunCommand { id, .. } = &m {
                out.send(&PluginMessage::toast(ToastLevel::Info, id.clone()))?;
            }
            seen.push(m);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            seen.len(),
            3,
            "init, run_command, shutdown; nothing after shutdown"
        );
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            out,
            "{\"type\":\"toast\",\"level\":\"info\",\"text\":\"hello\"}\n"
        );
    }
}
