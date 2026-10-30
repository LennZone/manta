//! Undo and Redo (⌘Z, ⇧⌘Z) of Manta's own file operations, like Finder:
//! moving to the Trash, renaming, new folders and files. Copies and moves
//! aren't recorded.
//!
//! Each history entry is the [`Op`] that reverts one step. Running it gives
//! the op that reverts *that* ([`Outcome::inverse`]), which goes on the
//! other stack, so Undo and Redo are the same mechanism. The history lives
//! as long as the window; a new operation clears Redo.

use std::path::{Path, PathBuf};

use manta_core::{SharedVfs, VfsPath};

use crate::trash::Trasher;

/// Steps kept per stack; the oldest go first.
const LIMIT: usize = 100;

#[derive(Clone)]
pub enum Op {
    /// Move items out of the Trash: `(where it is in the Trash, where it was)`.
    PutBack(Vec<(PathBuf, PathBuf)>),
    /// Move local items to the Trash.
    Trash(Vec<PathBuf>),
    Rename {
        fs: SharedVfs,
        from: VfsPath,
        to: VfsPath,
    },
    /// Make an empty folder (`dir`) or file again.
    Create {
        fs: SharedVfs,
        path: VfsPath,
        dir: bool,
    },
    /// Delete what [`Op::Create`] made, only while it is still empty
    /// (servers have no Trash).
    Remove {
        fs: SharedVfs,
        path: VfsPath,
        dir: bool,
    },
}

/// What running an [`Op`] did.
#[derive(Default)]
pub struct Outcome {
    /// Reverts what ran: goes on the other stack.
    pub inverse: Option<Op>,
    /// What didn't run: stays on this stack, to try again.
    pub rest: Option<Op>,
    /// Status line text when something ran.
    pub done: Option<String>,
    /// `(title, detail)` of the first failure.
    pub error: Option<(String, String)>,
}

impl Op {
    /// Reverts making `path` (a new folder or file): local items go to the
    /// Trash, as in Finder; elsewhere they are deleted while still empty.
    pub fn remove_new(fs: SharedVfs, path: VfsPath, dir: bool) -> Op {
        match fs.local_path(&path).filter(|_| crate::app::can_trash(&fs)) {
            Some(local) => Op::Trash(vec![local]),
            None => Op::Remove { fs, path, dir },
        }
    }

    /// Folders whose listings change; local ops are on `local`.
    pub fn refresh(&self, local: &SharedVfs) -> Vec<(SharedVfs, VfsPath)> {
        let mut out: Vec<(SharedVfs, VfsPath)> = Vec::new();
        let mut add = |fs: &SharedVfs, p: &VfsPath| {
            let dir = p.parent().unwrap_or_else(VfsPath::root);
            if !out
                .iter()
                .any(|(f, d)| manta_ops::same_fs(f, fs) && *d == dir)
            {
                out.push((fs.clone(), dir));
            }
        };
        match self {
            Op::PutBack(items) => {
                for (_, to) in items {
                    add(local, &local_vfs_path(to));
                }
            }
            Op::Trash(paths) => {
                for p in paths {
                    add(local, &local_vfs_path(p));
                }
            }
            Op::Rename { fs, from, to } => {
                add(fs, from);
                add(fs, to);
            }
            Op::Create { fs, path, .. } | Op::Remove { fs, path, .. } => add(fs, path),
        }
        out
    }

    /// Run the op (on a worker thread: it touches the disk or a server).
    pub fn run(self, trasher: &dyn Trasher) -> Outcome {
        match self {
            Op::PutBack(items) => put_back(trasher, items),
            Op::Trash(paths) => trash(trasher, &paths),
            Op::Rename { fs, from, to } => {
                let old = from.file_name().to_string();
                let name = to.file_name().to_string();
                // Never replace another item. A change of case only
                // ("readme" → "README") finds the item itself on
                // case-insensitive disks.
                let case_only =
                    from != to && from.to_string().to_lowercase() == to.to_string().to_lowercase();
                let result = if !case_only && fs.stat(&to).is_ok() {
                    Err(t!("error.undo_exists", name = name))
                } else {
                    fs.rename(&from, &to).map_err(|e| e.to_string())
                };
                match result {
                    Ok(()) => Outcome {
                        done: Some(t!("status.renamed", old = old, name = name)),
                        inverse: Some(Op::Rename {
                            fs,
                            from: to,
                            to: from,
                        }),
                        ..Default::default()
                    },
                    Err(e) => Outcome {
                        error: Some((t!("error.rename", name = old), e)),
                        rest: Some(Op::Rename { fs, from, to }),
                        ..Default::default()
                    },
                }
            }
            Op::Create { fs, path, dir } => {
                let name = path.file_name().to_string();
                let result = if dir {
                    fs.create_dir(&path)
                } else {
                    crate::app::create_empty_file(&*fs, &path)
                };
                match result {
                    Ok(()) => Outcome {
                        done: Some(if dir {
                            t!("status.created_folder", name = name)
                        } else {
                            t!("status.created_file", name = name)
                        }),
                        inverse: Some(Op::remove_new(fs, path, dir)),
                        ..Default::default()
                    },
                    Err(e) => Outcome {
                        error: Some((
                            if dir {
                                t!("error.create_folder", name = name)
                            } else {
                                t!("error.create_file", name = name)
                            },
                            e.to_string(),
                        )),
                        rest: Some(Op::Create { fs, path, dir }),
                        ..Default::default()
                    },
                }
            }
            Op::Remove { fs, path, dir } => {
                let name = path.file_name().to_string();
                let empty = if dir {
                    fs.list(&path).map(|l| l.is_empty())
                } else {
                    fs.stat(&path).map(|e| e.size == 0)
                };
                let result = match empty {
                    Ok(true) if dir => fs.remove_dir(&path).map_err(|e| e.to_string()),
                    Ok(true) => fs.remove_file(&path).map_err(|e| e.to_string()),
                    Ok(false) => Err(t!("error.undo_not_empty").to_string()),
                    Err(e) => Err(e.to_string()),
                };
                match result {
                    Ok(()) => Outcome {
                        done: Some(t!("status.removed", name = name)),
                        inverse: Some(Op::Create { fs, path, dir }),
                        ..Default::default()
                    },
                    Err(e) => Outcome {
                        error: Some((t!("error.remove", name = name), e)),
                        rest: Some(Op::Remove { fs, path, dir }),
                        ..Default::default()
                    },
                }
            }
        }
    }
}

fn put_back(trasher: &dyn Trasher, items: Vec<(PathBuf, PathBuf)>) -> Outcome {
    let mut back = Vec::new();
    let mut left = Vec::new();
    let mut error = None;
    for (from, to) in items {
        let result = if to.symlink_metadata().is_ok() {
            Err(t!("error.undo_exists", name = name_of(&to)))
        } else {
            trasher.put_back(&from, &to)
        };
        match result {
            Ok(()) => back.push(to),
            Err(e) => {
                error.get_or_insert((name_of(&to), e));
                left.push((from, to));
            }
        }
    }
    let n = back.len();
    Outcome {
        done: match back.first() {
            None => None,
            Some(p) if n == 1 => Some(t!("status.put_back.single", name = name_of(p))),
            Some(_) => Some(t!("status.put_back", count = n)),
        },
        error: error.map(|(name, e)| {
            let title = if left.len() == 1 {
                t!("error.put_back.single", name = name)
            } else {
                t!("error.put_back", count = left.len())
            };
            (title, e)
        }),
        inverse: (!back.is_empty()).then_some(Op::Trash(back)),
        rest: (!left.is_empty()).then_some(Op::PutBack(left)),
    }
}

fn trash(trasher: &dyn Trasher, paths: &[PathBuf]) -> Outcome {
    let out = crate::trash::trash_all(trasher, paths);
    let n = out.moved.len();
    let failed = out.failed.len();
    Outcome {
        done: match out.moved.first() {
            None => None,
            Some(p) if n == 1 => Some(t!("status.trashed.single", name = name_of(p))),
            Some(_) => Some(t!("status.trashed", count = n)),
        },
        error: out.failed.first().map(|(p, e)| {
            let title = if failed == 1 {
                t!("error.trash.single", name = name_of(p))
            } else {
                t!("error.trash", count = failed)
            };
            (title, e.clone())
        }),
        inverse: (!out.put_back.is_empty()).then_some(Op::PutBack(out.put_back)),
        rest: (failed > 0).then(|| Op::Trash(out.failed.into_iter().map(|(p, _)| p).collect())),
    }
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn local_vfs_path(p: &Path) -> VfsPath {
    VfsPath::new(&p.to_string_lossy())
}

/// The Undo and Redo stacks.
#[derive(Default)]
pub struct History {
    undo: Vec<Op>,
    redo: Vec<Op>,
    /// An Undo or Redo is running: the next one waits for it (steps
    /// depend on each other).
    pub busy: bool,
}

impl History {
    /// The user did something: `op` reverts it.
    pub fn record(&mut self, op: Op) {
        push(&mut self.undo, op);
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.busy && !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.busy && !self.redo.is_empty()
    }

    /// The next step to run (`redo`: from Redo), marking the history busy.
    pub fn take(&mut self, redo: bool) -> Option<Op> {
        if self.busy {
            return None;
        }
        let op = if redo { &mut self.redo } else { &mut self.undo }.pop();
        self.busy = op.is_some();
        op
    }

    /// The step taken with [`History::take`] finished.
    pub fn finish(&mut self, redo: bool, outcome: &mut Outcome) {
        self.busy = false;
        let (this, other) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some(rest) = outcome.rest.take() {
            push(this, rest);
        }
        if let Some(inverse) = outcome.inverse.take() {
            push(other, inverse);
        }
    }
}

fn push(stack: &mut Vec<Op>, op: Op) {
    stack.push(op);
    if stack.len() > LIMIT {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(name: &str) -> Op {
        Op::Trash(vec![PathBuf::from(name)])
    }

    fn name(op: &Op) -> String {
        match op {
            Op::Trash(p) => p[0].to_string_lossy().into_owned(),
            _ => unreachable!(),
        }
    }

    /// Undo hands the inverse to Redo and back; a new operation clears Redo.
    #[test]
    fn steps_move_between_the_stacks() {
        let mut h = History::default();
        h.record(op("a"));
        h.record(op("b"));
        let step = h.take(false).unwrap();
        assert_eq!(name(&step), "b", "last in, first undone");
        assert!(h.busy && !h.can_undo() && h.take(false).is_none());
        h.finish(
            false,
            &mut Outcome {
                inverse: Some(op("b again")),
                ..Default::default()
            },
        );
        assert!(h.can_undo() && h.can_redo());
        assert_eq!(name(&h.take(true).unwrap()), "b again");
        h.finish(true, &mut Outcome::default());
        h.record(op("c"));
        assert!(!h.can_redo(), "a new operation clears Redo");
    }

    /// What failed stays on its stack, to try again.
    #[test]
    fn the_rest_stays() {
        let mut h = History::default();
        h.record(op("a"));
        let _ = h.take(false);
        h.finish(
            false,
            &mut Outcome {
                rest: Some(op("a")),
                ..Default::default()
            },
        );
        assert!(h.can_undo() && !h.can_redo());
    }

    #[test]
    fn keeps_the_last_steps() {
        let mut h = History::default();
        for i in 0..LIMIT + 5 {
            h.record(op(&i.to_string()));
        }
        assert_eq!(h.undo.len(), LIMIT);
        assert_eq!(name(&h.undo[0]), "5", "the oldest went first");
    }
}
