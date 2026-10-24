//! Get Info (⌘I, „Informationen“ in the context menu): Finder-like Info
//! windows. Each is a native auxiliary window (non-modal, see
//! `aux_window.rs`): one per item (opening Info for an item that already
//! has one brings it to the front), one summary window for a multiple
//! selection, at most [`MAX_WINDOWS`] at a time (the oldest closes).
//!
//! What a window shows:
//! - icon, the name (editable: Return renames), kind, size (folders and
//!   selections are counted in the background), where, created / modified
//!   / last opened (Spotlight's `kMDItemLastUsedDate`),
//! - tags (add / remove, like the context menu), Locked (`uchg`) and
//!   Hidden (`UF_HIDDEN`) for items on this Mac,
//! - the Spotlight comment (`kMDItemFinderComment`; read-only: macOS has no
//!   public API to set it apart from scripting Finder),
//! - "Open with": the default app and an "Open With" menu of the apps that
//!   can open the file (`NSWorkspace`). macOS has no public API to change
//!   the app for one file, so there is no "Change All…",
//! - permissions: owner / group / everyone as read / write / execute and
//!   in octal, editable (chmod) on this Mac and on SFTP servers.
//!
//! Items on servers, in containers or in archives show what the backend
//! provides (size, modified, permissions).
//!
//! System lookups (Spotlight attributes, apps) go through [`InfoSystem`];
//! tests use a fake.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use eframe::egui::{self, RichText};
use manta_core::{Entry, Protocol, SharedVfs, VfsPath};

use crate::aux_window::{AuxKind, AuxWindow, Spec};
use crate::tags::{self, Tag};
use crate::theme::{self, pal, space, text};
use crate::{icons, util, widgets};

/// Most Info windows open at once; the oldest closes for a new one.
pub const MAX_WINDOWS: usize = 6;

/// `UF_IMMUTABLE` ("Locked" in Finder) and `UF_HIDDEN`.
pub const UF_IMMUTABLE: u32 = 0x0000_0002;
pub const UF_HIDDEN: u32 = 0x0000_8000;

/// An app that can open a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub path: PathBuf,
}

impl App {
    pub fn at(path: PathBuf) -> App {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        App { name, path }
    }
}

/// The system lookups behind an Info window (a seam: tests use a fake).
pub trait InfoSystem: Send + Sync {
    /// Spotlight's last-opened date and Finder comment of `path`.
    fn spotlight(&self, path: &Path) -> (Option<DateTime<Utc>>, Option<String>);
    /// The app that opens `path` by default.
    fn default_app(&self, path: &Path) -> Option<App>;
    /// Every app that can open `path`.
    fn apps(&self, path: &Path) -> Vec<App>;
}

/// The real lookups: `mdls` and `NSWorkspace`.
#[cfg_attr(test, allow(dead_code))]
pub struct MacInfo;

impl InfoSystem for MacInfo {
    fn spotlight(&self, path: &Path) -> (Option<DateTime<Utc>>, Option<String>) {
        let out = std::process::Command::new("/usr/bin/mdls")
            .args([
                "-raw",
                "-nullMarker",
                "",
                "-name",
                "kMDItemLastUsedDate",
                "-name",
                "kMDItemFinderComment",
            ])
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        let Ok(out) = out else {
            return (None, None);
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = text.split('\0');
        let last = parts.next().and_then(parse_mdls_date);
        let comment = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        (last, comment)
    }

    fn default_app(&self, path: &Path) -> Option<App> {
        workspace::default_app(path)
    }

    fn apps(&self, path: &Path) -> Vec<App> {
        workspace::apps(path)
    }
}

/// `2026-10-24 12:02:03 +0000` → a date.
fn parse_mdls_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S %z")
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
mod workspace {
    use super::App;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    use std::path::{Path, PathBuf};

    fn url(path: &Path) -> objc2::rc::Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    fn path_of(u: &NSURL) -> Option<PathBuf> {
        u.path().map(|p| PathBuf::from(p.to_string()))
    }

    pub fn default_app(path: &Path) -> Option<App> {
        objc2::rc::autoreleasepool(|_| {
            let ws = NSWorkspace::sharedWorkspace();
            let app = ws.URLForApplicationToOpenURL(&url(path))?;
            path_of(&app).map(App::at)
        })
    }

    pub fn apps(path: &Path) -> Vec<App> {
        objc2::rc::autoreleasepool(|_| {
            let ws = NSWorkspace::sharedWorkspace();
            let list = ws.URLsForApplicationsToOpenURL(&url(path));
            let mut out: Vec<App> = list
                .iter()
                .filter_map(|u| path_of(&u))
                .map(App::at)
                .collect();
            out.sort_by(|a, b| util::natural_cmp(&a.name, &b.name));
            out.dedup_by(|a, b| a.path == b.path);
            out
        })
    }

    use crate::util;
}

#[cfg(not(target_os = "macos"))]
mod workspace {
    use super::App;
    use std::path::Path;
    pub fn default_app(_path: &Path) -> Option<App> {
        None
    }
    pub fn apps(_path: &Path) -> Vec<App> {
        Vec::new()
    }
}

/// Test lookups: fixed answers.
#[cfg(test)]
#[derive(Default)]
pub struct FakeInfo {
    pub last_opened: Option<DateTime<Utc>>,
    pub comment: Option<String>,
    pub default: Option<App>,
    pub apps: Vec<App>,
}

#[cfg(test)]
impl InfoSystem for FakeInfo {
    fn spotlight(&self, _path: &Path) -> (Option<DateTime<Utc>>, Option<String>) {
        (self.last_opened, self.comment.clone())
    }
    fn default_app(&self, _path: &Path) -> Option<App> {
        self.default.clone()
    }
    fn apps(&self, _path: &Path) -> Vec<App> {
        self.apps.clone()
    }
}

// ------------------------------------------------------------------ details

/// What is known about one item on this Mac (read in the background).
#[derive(Debug, Clone, Default)]
pub struct Details {
    pub created: Option<DateTime<Utc>>,
    pub last_opened: Option<DateTime<Utc>>,
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u32>,
    pub locked: bool,
    pub hidden: bool,
    pub tags: Vec<Tag>,
    pub default_app: Option<App>,
    pub apps: Vec<App>,
}

/// The user / group name of an id.
fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: buffers live for the call; `out` points to `pwd` on success.
    let r = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut out) };
    if r != 0 || out.is_null() || pwd.pw_name.is_null() {
        return None;
    }
    // SAFETY: `pw_name` is a NUL-terminated string in `buf`.
    Some(
        unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn group_name(gid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut grp: libc::group = unsafe { std::mem::zeroed() };
    let mut out: *mut libc::group = std::ptr::null_mut();
    // SAFETY: as in `user_name`.
    let r = unsafe { libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut out) };
    if r != 0 || out.is_null() || grp.gr_name.is_null() {
        return None;
    }
    // SAFETY: `gr_name` is a NUL-terminated string in `buf`.
    Some(
        unsafe { std::ffi::CStr::from_ptr(grp.gr_name) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// The file flags (`st_flags`) of `path` (without following a symlink).
pub fn flags(path: &Path) -> std::io::Result<u32> {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        Ok(std::fs::symlink_metadata(path)?.st_flags())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Ok(0)
    }
}

/// Set or clear one of the user flags (`UF_IMMUTABLE`, `UF_HIDDEN`).
pub fn set_flag(path: &Path, flag: u32, on: bool) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let old = flags(path)?;
    let new = if on { old | flag } else { old & !flag };
    if new == old {
        return Ok(());
    }
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: a valid C string; `lchflags` doesn't follow symlinks.
    let r = unsafe { libc::lchflags(c.as_ptr(), new as _) };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Read everything an Info window shows about a local item.
pub fn local_details(path: &Path, sys: &dyn InfoSystem) -> Details {
    use std::os::unix::fs::MetadataExt;
    let mut d = Details::default();
    if let Ok(m) = std::fs::symlink_metadata(path) {
        d.created = m.created().ok().map(DateTime::<Utc>::from);
        d.owner = user_name(m.uid()).or_else(|| Some(m.uid().to_string()));
        d.group = group_name(m.gid()).or_else(|| Some(m.gid().to_string()));
        d.mode = Some(m.mode() & 0o7777);
        let f = flags(path).unwrap_or(0);
        d.locked = f & UF_IMMUTABLE != 0;
        d.hidden = f & UF_HIDDEN != 0;
        if m.is_file() || path.extension().is_some_and(|e| e == "app") {
            d.default_app = sys.default_app(path);
            d.apps = sys.apps(path);
        }
    }
    d.tags = tags::read_tags(path).unwrap_or_default();
    let (last, comment) = sys.spotlight(path);
    d.last_opened = last;
    d.comment = comment;
    d
}

// ------------------------------------------------------------------ sizes

/// Counted size of a folder or a selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeCount {
    pub bytes: u64,
    pub files: u64,
    pub done: bool,
}

/// Count `items` (recursively) in the background; the result grows while
/// counting. Stops when `cancel` is set.
fn count_size(
    fs: SharedVfs,
    items: Vec<Entry>,
    out: Arc<Mutex<SizeCount>>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) {
    let run = move || {
        let mut stack: Vec<VfsPath> = Vec::new();
        let mut total = SizeCount::default();
        for e in &items {
            if e.is_dir() && e.link_target.is_none() {
                stack.push(e.path.clone());
            } else {
                total.bytes += e.size;
                total.files += 1;
            }
        }
        let mut last = std::time::Instant::now();
        while let Some(dir) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let entries = match fs.local_path(&dir) {
                Some(p) => local_list(&p),
                None => fs.list(&dir).unwrap_or_default(),
            };
            for e in entries {
                if e.is_dir() && e.link_target.is_none() {
                    stack.push(e.path);
                } else {
                    total.bytes += e.size;
                    total.files += 1;
                }
            }
            if last.elapsed() > Duration::from_millis(150) {
                *out.lock().unwrap() = total;
                ctx.request_repaint();
                last = std::time::Instant::now();
            }
        }
        total.done = true;
        *out.lock().unwrap() = total;
        ctx.request_repaint();
    };
    if std::thread::Builder::new()
        .name("manta-info-size".into())
        .spawn(run)
        .is_err()
    {
        log::warn!("info: can't start the size counter");
    }
}

/// A local folder's items, cheaply (no symlink resolution).
fn local_list(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let kind = if m.is_dir() {
                manta_core::EntryKind::Directory
            } else if m.file_type().is_symlink() {
                manta_core::EntryKind::Symlink
            } else {
                manta_core::EntryKind::File
            };
            Some(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: VfsPath::new(&e.path().to_string_lossy()),
                kind,
                size: if m.is_dir() { 0 } else { m.len() },
                modified: None,
                permissions: None,
                link_target: None,
                hidden: false,
            })
        })
        .collect()
}

// ------------------------------------------------------------------ windows

/// What an Info window asks the app to do.
#[derive(Clone)]
pub enum InfoRequest {
    /// Rename `from` to `name` (in the same folder).
    Rename {
        fs: SharedVfs,
        from: VfsPath,
        name: String,
    },
    /// Something in `dir` changed (tags, flags, permissions): refresh.
    Changed { fs: SharedVfs, dir: VfsPath },
    /// Open `file` with the app at `app`.
    OpenWith { app: PathBuf, file: PathBuf },
}

/// What an Info window is about.
pub struct Target {
    pub fs: SharedVfs,
    /// One item, or the selection (summary window).
    pub items: Vec<Entry>,
    /// Where the items are, as the user sees it ("Local", "SFTP · web-01").
    pub place: String,
}

/// One Info window.
pub struct InfoWindow {
    pub id: u64,
    window: AuxWindow,
    pub target: Target,
    /// The item on this Mac (single local item).
    local: Option<PathBuf>,
    details: Arc<Mutex<Option<Details>>>,
    size: Arc<Mutex<SizeCount>>,
    counting: bool,
    cancel: Arc<AtomicBool>,
    /// The name field.
    pub name_edit: String,
    /// The octal permissions field.
    octal_edit: String,
    new_tag: String,
    pub error: Option<String>,
}

impl Drop for InfoWindow {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl InfoWindow {
    fn single(&self) -> Option<&Entry> {
        match self.target.items.as_slice() {
            [e] => Some(e),
            _ => None,
        }
    }

    /// The details read so far (single local items).
    pub fn details(&self) -> Option<Details> {
        self.details.lock().unwrap().clone()
    }

    /// The counted size so far.
    pub fn size(&self) -> SizeCount {
        *self.size.lock().unwrap()
    }

    /// Whether the window is about this item.
    pub fn is_for(&self, fs: &SharedVfs, path: &VfsPath) -> bool {
        util::same_fs(&self.target.fs, fs) && self.single().is_some_and(|e| &e.path == path)
    }

    fn title(&self) -> String {
        match self.single() {
            Some(e) => t!("info.title", name = util::display_name(&e.name)),
            None => t!("info.title_multi", count = self.target.items.len()),
        }
    }

    /// Permissions can be changed here (this Mac, SFTP).
    fn chmod_ok(&self) -> bool {
        !self.target.fs.is_read_only()
            && matches!(self.target.fs.protocol(), Protocol::Local | Protocol::Sftp)
            && self.single().is_some_and(|e| e.permissions.is_some())
    }

    /// The current mode (local: as read; elsewhere: the listing's).
    fn mode(&self) -> Option<u32> {
        if self.local.is_some() {
            if let Some(d) = self.details.lock().unwrap().as_ref() {
                return d.mode;
            }
        }
        self.single().and_then(|e| e.permissions)
    }

    fn set_mode(&mut self, mode: u32, out: &mut Vec<InfoRequest>) {
        let Some(e) = self.single().cloned() else {
            return;
        };
        match self.target.fs.set_permissions(&e.path, mode) {
            Ok(()) => {
                if let Some(d) = self.details.lock().unwrap().as_mut() {
                    d.mode = Some(mode);
                }
                if let Some(it) = self.target.items.first_mut() {
                    it.permissions = Some(mode);
                }
                self.octal_edit = format!("{:03o}", mode & 0o777);
                self.error = None;
                self.changed(out);
            }
            Err(err) => {
                self.error = Some(crate::i18n::localize_error(&err.to_string()));
            }
        }
    }

    fn changed(&self, out: &mut Vec<InfoRequest>) {
        if let Some(e) = self.single() {
            if let Some(dir) = e.path.parent() {
                out.push(InfoRequest::Changed {
                    fs: self.target.fs.clone(),
                    dir,
                });
            }
        }
    }

    fn set_tag(&mut self, tag: &Tag, on: bool, out: &mut Vec<InfoRequest>) {
        let Some(p) = self.local.clone() else {
            return;
        };
        match tags::set_tag(&p, tag, on) {
            Ok(t) => {
                if let Some(d) = self.details.lock().unwrap().as_mut() {
                    d.tags = t;
                }
                self.error = None;
                self.changed(out);
            }
            Err(e) => self.error = Some(crate::i18n::localize_error(&e.to_string())),
        }
    }

    fn set_flag(&mut self, flag: u32, on: bool, out: &mut Vec<InfoRequest>) {
        let Some(p) = self.local.clone() else {
            return;
        };
        match set_flag(&p, flag, on) {
            Ok(()) => {
                if let Some(d) = self.details.lock().unwrap().as_mut() {
                    if flag == UF_IMMUTABLE {
                        d.locked = on;
                    } else {
                        d.hidden = on;
                    }
                }
                self.error = None;
                self.changed(out);
            }
            Err(e) => self.error = Some(crate::i18n::localize_error(&e.to_string())),
        }
    }

    /// Test seams for the window's controls.
    #[cfg(test)]
    pub fn apply_mode_for_test(&mut self, mode: u32) {
        self.set_mode(mode, &mut Vec::new());
    }

    #[cfg(test)]
    pub fn apply_flag_for_test(&mut self, flag: u32, on: bool) {
        self.set_flag(flag, on, &mut Vec::new());
    }

    #[cfg(test)]
    pub fn apply_tag_for_test(&mut self, tag: &Tag, on: bool) {
        self.set_tag(tag, on, &mut Vec::new());
    }

    /// Draw the window. Returns `false` once it was closed.
    fn ui(&mut self, ctx: &egui::Context, out: &mut Vec<InfoRequest>) -> bool {
        let title = self.title();
        let spec = Spec {
            kind: AuxKind::Info(self.id),
            title: &title,
            size: egui::vec2(320.0, 560.0),
            min_size: egui::vec2(280.0, 300.0),
            margin: egui::Margin::same(space::LG as i8),
            activate: true,
            zoomable: false,
        };
        let mut window = std::mem::take(&mut self.window);
        let (_, closed) = window.show(ctx, &spec, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.contents(ui, out));
        });
        self.window = window;
        if self.counting && !self.size().done {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if closed.0 {
            self.window.hidden(ctx, AuxKind::Info(self.id));
        }
        !closed.0
    }

    fn contents(&mut self, ui: &mut egui::Ui, out: &mut Vec<InfoRequest>) {
        let p = pal(ui);
        ui.spacing_mut().item_spacing.y = space::SM;
        let single = self.single().cloned();
        let details = self.details();
        // Header: icon, name, kind and size.
        ui.horizontal(|ui| {
            let (glyph, tint, fill) = match &single {
                Some(e) => icons::entry_icon(e, p),
                None => (icons::COPY, p.generic, false),
            };
            let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
            widgets::paint_icon(ui, r, glyph, 44.0, tint, fill);
            ui.vertical(|ui| {
                match &single {
                    Some(e) => {
                        let id = egui::Id::unique(("info-name", self.id));
                        let resp = widgets::show_input(
                            ui,
                            widgets::input(&mut self.name_edit)
                                .id(id)
                                .desired_width(ui.available_width()),
                            t!("info.name"),
                        );
                        let rename =
                            resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let editable = !self.target.fs.is_read_only();
                        if rename && editable {
                            let new = self.name_edit.trim().to_string();
                            let old = util::display_name(&e.name).to_string();
                            if new != old {
                                match util::validate_file_name(&new) {
                                    Err(err) => self.error = Some(err),
                                    Ok(()) => {
                                        out.push(InfoRequest::Rename {
                                            fs: self.target.fs.clone(),
                                            from: e.path.clone(),
                                            name: new.clone(),
                                        });
                                        if let Some(parent) = e.path.parent() {
                                            let to = parent.join(&new);
                                            if let Some(it) = self.target.items.first_mut() {
                                                it.path = to.clone();
                                                it.name = new.clone();
                                            }
                                            self.local = self.target.fs.local_path(&to);
                                        }
                                        self.error = None;
                                    }
                                }
                            }
                        } else if rename {
                            self.name_edit = util::display_name(&e.name).to_string();
                        }
                    }
                    None => {
                        ui.label(
                            RichText::new(t!("info.items", count = self.target.items.len()))
                                .font(theme::semibold(text::BODY))
                                .color(p.text),
                        );
                    }
                }
                let kind = match &single {
                    Some(e) => icons::file_kind(&e.name, e.is_dir()).label().to_string(),
                    None => t!("info.kind_multi").to_string(),
                };
                ui.label(RichText::new(kind).color(p.text_secondary));
            });
        });
        if let Some(err) = &self.error {
            ui.label(RichText::new(err).color(p.danger));
        }
        ui.separator();
        widgets::section_title(ui, t!("info.general"));
        let size_text = self.size_text(single.as_ref());
        let row = |ui: &mut egui::Ui, label: &str, value: &str| {
            ui.label(RichText::new(label).color(p.text_secondary));
            let r = ui.add(egui::Label::new(RichText::new(value).color(p.text)).wrap());
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{label} {value}"))
            });
            ui.end_row();
        };
        egui::Grid::new(("info-general", self.id))
            .num_columns(2)
            .spacing([space::LG, space::SM])
            .show(ui, |ui| {
                if let Some(e) = &single {
                    row(
                        ui,
                        t!("info.kind"),
                        icons::file_kind(&e.name, e.is_dir()).label(),
                    );
                }
                row(ui, t!("info.size"), &size_text);
                row(ui, t!("info.where"), &self.where_text());
                if let Some(c) = details.as_ref().and_then(|d| d.created) {
                    row(ui, t!("info.created"), &crate::views::format_date(c));
                }
                if let Some(m) = single.as_ref().and_then(|e| e.modified) {
                    row(ui, t!("info.modified"), &crate::views::format_date(m));
                }
                if self.local.is_some() {
                    let last = details
                        .as_ref()
                        .and_then(|d| d.last_opened)
                        .map(crate::views::format_date)
                        .unwrap_or_else(|| "—".into());
                    row(ui, t!("info.last_opened"), &last);
                }
            });
        if self.local.is_some() {
            if let Some(d) = &details {
                self.tags_ui(ui, &d.tags, out);
                ui.horizontal(|ui| {
                    let mut locked = d.locked;
                    if ui
                        .checkbox(&mut locked, t!("info.locked"))
                        .on_hover_text(t!("info.locked_tip"))
                        .changed()
                    {
                        self.set_flag(UF_IMMUTABLE, locked, out);
                    }
                    let mut hidden = d.hidden;
                    if ui
                        .checkbox(&mut hidden, t!("info.hidden"))
                        .on_hover_text(t!("info.hidden_tip"))
                        .changed()
                    {
                        self.set_flag(UF_HIDDEN, hidden, out);
                    }
                });
                ui.separator();
                widgets::section_title(ui, t!("info.comments"));
                let c = d.comment.clone().unwrap_or_default();
                let shown = if c.is_empty() {
                    t!("info.no_comment").to_string()
                } else {
                    c
                };
                ui.label(RichText::new(shown).color(p.text_secondary))
                    .on_hover_text(t!("info.comment_tip"));
                if single.as_ref().is_some_and(|e| !e.is_dir()) || !d.apps.is_empty() {
                    ui.separator();
                    widgets::section_title(ui, t!("info.open_with"));
                    self.open_with_ui(ui, d, out);
                }
            } else {
                ui.add(egui::Spinner::new().size(14.0));
            }
        }
        if single.is_some() {
            ui.separator();
            widgets::section_title(ui, t!("info.permissions"));
            self.permissions_ui(ui, details.as_ref(), out);
        }
    }

    fn size_text(&self, single: Option<&Entry>) -> String {
        let counted = self.counting || single.is_none_or(|e| e.is_dir());
        if !counted {
            let s = single.map_or(0, |e| e.size);
            // "214 Byte" needs no "(214 Byte)" after it.
            if s < 1000 {
                return util::format_size(s);
            }
            return t!(
                "info.size_bytes",
                size = util::format_size(s),
                bytes = util::format_count(s)
            );
        }
        let c = self.size();
        let text = t!(
            "info.size_items",
            size = util::format_size(c.bytes),
            count = c.files
        );
        if c.done {
            text
        } else {
            t!("info.counting", size = text)
        }
    }

    fn where_text(&self) -> String {
        let parent = match self.target.items.as_slice() {
            [e] => e.path.parent(),
            items => {
                // The common folder, if they share one (always in a pane).
                let first = items.first().and_then(|e| e.path.parent());
                first.filter(|f| items.iter().all(|e| e.path.parent().as_ref() == Some(f)))
            }
        };
        let path = match parent {
            Some(p) => match self.target.fs.local_path(&p) {
                Some(l) => crate::search::where_label(&l.to_string_lossy(), ""),
                None => p.to_string(),
            },
            None => "—".into(),
        };
        if self.local.is_some() || self.target.fs.local_path(&VfsPath::root()).is_some() {
            path
        } else {
            format!("{} {path}", self.target.place)
        }
    }

    fn tags_ui(&mut self, ui: &mut egui::Ui, current: &[Tag], out: &mut Vec<InfoRequest>) {
        let p = pal(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(t!("info.tags")).color(p.text_secondary));
            let mut remove = None;
            for t in current {
                let dot = if t.color == tags::TagColor::None {
                    p.text_tertiary
                } else {
                    t.color.rgb(p.dark)
                };
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    "● ",
                    0.0,
                    egui::TextFormat::simple(theme::regular(text::SMALL), dot),
                );
                job.append(
                    &t.name,
                    0.0,
                    egui::TextFormat::simple(theme::regular(text::SMALL), p.text),
                );
                job.append(
                    &format!("  {}", icons::X),
                    0.0,
                    egui::TextFormat::simple(theme::icon_font(10.0), p.text_secondary),
                );
                let resp = ui
                    .add(egui::Button::new(job).corner_radius(10.0))
                    .on_hover_text(t!("tags.remove_named", name = t.name));
                crate::a11y::name(
                    &resp,
                    egui::accesskit::Role::Button,
                    &t!("tags.remove_named", name = t.name),
                );
                if resp.clicked() {
                    remove = Some(t.clone());
                }
            }
            if let Some(t) = remove {
                self.set_tag(&t, false, out);
            }
            // Add: a menu with the colours and known tags, like the context menu.
            let known = tags::known(ui.ctx());
            let refs: [&[Tag]; 1] = [current];
            let menu = tags::TagMenu::new(&known, &refs);
            let mut edit = None;
            ui.menu_button(format!("{}  {}", icons::PLUS, t!("info.add_tag")), |ui| {
                edit = tags::tags_menu(ui, &menu);
            });
            if let Some(e) = edit {
                self.set_tag(&e.tag, e.on, out);
            }
            let _ = &self.new_tag;
        });
    }

    fn open_with_ui(&mut self, ui: &mut egui::Ui, d: &Details, out: &mut Vec<InfoRequest>) {
        let p = pal(ui);
        let default = d
            .default_app
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| t!("info.no_app").to_string());
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("info.default_app")).color(p.text_secondary));
            ui.label(RichText::new(default).color(p.text));
        });
        let Some(file) = self.local.clone() else {
            return;
        };
        if d.apps.is_empty() {
            return;
        }
        ui.menu_button(t!("info.open_with_menu"), |ui| {
            ui.set_min_width(200.0);
            for a in &d.apps {
                let is_default = d.default_app.as_ref().is_some_and(|x| x.path == a.path);
                let label = if is_default {
                    t!("info.app_default", app = a.name)
                } else {
                    a.name.clone()
                };
                if ui
                    .button(label)
                    .on_hover_text(a.path.display().to_string())
                    .clicked()
                {
                    out.push(InfoRequest::OpenWith {
                        app: a.path.clone(),
                        file: file.clone(),
                    });
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(t!("info.open_with_tip"));
    }

    fn permissions_ui(
        &mut self,
        ui: &mut egui::Ui,
        d: Option<&Details>,
        out: &mut Vec<InfoRequest>,
    ) {
        let p = pal(ui);
        let Some(mode) = self.mode() else {
            ui.label(RichText::new(t!("info.no_permissions")).color(p.text_secondary));
            return;
        };
        let editable = self.chmod_ok();
        let who = [
            (t!("info.owner"), d.and_then(|d| d.owner.clone()), 6),
            (t!("info.group"), d.and_then(|d| d.group.clone()), 3),
            (t!("info.everyone"), None, 0),
        ];
        let mut new_mode = mode;
        egui::Grid::new(("info-perms", self.id))
            .num_columns(4)
            .spacing([space::MD, space::XS])
            .show(ui, |ui| {
                ui.label("");
                for h in [t!("info.read"), t!("info.write"), t!("info.execute")] {
                    ui.label(
                        RichText::new(h)
                            .font(theme::medium(text::CAPTION))
                            .color(p.text_secondary),
                    );
                }
                ui.end_row();
                for (label, name, shift) in who {
                    let l = match &name {
                        Some(n) => format!("{label} ({n})"),
                        None => label.to_string(),
                    };
                    ui.label(RichText::new(&l).color(p.text));
                    for (bit, what) in [
                        (4u32, t!("info.read")),
                        (2, t!("info.write")),
                        (1, t!("info.execute")),
                    ] {
                        let mask = bit << shift;
                        let mut on = new_mode & mask != 0;
                        let resp = ui.add_enabled(editable, egui::Checkbox::without_text(&mut on));
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::Role::CheckBox,
                                editable,
                                on,
                                format!("{l}: {what}"),
                            )
                        });
                        if resp.changed() {
                            new_mode = if on {
                                new_mode | mask
                            } else {
                                new_mode & !mask
                            };
                        }
                    }
                    ui.end_row();
                }
            });
        if new_mode != mode {
            self.set_mode(new_mode, out);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("info.octal")).color(p.text_secondary));
            if self.octal_edit.is_empty() {
                self.octal_edit = format!("{:03o}", mode & 0o777);
            }
            let id = egui::Id::unique(("info-octal", self.id));
            let resp = ui.add_enabled(
                editable,
                egui::TextEdit::singleline(&mut self.octal_edit)
                    .id(id)
                    .desired_width(48.0)
                    .font(theme::mono(text::BODY)),
            );
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::TextInput, editable, t!("info.octal"))
            });
            let commit = resp.lost_focus();
            if commit {
                match u32::from_str_radix(self.octal_edit.trim(), 8) {
                    Ok(m) if m <= 0o7777 => {
                        let m = (mode & !0o7777) | m;
                        if m != mode {
                            self.set_mode(m, out);
                        }
                    }
                    _ => {
                        self.error = Some(t!("info.bad_octal").to_string());
                        self.octal_edit = format!("{:03o}", mode & 0o777);
                    }
                }
            } else if !resp.has_focus() {
                let want = format!("{:03o}", mode & 0o777);
                if self.octal_edit != want {
                    self.octal_edit = want;
                }
            }
            ui.label(
                RichText::new(util::format_permissions(
                    Some(mode),
                    single_kind(&self.target),
                ))
                .font(theme::mono(text::CAPTION))
                .color(p.text_secondary),
            );
        });
        if !editable {
            ui.label(
                RichText::new(t!("info.permissions_read_only"))
                    .font(theme::regular(text::CAPTION))
                    .color(p.text_tertiary),
            );
        }
    }
}

fn single_kind(t: &Target) -> manta_core::EntryKind {
    t.items
        .first()
        .map_or(manta_core::EntryKind::File, |e| e.kind)
}

/// The open Info windows.
pub struct InfoWindows {
    pub windows: Vec<InfoWindow>,
    next: u64,
    sys: Arc<dyn InfoSystem>,
    /// Read details on the UI thread (tests: deterministic).
    inline: bool,
}

impl InfoWindows {
    pub fn new(sys: Arc<dyn InfoSystem>, inline: bool) -> InfoWindows {
        InfoWindows {
            windows: Vec::new(),
            next: 1,
            sys,
            inline,
        }
    }

    /// The real system lookups, in the background.
    #[cfg_attr(test, allow(dead_code))]
    pub fn system() -> InfoWindows {
        InfoWindows::new(Arc::new(MacInfo), false)
    }

    /// The aux windows that are open (focus tracking).
    pub fn kinds(&self) -> Vec<AuxKind> {
        self.windows.iter().map(|w| AuxKind::Info(w.id)).collect()
    }

    /// Open (or bring to the front) the Info window for `target`.
    pub fn open(&mut self, ctx: &egui::Context, target: Target) {
        if target.items.is_empty() {
            return;
        }
        if let [e] = target.items.as_slice() {
            if let Some(w) = self
                .windows
                .iter_mut()
                .find(|w| w.is_for(&target.fs, &e.path))
            {
                w.window.request_focus();
                return;
            }
        }
        while self.windows.len() >= MAX_WINDOWS {
            let old = self.windows.remove(0);
            let mut win = old;
            win.window.hidden(ctx, AuxKind::Info(win.id));
        }
        let id = self.next;
        self.next += 1;
        // Cascade like Finder: each new window a little lower right.
        let step = (self.windows.len() as f32) * 22.0;
        let local = match target.items.as_slice() {
            [e] => (target.fs.protocol() == Protocol::Local && !target.fs.is_read_only())
                .then(|| target.fs.local_path(&e.path))
                .flatten(),
            _ => None,
        };
        let details = Arc::new(Mutex::new(None));
        if let Some(path) = &local {
            if self.inline {
                *details.lock().unwrap() = Some(local_details(path, &*self.sys));
            } else {
                let path = path.clone();
                let sys = self.sys.clone();
                let out = details.clone();
                let ctx = ctx.clone();
                let _ = std::thread::Builder::new()
                    .name("manta-info".into())
                    .spawn(move || {
                        let d = local_details(&path, &*sys);
                        *out.lock().unwrap() = Some(d);
                        ctx.request_repaint();
                    });
            }
        }
        let counting = target.items.len() > 1 || target.items.iter().any(|e| e.is_dir());
        let size = Arc::new(Mutex::new(SizeCount::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        if counting {
            count_size(
                target.fs.clone(),
                target.items.clone(),
                size.clone(),
                cancel.clone(),
                ctx.clone(),
            );
        }
        let name_edit = target
            .items
            .first()
            .map(|e| util::display_name(&e.name).to_string())
            .unwrap_or_default();
        self.windows.push(InfoWindow {
            id,
            window: AuxWindow::with_offset(egui::vec2(step, step)),
            target,
            local,
            details,
            size,
            counting,
            cancel,
            name_edit,
            octal_edit: String::new(),
            new_tag: String::new(),
            error: None,
        });
    }

    /// Draw the open windows; returns what they ask the app to do.
    pub fn ui(&mut self, ctx: &egui::Context) -> Vec<InfoRequest> {
        let mut out = Vec::new();
        self.windows.retain_mut(|w| w.ui(ctx, &mut out));
        out
    }

    /// Wait for the background reads (tests).
    #[cfg(test)]
    pub fn wait(&self, timeout: Duration) {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            let busy = self.windows.iter().any(|w| {
                (w.counting && !w.size().done) || (w.local.is_some() && w.details().is_none())
            });
            if !busy {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdls_dates() {
        let d = parse_mdls_date("2026-10-24 12:02:03 +0000").unwrap();
        assert_eq!(d.to_rfc3339(), "2026-10-24T12:02:03+00:00");
        assert!(parse_mdls_date("").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn flags_on_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        set_flag(&f, UF_HIDDEN, true).unwrap();
        assert_ne!(flags(&f).unwrap() & UF_HIDDEN, 0);
        set_flag(&f, UF_IMMUTABLE, true).unwrap();
        assert!(
            std::fs::write(&f, b"y").is_err(),
            "locked files can't be written"
        );
        set_flag(&f, UF_IMMUTABLE, false).unwrap();
        set_flag(&f, UF_HIDDEN, false).unwrap();
        assert_eq!(flags(&f).unwrap() & (UF_HIDDEN | UF_IMMUTABLE), 0);
        std::fs::write(&f, b"y").unwrap();
    }

    #[test]
    fn local_details_read_owner_mode_and_apps() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("notes.txt");
        std::fs::write(&f, b"hello").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
        let sys = FakeInfo {
            comment: Some("Remember".into()),
            default: Some(App::at("/Applications/TextEdit.app".into())),
            apps: vec![App::at("/Applications/TextEdit.app".into())],
            ..Default::default()
        };
        let d = local_details(&f, &sys);
        assert_eq!(d.mode, Some(0o640));
        assert!(d.owner.is_some());
        assert!(d.created.is_some());
        assert_eq!(d.comment.as_deref(), Some("Remember"));
        assert_eq!(d.default_app.unwrap().name, "TextEdit");
        assert!(!d.locked && !d.hidden);
    }

    #[test]
    fn folder_sizes_are_counted() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::write(tmp.path().join("a/one"), [0u8; 100]).unwrap();
        std::fs::write(tmp.path().join("a/b/two"), [0u8; 50]).unwrap();
        let fs: SharedVfs = Arc::new(manta_vfs::LocalFs::new());
        let entry = manta_vfs::LocalFs::new()
            .stat(&VfsPath::new(&tmp.path().join("a").to_string_lossy()))
            .unwrap();
        let out = Arc::new(Mutex::new(SizeCount::default()));
        count_size(
            fs,
            vec![entry],
            out.clone(),
            Arc::new(AtomicBool::new(false)),
            egui::Context::default(),
        );
        let start = std::time::Instant::now();
        while !out.lock().unwrap().done && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let c = *out.lock().unwrap();
        assert!(c.done);
        assert_eq!((c.bytes, c.files), (150, 2));
    }

    use manta_core::Vfs;
}
