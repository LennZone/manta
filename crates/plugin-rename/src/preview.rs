//! The live preview: every item's new name, a character-level diff for the
//! highlights, and the conflicts that block renaming.

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDateTime, TimeZone};

use crate::i18n::tr;
use crate::rules::{compile, ItemCtx, Rule};

/// An entry to rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub is_dir: bool,
    /// Seconds since the Unix epoch.
    pub modified: Option<i64>,
}

/// Why a new name can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    Empty,
    /// Contains `/`, `:` or a control character.
    InvalidChar(char),
    /// Longer than 255 bytes.
    TooLong,
    /// Several items would get this name.
    Duplicate,
    /// An item that keeps its name (or isn't renamed) already has it.
    Exists,
}

impl Issue {
    pub fn label(&self) -> String {
        match self {
            Issue::Empty => tr!("issue.empty").into(),
            Issue::InvalidChar(c @ ('/' | ':')) => tr!("issue.contains", char = c),
            Issue::InvalidChar(_) => tr!("issue.invalid_char").into(),
            Issue::TooLong => tr!("issue.too_long").into(),
            Issue::Duplicate => tr!("issue.duplicate").into(),
            Issue::Exists => tr!("issue.exists").into(),
        }
    }
}

/// One part of a name in the diff: `changed` = removed (old name) or
/// inserted (new name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub old: String,
    pub new: String,
    pub is_dir: bool,
    pub changed: bool,
    pub issue: Option<Issue>,
    pub old_spans: Vec<Span>,
    pub new_spans: Vec<Span>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preview {
    pub rows: Vec<Row>,
    /// Rows whose name changes and that have no issue.
    pub changed: usize,
    /// Rows with an issue.
    pub conflicts: usize,
    /// (rule index, message) for rules that can't run.
    pub rule_errors: Vec<(usize, String)>,
}

impl Preview {
    /// Rename is possible: something changes, nothing conflicts, every
    /// rule is valid.
    pub fn can_apply(&self) -> bool {
        self.changed > 0 && self.conflicts == 0 && self.rule_errors.is_empty()
    }

    /// The (old, new) pairs to send.
    pub fn renames(&self) -> Vec<(String, String)> {
        self.rows
            .iter()
            .filter(|r| r.changed && r.issue.is_none())
            .map(|r| (r.old.clone(), r.new.clone()))
            .collect()
    }
}

pub fn name_issue(name: &str) -> Option<Issue> {
    if name.is_empty() || name == "." || name == ".." {
        return Some(Issue::Empty);
    }
    if let Some(c) = name
        .chars()
        .find(|c| *c == '/' || *c == ':' || c.is_control())
    {
        return Some(Issue::InvalidChar(c));
    }
    if name.len() > 255 {
        return Some(Issue::TooLong);
    }
    None
}

fn local_time(secs: i64) -> Option<NaiveDateTime> {
    chrono::Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|t| t.naive_local())
}

/// Compute the preview of `items` (in pane order). `all_names` holds every
/// entry of the folder (for "already exists"). With `case_insensitive`,
/// names differing only in case collide.
pub fn compute(
    items: &[Item],
    all_names: &[String],
    rules: &[Rule],
    today: NaiveDateTime,
    case_insensitive: bool,
) -> Preview {
    let key = |s: &str| {
        if case_insensitive {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let compiled = compile(rules);
    let new_names: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(index, it)| {
            let ctx = ItemCtx {
                index,
                is_dir: it.is_dir,
                modified: it.modified.and_then(local_time),
                today,
            };
            compiled.apply(&it.name, &ctx)
        })
        .collect();

    // Names that stay where they are: entries that aren't in the list, and
    // list items whose name doesn't change.
    let item_names: HashSet<&str> = items.iter().map(|i| i.name.as_str()).collect();
    let mut staying: HashSet<String> = all_names
        .iter()
        .filter(|n| !item_names.contains(n.as_str()))
        .map(|n| key(n))
        .collect();
    for (it, new) in items.iter().zip(&new_names) {
        if it.name == *new {
            staying.insert(key(new));
        }
    }
    let mut targets: HashMap<String, usize> = HashMap::new();
    for (it, new) in items.iter().zip(&new_names) {
        if it.name != *new {
            *targets.entry(key(new)).or_default() += 1;
        }
    }

    let mut p = Preview {
        rule_errors: compiled.errors.clone(),
        ..Default::default()
    };
    for (it, new) in items.iter().zip(new_names) {
        let changed = it.name != new;
        let issue = if !changed {
            None
        } else if let Some(i) = name_issue(&new) {
            Some(i)
        } else if targets.get(&key(&new)).copied().unwrap_or(0) > 1 {
            Some(Issue::Duplicate)
        } else if staying.contains(&key(&new)) {
            Some(Issue::Exists)
        } else {
            None
        };
        let (old_spans, new_spans) = if changed {
            diff(&it.name, &new)
        } else {
            (plain(&it.name), plain(&new))
        };
        match &issue {
            Some(_) => p.conflicts += 1,
            None if changed => p.changed += 1,
            None => {}
        }
        p.rows.push(Row {
            old: it.name.clone(),
            new,
            is_dir: it.is_dir,
            changed,
            issue,
            old_spans,
            new_spans,
        });
    }
    p
}

fn plain(s: &str) -> Vec<Span> {
    vec![Span {
        text: s.to_string(),
        changed: false,
    }]
}

fn push(v: &mut Vec<Span>, c: char, changed: bool) {
    match v.last_mut() {
        Some(s) if s.changed == changed => s.text.push(c),
        _ => v.push(Span {
            text: c.to_string(),
            changed,
        }),
    }
}

/// Character-level diff (longest common subsequence): the old name with
/// removed characters marked, the new name with inserted ones marked.
pub fn diff(old: &str, new: &str) -> (Vec<Span>, Vec<Span>) {
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new.chars().collect();
    // Common prefix / suffix first: cheap and keeps the table small.
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let (n, m) = (am.len(), bm.len());
    let mut keep_a = vec![false; n];
    let mut keep_b = vec![false; m];
    if n * m <= 250_000 {
        let mut t = vec![0u16; (n + 1) * (m + 1)];
        let w = m + 1;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                t[i * w + j] = if am[i] == bm[j] {
                    t[(i + 1) * w + j + 1] + 1
                } else {
                    t[(i + 1) * w + j].max(t[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if am[i] == bm[j] {
                keep_a[i] = true;
                keep_b[j] = true;
                i += 1;
                j += 1;
            } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    let mut olds = Vec::new();
    let mut news = Vec::new();
    for &c in &a[..pre] {
        push(&mut olds, c, false);
    }
    for (i, &c) in am.iter().enumerate() {
        push(&mut olds, c, !keep_a[i]);
    }
    for &c in &a[a.len() - suf..] {
        push(&mut olds, c, false);
    }
    for &c in &b[..pre] {
        push(&mut news, c, false);
    }
    for (j, &c) in bm.iter().enumerate() {
        push(&mut news, c, !keep_b[j]);
    }
    for &c in &b[b.len() - suf..] {
        push(&mut news, c, false);
    }
    (olds, news)
}
