//! Native drag & drop between Manta and other apps (Finder, the Desktop,
//! Mail, cloud sync apps, …).
//!
//! # Drag OUT (Manta → other apps)
//!
//! egui's own drag & drop ([`egui::DragAndDrop`]) only lives inside the
//! window. When an internal drag leaves the window, [`drag_out_when_leaving`]
//! hands it over to AppKit: a real `NSDraggingSession` starts on winit's
//! content view and from then on macOS moves the files, exactly like a drag
//! that started in Finder.
//!
//! * **Trigger** ([`Trigger`]): a drag that was seen *inside* the window
//!   (primary button down) whose pointer then leaves the window's rect fires
//!   once. It re-arms after the button is released or the hook is not called
//!   for a frame. The internal payload is cleared when the native session
//!   starts (AppKit owns the mouse from then on; dragging back into Manta
//!   arrives as an ordinary file drop, see below).
//! * **Local files** go on the pasteboard as file URLs (`public.file-url`,
//!   AppKit also derives the legacy `NSFilenamesPboardType` from them), with
//!   the Finder icon as drag image. Several items fly as a stack; macOS draws
//!   the count badge.
//! * **Remote items** (SFTP/FTP/S3/containers/archives/…) are file promises
//!   (`NSFilePromiseProvider`): the destination picks a folder and Manta
//!   downloads the item there through [`RemoteItem::fetch`] on a background
//!   `NSOperationQueue`. Errors go back to the destination through the
//!   completion handler and to the app as [`DragEvent::PromiseDone`].
//! * **Operations** ([`source_mask`]): outside Manta only *Copy* is offered
//!   (*Move* as well for local files while ⌘ is held; copy stays the default).
//!   Inside Manta Copy | Move | Generic. The result is reported as
//!   [`DragEvent::Ended`]; Manta never deletes anything because of it, a Move
//!   only means the app should refresh the source folder.
//!
//! # Drag IN (other apps → a Manta pane)
//!
//! winit registers the window for file drags and egui exposes them as
//! `raw.hovered_files` / `raw.dropped_files`. [`external_hover`] and
//! [`take_external_drop_in`] add what the pane code needs on top: the pointer
//! position (winit sends no mouse moves during an external drag, so it is read
//! from AppKit), one-consumer-per-frame semantics and whether the drop is our
//! own drag coming back. File *promises* (Mail attachments, Photos) are not
//! delivered by winit, which only accepts `NSFilenamesPboardType`.
//!
//! All AppKit work lives in `mac` and runs on the main thread, deferred out of
//! winit's event handling with `performSelectorOnMainThread:` like
//! `quicklook.rs` does. Unit tests never touch AppKit.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use manta_core::{SharedVfs, VfsPath};

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod mac;
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
pub mod selftest;

/// Downloads one remote item to the given local path (file or folder).
/// Runs on a background thread; must block until done.
pub type FetchFn = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// An item that only exists remotely: dragged out as a file promise.
#[derive(Clone)]
pub struct RemoteItem {
    /// Where it comes from, for logs and toasts ("user@host (SFTP)").
    pub backend: String,
    /// File name the destination should create.
    pub name: String,
    pub size: Option<u64>,
    pub is_dir: bool,
    pub fetch: FetchFn,
}

impl std::fmt::Debug for RemoteItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteItem")
            .field("backend", &self.backend)
            .field("name", &self.name)
            .field("size", &self.size)
            .field("is_dir", &self.is_dir)
            .finish_non_exhaustive()
    }
}

/// One dragged item.
#[derive(Clone, Debug)]
pub enum DragItem {
    /// A file or folder on this Mac.
    Local(PathBuf),
    /// Downloaded on demand when dropped (file promise).
    Remote(RemoteItem),
}

impl DragItem {
    /// The item at `path` on `fs`: local when the backend has a real OS path,
    /// otherwise a promise that downloads through the VFS (folders
    /// recursively).
    pub fn from_vfs(fs: &SharedVfs, path: &VfsPath, is_dir: bool, size: Option<u64>) -> DragItem {
        if let Some(p) = fs.local_path(path) {
            return DragItem::Local(p);
        }
        let fs2 = fs.clone();
        let src = path.clone();
        DragItem::Remote(RemoteItem {
            backend: fs.label(),
            name: Some(path.file_name())
                .filter(|n| !n.is_empty())
                .unwrap_or("item")
                .to_string(),
            size,
            is_dir,
            fetch: Arc::new(move |dest| download_vfs(&fs2, &src, is_dir, dest)),
        })
    }
}

/// [`DragItem::from_vfs`] for a whole selection (`dirs[i]`: `paths[i]` is a
/// folder; missing entries count as files).
pub fn items_from_vfs(fs: &SharedVfs, paths: &[VfsPath], dirs: &[bool]) -> Vec<DragItem> {
    paths
        .iter()
        .enumerate()
        .map(|(i, p)| DragItem::from_vfs(fs, p, dirs.get(i).copied().unwrap_or(false), None))
        .collect()
}

/// Copy `src` (a file, or a folder recursively) from `fs` to the local `dest`.
pub fn download_vfs(
    fs: &SharedVfs,
    src: &VfsPath,
    is_dir: bool,
    dest: &Path,
) -> Result<(), String> {
    if is_dir {
        std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        for e in fs.list(src).map_err(|e| e.to_string())? {
            download_vfs(
                fs,
                &e.path,
                e.is_dir(),
                &dest.join(promise_file_name(&e.name)),
            )?;
        }
        return Ok(());
    }
    let mut r = fs.open_read(src).map_err(|e| e.to_string())?;
    let mut w = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    std::io::copy(&mut r, &mut w).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pure mapping (unit-tested)

/// UTI of a promised file (the destination uses the name's extension).
pub const UTI_DATA: &str = "public.data";
/// UTI of a promised folder.
pub const UTI_FOLDER: &str = "public.folder";

/// What one dragged item puts on the pasteboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteboardEntry {
    /// `public.file-url` (an `NSURL` file URL).
    FileUrl(PathBuf),
    /// `NSFilePromiseProvider` with this name and UTI.
    Promise {
        file_name: String,
        file_type: &'static str,
    },
}

pub fn pasteboard_plan(items: &[DragItem]) -> Vec<PasteboardEntry> {
    items
        .iter()
        .map(|i| match i {
            DragItem::Local(p) => PasteboardEntry::FileUrl(p.clone()),
            DragItem::Remote(r) => PasteboardEntry::Promise {
                file_name: promise_file_name(&r.name),
                file_type: if r.is_dir { UTI_FOLDER } else { UTI_DATA },
            },
        })
        .collect()
}

/// A safe single path component for a promised file: no `/` or `:`, no
/// control characters, not empty, not `.`/`..`, at most 255 bytes.
pub fn promise_file_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == ':' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    s = s.trim().to_string();
    if s.is_empty() || s == "." || s == ".." {
        return "Untitled".into();
    }
    if s.len() > 255 {
        let mut cut = 255;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// `NSDragOperation` bits.
pub mod op {
    pub const NONE: u64 = 0;
    pub const COPY: u64 = 1;
    pub const LINK: u64 = 2;
    pub const GENERIC: u64 = 4;
    pub const MOVE: u64 = 16;
    pub const DELETE: u64 = 32;
}

/// The operations offered to a destination. Outside Manta: Copy, plus Move
/// for local files while ⌘ is held (Finder's "force move"). Promises can
/// only be copied. Inside Manta: Copy | Move | Generic.
pub fn source_mask(outside_app: bool, all_local: bool, cmd_held: bool) -> u64 {
    if !outside_app {
        return op::COPY | op::MOVE | op::GENERIC;
    }
    if all_local && cmd_held {
        op::COPY | op::MOVE
    } else {
        op::COPY
    }
}

/// What the destination did, from `draggingSession:endedAtPoint:operation:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropOp {
    /// Cancelled or refused.
    None,
    Copy,
    /// The destination moved the files: refresh the source folder.
    Move,
    Link,
    /// Dropped on the Trash.
    Delete,
    Other,
}

impl DropOp {
    pub fn from_bits(bits: u64) -> DropOp {
        if bits == op::NONE {
            DropOp::None
        } else if bits & op::MOVE != 0 {
            DropOp::Move
        } else if bits & op::DELETE != 0 {
            DropOp::Delete
        } else if bits & op::COPY != 0 {
            DropOp::Copy
        } else if bits & op::LINK != 0 {
            DropOp::Link
        } else {
            DropOp::Other
        }
    }

    /// The source folder may have changed.
    pub fn source_changed(self) -> bool {
        matches!(self, DropOp::Move | DropOp::Delete)
    }
}

/// Reports from native sessions; poll with [`poll_events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DragEvent {
    /// AppKit accepted the session (`draggingSession:willBeginAtPoint:`).
    Started { items: usize, promises: usize },
    /// First `sourceOperationMaskForDraggingContext:` of the session.
    MaskRequested { outside_app: bool, mask: u64 },
    /// The session ended. `local` are the dragged local paths; refresh their
    /// folders when `op.source_changed()`. Never delete anything because
    /// of this.
    Ended { op: DropOp, local: Vec<PathBuf> },
    /// A destination asked for a promised item; the download started.
    PromiseWriting {
        name: String,
        backend: String,
        dest: PathBuf,
        size: Option<u64>,
    },
    /// The download finished (`Err`: also reported to the destination).
    PromiseDone {
        name: String,
        dest: PathBuf,
        result: Result<(), String>,
    },
    /// The session couldn't start.
    Failed(String),
}

/// Fulfil one promise: fetch `item` to `dest`, report progress, remove a
/// partial result on failure.
pub fn write_promise(
    item: &RemoteItem,
    dest: &Path,
    emit: &mut dyn FnMut(DragEvent),
) -> Result<(), String> {
    emit(DragEvent::PromiseWriting {
        name: item.name.clone(),
        backend: item.backend.clone(),
        dest: dest.to_path_buf(),
        size: item.size,
    });
    let result = (item.fetch)(dest);
    if result.is_err() {
        if item.is_dir {
            let _ = std::fs::remove_dir_all(dest);
        } else {
            let _ = std::fs::remove_file(dest);
        }
    }
    emit(DragEvent::PromiseDone {
        name: item.name.clone(),
        dest: dest.to_path_buf(),
        result: result.clone(),
    });
    result
}

// ---------------------------------------------------------------------------
// Copy or move? (for drops from other apps and between panes)

/// Manta's rule for a drop: copy by default, so nothing ever disappears
/// from where it was by accident. ⌘ moves, ⌥ copies explicitly (and wins
/// over ⌘). Unlike Finder this doesn't depend on the volume.
/// `moves_by_default`: the user chose "Always move" in Settings.
pub fn drop_is_move(moves_by_default: bool, alt: bool, cmd: bool) -> bool {
    if alt {
        false
    } else {
        cmd || moves_by_default
    }
}

fn drops_move_id() -> egui::Id {
    egui::Id::unique("manta-drops-move-by-default")
}

/// The app's drop setting for this frame (Settings → "When you drop
/// files": "Always move"), read by the panes' drop hints.
pub fn set_drops_move_by_default(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(drops_move_id(), on));
}

/// See [`set_drops_move_by_default`].
pub fn drops_move_by_default(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(drops_move_id()))
        .unwrap_or(false)
}

/// The (⌥, ⌘) keys held during a drag. AppKit's view on macOS: while
/// another app drags over Manta, egui gets no key events.
pub fn drag_modifiers(ctx: &egui::Context) -> (bool, bool) {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let _ = ctx;
        mac::modifier_keys()
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        ctx.input(|i| (i.modifiers.alt, i.modifiers.mac_cmd || i.modifiers.command))
    }
}

// ---------------------------------------------------------------------------
// Event channel (filled from AppKit callbacks, also on background threads)

static EVENTS: Mutex<Vec<DragEvent>> = Mutex::new(Vec::new());
static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Local paths of the last session (to recognise our own drag coming back).
static LAST_LOCAL: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub(crate) fn emit(ev: DragEvent) {
    log::info!("native drag: {ev:?}");
    if let Ok(mut q) = EVENTS.lock() {
        if q.len() >= 256 {
            q.remove(0);
        }
        q.push(ev);
    }
    if let Some(ctx) = REPAINT.lock().ok().and_then(|c| c.clone()) {
        ctx.request_repaint();
    }
}

pub(crate) fn set_active(on: bool) {
    ACTIVE.store(on, Ordering::SeqCst);
}

/// Drain what happened since the last call (toasts, refresh after Move,
/// promise download results). Call once per frame from the app.
pub fn poll_events() -> Vec<DragEvent> {
    EVENTS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// A native session is running (the mouse belongs to AppKit).
pub fn session_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Drag out

/// Start a native drag of `items` now. Clears egui's internal drag payload
/// (AppKit owns the mouse from here). Returns `false` when there is nothing
/// to drag, a session is already running or the platform has no native drag.
/// Must be called on the UI (main) thread; the session itself starts on the
/// next run-loop turn.
pub fn begin_external(ctx: &egui::Context, items: Vec<DragItem>) -> bool {
    if items.is_empty() || session_active() {
        return false;
    }
    if let Ok(mut c) = REPAINT.lock() {
        *c = Some(ctx.clone());
    }
    if let Ok(mut l) = LAST_LOCAL.lock() {
        *l = items
            .iter()
            .filter_map(|i| match i {
                DragItem::Local(p) => Some(p.clone()),
                DragItem::Remote(_) => None,
            })
            .collect();
    }
    log::info!(
        "native drag: begin {} item(s): {:?}",
        items.len(),
        pasteboard_plan(&items)
    );
    if !platform_begin(items) {
        return false;
    }
    egui::DragAndDrop::clear_payload(ctx);
    ctx.request_repaint();
    true
}

#[cfg(all(target_os = "macos", not(test)))]
fn platform_begin(items: Vec<DragItem>) -> bool {
    mac::begin(items)
}

#[cfg(test)]
thread_local! {
    /// Tests: what `begin_external` would have put on the pasteboard.
    static TEST_BEGUN: std::cell::RefCell<Vec<Vec<PasteboardEntry>>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn platform_begin(items: Vec<DragItem>) -> bool {
    TEST_BEGUN.with(|b| b.borrow_mut().push(pasteboard_plan(&items)));
    true
}

#[cfg(all(not(target_os = "macos"), not(test)))]
fn platform_begin(_items: Vec<DragItem>) -> bool {
    false
}

/// Phase of the drag-out trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerState {
    /// No drag, or not seen inside the window yet.
    #[default]
    Idle,
    /// Dragging inside the window.
    Armed,
    /// Left the window: the native session was requested (once).
    Fired,
}

/// "Pointer leaves the window while dragging" → fire once.
#[derive(Debug, Clone, Copy, Default)]
pub struct Trigger {
    pub state: TriggerState,
    last_frame: Option<u64>,
}

impl Trigger {
    /// One frame. `dragging`: an internal drag payload exists; `outside`:
    /// the pointer is outside the window. True exactly once per drag, when
    /// it leaves the window after having been inside. A skipped frame (the
    /// hook wasn't called) or a released button re-arms.
    pub fn step(&mut self, frame: u64, dragging: bool, pointer_down: bool, outside: bool) -> bool {
        if self.last_frame.is_some_and(|f| frame > f + 1) {
            self.state = TriggerState::Idle;
        }
        self.last_frame = Some(frame);
        if !dragging || !pointer_down {
            self.state = TriggerState::Idle;
            return false;
        }
        match self.state {
            TriggerState::Idle => {
                if !outside {
                    self.state = TriggerState::Armed;
                }
                false
            }
            TriggerState::Armed if outside => {
                self.state = TriggerState::Fired;
                true
            }
            TriggerState::Armed | TriggerState::Fired => false,
        }
    }
}

fn trigger_id() -> egui::Id {
    egui::Id::unique("manta-native-drag-trigger")
}

/// The pointer is outside the window this frame.
fn pointer_outside(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        let rect = i.viewport_rect();
        let gone = i
            .raw
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::PointerGone));
        gone || i.pointer.latest_pos().is_some_and(|p| !rect.contains(p))
    })
}

/// **The drag-out hook.** Call once per frame while an internal file drag
/// is active (e.g. while `egui::DragAndDrop::payload::<DragPayload>` is
/// `Some`). When the pointer leaves the window the items from `items` (only
/// evaluated then) are handed to a native session. Returns true on the frame
/// the session was requested.
pub fn drag_out_when_leaving(ctx: &egui::Context, items: impl FnOnce() -> Vec<DragItem>) -> bool {
    let down = ctx.input(|i| i.pointer.primary_down());
    let outside = pointer_outside(ctx);
    let frame = ctx.cumulative_frame_nr();
    let fire = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Trigger>(trigger_id())
            .step(frame, true, down, outside)
    });
    fire && begin_external(ctx, items())
}

// ---------------------------------------------------------------------------
// Drag in

/// Files from another app hovering over the window.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalHover {
    pub paths: Vec<PathBuf>,
    /// Pointer in egui points (`None`: unknown).
    pub pos: Option<egui::Pos2>,
}

/// Files dropped onto the window by another app (or our own drag coming
/// back, `from_self`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalDrop {
    pub paths: Vec<PathBuf>,
    pub pos: Option<egui::Pos2>,
    /// Exactly the local files of Manta's last native drag.
    pub from_self: bool,
}

/// Pointer position for external drags: AppKit's mouse location on macOS
/// (winit sends no moves while another app drags over us), egui's last
/// known position elsewhere and in tests.
fn drop_pointer_pos(ctx: &egui::Context) -> Option<egui::Pos2> {
    #[cfg(all(target_os = "macos", not(test)))]
    if let Some(p) = mac::mouse_in_window() {
        let z = ctx.zoom_factor().max(0.01);
        return Some(egui::pos2(p.0 as f32 / z, p.1 as f32 / z));
    }
    ctx.input(|i| i.pointer.latest_pos())
}

/// Files dragged over the window from another app, with the pointer
/// position. Keeps repainting while they hover so a drop highlight can
/// follow the pointer.
pub fn external_hover(ctx: &egui::Context) -> Option<ExternalHover> {
    let paths: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .hovered_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    });
    if paths.is_empty() {
        return None;
    }
    ctx.request_repaint();
    Some(ExternalHover {
        paths,
        pos: drop_pointer_pos(ctx),
    })
}

#[derive(Clone, Copy, Default)]
struct DropTaken(u64);

fn drop_taken_id() -> egui::Id {
    egui::Id::unique("manta-native-drop-taken")
}

/// This frame's external drop without consuming it.
pub fn peek_external_drop(ctx: &egui::Context) -> Option<ExternalDrop> {
    let frame = ctx.cumulative_frame_nr();
    let taken = ctx
        .data(|d| d.get_temp::<DropTaken>(drop_taken_id()))
        .is_some_and(|t| t.0 == frame);
    if taken {
        return None;
    }
    let paths: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    });
    if paths.is_empty() {
        return None;
    }
    let from_self = LAST_LOCAL
        .lock()
        .is_ok_and(|l| !l.is_empty() && *l == paths);
    Some(ExternalDrop {
        paths,
        pos: drop_pointer_pos(ctx),
        from_self,
    })
}

/// Take this frame's external drop (once per frame, whoever asks first).
pub fn take_external_drop(ctx: &egui::Context) -> Option<ExternalDrop> {
    let drop = peek_external_drop(ctx)?;
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(drop_taken_id(), DropTaken(frame)));
    log::info!(
        "native drop: {} file(s) at {:?} (from_self={})",
        drop.paths.len(),
        drop.pos,
        drop.from_self
    );
    Some(drop)
}

/// Take this frame's external drop only if it landed in `rect` (a pane, a
/// folder row). Unknown positions don't match.
pub fn take_external_drop_in(ctx: &egui::Context, rect: egui::Rect) -> Option<ExternalDrop> {
    let drop = peek_external_drop(ctx)?;
    if !drop.pos.is_some_and(|p| rect.contains(p)) {
        return None;
    }
    take_external_drop(ctx)
}

/// Test hooks: fake drops from other apps and the native sessions that
/// would have started.
#[cfg(test)]
pub(crate) mod test_hooks {
    use super::*;

    /// What egui-winit hands egui for a dropped file.
    #[derive(Debug)]
    struct TestDrop(PathBuf);

    impl egui::DroppedFile for TestDrop {
        fn path(&self) -> &Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }

    /// A file another app dropped (for `RawInput::dropped_files`).
    pub fn dropped(path: &Path) -> Arc<dyn egui::DroppedFile + Send + Sync> {
        Arc::new(TestDrop(path.to_path_buf()))
    }

    /// A file another app drags over the window (`RawInput::hovered_files`).
    pub fn hovered(path: &Path) -> egui::HoveredFile {
        egui::HoveredFile {
            path: Some(path.to_path_buf()),
            ..Default::default()
        }
    }

    /// The native sessions started on this thread so far (their pasteboard
    /// contents), cleared.
    pub fn take_begun() -> Vec<Vec<PasteboardEntry>> {
        TEST_BEGUN.with(|b| std::mem::take(&mut *b.borrow_mut()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn remote(name: &str, is_dir: bool, fetch: FetchFn) -> DragItem {
        DragItem::Remote(RemoteItem {
            backend: "me@host (SFTP)".into(),
            name: name.into(),
            size: Some(3),
            is_dir,
            fetch,
        })
    }

    fn ok_fetch() -> FetchFn {
        Arc::new(|p: &Path| std::fs::write(p, b"abc").map_err(|e| e.to_string()))
    }

    #[test]
    fn pasteboard_plan_maps_local_to_urls_and_remote_to_promises() {
        let items = vec![
            DragItem::Local(PathBuf::from("/tmp/a.txt")),
            remote("report.pdf", false, ok_fetch()),
            remote("photos", true, ok_fetch()),
        ];
        assert_eq!(
            pasteboard_plan(&items),
            vec![
                PasteboardEntry::FileUrl(PathBuf::from("/tmp/a.txt")),
                PasteboardEntry::Promise {
                    file_name: "report.pdf".into(),
                    file_type: UTI_DATA
                },
                PasteboardEntry::Promise {
                    file_name: "photos".into(),
                    file_type: UTI_FOLDER
                },
            ]
        );
    }

    #[test]
    fn promise_file_names_are_safe_components() {
        assert_eq!(promise_file_name("a/b:c.txt"), "a-b-c.txt");
        assert_eq!(promise_file_name("  x  "), "x");
        assert_eq!(promise_file_name(""), "Untitled");
        assert_eq!(promise_file_name(".."), "Untitled");
        assert_eq!(promise_file_name("tab\there"), "tab-here");
        let long = "ä".repeat(200);
        let n = promise_file_name(&long);
        assert!(n.len() <= 255 && n.is_char_boundary(n.len()));
        assert_eq!(promise_file_name("Grüße 2026.txt"), "Grüße 2026.txt");
    }

    #[test]
    fn masks_copy_outside_and_allow_move_inside() {
        assert_eq!(source_mask(true, true, false), op::COPY);
        assert_eq!(source_mask(true, true, true), op::COPY | op::MOVE);
        assert_eq!(
            source_mask(true, false, true),
            op::COPY,
            "promises are never moved"
        );
        assert_eq!(
            source_mask(false, false, false),
            op::COPY | op::MOVE | op::GENERIC
        );
        assert_eq!(DropOp::from_bits(0), DropOp::None);
        assert_eq!(DropOp::from_bits(op::COPY), DropOp::Copy);
        assert_eq!(DropOp::from_bits(op::MOVE), DropOp::Move);
        assert_eq!(DropOp::from_bits(op::DELETE), DropOp::Delete);
        assert_eq!(DropOp::from_bits(op::GENERIC), DropOp::Other);
        assert!(DropOp::Move.source_changed() && !DropOp::Copy.source_changed());
    }

    #[test]
    fn copy_is_the_default_and_command_moves() {
        // (moves by default, ⌥, ⌘) → move?
        assert!(!drop_is_move(false, false, false), "default: copy");
        assert!(drop_is_move(false, false, true), "⌘ moves");
        assert!(!drop_is_move(false, true, false), "⌥ copies");
        assert!(!drop_is_move(false, true, true), "⌥ wins over ⌘");
        // Settings → "Always move".
        assert!(drop_is_move(true, false, false));
        assert!(!drop_is_move(true, true, false), "⌥ still copies");
    }

    #[test]
    fn trigger_fires_once_when_leaving_after_being_inside() {
        let mut t = Trigger::default();
        assert!(
            !t.step(1, true, true, true),
            "starting outside doesn't fire"
        );
        assert!(!t.step(2, true, true, false));
        assert_eq!(t.state, TriggerState::Armed);
        assert!(t.step(3, true, true, true), "inside → outside fires");
        assert!(!t.step(4, true, true, true), "only once");
        assert!(!t.step(5, true, true, false), "coming back doesn't re-arm");
        assert!(!t.step(6, true, true, true));
        // Release re-arms.
        assert!(!t.step(7, true, false, false));
        assert_eq!(t.state, TriggerState::Idle);
        assert!(!t.step(8, true, true, false));
        assert!(t.step(9, true, true, true));
        // A gap (the hook wasn't called: no drag) resets too.
        assert!(!t.step(20, true, true, false));
        assert!(t.step(21, true, true, true));
        // No payload: never fires.
        let mut t = Trigger::default();
        assert!(!t.step(1, false, true, false));
        assert!(!t.step(2, false, true, true));
    }

    #[test]
    fn promise_write_reports_success() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("report.pdf");
        let DragItem::Remote(item) = remote("report.pdf", false, ok_fetch()) else {
            unreachable!()
        };
        let mut evs = Vec::new();
        assert_eq!(write_promise(&item, &dest, &mut |e| evs.push(e)), Ok(()));
        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        assert!(matches!(evs[0], DragEvent::PromiseWriting { .. }));
        assert_eq!(
            evs[1],
            DragEvent::PromiseDone {
                name: "report.pdf".into(),
                dest,
                result: Ok(())
            }
        );
    }

    #[test]
    fn promise_write_propagates_fetch_errors_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("big.iso");
        let fetch: FetchFn = Arc::new(|p: &Path| {
            let mut f = std::fs::File::create(p).map_err(|e| e.to_string())?;
            f.write_all(b"partial").map_err(|e| e.to_string())?;
            Err("connection reset".into())
        });
        let DragItem::Remote(item) = remote("big.iso", false, fetch) else {
            unreachable!()
        };
        let mut evs = Vec::new();
        assert_eq!(
            write_promise(&item, &dest, &mut |e| evs.push(e)),
            Err("connection reset".into())
        );
        assert!(!dest.exists(), "partial download removed");
        assert_eq!(
            evs.last().unwrap(),
            &DragEvent::PromiseDone {
                name: "big.iso".into(),
                dest,
                result: Err("connection reset".into())
            }
        );
    }

    /// A "remote" backend: the local file system without local paths.
    struct NoLocal(SharedVfs);

    impl manta_core::Vfs for NoLocal {
        fn label(&self) -> String {
            "test (SFTP)".into()
        }
        fn protocol(&self) -> manta_core::Protocol {
            manta_core::Protocol::Sftp
        }
        fn home(&self) -> manta_core::VfsResult<VfsPath> {
            self.0.home()
        }
        fn list(&self, dir: &VfsPath) -> manta_core::VfsResult<Vec<manta_core::Entry>> {
            self.0.list(dir)
        }
        fn stat(&self, path: &VfsPath) -> manta_core::VfsResult<manta_core::Entry> {
            self.0.stat(path)
        }
        fn create_dir(&self, path: &VfsPath) -> manta_core::VfsResult<()> {
            self.0.create_dir(path)
        }
        fn remove_file(&self, path: &VfsPath) -> manta_core::VfsResult<()> {
            self.0.remove_file(path)
        }
        fn remove_dir(&self, path: &VfsPath) -> manta_core::VfsResult<()> {
            self.0.remove_dir(path)
        }
        fn rename(&self, from: &VfsPath, to: &VfsPath) -> manta_core::VfsResult<()> {
            self.0.rename(from, to)
        }
        fn open_read(
            &self,
            path: &VfsPath,
        ) -> manta_core::VfsResult<Box<dyn std::io::Read + Send>> {
            self.0.open_read(path)
        }
        fn open_write(
            &self,
            path: &VfsPath,
        ) -> manta_core::VfsResult<Box<dyn std::io::Write + Send>> {
            self.0.open_write(path)
        }
    }

    #[test]
    fn vfs_items_are_local_or_promises_and_folders_download_recursively() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir(src.path().join("docs")).unwrap();
        std::fs::write(src.path().join("docs/a.txt"), b"A").unwrap();
        std::fs::create_dir(src.path().join("docs/sub")).unwrap();
        std::fs::write(src.path().join("docs/sub/b.txt"), b"B").unwrap();
        let local: SharedVfs = Arc::new(manta_vfs::local::LocalFs);
        let docs = VfsPath::new(&src.path().join("docs").to_string_lossy());
        assert!(matches!(
            DragItem::from_vfs(&local, &docs, true, None),
            DragItem::Local(_)
        ));

        let remote_fs: SharedVfs = Arc::new(NoLocal(local));
        let items = items_from_vfs(&remote_fs, std::slice::from_ref(&docs), &[true]);
        let DragItem::Remote(item) = &items[0] else {
            panic!("expected a promise")
        };
        assert_eq!(
            (item.name.as_str(), item.is_dir, item.backend.as_str()),
            ("docs", true, "test (SFTP)")
        );
        let out = tempfile::tempdir().unwrap();
        let dest = out.path().join("docs");
        write_promise(item, &dest, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"A");
        assert_eq!(std::fs::read(dest.join("sub/b.txt")).unwrap(), b"B");

        // A missing remote file: the error comes back.
        let gone = VfsPath::new(&src.path().join("nope.txt").to_string_lossy());
        let DragItem::Remote(item) = DragItem::from_vfs(&remote_fs, &gone, false, None) else {
            panic!()
        };
        assert!(write_promise(&item, &out.path().join("nope.txt"), &mut |_| {}).is_err());
    }

    /// The hook in a real egui frame loop: drag inside, leave the window →
    /// one native session with the right pasteboard entries, payload cleared.
    #[test]
    fn hook_starts_one_session_when_the_pointer_leaves_the_window() {
        use egui_kittest::Harness;
        TEST_BEGUN.with(|b| b.borrow_mut().clear());
        let fired = std::rc::Rc::new(std::cell::Cell::new(0));
        let f2 = fired.clone();
        let mut h = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                if egui::DragAndDrop::has_payload_of_type::<u32>(&ctx)
                    && drag_out_when_leaving(&ctx, || {
                        vec![DragItem::Local(PathBuf::from("/tmp/x.txt"))]
                    })
                {
                    f2.set(f2.get() + 1);
                }
            });
        let press = |pressed: bool, pos: egui::Pos2| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(100.0, 100.0)));
        h.input_mut()
            .events
            .push(press(true, egui::pos2(100.0, 100.0)));
        h.step();
        egui::DragAndDrop::set_payload(&h.ctx, 7u32);
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(200.0, 150.0)));
        h.step();
        assert_eq!(fired.get(), 0);
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(450.0, 150.0)));
        h.step();
        assert_eq!(fired.get(), 1);
        assert!(
            !egui::DragAndDrop::has_any_payload(&h.ctx),
            "internal payload cleared"
        );
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(500.0, 150.0)));
        h.step();
        assert_eq!(fired.get(), 1);
        TEST_BEGUN.with(|b| {
            assert_eq!(
                *b.borrow(),
                vec![vec![PasteboardEntry::FileUrl(PathBuf::from("/tmp/x.txt"))]]
            );
        });
    }

    /// Drag IN: a synthesised winit file drop (as egui-winit delivers it)
    /// reaches exactly one consumer, with the pointer position.
    #[test]
    fn external_drop_is_taken_once_by_the_pane_under_the_pointer() {
        use egui_kittest::Harness;
        let got: std::rc::Rc<std::cell::RefCell<Vec<(usize, ExternalDrop)>>> = Default::default();
        let hover: std::rc::Rc<std::cell::RefCell<Option<ExternalHover>>> = Default::default();
        let (g2, h2) = (got.clone(), hover.clone());
        let mut h = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                *h2.borrow_mut() = external_hover(&ctx);
                let panes = [
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 300.0)),
                    egui::Rect::from_min_max(egui::pos2(200.0, 0.0), egui::pos2(400.0, 300.0)),
                ];
                for (i, r) in panes.iter().enumerate() {
                    if let Some(d) = take_external_drop_in(&ctx, *r) {
                        g2.borrow_mut().push((i, d));
                    }
                }
                // A second consumer in the same frame gets nothing.
                assert!(take_external_drop(&ctx).is_none());
            });
        let file = PathBuf::from("/Users/me/Desktop/notes.txt");
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(300.0, 100.0)));
        h.input_mut().hovered_files.push(egui::HoveredFile {
            path: Some(file.clone()),
            ..Default::default()
        });
        h.step();
        let hv = hover.borrow().clone().expect("hover reported");
        assert_eq!(hv.paths, vec![file.clone()]);
        assert_eq!(hv.pos, Some(egui::pos2(300.0, 100.0)));
        h.input_mut().hovered_files.clear();
        h.input_mut().dropped_files.push(test_hooks::dropped(&file));
        h.step();
        let got = got.borrow();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 1, "the right pane got it");
        assert_eq!(got[0].1.paths, vec![file]);
        assert!(!got[0].1.from_self);
        assert!(hover.borrow().is_none());
    }
}
