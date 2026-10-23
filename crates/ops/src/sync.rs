//! Turning a comparison into transfer jobs.
//!
//! Rules (files and `OnlyX` directories; dirs present on both sides never
//! produce a job themselves, their children do):
//! - `LeftToRight`: copy `OnlyLeft`, `LeftNewer`, `Different` to the right.
//!   `RightNewer` is left alone (a newer target is never clobbered).
//!   With `delete_extra`, `OnlyRight` items are deleted on the right.
//! - `RightToLeft`: mirror image.
//! - `Bidirectional`: `OnlyLeft`/`LeftNewer` go right, `OnlyRight`/`RightNewer`
//!   go left; `Different` (same mtime, different size) is ambiguous and
//!   skipped; `delete_extra` is ignored.
//! - Dir/file type mismatches (`is_dir` + `Different`) are skipped.
//! - Items below an `OnlyX` directory that is copied/deleted as a whole are
//!   ignored, so nothing is transferred twice.
//!
//! Grouping: one `Copy` job (conflict `Overwrite`) per direction and
//! destination directory, plus at most one `Delete` job.

use std::collections::BTreeMap;

use manta_core::{SharedVfs, VfsPath};

use crate::{CompareItem, CompareStatus, ConflictPolicy, Job, JobKind, SyncDirection};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Dir {
    LeftToRight,
    RightToLeft,
}

enum Action {
    Copy(Dir),
    DeleteRight,
    DeleteLeft,
}

fn action_for(item: &CompareItem, direction: SyncDirection, delete_extra: bool) -> Option<Action> {
    use CompareStatus::*;
    if item.is_dir && !matches!(item.status, OnlyLeft | OnlyRight) {
        return None;
    }
    match (direction, item.status) {
        (_, Identical) => None,
        (SyncDirection::LeftToRight, OnlyLeft | LeftNewer | Different) => {
            Some(Action::Copy(Dir::LeftToRight))
        }
        (SyncDirection::LeftToRight, OnlyRight) if delete_extra => Some(Action::DeleteRight),
        (SyncDirection::RightToLeft, OnlyRight | RightNewer | Different) => {
            Some(Action::Copy(Dir::RightToLeft))
        }
        (SyncDirection::RightToLeft, OnlyLeft) if delete_extra => Some(Action::DeleteLeft),
        (SyncDirection::Bidirectional, OnlyLeft | LeftNewer) => {
            Some(Action::Copy(Dir::LeftToRight))
        }
        (SyncDirection::Bidirectional, OnlyRight | RightNewer) => {
            Some(Action::Copy(Dir::RightToLeft))
        }
        _ => None,
    }
}

fn parent_rel(rel: &str) -> &str {
    rel.rfind('/').map_or("", |i| &rel[..i])
}

pub(crate) fn plan_sync(
    items: &[CompareItem],
    left_fs: &SharedVfs,
    left: &VfsPath,
    right_fs: &SharedVfs,
    right: &VfsPath,
    direction: SyncDirection,
    delete_extra: bool,
) -> Vec<Job> {
    let mut sorted: Vec<&CompareItem> = items.iter().collect();
    sorted.sort_by(|a, b| a.rel_path.split('/').cmp(b.rel_path.split('/')));

    // Directories handled as a whole; descendants are skipped.
    let mut whole_dirs: Vec<&str> = Vec::new();
    let mut copies: BTreeMap<(Dir, String), Vec<VfsPath>> = BTreeMap::new();
    let mut del_left = Vec::new();
    let mut del_right = Vec::new();

    for item in sorted {
        let rel = item.rel_path.trim_matches('/');
        if rel.is_empty() {
            continue;
        }
        let covered = whole_dirs
            .iter()
            .any(|d| rel.len() > d.len() && rel.starts_with(d) && rel.as_bytes()[d.len()] == b'/');
        if covered {
            continue;
        }
        let Some(action) = action_for(item, direction, delete_extra) else {
            continue;
        };
        if item.is_dir {
            whole_dirs.push(rel);
        }
        match action {
            Action::Copy(d) => {
                let src_root = if d == Dir::LeftToRight { left } else { right };
                copies
                    .entry((d, parent_rel(rel).to_string()))
                    .or_default()
                    .push(src_root.join(rel));
            }
            Action::DeleteRight => del_right.push(right.join(rel)),
            Action::DeleteLeft => del_left.push(left.join(rel)),
        }
    }

    let mut jobs = Vec::new();
    for ((d, parent), sources) in copies {
        let (src_fs, dst_fs, dst_root) = match d {
            Dir::LeftToRight => (left_fs, right_fs, right),
            Dir::RightToLeft => (right_fs, left_fs, left),
        };
        jobs.push(Job {
            kind: JobKind::Copy,
            src_fs: src_fs.clone(),
            sources,
            dst_fs: Some(dst_fs.clone()),
            dst_dir: Some(if parent.is_empty() {
                dst_root.clone()
            } else {
                dst_root.join(&parent)
            }),
            conflict: ConflictPolicy::Overwrite,
        });
    }
    for (fs, sources) in [(right_fs, del_right), (left_fs, del_left)] {
        if !sources.is_empty() {
            jobs.push(Job {
                kind: JobKind::Delete,
                src_fs: fs.clone(),
                sources,
                dst_fs: None,
                dst_dir: None,
                conflict: ConflictPolicy::Overwrite,
            });
        }
    }
    jobs
}
