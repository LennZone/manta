//! Turns a parsed [`GitStatus`] into host decorations and path-bar chips.
//! Pure functions, unit tested.

use std::collections::{BTreeMap, HashSet};

use manta_plugin::{Badge, Chip, Decoration, Tone};

use crate::i18n::tr;
use crate::status::{EntryKind, GitStatus, StatusEntry};

/// An operation that is in progress in the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InProgress {
    Merge,
    Rebase { step: Option<(u32, u32)> },
    CherryPick,
    Revert,
    Bisect,
    ApplyMailbox,
}

impl InProgress {
    pub fn describe(&self) -> String {
        match self {
            InProgress::Merge => tr!("git.progress.merge").into(),
            InProgress::Rebase {
                step: Some((n, total)),
            } => {
                tr!("git.progress.rebase_step", n = n, total = total)
            }
            InProgress::Rebase { step: None } => tr!("git.progress.rebase").into(),
            InProgress::CherryPick => tr!("git.progress.cherry_pick").into(),
            InProgress::Revert => tr!("git.progress.revert").into(),
            InProgress::Bisect => tr!("git.progress.bisect").into(),
            InProgress::ApplyMailbox => tr!("git.progress.am").into(),
        }
    }

    /// Short label for the panel header, e.g. "MERGING".
    pub fn label(&self) -> &'static str {
        match self {
            InProgress::Merge => tr!("git.label.merge"),
            InProgress::Rebase { .. } => tr!("git.label.rebase"),
            InProgress::CherryPick => tr!("git.label.cherry_pick"),
            InProgress::Revert => tr!("git.label.revert"),
            InProgress::Bisect => tr!("git.label.bisect"),
            InProgress::ApplyMailbox => tr!("git.label.am"),
        }
    }

    /// Whether this blocks a normal workflow (shown as danger).
    pub fn is_blocking(&self) -> bool {
        !matches!(self, InProgress::Bisect)
    }
}

/// How strongly an entry asks for attention. Ordered weakest → strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Untracked,
    Staged,
    Renamed,
    Modified,
    Conflict,
}

impl Severity {
    pub fn tone(self) -> Tone {
        match self {
            Severity::Untracked => Tone::Info,
            Severity::Staged => Tone::Success,
            Severity::Renamed => Tone::Accent,
            Severity::Modified => Tone::Warning,
            Severity::Conflict => Tone::Danger,
        }
    }
}

/// Badge text, severity and plain-words description of one entry.
pub fn classify(e: &StatusEntry) -> (&'static str, Severity, String) {
    match &e.kind {
        EntryKind::Unmerged => ("U", Severity::Conflict, tr!("git.state.conflict").into()),
        EntryKind::Untracked => ("?", Severity::Untracked, tr!("git.state.untracked").into()),
        EntryKind::Ignored => ("", Severity::Untracked, tr!("git.state.ignored").into()),
        EntryKind::Renamed { orig, copied } => {
            let text = match (*copied, e.y != '.') {
                (false, true) => tr!("git.state.renamed_unstaged", orig = orig),
                (false, false) => tr!("git.state.renamed_staged", orig = orig),
                (true, true) => tr!("git.state.copied_unstaged", orig = orig),
                (true, false) => tr!("git.state.copied_staged", orig = orig),
            };
            ("R", Severity::Renamed, text)
        }
        EntryKind::Ordinary => {
            let staged = e.x != '.';
            let unstaged = e.y != '.';
            match (staged, unstaged) {
                (true, true) => (
                    "M",
                    Severity::Modified,
                    tr!("git.state.partly_staged").into(),
                ),
                (false, true) if e.y == 'D' => (
                    "D",
                    Severity::Modified,
                    tr!("git.state.deleted_unstaged").into(),
                ),
                (false, true) if e.submodule => {
                    ("M", Severity::Modified, tr!("git.state.submodule").into())
                }
                (false, true) => (
                    "M",
                    Severity::Modified,
                    tr!("git.state.modified_unstaged").into(),
                ),
                (true, false) if e.x == 'A' => {
                    ("A", Severity::Staged, tr!("git.state.added_staged").into())
                }
                (true, false) if e.x == 'D' => (
                    "D",
                    Severity::Staged,
                    tr!("git.state.deletion_staged").into(),
                ),
                (true, false) => ("M", Severity::Staged, tr!("git.state.staged").into()),
                (false, false) => ("", Severity::Untracked, String::new()),
            }
        }
    }
}

pub fn entry_decoration(e: &StatusEntry) -> Decoration {
    let (text, sev, tip) = classify(e);
    if e.kind == EntryKind::Ignored {
        return ignored_decoration();
    }
    Decoration {
        badge: (!text.is_empty()).then(|| Badge {
            text: text.into(),
            tone: sev.tone(),
        }),
        name_tone: None,
        dimmed: false,
        tooltip: (!tip.is_empty()).then_some(tip),
    }
}

pub fn ignored_decoration() -> Decoration {
    Decoration {
        badge: None,
        name_tone: None,
        dimmed: true,
        tooltip: Some(tr!("git.state.ignored").into()),
    }
}

#[derive(Debug, Default)]
struct FolderAgg {
    count: usize,
    conflicts: usize,
    strongest: Option<Severity>,
}

/// Decorations for the direct children of the folder `view_rel` (path of the
/// shown folder relative to the work-tree root, `""` for the root itself).
///
/// Files get their own badge; folders aggregate all changed descendants into
/// a "•" badge in the strongest tone. Names in `ignored` that have no other
/// decoration are dimmed. Sorted by name for stable output.
pub fn view_decorations(
    status: &GitStatus,
    view_rel: &str,
    ignored: &HashSet<String>,
) -> Vec<(String, Decoration)> {
    let prefix = if view_rel.is_empty() {
        String::new()
    } else {
        format!("{view_rel}/")
    };
    let mut files: BTreeMap<String, Decoration> = BTreeMap::new();
    let mut folders: BTreeMap<String, FolderAgg> = BTreeMap::new();

    for e in &status.entries {
        if e.kind == EntryKind::Ignored {
            continue;
        }
        let Some(rest) = e.path.strip_prefix(&prefix) else {
            continue;
        };
        let rest = rest.trim_end_matches('/');
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            None => {
                files.insert(rest.to_string(), entry_decoration(e));
            }
            Some((dir, _)) => {
                let (_, sev, _) = classify(e);
                let agg = folders.entry(dir.to_string()).or_default();
                agg.count += 1;
                if e.is_conflict() {
                    agg.conflicts += 1;
                }
                agg.strongest = Some(agg.strongest.map_or(sev, |s| s.max(sev)));
            }
        }
    }

    for (name, agg) in folders {
        let sev = agg.strongest.unwrap_or(Severity::Modified);
        let mut tip = tr!("git.folder.changed", count = agg.count);
        if agg.conflicts > 0 {
            tip.push_str(&format!(
                " ({})",
                tr!("git.folder.conflicts", count = agg.conflicts)
            ));
        }
        files.entry(name).or_insert(Decoration {
            badge: Some(Badge {
                text: "•".into(),
                tone: sev.tone(),
            }),
            name_tone: None,
            dimmed: false,
            tooltip: Some(tip),
        });
    }

    for name in ignored {
        files.entry(name.clone()).or_insert_with(ignored_decoration);
    }

    files.into_iter().collect()
}

/// Inputs for the path-bar chip.
pub struct ChipInput<'a> {
    pub status: &'a GitStatus,
    pub in_progress: Option<&'a InProgress>,
    pub last_subject: Option<&'a str>,
}

/// The overall state in plain words, e.g. "3 changed files, 1 staged".
pub fn state_words(status: &GitStatus, in_progress: Option<&InProgress>) -> String {
    let mut parts = Vec::new();
    if let Some(p) = in_progress {
        parts.push(p.describe());
    }
    let conflicts = status.conflicts();
    if conflicts > 0 {
        parts.push(tr!("git.summary.conflicts", count = conflicts));
    }
    let changes = status.change_count();
    if changes == 0 {
        parts.push(tr!("git.summary.clean").into());
    } else {
        let staged = status.staged_count();
        let mut s = tr!("git.summary.changed", count = changes);
        if staged > 0 {
            s.push_str(", ");
            s.push_str(&tr!(
                "git.summary.staged",
                n = manta_plugin::i18n::format_count(staged as u64)
            ));
        }
        parts.push(s);
    }
    parts.join(" · ")
}

pub fn ahead_behind_text(ahead: u32, behind: u32) -> String {
    let mut s = String::new();
    if ahead > 0 {
        s.push_str(&format!("↑{ahead}"));
    }
    if behind > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("↓{behind}"));
    }
    s
}

pub fn chip_tone(status: &GitStatus, in_progress: Option<&InProgress>) -> Tone {
    if status.conflicts() > 0 || in_progress.is_some_and(InProgress::is_blocking) {
        Tone::Danger
    } else if !status.is_clean() {
        Tone::Warning
    } else {
        Tone::Success
    }
}

pub const TOGGLE_PANEL_CMD: &str = "toggle-panel";
pub const REFRESH_CMD: &str = "refresh";

pub fn build_chip(input: &ChipInput<'_>) -> Chip {
    let st = input.status;
    let b = &st.branch;
    let mut text = b.display_name();
    let ab = ahead_behind_text(b.ahead, b.behind);
    if !ab.is_empty() {
        text.push(' ');
        text.push_str(&ab);
    }
    let changes = st.change_count();
    if changes > 0 {
        text.push_str(&format!(" · {changes}"));
    }

    let mut tip = Vec::new();
    if b.is_detached() {
        tip.push(tr!("git.chip.detached", name = b.display_name()));
    } else {
        tip.push(tr!("git.chip.branch", name = b.display_name()));
    }
    match &b.upstream {
        Some(up) => {
            let rel = match (b.ahead, b.behind) {
                (0, 0) => tr!("git.chip.up_to_date").to_string(),
                (a, 0) => tr!("git.chip.ahead", a = a),
                (0, d) => tr!("git.chip.behind", d = d),
                (a, d) => tr!("git.chip.ahead_behind", a = a, d = d),
            };
            if b.has_ab {
                tip.push(tr!("git.chip.tracking", up = up, rel = rel));
            } else {
                tip.push(tr!("git.chip.tracking_gone", up = up));
            }
        }
        None if !b.is_detached() => tip.push(tr!("git.chip.no_upstream").into()),
        None => {}
    }
    match input.last_subject {
        Some(s) if !b.is_initial() => tip.push(tr!("git.chip.last_commit", subject = s)),
        _ if b.is_initial() => tip.push(tr!("git.chip.no_commits").into()),
        _ => {}
    }
    tip.push(state_words(st, input.in_progress));
    tip.push(tr!("git.chip.click").into());

    Chip {
        text,
        tone: chip_tone(st, input.in_progress),
        icon: Some("branch".into()),
        tooltip: Some(tip.join("\n")),
        command: Some(TOGGLE_PANEL_CMD.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{BranchInfo, EntryKind};

    fn e(path: &str, kind: EntryKind, x: char, y: char) -> StatusEntry {
        StatusEntry {
            path: path.into(),
            kind,
            x,
            y,
            submodule: false,
        }
    }

    fn status(entries: Vec<StatusEntry>) -> GitStatus {
        GitStatus {
            branch: BranchInfo {
                oid: Some("abcdef0123".into()),
                head: Some("main".into()),
                ..Default::default()
            },
            entries,
        }
    }

    fn find<'a>(d: &'a [(String, Decoration)], name: &str) -> &'a Decoration {
        &d.iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1
    }

    fn badge(d: &Decoration) -> (String, Tone) {
        let b = d.badge.as_ref().expect("badge");
        (b.text.clone(), b.tone)
    }

    #[test]
    fn file_badges() {
        let st = status(vec![
            e("mod.txt", EntryKind::Ordinary, '.', 'M'),
            e("staged.txt", EntryKind::Ordinary, 'M', '.'),
            e("added.txt", EntryKind::Ordinary, 'A', '.'),
            e("both.txt", EntryKind::Ordinary, 'M', 'M'),
            e("new.txt", EntryKind::Untracked, '?', '?'),
            e(
                "moved.txt",
                EntryKind::Renamed {
                    orig: "old.txt".into(),
                    copied: false,
                },
                'R',
                '.',
            ),
            e("conf.txt", EntryKind::Unmerged, 'U', 'U'),
        ]);
        let d = view_decorations(&st, "", &HashSet::new());
        assert_eq!(badge(find(&d, "mod.txt")), ("M".into(), Tone::Warning));
        assert_eq!(
            find(&d, "mod.txt").tooltip.as_deref(),
            Some("Modified, not staged")
        );
        assert_eq!(badge(find(&d, "staged.txt")), ("M".into(), Tone::Success));
        assert_eq!(
            find(&d, "staged.txt").tooltip.as_deref(),
            Some("Staged for commit")
        );
        assert_eq!(badge(find(&d, "added.txt")), ("A".into(), Tone::Success));
        assert_eq!(badge(find(&d, "both.txt")), ("M".into(), Tone::Warning));
        assert_eq!(badge(find(&d, "new.txt")), ("?".into(), Tone::Info));
        assert_eq!(find(&d, "new.txt").tooltip.as_deref(), Some("Untracked"));
        assert_eq!(badge(find(&d, "moved.txt")), ("R".into(), Tone::Accent));
        assert!(find(&d, "moved.txt")
            .tooltip
            .as_deref()
            .unwrap()
            .contains("old.txt"));
        assert_eq!(badge(find(&d, "conf.txt")), ("U".into(), Tone::Danger));
        assert_eq!(
            find(&d, "conf.txt").tooltip.as_deref(),
            Some("Merge conflict")
        );
    }

    #[test]
    fn folder_aggregation_uses_strongest_tone() {
        let st = status(vec![
            e("src/a.rs", EntryKind::Ordinary, 'M', '.'),
            e("src/deep/b.rs", EntryKind::Untracked, '?', '?'),
            e("src/deep/c.rs", EntryKind::Ordinary, '.', 'M'),
            e("docs/x.md", EntryKind::Untracked, '?', '?'),
            e("lib/y.rs", EntryKind::Unmerged, 'U', 'U'),
            e("top.txt", EntryKind::Ordinary, '.', 'M'),
        ]);
        let d = view_decorations(&st, "", &HashSet::new());
        assert_eq!(d.len(), 4);
        let src = find(&d, "src");
        assert_eq!(badge(src), ("•".into(), Tone::Warning));
        assert_eq!(src.tooltip.as_deref(), Some("3 changed files inside"));
        assert_eq!(badge(find(&d, "docs")), ("•".into(), Tone::Info));
        assert_eq!(
            find(&d, "docs").tooltip.as_deref(),
            Some("1 changed file inside")
        );
        assert_eq!(badge(find(&d, "lib")), ("•".into(), Tone::Danger));
        assert_eq!(
            find(&d, "lib").tooltip.as_deref(),
            Some("1 changed file inside (1 conflict)")
        );
    }

    #[test]
    fn german_tooltips_and_chip() {
        use manta_plugin::i18n::{with_lang, Lang};
        let st = status(vec![
            e("mod.txt", EntryKind::Ordinary, '.', 'M'),
            e("src/a.rs", EntryKind::Ordinary, 'M', '.'),
            e("src/b.rs", EntryKind::Unmerged, 'U', 'U'),
            e("new.txt", EntryKind::Untracked, '?', '?'),
        ]);
        with_lang(Lang::De, || {
            let d = view_decorations(&st, "", &HashSet::new());
            assert_eq!(
                find(&d, "mod.txt").tooltip.as_deref(),
                Some("Geändert, nicht vorgemerkt")
            );
            assert_eq!(
                find(&d, "new.txt").tooltip.as_deref(),
                Some("Nicht verfolgt")
            );
            assert_eq!(
                find(&d, "src").tooltip.as_deref(),
                Some("2 geänderte Dateien darin (1 Konflikt)")
            );
            let chip = build_chip(&ChipInput {
                status: &st,
                in_progress: Some(&InProgress::Merge),
                last_subject: Some("Fix"),
            });
            let tip = chip.tooltip.unwrap();
            assert!(tip.contains("Branch main"), "{tip}");
            assert!(tip.contains("Kein Upstream-Branch"), "{tip}");
            assert!(tip.contains("Letzter Commit: Fix"), "{tip}");
            assert!(
                tip.contains("Merge läuft · 1 Merge-Konflikt · 4 geänderte Dateien, 1 vorgemerkt"),
                "{tip}"
            );
        });
    }

    #[test]
    fn subfolder_view() {
        let st = status(vec![
            e("src/a.rs", EntryKind::Ordinary, '.', 'M'),
            e("src/deep/b.rs", EntryKind::Untracked, '?', '?'),
            e("srcx/other.rs", EntryKind::Ordinary, '.', 'M'),
            e("top.txt", EntryKind::Ordinary, '.', 'M'),
        ]);
        let d = view_decorations(&st, "src", &HashSet::new());
        let names: Vec<_> = d.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a.rs", "deep"]);
        assert_eq!(badge(find(&d, "deep")), ("•".into(), Tone::Info));
    }

    #[test]
    fn ignored_are_dimmed_without_badge() {
        let st = status(vec![e("x.txt", EntryKind::Ordinary, '.', 'M')]);
        let ignored: HashSet<String> = ["target".to_string(), "x.txt".to_string()].into();
        let d = view_decorations(&st, "", &ignored);
        let t = find(&d, "target");
        assert!(t.dimmed && t.badge.is_none());
        assert_eq!(t.tooltip.as_deref(), Some("Ignored by .gitignore"));
        // A real change wins over the ignored flag.
        assert!(!find(&d, "x.txt").dimmed);
    }

    #[test]
    fn chip_text_and_tone() {
        let mut st = status(vec![]);
        st.branch.upstream = Some("origin/main".into());
        st.branch.has_ab = true;
        let chip = build_chip(&ChipInput {
            status: &st,
            in_progress: None,
            last_subject: Some("Init"),
        });
        assert_eq!(chip.text, "main");
        assert_eq!(chip.tone, Tone::Success);
        assert_eq!(chip.icon.as_deref(), Some("branch"));
        assert_eq!(chip.command.as_deref(), Some(TOGGLE_PANEL_CMD));
        let tip = chip.tooltip.unwrap();
        assert!(tip.contains("origin/main (up to date)"), "{tip}");
        assert!(tip.contains("Last commit: Init"));
        assert!(tip.contains("Working tree clean"));

        st.branch.ahead = 2;
        st.branch.behind = 1;
        st.entries.push(e("a", EntryKind::Ordinary, '.', 'M'));
        st.entries.push(e("b", EntryKind::Untracked, '?', '?'));
        let chip = build_chip(&ChipInput {
            status: &st,
            in_progress: None,
            last_subject: None,
        });
        assert_eq!(chip.text, "main ↑2 ↓1 · 2");
        assert_eq!(chip.tone, Tone::Warning);
        assert!(chip.tooltip.unwrap().contains("2 ahead, 1 behind"));

        let chip = build_chip(&ChipInput {
            status: &st,
            in_progress: Some(&InProgress::Merge),
            last_subject: None,
        });
        assert_eq!(chip.tone, Tone::Danger);
        assert!(chip.tooltip.unwrap().contains("Merge in progress"));

        st.entries.push(e("c", EntryKind::Unmerged, 'U', 'U'));
        let chip = build_chip(&ChipInput {
            status: &st,
            in_progress: None,
            last_subject: None,
        });
        assert_eq!(chip.tone, Tone::Danger);
    }

    #[test]
    fn chip_detached_and_initial() {
        let st = GitStatus {
            branch: BranchInfo {
                oid: Some("0123456789abcdef".into()),
                head: None,
                ..Default::default()
            },
            entries: vec![],
        };
        let chip = build_chip(&ChipInput {
            status: &st,
            in_progress: None,
            last_subject: None,
        });
        assert_eq!(chip.text, "0123456");
        assert!(chip.tooltip.unwrap().starts_with("Detached HEAD"));

        let st = GitStatus {
            branch: BranchInfo {
                oid: None,
                head: Some("main".into()),
                ..Default::default()
            },
            entries: vec![],
        };
        let chip = build_chip(&ChipInput {
            status: &st,
            in_progress: None,
            last_subject: None,
        });
        let tip = chip.tooltip.unwrap();
        assert!(
            tip.contains("No commits yet") && tip.contains("No upstream"),
            "{tip}"
        );
    }

    #[test]
    fn state_words_plain() {
        let st = status(vec![
            e("a", EntryKind::Ordinary, 'M', '.'),
            e("b", EntryKind::Unmerged, 'U', 'U'),
        ]);
        let w = state_words(&st, Some(&InProgress::Rebase { step: Some((2, 5)) }));
        assert_eq!(
            w,
            "Rebase in progress (step 2 of 5) · 1 merge conflict · 2 changed files, 1 staged"
        );
    }
}
