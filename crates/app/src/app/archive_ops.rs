//! Archives in the GUI: opening an archive as a folder in the pane (local,
//! remote or nested, in the background with progress), leaving it again,
//! the read-only rules, "Extract Here" / "Extract to Other Pane" and the
//! "Compress…" sheet.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use manta_core::{Entry, EntryKind, SharedVfs, VfsError, VfsPath};
use manta_ops::{CompressJob, ConflictPolicy, Job, JobKind};
use manta_plugin::{ArchiveSpec, FileActionKind};
use manta_vfs::archive::{self, ArchiveFormat, ArchiveFs, OpenOptions};

use super::{navigate_tab, MantaApp, Side};

/// What the user tried to do in a read-only location.
#[derive(Debug, Clone, Copy)]
pub(super) enum ReadOnlyOp {
    Rename,
    NewFolder,
    NewFile,
    Delete,
    Paste,
    Compress,
}
use crate::compress_sheet::CompressForm;
use crate::dialogs::Dialog;
use crate::pane::{ArchiveMount, ArchiveOpening, Location};
use crate::toasts::ToastKind;
use crate::util::same_fs;
use crate::worker::WorkResult;

/// What to do once an archive is open.
pub(crate) enum ArchivePurpose {
    /// Show it in the tab.
    Browse,
    /// Extract everything into `dst` (into a new folder named after the
    /// archive, unless the archive holds a single item).
    ExtractAll { dst: Location },
}

/// An archive opened in the background.
pub(crate) struct OpenedArchive {
    pub fs: SharedVfs,
    /// `ExtractAll`: the top-level entries and the folder to extract into.
    pub extract: Option<(Vec<VfsPath>, VfsPath)>,
}

/// Everything the UI needs when an open finishes.
pub(crate) struct ArchiveOpened {
    pub tab_id: u64,
    pub id: u64,
    pub entry: Entry,
    /// The folder the archive file is in.
    pub outer: Location,
    pub purpose: ArchivePurpose,
    pub result: Result<OpenedArchive, String>,
}

/// Blocking: copy `path` from `fs` into a temp file (progress in `done`).
fn download(
    fs: &SharedVfs,
    path: &VfsPath,
    done: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<tempfile::TempPath, VfsError> {
    let temp = tempfile::Builder::new()
        .prefix(archive::TEMP_PREFIX)
        .suffix(".download")
        .tempfile()?;
    let (mut file, tpath) = temp.into_parts();
    let mut r = fs.open_read(path)?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut n_total = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(VfsError::Cancelled);
        }
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        file.write_all(&buf[..n])?;
        n_total += n as u64;
        done.store(n_total, Ordering::Relaxed);
    }
    file.flush()?;
    Ok(tpath)
}

/// "backup.zip" → its folder name for "Extract Here".
fn extract_folder_name(name: &str) -> String {
    let stem = ArchiveFormat::stem(name);
    if stem.is_empty() || stem == name {
        t!("archive.contents_folder", name = name)
    } else {
        stem.to_string()
    }
}

impl MantaApp {
    /// The [`ArchiveSpec`] for terminal echoes of the archive `mount` shows.
    pub(super) fn archive_spec(&self, mount: &ArchiveMount) -> ArchiveSpec {
        let outer = &mount.outer;
        let local = outer.archive.is_none() && outer.fs.local_path(&outer.path).is_some();
        ArchiveSpec {
            path: mount.file_path(),
            format: mount.format.clone(),
            local,
            remote: if local {
                None
            } else {
                self.endpoint(&outer.fs)
            },
            level: None,
            exclude_junk: false,
        }
    }

    /// Show a notice and return true if the pane can't be written to.
    pub(super) fn refuse_read_only(&mut self, side: Side, op: ReadOnlyOp) -> bool {
        let tab = self.pane(side).tab();
        if !tab.loc.read_only() {
            return false;
        }
        let name = tab
            .loc
            .archive
            .as_ref()
            .map(|m| m.file_name.clone())
            .unwrap_or_else(|| crate::i18n::fs_label(&tab.loc.fs.label()));
        self.toasts.push(
            ToastKind::Info,
            match op {
                ReadOnlyOp::Rename => t!("archive.cant_rename", name = name),
                ReadOnlyOp::NewFolder => t!("archive.cant_new_folder", name = name),
                ReadOnlyOp::NewFile => t!("archive.cant_new_file", name = name),
                ReadOnlyOp::Delete => t!("archive.cant_delete", name = name),
                ReadOnlyOp::Paste => t!("archive.cant_paste", name = name),
                ReadOnlyOp::Compress => t!("archive.cant_compress", name = name),
            },
            Some(t!("archive.read_only_detail").into()),
        );
        true
    }

    /// Open the archive `entry` of the active tab of `side` in the
    /// background: download it first if it isn't a local file (remote or
    /// nested archive), then index it.
    pub(super) fn open_archive(&mut self, side: Side, entry: Entry, purpose: ArchivePurpose) {
        let id = self.next_id();
        let pi = self.pos(side);
        let tab = self.panes[pi].tab_mut();
        if let Some(o) = &tab.opening {
            o.cancel.store(true, Ordering::Relaxed);
        }
        let done = Arc::new(AtomicU64::new(0));
        let downloading = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        tab.opening = Some(ArchiveOpening {
            id,
            name: entry.name.clone(),
            done: done.clone(),
            total: entry.size,
            downloading: downloading.clone(),
            cancel: cancel.clone(),
            since: Instant::now(),
        });
        let tab_id = tab.id;
        let outer = tab.loc.clone();
        let fs = outer.fs.clone();
        let path = entry.path.clone();
        let name = entry.name.clone();
        let extract_dst = match &purpose {
            ArchivePurpose::ExtractAll { dst } => Some((dst.fs.clone(), dst.path.clone())),
            ArchivePurpose::Browse => None,
        };
        let msg_entry = entry.clone();
        self.worker.spawn(
            "open-archive",
            move || {
                let opts = OpenOptions {
                    progress: Some(done.clone()),
                    cancel: Some(cancel.clone()),
                    ..Default::default()
                };
                let err = |e: VfsError| match e {
                    VfsError::Cancelled => "cancelled".to_string(),
                    e => e.to_string(),
                };
                let ar = match fs.local_path(&path) {
                    Some(os) => ArchiveFs::open_named(&os, &name, &opts).map_err(err)?,
                    None => {
                        downloading.store(true, Ordering::Relaxed);
                        let temp = download(&fs, &path, &done, &cancel).map_err(err)?;
                        downloading.store(false, Ordering::Relaxed);
                        done.store(0, Ordering::Relaxed);
                        let mut ar = ArchiveFs::open_named(&temp, &name, &opts).map_err(err)?;
                        ar.adopt_temp(temp);
                        ar
                    }
                };
                let fs: SharedVfs = Arc::new(ar);
                let extract = match extract_dst {
                    None => None,
                    Some((dst_fs, dst_dir)) => {
                        let top = fs.list(&VfsPath::root()).map_err(|e| e.to_string())?;
                        let sources: Vec<VfsPath> = top.iter().map(|e| e.path.clone()).collect();
                        let dir = if top.len() == 1 {
                            dst_dir
                        } else {
                            let want = dst_dir.join(&extract_folder_name(&name));
                            if dst_fs.stat(&want).is_ok() {
                                manta_ops::unique_name(&*dst_fs, &want, true)
                            } else {
                                want
                            }
                        };
                        Some((sources, dir))
                    }
                };
                Ok(OpenedArchive { fs, extract })
            },
            move |result| {
                WorkResult::ArchiveOpened(Box::new(ArchiveOpened {
                    tab_id,
                    id,
                    entry: msg_entry,
                    outer,
                    purpose,
                    result,
                }))
            },
        );
    }

    pub(super) fn cancel_archive_open(&mut self, side: Side) {
        let pi = self.pos(side);
        if let Some(o) = self.panes[pi].tab_mut().opening.take() {
            o.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn on_archive_opened(&mut self, msg: ArchiveOpened) {
        let ArchiveOpened {
            tab_id,
            id,
            entry,
            outer,
            purpose,
            result,
        } = msg;
        let Some(tab) = self
            .panes
            .iter_mut()
            .flat_map(|p| p.tabs.iter_mut())
            .find(|t| t.id == tab_id)
        else {
            return;
        };
        if tab.opening.as_ref().is_none_or(|o| o.id != id) {
            return; // cancelled or superseded
        }
        tab.opening = None;
        let opened = match result {
            Ok(o) => o,
            Err(e) if e == "cancelled" => return,
            Err(e) => {
                self.set_error_detail(t!("error.couldnt_open", name = entry.name), e);
                return;
            }
        };
        let (format, rejected) = match archive::as_archive(&*opened.fs) {
            Some(a) => (a.format().label().to_lowercase(), a.rejected().len()),
            None => (String::new(), 0),
        };
        if rejected > 0 {
            self.toasts.push(
                ToastKind::Warning,
                t!(
                    "archive.skipped_unsafe",
                    count = rejected,
                    name = entry.name
                ),
                Some(t!("archive.unsafe_detail").into()),
            );
        }
        let mount = Arc::new(ArchiveMount {
            outer: outer.clone(),
            file_name: entry.name.clone(),
            format,
        });
        match purpose {
            ArchivePurpose::Browse => {
                let loc = Location {
                    fs: opened.fs,
                    conn: outer.conn,
                    path: VfsPath::root(),
                    archive: Some(mount),
                };
                let worker = &self.worker;
                let tab = self
                    .panes
                    .iter_mut()
                    .flat_map(|p| p.tabs.iter_mut())
                    .find(|t| t.id == tab_id)
                    .expect("tab");
                navigate_tab(worker, tab, loc, false, true);
            }
            ArchivePurpose::ExtractAll { dst } => {
                let Some((sources, dir)) = opened.extract else {
                    return;
                };
                if sources.is_empty() {
                    self.set_info(t!("archive.empty", name = entry.name));
                    return;
                }
                let side = self.side_of_tab(tab_id);
                let dst = dst.with_path(dir);
                self.enqueue_extract(
                    side,
                    opened.fs,
                    &mount,
                    sources,
                    dst,
                    ConflictPolicy::Rename,
                );
            }
        }
    }

    /// Queue an Extract job (archive → `dst`) and report it to plugins.
    fn enqueue_extract(
        &mut self,
        side: Side,
        fs: SharedVfs,
        mount: &ArchiveMount,
        sources: Vec<VfsPath>,
        dst: Location,
        conflict: ConflictPolicy,
    ) {
        let whole = sources
            .iter()
            .all(|s| s.parent().is_some_and(|p| p.is_root()))
            && fs
                .list(&VfsPath::root())
                .is_ok_and(|v| v.len() == sources.len());
        let mut a = self.new_action(
            side,
            FileActionKind::Extract,
            &fs,
            if whole { Vec::new() } else { sources.clone() },
            Some((&dst.fs, dst.path.clone())),
        );
        a.archive = Some(Box::new(self.archive_spec(mount)));
        let action = self.start_action(a);
        let refresh = vec![
            (
                dst.fs.clone(),
                dst.path.parent().unwrap_or_else(VfsPath::root),
            ),
            (dst.fs.clone(), dst.path.clone()),
        ];
        let job = Job {
            kind: JobKind::Extract,
            src_fs: fs,
            sources,
            dst_fs: Some(dst.fs),
            dst_dir: Some(dst.path),
            conflict,
        };
        let job_id = self.enqueue(job, refresh);
        self.actions.attach_job(action, job_id);
    }

    /// Extract Here: inside an archive, the selection goes to the folder
    /// that contains the archive; on an archive file, the whole archive is
    /// extracted next to it.
    pub(super) fn extract_here(&mut self, side: Side) {
        self.extract_to(side, None);
    }

    /// Extract to the other pane's folder.
    pub(super) fn extract_to_other(&mut self, side: Side) {
        let Some(to) = self.target_of(side) else {
            self.set_info(t!("panes.needs_two"));
            return;
        };
        let dst = self.pane(to).tab().loc.clone();
        self.extract_to(side, Some(dst));
    }

    fn extract_to(&mut self, side: Side, dst: Option<Location>) {
        let tab = self.pane(side).tab();
        let sel = tab.selected_entries();
        if let Some(mount) = tab.loc.archive.clone() {
            // Inside an archive.
            let dst = dst.unwrap_or_else(|| mount.outer.clone());
            if dst.read_only() {
                self.set_info(t!("archive.dest_read_only"));
                return;
            }
            let fs = tab.loc.fs.clone();
            let sources: Vec<VfsPath> = if sel.is_empty() {
                tab.entries.iter().map(|e| e.path.clone()).collect()
            } else {
                sel.into_iter().map(|e| e.path).collect()
            };
            if sources.is_empty() {
                self.set_info(t!("archive.nothing"));
                return;
            }
            self.enqueue_extract(side, fs, &mount, sources, dst, ConflictPolicy::Rename);
            return;
        }
        // An archive file in a normal folder.
        let archives: Vec<Entry> = sel
            .into_iter()
            .filter(|e| !e.is_dir() && ArchiveFormat::detect(&e.name).is_some())
            .collect();
        let [entry] = archives.as_slice() else {
            self.set_info(t!("archive.select_one"));
            return;
        };
        let dst = dst.unwrap_or_else(|| tab.loc.clone());
        if dst.read_only() {
            self.set_info(t!("archive.dest_read_only"));
            return;
        }
        let entry = entry.clone();
        self.open_archive(side, entry, ArchivePurpose::ExtractAll { dst });
    }

    /// Enter on an archive: open it as a folder (if that's switched on).
    /// Returns true if it took care of the entry.
    pub(super) fn maybe_browse_archive(&mut self, side: Side, entry: &Entry, force: bool) -> bool {
        if entry.kind == EntryKind::Directory {
            return false;
        }
        let browse = if force {
            ArchiveFormat::detect(&entry.name).is_some()
        } else {
            self.prefs.open_archives && ArchiveFormat::opens_as_folder(&entry.name)
        };
        if browse {
            self.open_archive(side, entry.clone(), ArchivePurpose::Browse);
        }
        browse
    }

    /// Up from an archive's root: back to the folder that contains it, with
    /// the archive selected. Returns false if not at an archive root.
    pub(super) fn leave_archive(&mut self, side: Side) -> bool {
        let tab = self.pane(side).tab();
        let Some(mount) = tab.loc.archive.clone().filter(|_| tab.loc.path.is_root()) else {
            return false;
        };
        self.navigate(side, mount.outer.clone(), false);
        self.pane_mut(side).tab_mut().pending_select = Some(mount.file_name.clone());
        true
    }

    /// Navigate to a location (e.g. a breadcrumb outside the archive) and
    /// select `select` there.
    pub(super) fn navigate_to(&mut self, side: Side, loc: Location, select: Option<String>) {
        self.activate(side);
        self.navigate(side, loc, false);
        if let Some(name) = select {
            self.pane_mut(side).tab_mut().pending_select = Some(name);
        }
    }

    pub(super) fn show_compress(&mut self, side: Side) {
        if self.refuse_read_only(side, ReadOnlyOp::Compress) {
            return;
        }
        let tab = self.pane(side).tab();
        let sources: Vec<VfsPath> = tab.selected_entries().into_iter().map(|e| e.path).collect();
        if sources.is_empty() {
            self.set_info(t!("archive.select_compress"));
            return;
        }
        let folders = self.folder_paths(&tab.loc.fs, &sources);
        let existing: Vec<String> = tab.entries.iter().map(|e| e.name.clone()).collect();
        let size = tab.selection_size();
        let form = CompressForm::new(
            side,
            tab.loc.fs.clone(),
            tab.loc.path.clone(),
            self.conn_label(tab.loc.conn),
            sources,
            folders,
            existing,
            Some(size),
        );
        self.dialog = Some(Dialog::Compress(form));
    }

    pub(super) fn submit_compress(&mut self, form: CompressForm) {
        let name = match form.validated() {
            Ok(n) => n,
            Err(_) => {
                self.dialog = Some(Dialog::Compress(form));
                return;
            }
        };
        let dst_path = form.dir.join(&name);
        let mut a = self.new_action(
            form.side,
            FileActionKind::Compress,
            &form.fs,
            form.sources.clone(),
            Some((&form.fs, form.dir.clone())),
        );
        let local = form.fs.local_path(&dst_path).is_some();
        a.archive = Some(Box::new(ArchiveSpec {
            path: dst_path.clone(),
            format: form.format.label().to_lowercase(),
            local,
            remote: if local { None } else { self.endpoint(&form.fs) },
            level: Some(form.level),
            exclude_junk: form.exclude_junk,
        }));
        let action = self.start_action(a);
        let job = CompressJob {
            src_fs: form.fs.clone(),
            sources: form.sources,
            dst_fs: form.fs.clone(),
            dst_path,
            format: form.format,
            level: form.level,
            exclude_junk: form.exclude_junk,
        };
        let id = self.transfers.enqueue_compress(job);
        self.job_refresh
            .insert(id, vec![(form.fs.clone(), form.dir.clone())]);
        self.show_transfers = true;
        self.actions.attach_job(action, id);
        // Select the new archive once it shows up.
        for tab in self.panes.iter_mut().flat_map(|p| p.tabs.iter_mut()) {
            if same_fs(&tab.loc.fs, &form.fs) && tab.loc.path == form.dir {
                tab.pending_select = Some(name.clone());
            }
        }
    }
}
