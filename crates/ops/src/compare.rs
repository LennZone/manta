//! Directory comparison.
//!
//! Rules:
//! - Files (and symlinks, compared like files) present on both sides: if both
//!   mtimes are known and differ by more than the tolerance -> `LeftNewer` /
//!   `RightNewer`; otherwise equal size -> `Identical`, else `Different`.
//! - Directories present on both sides are recursed into (if `recursive`) and
//!   reported with `Identical` when everything below is identical, otherwise
//!   `Different`. Without `recursive` they are reported `Identical`.
//! - A directory present on only one side is reported once (`OnlyLeft` /
//!   `OnlyRight`); its contents are *not* listed.
//! - A name that is a directory on one side and a file on the other is
//!   reported as `is_dir: true, status: Different` with the file side's size.
//! - Any listing error aborts the comparison (a partial result could make a
//!   sync delete or overwrite the wrong things).

use std::collections::BTreeMap;

use manta_core::{Entry, SharedVfs, VfsPath, VfsResult};

use crate::{CompareItem, CompareOptions, CompareStatus};

pub(crate) fn compare_dirs(
    left_fs: &SharedVfs,
    left: &VfsPath,
    right_fs: &SharedVfs,
    right: &VfsPath,
    opts: &CompareOptions,
) -> VfsResult<Vec<CompareItem>> {
    let mut out = Vec::new();
    compare_rec(left_fs, left, right_fs, right, "", opts, &mut out)?;
    out.sort_by(|a, b| a.rel_path.split('/').cmp(b.rel_path.split('/')));
    Ok(out)
}

fn visible_entries(
    fs: &SharedVfs,
    dir: &VfsPath,
    opts: &CompareOptions,
) -> VfsResult<BTreeMap<String, Entry>> {
    Ok(fs
        .list(dir)?
        .into_iter()
        .filter(|e| e.name != "." && e.name != ".." && !e.name.is_empty())
        .filter(|e| opts.include_hidden || !(e.hidden || e.name.starts_with('.')))
        .map(|e| (e.name.clone(), e))
        .collect())
}

/// Compare two files' metadata.
pub fn compare_files(left: &Entry, right: &Entry, mtime_tolerance_secs: i64) -> CompareStatus {
    if let (Some(l), Some(r)) = (left.modified, right.modified) {
        let diff_ms = (l - r).num_milliseconds();
        let tol_ms = mtime_tolerance_secs.max(0) * 1000;
        if diff_ms > tol_ms {
            return CompareStatus::LeftNewer;
        }
        if diff_ms < -tol_ms {
            return CompareStatus::RightNewer;
        }
    }
    if left.size == right.size {
        CompareStatus::Identical
    } else {
        CompareStatus::Different
    }
}

/// Returns true if everything at and below this level is identical.
fn compare_rec(
    left_fs: &SharedVfs,
    left: &VfsPath,
    right_fs: &SharedVfs,
    right: &VfsPath,
    prefix: &str,
    opts: &CompareOptions,
    out: &mut Vec<CompareItem>,
) -> VfsResult<bool> {
    let mut l = visible_entries(left_fs, left, opts)?;
    let mut r = visible_entries(right_fs, right, opts)?;
    let names: std::collections::BTreeSet<String> = l.keys().chain(r.keys()).cloned().collect();
    let mut all_identical = true;

    for name in names {
        let rel_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let item = match (l.remove(&name), r.remove(&name)) {
            (Some(le), None) => CompareItem {
                rel_path,
                is_dir: le.is_dir(),
                status: CompareStatus::OnlyLeft,
                left_size: (!le.is_dir()).then_some(le.size),
                right_size: None,
            },
            (None, Some(re)) => CompareItem {
                rel_path,
                is_dir: re.is_dir(),
                status: CompareStatus::OnlyRight,
                left_size: None,
                right_size: (!re.is_dir()).then_some(re.size),
            },
            (Some(le), Some(re)) if le.is_dir() && re.is_dir() => {
                let idx = out.len();
                out.push(CompareItem {
                    rel_path: rel_path.clone(),
                    is_dir: true,
                    status: CompareStatus::Identical,
                    left_size: None,
                    right_size: None,
                });
                if opts.recursive {
                    let same = compare_rec(
                        left_fs,
                        &left.join(&name),
                        right_fs,
                        &right.join(&name),
                        &rel_path,
                        opts,
                        out,
                    )?;
                    if !same {
                        out[idx].status = CompareStatus::Different;
                        all_identical = false;
                    }
                }
                continue;
            }
            (Some(le), Some(re)) if le.is_dir() || re.is_dir() => CompareItem {
                rel_path,
                is_dir: true,
                status: CompareStatus::Different,
                left_size: (!le.is_dir()).then_some(le.size),
                right_size: (!re.is_dir()).then_some(re.size),
            },
            (Some(le), Some(re)) => CompareItem {
                rel_path,
                is_dir: false,
                status: compare_files(&le, &re, opts.mtime_tolerance_secs),
                left_size: Some(le.size),
                right_size: Some(re.size),
            },
            (None, None) => continue,
        };
        if item.status != CompareStatus::Identical {
            all_identical = false;
        }
        out.push(item);
    }
    Ok(all_identical)
}
