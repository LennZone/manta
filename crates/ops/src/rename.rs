//! Batch rename: plan a safe order for many renames inside one folder and
//! carry them out on any [`Vfs`].
//!
//! Renames can depend on each other: `a → b` must wait until `b → c` has
//! moved `b` out of the way, and a swap (`a ↔ b`) or a longer cycle
//! (`a → b → c → a`) can only be done by parking one entry under a
//! temporary name first. On a case-insensitive volume (APFS by default) a
//! case-only rename (`readme → README`) is a cycle of one and also goes
//! through a temporary name. [`plan_renames`] works out that order;
//! [`rename_many`] lists the folder, plans and executes it, never
//! overwriting an entry it didn't move away itself.

use std::collections::{BTreeSet, HashMap, HashSet};

use manta_core::{Vfs, VfsPath};

/// One `rename(from, to)` call inside the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameStep {
    pub from: String,
    pub to: String,
    /// Index of the requested rename this step belongs to.
    pub op: usize,
}

/// The order in which to carry out a batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenamePlan {
    pub steps: Vec<RenameStep>,
    /// Requested renames that change nothing (old == new), by index.
    pub noops: Vec<usize>,
}

/// What [`rename_many`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenameOutcome {
    /// (old name, new name) of every rename that was done, in request order.
    pub ok: Vec<(String, String)>,
    /// (old name, error) of every rename that failed or was refused.
    pub failed: Vec<(String, String)>,
}

/// Is `name` usable as a single path component?
pub fn name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("the name is empty")
    } else if name == "." || name == ".." {
        Some("“.” and “..” can't be used")
    } else if name.contains('/') {
        Some("names can't contain “/”")
    } else if name.contains('\0') {
        Some("names can't contain NUL")
    } else if name.len() > 255 {
        Some("the name is longer than 255 bytes")
    } else {
        None
    }
}

fn fold(name: &str, case_insensitive: bool) -> String {
    if case_insensitive {
        name.to_lowercase()
    } else {
        name.to_string()
    }
}

/// Plan the renames `(old, new)` of entries in a folder that currently
/// holds `existing` (all names, not only the renamed ones).
///
/// Refuses the whole batch (`Err` with a message) if a name is invalid, an
/// old name is missing or listed twice, two renames share a target, or a
/// target is taken by an entry that isn't renamed away. With
/// `case_insensitive`, names that differ only in case are the same name.
pub fn plan_renames(
    existing: &[String],
    renames: &[(String, String)],
    case_insensitive: bool,
) -> Result<RenamePlan, String> {
    let key = |n: &str| fold(n, case_insensitive);
    let present: HashSet<String> = existing.iter().map(|n| key(n)).collect();
    let mut plan = RenamePlan::default();
    let mut ops: Vec<usize> = Vec::new();
    let mut sources: HashSet<String> = HashSet::new();
    let mut targets: HashMap<String, usize> = HashMap::new();
    for (i, (old, new)) in renames.iter().enumerate() {
        if let Some(p) = name_problem(new) {
            return Err(format!("Can't rename “{old}” to “{new}”: {p}"));
        }
        if !existing.iter().any(|e| e == old) {
            return Err(format!("“{old}” doesn't exist (any more)"));
        }
        if !sources.insert(old.clone()) {
            return Err(format!("“{old}” is renamed twice"));
        }
        if old == new {
            plan.noops.push(i);
            continue;
        }
        if let Some(&j) = targets.get(&key(new)) {
            let other = &renames[j].0;
            return Err(format!("“{old}” and “{other}” would both be named “{new}”"));
        }
        targets.insert(key(new), i);
        ops.push(i);
    }
    // Keys that stay taken: present entries that aren't renamed away.
    let moving: HashSet<String> = ops.iter().map(|&i| key(&renames[i].0)).collect();
    for &i in &ops {
        let (old, new) = &renames[i];
        let k = key(new);
        if present.contains(&k) && !moving.contains(&k) {
            return Err(format!(
                "Can't rename “{old}” to “{new}”: an item with that name already exists"
            ));
        }
        // A no-op rename keeps its name too.
        if plan.noops.iter().any(|&n| key(&renames[n].0) == k) {
            return Err(format!(
                "Can't rename “{old}” to “{new}”: an item with that name already exists"
            ));
        }
    }

    // Occupied keys, the op waiting for each key and the ops ready to go.
    let mut occupied: HashSet<String> = present.clone();
    let mut current: HashMap<usize, String> =
        ops.iter().map(|&i| (i, renames[i].0.clone())).collect();
    let waiting: HashMap<String, usize> = ops.iter().map(|&i| (key(&renames[i].1), i)).collect();
    let mut remaining: BTreeSet<usize> = ops.iter().copied().collect();
    let mut ready: Vec<usize> = ops
        .iter()
        .rev()
        .copied()
        .filter(|&i| !occupied.contains(&key(&renames[i].1)))
        .collect();
    let mut temp_n = 0usize;
    let taken_names: HashSet<String> = existing
        .iter()
        .map(|n| key(n))
        .chain(renames.iter().map(|(_, n)| key(n)))
        .collect();
    loop {
        while let Some(i) = ready.pop() {
            if !remaining.remove(&i) {
                continue;
            }
            let from = current[&i].clone();
            let to = renames[i].1.clone();
            plan.steps.push(RenameStep {
                from: from.clone(),
                to: to.clone(),
                op: i,
            });
            occupied.remove(&key(&from));
            occupied.insert(key(&to));
            if let Some(&w) = waiting.get(&key(&from)) {
                if remaining.contains(&w) {
                    ready.push(w);
                }
            }
        }
        // Everything left waits on something: a cycle. Park its first op
        // under a temporary name, which frees the key the next op waits on.
        let Some(&i) = remaining.iter().next() else {
            break;
        };
        let tmp = loop {
            temp_n += 1;
            let t = format!(".manta-rename-{temp_n}");
            if !taken_names.contains(&key(&t)) && !occupied.contains(&key(&t)) {
                break t;
            }
        };
        let from = current[&i].clone();
        plan.steps.push(RenameStep {
            from: from.clone(),
            to: tmp.clone(),
            op: i,
        });
        occupied.remove(&key(&from));
        occupied.insert(key(&tmp));
        current.insert(i, tmp);
        if let Some(&w) = waiting.get(&key(&from)) {
            if remaining.contains(&w) {
                ready.push(w);
            }
        }
    }
    Ok(plan)
}

fn swap_case(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_lowercase() {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                c.to_lowercase().collect::<Vec<_>>()
            }
        })
        .collect()
}

/// Does `dir` treat names that differ only in case as the same name?
/// Blocking: may `stat` one entry under a case-swapped name.
pub fn is_case_insensitive(fs: &dyn Vfs, dir: &VfsPath, names: &[String]) -> bool {
    let set: HashSet<&str> = names.iter().map(String::as_str).collect();
    for n in names {
        let swapped = swap_case(n);
        if swapped == *n {
            continue;
        }
        if set.contains(swapped.as_str()) {
            return false; // both spellings exist side by side
        }
        return fs.stat(&dir.join(&swapped)).is_ok();
    }
    false
}

/// Blocking: rename entries of `dir` (old name, new name) on `fs` in a safe
/// order (see the module docs). Never overwrites: a step whose target
/// exists is skipped and reported as failed. An entry parked under a
/// temporary name whose final rename fails is moved back.
pub fn rename_many(fs: &dyn Vfs, dir: &VfsPath, renames: &[(String, String)]) -> RenameOutcome {
    let fail_all = |e: String| RenameOutcome {
        ok: Vec::new(),
        failed: renames
            .iter()
            .map(|(o, _)| (o.clone(), e.clone()))
            .collect(),
    };
    let existing: Vec<String> = match fs.list(dir) {
        Ok(v) => v.into_iter().map(|e| e.name).collect(),
        Err(e) => return fail_all(format!("Couldn't list the folder: {e}")),
    };
    let ci = is_case_insensitive(fs, dir, &existing);
    let plan = match plan_renames(&existing, renames, ci) {
        Ok(p) => p,
        Err(e) => return fail_all(e),
    };
    execute(fs, dir, renames, &plan, &existing, ci)
}

/// Carry out a plan made by [`plan_renames`].
pub fn execute(
    fs: &dyn Vfs,
    dir: &VfsPath,
    renames: &[(String, String)],
    plan: &RenamePlan,
    existing: &[String],
    case_insensitive: bool,
) -> RenameOutcome {
    let key = |n: &str| fold(n, case_insensitive);
    let mut occupied: HashSet<String> = existing.iter().map(|n| key(n)).collect();
    let mut failed: HashMap<usize, String> = HashMap::new();
    let mut at: HashMap<usize, String> = HashMap::new();
    for step in &plan.steps {
        if failed.contains_key(&step.op) {
            continue;
        }
        // Double-check on disk: backends like `std::fs::rename` overwrite.
        let blocked = occupied.contains(&key(&step.to));
        if blocked || fs.stat(&dir.join(&step.to)).is_ok() {
            failed.insert(step.op, format!("“{}” already exists", step.to));
            continue;
        }
        match fs.rename(&dir.join(&step.from), &dir.join(&step.to)) {
            Ok(()) => {
                occupied.remove(&key(&step.from));
                occupied.insert(key(&step.to));
                at.insert(step.op, step.to.clone());
            }
            Err(e) => {
                failed.insert(step.op, e.to_string());
            }
        }
    }
    // Move entries stuck under a temporary name back.
    for (&op, err) in failed.iter_mut() {
        if let Some(tmp) = at.get(&op) {
            let old = &renames[op].0;
            if tmp != old {
                if let Err(e) = fs.rename(&dir.join(tmp), &dir.join(old)) {
                    err.push_str(&format!(" (left as “{tmp}”: {e})"));
                }
            }
        }
    }
    let mut out = RenameOutcome::default();
    for (i, (old, new)) in renames.iter().enumerate() {
        if plan.noops.contains(&i) {
            continue;
        }
        match failed.get(&i) {
            Some(e) => out.failed.push((old.clone(), e.clone())),
            None if at.get(&i) == Some(new) => out.ok.push((old.clone(), new.clone())),
            None => out.failed.push((old.clone(), "not renamed".into())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn r(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    /// Simulate a plan on a set of names; panics on an overwrite.
    fn simulate(existing: &[&str], plan: &RenamePlan, ci: bool) -> BTreeSet<String> {
        let mut names: Vec<String> = s(existing);
        for st in &plan.steps {
            let pos = names
                .iter()
                .position(|n| *n == st.from)
                .expect("source exists");
            let k = fold(&st.to, ci);
            assert!(
                !names
                    .iter()
                    .enumerate()
                    .any(|(j, n)| j != pos && fold(n, ci) == k),
                "step {st:?} would overwrite"
            );
            names[pos] = st.to.clone();
        }
        names.into_iter().collect()
    }

    #[test]
    fn simple_and_chain() {
        let ex = ["a", "b", "c"];
        let p = plan_renames(&s(&ex), &r(&[("a", "b"), ("b", "c"), ("c", "d")]), false).unwrap();
        assert_eq!(p.steps.len(), 3, "a chain needs no temp names: {p:?}");
        assert_eq!(p.steps[0].from, "c");
        assert_eq!(
            simulate(&ex, &p, false),
            s(&["b", "c", "d"]).into_iter().collect()
        );
    }

    #[test]
    fn swap_uses_one_temp() {
        let ex = ["a", "b", "x"];
        let p = plan_renames(&s(&ex), &r(&[("a", "b"), ("b", "a")]), false).unwrap();
        assert_eq!(p.steps.len(), 3, "{p:?}");
        assert!(p.steps[0].to.starts_with(".manta-rename-"));
        assert_eq!(
            simulate(&ex, &p, false),
            s(&["a", "b", "x"]).into_iter().collect()
        );
    }

    #[test]
    fn three_cycle() {
        let ex = ["a", "b", "c"];
        let p = plan_renames(&s(&ex), &r(&[("a", "b"), ("b", "c"), ("c", "a")]), false).unwrap();
        assert_eq!(p.steps.len(), 4, "{p:?}");
        let mut names = s(&ex);
        for st in &p.steps {
            let i = names.iter().position(|n| *n == st.from).unwrap();
            names[i] = st.to.clone();
        }
        assert_eq!(
            names,
            s(&["b", "c", "a"]),
            "each entry ends up under its new name"
        );
    }

    #[test]
    fn case_only_rename() {
        let ex = ["readme", "other"];
        // Case-insensitive: through a temp name.
        let p = plan_renames(&s(&ex), &r(&[("readme", "README")]), true).unwrap();
        assert_eq!(p.steps.len(), 2, "{p:?}");
        assert_eq!(p.steps[1].to, "README");
        assert_eq!(
            simulate(&ex, &p, true),
            s(&["README", "other"]).into_iter().collect()
        );
        // Case-sensitive: directly.
        let p = plan_renames(&s(&ex), &r(&[("readme", "README")]), false).unwrap();
        assert_eq!(p.steps.len(), 1);
    }

    #[test]
    fn refusals() {
        let ex = s(&["a", "b", "B.txt", "c"]);
        // Target taken by an untouched entry.
        assert!(plan_renames(&ex, &r(&[("a", "c")]), false)
            .unwrap_err()
            .contains("already exists"));
        // Case-insensitive collision with an untouched entry.
        assert!(plan_renames(&ex, &r(&[("a", "b.txt")]), true).is_err());
        assert!(plan_renames(&ex, &r(&[("a", "b.txt")]), false).is_ok());
        // Duplicate targets.
        assert!(plan_renames(&ex, &r(&[("a", "x"), ("b", "x")]), false)
            .unwrap_err()
            .contains("both"));
        assert!(plan_renames(&ex, &r(&[("a", "x"), ("b", "X")]), true).is_err());
        // Missing source, bad names, renamed twice.
        assert!(plan_renames(&ex, &r(&[("zz", "y")]), false).is_err());
        assert!(plan_renames(&ex, &r(&[("a", "x/y")]), false).is_err());
        assert!(plan_renames(&ex, &r(&[("a", "")]), false).is_err());
        assert!(plan_renames(&ex, &r(&[("a", "x"), ("a", "y")]), false).is_err());
        // Renaming onto an entry that stays (no-op pair) is refused.
        assert!(plan_renames(&ex, &r(&[("a", "b"), ("b", "b")]), false).is_err());
        // No-ops are reported, not planned.
        let p = plan_renames(&ex, &r(&[("a", "a")]), false).unwrap();
        assert!(p.steps.is_empty() && p.noops == vec![0]);
    }

    #[test]
    fn temp_names_avoid_existing_entries() {
        let ex = ["a", "b", ".manta-rename-1"];
        let p = plan_renames(&s(&ex), &r(&[("a", "b"), ("b", "a")]), false).unwrap();
        assert_eq!(p.steps[0].to, ".manta-rename-2");
    }

    #[test]
    fn many_swaps_and_chains_are_safe() {
        // Rotate 50 names by one plus a few independent ones.
        let names: Vec<String> = (0..50).map(|i| format!("f{i:02}")).collect();
        let mut ren: Vec<(String, String)> = (0..50)
            .map(|i| (names[i].clone(), names[(i + 1) % 50].clone()))
            .collect();
        let mut ex = names.clone();
        ex.push("solo".into());
        ren.push(("solo".into(), "SOLO".into()));
        let p = plan_renames(&ex, &ren, true).unwrap();
        let exr: Vec<&str> = ex.iter().map(String::as_str).collect();
        let out = simulate(&exr, &p, true);
        let mut want: BTreeSet<String> = names.into_iter().collect();
        want.insert("SOLO".into());
        assert_eq!(out, want);
        let temps = p
            .steps
            .iter()
            .filter(|s| s.to.starts_with(".manta"))
            .count();
        assert_eq!(
            temps, 2,
            "one temp for the cycle, one for the case-only rename"
        );
    }
}
