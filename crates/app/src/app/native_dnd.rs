//! Drag & drop with other apps, on the app side (see `crate::native_drag`):
//! handing an internal file drag to macOS when it leaves the window, the
//! results of those native sessions (toasts, refreshing folders the
//! destination moved files out of) and files dropped onto a pane by another
//! app, which become ordinary copy / move jobs on the transfer manager.

use std::path::PathBuf;

use eframe::egui;
use manta_core::VfsPath;

use super::archive_ops::ReadOnlyOp;
use super::{MantaApp, Side};
use crate::dialogs::TransferForm;
use crate::native_drag::{self, DragEvent};
use crate::pane::DragPayload;
use crate::util::same_fs;

impl MantaApp {
    /// Once per frame after the panes: an internal file drag that leaves the
    /// window continues as a native drag (Finder, Mail, cloud apps, …), and
    /// what earlier native sessions reported is handled.
    pub(super) fn native_drag_frame(&mut self, ctx: &egui::Context) {
        if let Some(p) = egui::DragAndDrop::payload::<DragPayload>(ctx) {
            native_drag::drag_out_when_leaving(ctx, || {
                native_drag::items_from_vfs(&p.fs, &p.paths, &p.dirs)
            });
        }
        for ev in native_drag::poll_events() {
            self.native_drag_event(ev);
        }
    }

    pub(super) fn native_drag_event(&mut self, ev: DragEvent) {
        match ev {
            // The destination moved (or trashed) the files: they are gone
            // from their folders. Manta itself never deletes anything here.
            DragEvent::Ended { op, local } if op.source_changed() => {
                let local_fs = self.local.clone();
                let mut dirs: Vec<VfsPath> = Vec::new();
                for p in &local {
                    let dir = VfsPath::new(&p.to_string_lossy())
                        .parent()
                        .unwrap_or_else(VfsPath::root);
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
                for dir in dirs {
                    self.refresh_matching(&local_fs, &dir);
                }
            }
            DragEvent::PromiseWriting { name, backend, .. } => {
                self.set_info(t!(
                    "pane.drag.downloading",
                    name = name,
                    backend = crate::i18n::fs_label(&backend)
                ));
            }
            DragEvent::PromiseDone { name, dest, result } => match result {
                Ok(()) => {
                    self.set_success(t!("pane.drag.downloaded", name = name));
                    // A pane may be showing the folder it landed in.
                    if let Some(dir) = dest.parent() {
                        let local_fs = self.local.clone();
                        self.refresh_matching(&local_fs, &VfsPath::new(&dir.to_string_lossy()));
                    }
                }
                Err(e) => self.set_error_detail(t!("pane.drag.download_failed", name = name), e),
            },
            DragEvent::Failed(e) => self.set_error_detail(t!("pane.drag.failed"), e),
            DragEvent::Started { .. }
            | DragEvent::MaskRequested { .. }
            | DragEvent::Ended { .. } => {}
        }
    }

    /// Files from another app dropped onto pane `to`: copy (or move) them
    /// into `dir` on that pane's backend. Like a drop between panes this
    /// asks first (Copy preselected) unless Settings say "Always copy" /
    /// "Always move"; the job runs on the transfer manager (pause / resume,
    /// journal).
    pub(super) fn drop_external(
        &mut self,
        to: Side,
        dir: VfsPath,
        paths: Vec<PathBuf>,
        mv: bool,
        from_self: bool,
    ) {
        if !self.has_pane(to) {
            return;
        }
        self.activate(to);
        if self.refuse_read_only(to, ReadOnlyOp::Paste) {
            return;
        }
        let src_fs = self.local.clone();
        let dst = self.pane(to).tab().loc.clone();
        let all: Vec<VfsPath> = paths
            .iter()
            .map(|p| VfsPath::new(&p.to_string_lossy()))
            .collect();
        let local_dst = same_fs(&src_fs, &dst.fs);
        // Items already in that folder stay where they are; a folder can't
        // go into itself.
        let sources: Vec<VfsPath> = all
            .iter()
            .filter(|s| !(local_dst && s.parent().unwrap_or_else(VfsPath::root) == dir))
            .filter(|s| !(local_dst && dir.starts_with(s)))
            .cloned()
            .collect();
        if sources.is_empty() {
            // Our own drag dropped back where it came from: nothing to say.
            if !from_self && !all.is_empty() {
                let into_itself = local_dst && all.iter().any(|s| dir.starts_with(s) && *s != dir);
                self.set_info(if into_itself {
                    t!("status.into_itself")
                } else {
                    t!("status.same_folder")
                });
            }
            return;
        }
        log::info!(
            "drop from another app: {} item(s) → {} {} ({})",
            sources.len(),
            dst.fs.label(),
            dir,
            if mv { "move" } else { "copy" }
        );
        let folders = self.folder_paths(&src_fs, &sources);
        let form = TransferForm {
            extract: false,
            archive: None,
            src_side: to,
            dst_side: to,
            targets: Vec::new(),
            folders,
            mv,
            src_fs,
            sources,
            dst_label: self.place_label(&crate::pane::Location::new(
                dst.fs.clone(),
                dst.conn,
                dir.clone(),
            )),
            dst_fs: dst.fs,
            dst_dir: dir,
            conflict: self.prefs.conflict.policy(),
            choose: true,
        };
        self.ask_or_submit(form);
    }
}
