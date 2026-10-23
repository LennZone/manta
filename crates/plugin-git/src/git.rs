//! Thin wrappers around the `git` CLI. Everything here blocks; call it only
//! from worker threads.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::i18n::tr;
use crate::model::InProgress;
use crate::status::{parse_porcelain_v2, GitStatus};

/// Where a repository lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoLocation {
    /// Work-tree root (canonical, as reported by git).
    pub root: PathBuf,
    /// Absolute git dir (usually `<root>/.git`).
    pub git_dir: PathBuf,
}

/// A `git` command for `dir` with a predictable, non-interactive environment.
pub fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null());
    c
}

/// `git` command for read-only queries: never takes optional locks, so it
/// does not rewrite `.git/index` (which would wake our own file watcher).
pub fn git_ro(dir: &Path) -> Command {
    let mut c = git(dir);
    c.env("GIT_OPTIONAL_LOCKS", "0");
    c
}

pub fn available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn stderr_text(out: &std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    let err = if err.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        err.to_string()
    };
    // Keep toasts short: first few meaningful lines.
    let lines: Vec<&str> = err
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(4)
        .collect();
    let text = lines.join("\n");
    let text = text
        .strip_prefix("fatal: ")
        .or_else(|| text.strip_prefix("error: "))
        .unwrap_or(&text);
    if text.is_empty() {
        tr!("git.error.failed").to_string()
    } else {
        text.to_string()
    }
}

fn run(mut cmd: Command) -> Result<Vec<u8>, String> {
    let out = cmd
        .output()
        .map_err(|e| tr!("git.error.cannot_run", error = e))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(stderr_text(&out))
    }
}

/// Run and read at most `max_bytes` of stdout (the child is killed if it
/// produces more). Returns `(stdout, truncated)`.
fn run_capped(mut cmd: Command, max_bytes: usize) -> Result<(Vec<u8>, bool), String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tr!("git.error.cannot_run", error = e))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut buf = Vec::new();
    let read = (&mut stdout)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut buf);
    let truncated = buf.len() > max_bytes;
    if truncated {
        buf.truncate(max_bytes);
        let _ = child.kill();
    }
    drop(stdout);
    let mut err = Vec::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_end(&mut err);
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    read.map_err(|e| e.to_string())?;
    if !truncated && !status.success() {
        let msg = String::from_utf8_lossy(&err).trim().to_string();
        return Err(if msg.is_empty() {
            tr!("git.error.failed").into()
        } else {
            msg
        });
    }
    Ok((buf, truncated))
}

/// Find the repository containing `dir`. `Ok(None)` if `dir` is not inside a
/// work tree (including bare repos and the inside of `.git`).
pub fn detect(dir: &Path) -> Result<Option<RepoLocation>, String> {
    let out = git_ro(dir)
        .args(["rev-parse", "--show-toplevel", "--absolute-git-dir"])
        .output()
        .map_err(|e| tr!("git.error.cannot_run", error = e))?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    match (lines.next(), lines.next()) {
        (Some(root), Some(git_dir)) if !root.is_empty() => Ok(Some(RepoLocation {
            root: PathBuf::from(root),
            git_dir: PathBuf::from(git_dir),
        })),
        _ => Ok(None),
    }
}

pub fn status(root: &Path) -> Result<GitStatus, String> {
    let raw = run({
        let mut c = git_ro(root);
        c.args([
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ]);
        c
    })?;
    parse_porcelain_v2(&raw).map_err(|e| e.to_string())
}

/// Which of `names` (entries of `dir`) are ignored. Uses `check-ignore
/// --stdin` so only the shown folder is checked, never the whole tree.
pub fn check_ignored(dir: &Path, names: &[String]) -> Result<HashSet<String>, String> {
    if names.is_empty() {
        return Ok(HashSet::new());
    }
    let mut child = git_ro(dir)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tr!("git.error.cannot_run", error = e))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let mut input = Vec::new();
    for n in names {
        input.extend_from_slice(n.as_bytes());
        input.push(0);
    }
    // Write from a scoped thread so a large input cannot deadlock against a
    // full stdout pipe.
    let out = std::thread::scope(|s| {
        s.spawn(move || {
            let _ = stdin.write_all(&input);
        });
        child.wait_with_output()
    })
    .map_err(|e| e.to_string())?;
    // Exit code 1 = nothing ignored; 128 = fatal.
    match out.status.code() {
        Some(0) | Some(1) => {}
        _ => return Err(stderr_text(&out)),
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .collect())
}

fn read_u32(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Detect a merge/rebase/cherry-pick in progress from the git dir.
pub fn in_progress(git_dir: &Path) -> Option<InProgress> {
    let has = |name: &str| git_dir.join(name).exists();
    if has("rebase-merge") {
        let d = git_dir.join("rebase-merge");
        let step = read_u32(&d.join("msgnum")).zip(read_u32(&d.join("end")));
        return Some(InProgress::Rebase { step });
    }
    if has("rebase-apply") {
        let d = git_dir.join("rebase-apply");
        if d.join("applying").exists() {
            return Some(InProgress::ApplyMailbox);
        }
        let step = read_u32(&d.join("next")).zip(read_u32(&d.join("last")));
        return Some(InProgress::Rebase { step });
    }
    if has("MERGE_HEAD") {
        return Some(InProgress::Merge);
    }
    if has("CHERRY_PICK_HEAD") {
        return Some(InProgress::CherryPick);
    }
    if has("REVERT_HEAD") {
        return Some(InProgress::Revert);
    }
    if has("BISECT_LOG") {
        return Some(InProgress::Bisect);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub short: String,
    pub oid: String,
    pub author: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
    pub subject: String,
    pub body: String,
}

pub fn parse_log(raw: &[u8]) -> Vec<Commit> {
    String::from_utf8_lossy(raw)
        .split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches(['\n', '\r']);
            if rec.is_empty() {
                return None;
            }
            let f: Vec<&str> = rec.splitn(7, '\x1f').collect();
            if f.len() < 6 {
                return None;
            }
            Some(Commit {
                short: f[0].into(),
                oid: f[1].into(),
                author: f[2].into(),
                email: f[3].into(),
                time: f[4].trim().parse().unwrap_or(0),
                subject: f[5].into(),
                body: f
                    .get(6)
                    .map(|b| b.trim_end().to_string())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// The last `n` commits of HEAD (empty before the first commit).
pub fn log(root: &Path, n: usize) -> Vec<Commit> {
    let mut c = git_ro(root);
    c.args(["log", "--no-color", &format!("-n{n}")])
        .arg("--pretty=format:%h%x1f%H%x1f%an%x1f%ae%x1f%at%x1f%s%x1f%b%x1e");
    run(c).map(|raw| parse_log(&raw)).unwrap_or_default()
}

// ------------------------------------------------------------------ actions

pub fn stage(root: &Path, paths: &[String]) -> Result<(), String> {
    let mut c = git(root);
    c.args(["add", "-A", "--"]).args(paths);
    run(c).map(|_| ())
}

pub fn stage_all(root: &Path) -> Result<(), String> {
    let mut c = git(root);
    c.args(["add", "-A"]);
    run(c).map(|_| ())
}

/// Unstage. Before the first commit there is no HEAD to restore from, so the
/// paths are removed from the index instead.
pub fn unstage(root: &Path, paths: &[String], initial: bool) -> Result<(), String> {
    let mut c = git(root);
    if initial {
        c.args(["rm", "--cached", "-r", "-q", "--"]);
    } else {
        c.args(["restore", "--staged", "--"]);
    }
    c.args(paths);
    run(c).map(|_| ())
}

/// Commit the index; returns the new short SHA.
pub fn commit(root: &Path, message: &str) -> Result<String, String> {
    let mut c = git(root);
    c.args(["commit", "-q", "-m", message]);
    run(c)?;
    let mut c = git_ro(root);
    c.args(["rev-parse", "--short", "HEAD"]);
    Ok(run(c)
        .map(|o| String::from_utf8_lossy(&o).trim().to_string())
        .unwrap_or_default())
}

// ------------------------------------------------------------------ diff

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSource {
    /// Work tree vs. index.
    Unstaged,
    /// Index vs. HEAD.
    Staged,
    /// A new file: shown as all-added lines.
    Untracked,
    /// Combined diff of a conflicted file.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffRequest {
    pub path: String,
    pub orig: Option<String>,
    pub source: DiffSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
    Hunk,
    /// "\ No newline at end of file" and similar.
    Meta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub lines: Vec<DiffLine>,
    pub truncated: bool,
    pub binary: bool,
    pub adds: usize,
    pub dels: usize,
}

pub const MAX_DIFF_LINES: usize = 2000;
const MAX_DIFF_BYTES: usize = 1024 * 1024;

fn parse_range(s: &str) -> u32 {
    // "-12,5" / "+3" → start line
    s.trim_start_matches(['-', '+'])
        .split(',')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Parse unified (or combined `diff --cc`) diff output into display lines,
/// capped at `max_lines`.
pub fn parse_diff(raw: &[u8], max_lines: usize) -> Diff {
    let text = String::from_utf8_lossy(raw);
    let mut d = Diff::default();
    let mut old = 0u32;
    let mut new = 0u32;
    // Width of the +/- prefix (1 for normal diffs, 2 for 3-way combined).
    let mut prefix = 1usize;
    let mut in_hunk = false;
    for line in text.lines() {
        if d.lines.len() >= max_lines {
            d.truncated = true;
            break;
        }
        if line.starts_with("@@") {
            let ats = line.chars().take_while(|&c| c == '@').count();
            prefix = ats.saturating_sub(1).max(1);
            let inner = line[ats..].split("@@").next().unwrap_or("");
            let ranges: Vec<&str> = inner.split_whitespace().collect();
            old = ranges
                .iter()
                .find(|r| r.starts_with('-'))
                .map_or(0, |r| parse_range(r));
            new = ranges
                .iter()
                .rev()
                .find(|r| r.starts_with('+'))
                .map_or(0, |r| parse_range(r));
            in_hunk = true;
            d.lines.push(DiffLine {
                kind: LineKind::Hunk,
                old: None,
                new: None,
                text: line.to_string(),
            });
            continue;
        }
        if !in_hunk {
            if line.starts_with("Binary files") || line.starts_with("GIT binary patch") {
                d.binary = true;
            }
            continue;
        }
        if line.starts_with('\\') {
            d.lines.push(DiffLine {
                kind: LineKind::Meta,
                old: None,
                new: None,
                text: line.to_string(),
            });
            continue;
        }
        if line.starts_with("diff ") {
            // Next file in a multi-file diff: headers follow.
            in_hunk = false;
            continue;
        }
        let split = line
            .char_indices()
            .nth(prefix)
            .map_or(line.len(), |(i, _)| i);
        let (marks, body) = line.split_at(split);
        let combined = prefix > 1;
        if marks.contains('+') {
            d.adds += 1;
            d.lines.push(DiffLine {
                kind: LineKind::Add,
                old: None,
                new: Some(new),
                text: body.to_string(),
            });
            new += 1;
        } else if marks.contains('-') {
            d.dels += 1;
            let o = (!combined).then_some(old);
            d.lines.push(DiffLine {
                kind: LineKind::Del,
                old: o,
                new: None,
                text: body.to_string(),
            });
            old += 1;
        } else {
            let o = (!combined).then_some(old);
            d.lines.push(DiffLine {
                kind: LineKind::Context,
                old: o,
                new: Some(new),
                text: body.to_string(),
            });
            old += 1;
            new += 1;
        }
    }
    d
}

fn untracked_diff(path: &Path) -> Result<Diff, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Ok(Diff::default());
    }
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    Read::take(f, MAX_DIFF_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    let mut d = Diff::default();
    if buf[..buf.len().min(8000)].contains(&0) {
        d.binary = true;
        return Ok(d);
    }
    let byte_truncated = buf.len() > MAX_DIFF_BYTES;
    buf.truncate(MAX_DIFF_BYTES);
    let text = String::from_utf8_lossy(&buf);
    let total = text.lines().count();
    d.lines.push(DiffLine {
        kind: LineKind::Hunk,
        old: None,
        new: None,
        text: format!("@@ -0,0 +1,{total} @@ new file"),
    });
    for (i, line) in text.lines().enumerate() {
        if d.lines.len() > MAX_DIFF_LINES {
            d.truncated = true;
            break;
        }
        d.adds += 1;
        d.lines.push(DiffLine {
            kind: LineKind::Add,
            old: None,
            new: Some(i as u32 + 1),
            text: line.to_string(),
        });
    }
    d.truncated |= byte_truncated;
    Ok(d)
}

pub fn diff(root: &Path, req: &DiffRequest) -> Result<Diff, String> {
    if req.source == DiffSource::Untracked {
        return untracked_diff(&root.join(&req.path));
    }
    let mut c = git_ro(root);
    c.args(["diff", "--no-color", "--no-ext-diff", "-M"]);
    if req.source == DiffSource::Staged {
        c.arg("--cached");
    }
    c.arg("--");
    if let Some(o) = &req.orig {
        c.arg(o);
    }
    c.arg(&req.path);
    let (raw, truncated) = run_capped(c, MAX_DIFF_BYTES)?;
    let mut d = parse_diff(&raw, MAX_DIFF_LINES);
    d.truncated |= truncated;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_diff() {
        let raw = b"diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@ fn x\n one\n-two\n+TWO\n three\n\\ No newline at end of file\n";
        let d = parse_diff(raw, 100);
        assert_eq!(d.adds, 1);
        assert_eq!(d.dels, 1);
        assert_eq!(d.lines[0].kind, LineKind::Hunk);
        assert_eq!(
            d.lines[1],
            DiffLine {
                kind: LineKind::Context,
                old: Some(1),
                new: Some(1),
                text: "one".into()
            }
        );
        assert_eq!(
            d.lines[2],
            DiffLine {
                kind: LineKind::Del,
                old: Some(2),
                new: None,
                text: "two".into()
            }
        );
        assert_eq!(
            d.lines[3],
            DiffLine {
                kind: LineKind::Add,
                old: None,
                new: Some(2),
                text: "TWO".into()
            }
        );
        assert_eq!(d.lines[4].old, Some(3));
        assert_eq!(d.lines[5].kind, LineKind::Meta);
        assert!(!d.truncated);
    }

    #[test]
    fn parse_combined_diff() {
        let raw = b"diff --cc a.txt\n@@@ -1,1 -1,1 +1,5 @@@\n++<<<<<<< HEAD\n +ours\n++=======\n+ theirs\n++>>>>>>> other\n";
        let d = parse_diff(raw, 100);
        assert_eq!(d.adds, 5);
        assert_eq!(d.lines[1].new, Some(1));
        assert_eq!(d.lines[1].text, "<<<<<<< HEAD");
        assert_eq!(d.lines[2].text, "ours");
    }

    #[test]
    fn parse_binary_and_cap() {
        let d = parse_diff(b"diff --git a/x b/x\nBinary files a/x and b/x differ\n", 10);
        assert!(d.binary && d.lines.is_empty());
        let mut raw = String::from("@@ -1,100 +1,100 @@\n");
        for i in 0..100 {
            raw.push_str(&format!("+line {i}\n"));
        }
        let d = parse_diff(raw.as_bytes(), 10);
        assert_eq!(d.lines.len(), 10);
        assert!(d.truncated);
    }

    #[test]
    fn parse_log_records() {
        let raw = b"abc1234\x1fabc1234full\x1fAnna\x1fa@x\x1f1700000000\x1fFix it\x1fBody line\n\nmore\n\x1e\ndef5678\x1fdef5678full\x1fBen\x1fb@x\x1f1600000000\x1fInit\x1f\x1e";
        let c = parse_log(raw);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].subject, "Fix it");
        assert_eq!(c[0].body, "Body line\n\nmore");
        assert_eq!(c[1].short, "def5678");
        assert_eq!(c[1].time, 1_600_000_000);
        assert_eq!(c[1].body, "");
    }
}
