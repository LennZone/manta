//! The Trash in the sidebar, on the app side (see `crate::trash`): its
//! full / empty state, opening it, "Empty Trash…" (⇧⌘⌫), and items dropped
//! onto it (from a pane or another app).
//!
//! The state is looked at again after Manta trashes or deletes something,
//! on FSEvents for the Trash folder (when Manta may read it) and every few
//! seconds otherwise (a `stat`, cheap).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use manta_core::{SharedVfs, VfsPath};

use super::{can_trash, MantaApp, Side};
use crate::dialogs::{DeleteForm, Dialog, EmptyTrashForm};
use crate::trash::{self, TrashStatus};
use crate::worker::WorkResult;

/// How often the state is looked at again without events.
const POLL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(crate) struct TrashPlace {
    /// The Trash folder (`None`: none here, or not looked up yet).
    pub dir: Option<PathBuf>,
    pub status: TrashStatus,
    /// When the state was last looked at (`None`: look now).
    checked: Option<Instant>,
}

impl TrashPlace {
    /// The Trash changed (or a folder in it): look again next frame.
    pub fn note_change(&mut self, dir: &Path) {
        if trash::is_in_trash(self.dir.as_deref(), dir) {
            self.checked = None;
        }
    }

    /// Look again next frame (Manta trashed something).
    pub fn stale(&mut self) {
        self.checked = None;
    }

    /// The folder to watch with FSEvents (only when it can be read).
    pub fn watch_dir(&self) -> Option<PathBuf> {
        self.dir.clone().filter(|_| self.status.readable)
    }

    fn refresh(&mut self) {
        if let Some(d) = &self.dir {
            self.status = trash::probe(d);
        }
        self.checked = Some(Instant::now());
    }
}

/// `loc` shows the Trash folder (or a folder inside it).
pub(crate) fn shows_trash(ctx: &egui::Context, loc: &crate::pane::Location) -> bool {
    loc.archive.is_none()
        && loc.conn.is_none()
        && loc.fs.protocol() == manta_core::Protocol::Local
        && loc
            .fs
            .local_path(&loc.path)
            .is_some_and(|p| trash::is_in_trash(ui_dir(ctx).as_deref(), &p))
}

fn ui_dir_id() -> egui::Id {
    egui::Id::unique("manta-trash-dir")
}

/// The Trash folder, for code that only has the context (panes).
pub(crate) fn ui_dir(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.data(|d| d.get_temp::<Option<PathBuf>>(ui_dir_id()))
        .flatten()
}

impl MantaApp {
    /// Once per frame: look up the Trash folder, and its state when due.
    pub(super) fn trash_frame(&mut self, ctx: &egui::Context) {
        if self.trash.dir.is_none() && self.trash.checked.is_none() {
            self.trash.dir = self.trasher.dir();
        }
        let watched = self.live.enabled() && self.trash.status.readable;
        let due = self
            .trash
            .checked
            .is_none_or(|t| t.elapsed() >= if watched { POLL * 6 } else { POLL });
        if due {
            self.trash.refresh();
            if self.trash.dir.is_some() && !watched && !cfg!(test) {
                ctx.request_repaint_after(POLL);
            }
        }
        let dir = self.trash.dir.clone();
        ctx.data_mut(|d| d.insert_temp(ui_dir_id(), dir));
    }

    /// The active pane shows `path` (in or of the Trash) and is local.
    fn in_trash(&self, fs: &SharedVfs, path: &VfsPath) -> bool {
        fs.protocol() == manta_core::Protocol::Local
            && fs
                .local_path(path)
                .is_some_and(|p| trash::is_in_trash(self.trash.dir.as_deref(), &p))
    }

    /// The items of `paths` (on `fs`) are already in the Trash.
    pub(super) fn all_in_trash(&self, fs: &SharedVfs, paths: &[VfsPath]) -> bool {
        !paths.is_empty() && paths.iter().all(|p| self.in_trash(fs, p))
    }

    /// Items dragged from a pane onto the Trash: local ones go to the Trash
    /// like ⌘⌫ (no question); servers, containers and cloud APIs have no
    /// Trash, so their items are deleted permanently after asking. One pane
    /// drag is always from one place, so a drop never mixes the two.
    pub(super) fn drop_on_trash(&mut self, from: Side, fs: SharedVfs, paths: Vec<VfsPath>) {
        if paths.is_empty() || self.all_in_trash(&fs, &paths) {
            return;
        }
        if fs.is_read_only() {
            // An archive: the sidebar refuses the drop already.
            return;
        }
        if can_trash(&fs) {
            self.trash_items(from, fs, paths);
        } else {
            let folders = self.folder_paths(&fs, &paths);
            self.dialog = Some(Dialog::Delete(DeleteForm {
                side: from,
                no_trash: true,
                in_trash: false,
                fs,
                paths,
                folders,
            }));
        }
    }

    /// Files from another app dropped onto the Trash: into the Trash.
    pub(super) fn trash_external(&mut self, paths: Vec<PathBuf>) {
        let fs = self.local.clone();
        let paths: Vec<VfsPath> = paths
            .iter()
            .map(|p| VfsPath::new(&p.to_string_lossy()))
            .collect();
        if paths.is_empty() || self.all_in_trash(&fs, &paths) {
            return;
        }
        log::info!(
            "drop from another app onto the Trash: {} item(s)",
            paths.len()
        );
        self.trash_items(self.active, fs, paths);
    }

    /// "Empty Trash…": ask (with the number of items and their size).
    pub(super) fn ask_empty_trash(&mut self) {
        let Some(dir) = self.trash.dir.clone() else {
            return;
        };
        self.trash.refresh();
        let st = self.trash.status;
        if st.items == Some(0) {
            self.set_info(t!("sidebar.trash.empty"));
            return;
        }
        self.dialog = Some(Dialog::EmptyTrash(EmptyTrashForm {
            items: st.items,
            bytes: None,
            measuring: st.readable,
            readable: st.readable,
        }));
        if st.readable {
            self.worker.spawn(
                "trash-size",
                move || Ok(trash::summary(&dir)),
                |r: Result<Option<trash::TrashSummary>, String>| {
                    WorkResult::TrashMeasured(r.ok().flatten())
                },
            );
        }
    }

    /// The size of the Trash arrived: show it in the question.
    pub(super) fn on_trash_measured(&mut self, summary: Option<trash::TrashSummary>) {
        if let Some(Dialog::EmptyTrash(f)) = &mut self.dialog {
            f.measuring = false;
            if let Some(s) = summary {
                f.items = Some(s.items);
                f.bytes = Some(s.bytes);
            }
        }
    }

    /// Confirmed: delete everything in the Trash for good, in the
    /// background (item by item; Finder when Manta can't read the folder).
    pub(super) fn empty_trash(&mut self) {
        let trasher = self.trasher.clone();
        self.set_info(t!("status.emptying_trash"));
        self.worker.spawn(
            "empty-trash",
            move || trash::empty(trasher.as_ref()),
            WorkResult::TrashEmptied,
        );
    }

    pub(super) fn on_trash_emptied(&mut self, result: Result<trash::EmptyOutcome, String>) {
        match result {
            Ok(out) if out.failed.is_empty() => {
                if out.by_finder {
                    self.set_success(t!("status.trash_emptied_finder"));
                } else {
                    self.set_success(t!("status.trash_emptied", count = out.removed));
                }
            }
            Ok(out) => {
                let names: Vec<String> = out
                    .failed
                    .iter()
                    .map(|(p, e)| {
                        let n = p
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        format!("{n}: {e}")
                    })
                    .collect();
                self.set_error_detail(
                    t!("error.empty_trash_some", count = out.failed.len()),
                    names.join("\n"),
                );
            }
            Err(e) => self.set_error_detail(t!("error.empty_trash").to_string(), e),
        }
        self.trash.refresh();
        if let Some(dir) = self.trash.dir.clone() {
            let fs = self.local.clone();
            self.refresh_matching(&fs, &VfsPath::new(&dir.to_string_lossy()));
        }
    }

    /// Finder shows the Trash (by bundle id: Manta may be the default
    /// file viewer, which would open it here again).
    pub(super) fn show_trash_in_finder(&mut self) {
        let Some(dir) = self.trash.dir.clone() else {
            return;
        };
        self.launch(crate::logging::finder_open_args(&dir));
    }

    /// System Settings at Privacy & Security › Full Disk Access.
    pub(super) fn open_full_disk_access(&mut self) {
        self.launch(vec![FULL_DISK_ACCESS_URL.into()]);
    }

    /// Run `/usr/bin/open` with `args` (tests: recorded instead).
    pub(in crate::app) fn launch(&mut self, args: Vec<std::ffi::OsString>) {
        #[cfg(test)]
        self.launched.push(
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
        );
        #[cfg(not(test))]
        if let Err(e) = std::process::Command::new("/usr/bin/open")
            .args(&args)
            .spawn()
        {
            log::warn!("can't run open {args:?}: {e}");
        }
    }
}

/// System Settings › Privacy & Security › Full Disk Access.
pub(crate) const FULL_DISK_ACCESS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";
