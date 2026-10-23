//! The Git plugin's strings: `(key, English, German)`, see
//! `manta_plugin::i18n`. German follows German Git GUIs (Tower, Fork):
//! "Commit", "Branch", "Merge", "Push"/"Pull" stay; staging is
//! "vormerken" ("Zum Commit vorgemerkt"), the working tree has
//! "Änderungen", untracked files are "Nicht verfolgt".

use manta_plugin::i18n::{Catalog, Entries, Lang};

mod es;
mod fr;
mod it;
mod ru;
mod zh;

pub static CATALOG: Catalog = Catalog::with(
    ENTRIES,
    &[
        (Lang::Zh, zh::ENTRIES),
        (Lang::Es, es::ENTRIES),
        (Lang::Fr, fr::ENTRIES),
        (Lang::It, it::ENTRIES),
        (Lang::Ru, ru::ENTRIES),
    ],
);

/// `tr!("key")`, `tr!("key", count = n)`, `tr!("key", name = value)`.
macro_rules! tr {
    ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
}
pub(crate) use tr;

const ENTRIES: Entries = &[
    // Plugin, commands, panel
    (
        "git.description",
        "Live git status in the file list, branch info in the path bar and a Git panel.",
        "Live-Git-Status in der Dateiliste, Branch-Infos in der Pfadleiste und ein Git-Panel.",
    ),
    (
        "git.cmd.toggle_panel",
        "Toggle Git Panel",
        "Git-Panel ein-/ausblenden",
    ),
    (
        "git.cmd.refresh",
        "Refresh Git Status",
        "Git-Status aktualisieren",
    ),
    ("git.panel.title", "Git", "Git"),
    // Operations in progress
    ("git.progress.merge", "Merge in progress", "Merge läuft"),
    (
        "git.progress.rebase_step",
        "Rebase in progress (step {n} of {total})",
        "Rebase läuft (Schritt {n} von {total})",
    ),
    ("git.progress.rebase", "Rebase in progress", "Rebase läuft"),
    (
        "git.progress.cherry_pick",
        "Cherry-pick in progress",
        "Cherry-Pick läuft",
    ),
    ("git.progress.revert", "Revert in progress", "Revert läuft"),
    ("git.progress.bisect", "Bisect in progress", "Bisect läuft"),
    (
        "git.progress.am",
        "Applying patches (git am)",
        "Patches werden angewendet (git am)",
    ),
    ("git.label.merge", "Merging", "Merge"),
    ("git.label.rebase", "Rebasing", "Rebase"),
    ("git.label.cherry_pick", "Cherry-picking", "Cherry-Pick"),
    ("git.label.revert", "Reverting", "Revert"),
    ("git.label.bisect", "Bisecting", "Bisect"),
    ("git.label.am", "Applying patches", "Patches anwenden"),
    // Entry states (badge tooltips)
    ("git.state.conflict", "Merge conflict", "Merge-Konflikt"),
    ("git.state.untracked", "Untracked", "Nicht verfolgt"),
    (
        "git.state.ignored",
        "Ignored by .gitignore",
        "Durch .gitignore ignoriert",
    ),
    (
        "git.state.renamed_unstaged",
        "Renamed from {orig}, with changes not staged",
        "Umbenannt von {orig}, mit nicht vorgemerkten Änderungen",
    ),
    (
        "git.state.renamed_staged",
        "Renamed from {orig}, staged for commit",
        "Umbenannt von {orig}, zum Commit vorgemerkt",
    ),
    (
        "git.state.copied_unstaged",
        "Copied from {orig}, with changes not staged",
        "Kopiert von {orig}, mit nicht vorgemerkten Änderungen",
    ),
    (
        "git.state.copied_staged",
        "Copied from {orig}, staged for commit",
        "Kopiert von {orig}, zum Commit vorgemerkt",
    ),
    (
        "git.state.partly_staged",
        "Modified, partly staged (more changes not staged)",
        "Geändert, teilweise vorgemerkt (weitere Änderungen nicht vorgemerkt)",
    ),
    (
        "git.state.deleted_unstaged",
        "Deleted, not staged",
        "Gelöscht, nicht vorgemerkt",
    ),
    (
        "git.state.submodule",
        "Submodule has changes",
        "Submodul hat Änderungen",
    ),
    (
        "git.state.modified_unstaged",
        "Modified, not staged",
        "Geändert, nicht vorgemerkt",
    ),
    (
        "git.state.added_staged",
        "Added, staged for commit",
        "Hinzugefügt, zum Commit vorgemerkt",
    ),
    (
        "git.state.deletion_staged",
        "Deletion staged for commit",
        "Löschung zum Commit vorgemerkt",
    ),
    (
        "git.state.staged",
        "Staged for commit",
        "Zum Commit vorgemerkt",
    ),
    (
        "git.state.renamed_short",
        "Renamed from {orig}, staged",
        "Umbenannt von {orig}, vorgemerkt",
    ),
    (
        "git.folder.changed.one",
        "{count} changed file inside",
        "{count} geänderte Datei darin",
    ),
    (
        "git.folder.changed.other",
        "{count} changed files inside",
        "{count} geänderte Dateien darin",
    ),
    (
        "git.folder.conflicts.one",
        "{count} conflict",
        "{count} Konflikt",
    ),
    (
        "git.folder.conflicts.other",
        "{count} conflicts",
        "{count} Konflikte",
    ),
    // Overall state
    (
        "git.summary.conflicts.one",
        "{count} merge conflict",
        "{count} Merge-Konflikt",
    ),
    (
        "git.summary.conflicts.other",
        "{count} merge conflicts",
        "{count} Merge-Konflikte",
    ),
    (
        "git.summary.clean",
        "Working tree clean",
        "Keine Änderungen",
    ),
    (
        "git.summary.changed.one",
        "{count} changed file",
        "{count} geänderte Datei",
    ),
    (
        "git.summary.changed.other",
        "{count} changed files",
        "{count} geänderte Dateien",
    ),
    ("git.summary.staged", "{n} staged", "{n} vorgemerkt"),
    // Path-bar chip tooltip
    (
        "git.chip.detached",
        "Detached HEAD at {name}",
        "Losgelöster HEAD bei {name}",
    ),
    ("git.chip.branch", "Branch {name}", "Branch {name}"),
    ("git.chip.up_to_date", "up to date", "aktuell"),
    ("git.chip.ahead", "{a} ahead", "{a} voraus"),
    ("git.chip.behind", "{d} behind", "{d} zurück"),
    (
        "git.chip.ahead_behind",
        "{a} ahead, {d} behind",
        "{a} voraus, {d} zurück",
    ),
    (
        "git.chip.tracking",
        "Tracking {up} ({rel})",
        "Folgt {up} ({rel})",
    ),
    (
        "git.chip.tracking_gone",
        "Tracking {up} (gone)",
        "Folgt {up} (entfernt)",
    ),
    (
        "git.chip.no_upstream",
        "No upstream branch",
        "Kein Upstream-Branch",
    ),
    (
        "git.chip.last_commit",
        "Last commit: {subject}",
        "Letzter Commit: {subject}",
    ),
    (
        "git.chip.no_commits",
        "No commits yet",
        "Noch keine Commits",
    ),
    (
        "git.chip.click",
        "Click to show the Git panel",
        "Klicke, um das Git-Panel zu zeigen",
    ),
    // Toasts and errors
    (
        "git.toast.committed",
        "Committed {sha}: {subject}",
        "Commit {sha} erstellt: {subject}",
    ),
    (
        "git.toast.stage_failed",
        "Stage failed: {error}",
        "Vormerken fehlgeschlagen: {error}",
    ),
    (
        "git.toast.unstage_failed",
        "Unstage failed: {error}",
        "Vormerkung aufheben fehlgeschlagen: {error}",
    ),
    (
        "git.toast.commit_failed",
        "Commit failed: {error}",
        "Commit fehlgeschlagen: {error}",
    ),
    ("git.error.failed", "git failed", "git ist fehlgeschlagen"),
    (
        "git.error.parse",
        "cannot parse git status: {error}",
        "Git-Status kann nicht gelesen werden: {error}",
    ),
    (
        "git.error.cannot_run",
        "cannot run git: {error}",
        "git kann nicht ausgeführt werden: {error}",
    ),
    // Panel: empty and loading states
    (
        "git.empty.not_local",
        "Not a local folder",
        "Kein lokaler Ordner",
    ),
    (
        "git.empty.not_local.body",
        "Git status is available for folders on this Mac, not on servers.",
        "Der Git-Status ist für Ordner auf diesem Mac verfügbar, nicht auf Servern.",
    ),
    (
        "git.empty.not_repo",
        "Not a git repository",
        "Kein Git-Repository",
    ),
    (
        "git.empty.not_repo.body",
        "Open a folder inside a git work tree, or run “git init” here.",
        "Öffne einen Ordner in einer Git-Arbeitskopie oder führe hier „git init“ aus.",
    ),
    (
        "git.loading.repo",
        "Looking for a repository…",
        "Repository wird gesucht …",
    ),
    (
        "git.loading.status",
        "Loading status…",
        "Status wird geladen …",
    ),
    (
        "git.error.status",
        "Git status failed",
        "Git-Status fehlgeschlagen",
    ),
    // Panel: header
    (
        "git.header.refresh",
        "Refresh Git status",
        "Git-Status aktualisieren",
    ),
    ("git.header.detached", "detached", "losgelöst"),
    (
        "git.header.to_push.one",
        "{count} commit to push",
        "{count} Commit zum Pushen",
    ),
    (
        "git.header.to_push.other",
        "{count} commits to push",
        "{count} Commits zum Pushen",
    ),
    (
        "git.header.to_pull.one",
        "{count} commit to pull",
        "{count} Commit zum Pullen",
    ),
    (
        "git.header.to_pull.other",
        "{count} commits to pull",
        "{count} Commits zum Pullen",
    ),
    (
        "git.header.tracking_gone",
        "Tracking {up} (gone)",
        "Folgt {up} (entfernt)",
    ),
    (
        "git.header.tracking_current",
        "Tracking {up} · up to date",
        "Folgt {up} · aktuell",
    ),
    (
        "git.header.tracking_ahead",
        "Tracking {up} · {a} ahead",
        "Folgt {up} · {a} voraus",
    ),
    (
        "git.header.tracking_behind",
        "Tracking {up} · {d} behind",
        "Folgt {up} · {d} zurück",
    ),
    (
        "git.header.tracking_both",
        "Tracking {up} · {a} ahead, {d} behind",
        "Folgt {up} · {a} voraus, {d} zurück",
    ),
    (
        "git.header.detached_head",
        "Detached HEAD",
        "Losgelöster HEAD",
    ),
    (
        "git.hint.merge_conflicts",
        "Resolve the conflicts, stage the files, then commit.",
        "Löse die Konflikte, merke die Dateien vor und committe dann.",
    ),
    (
        "git.hint.merge_done",
        "All conflicts resolved. Commit to finish the merge.",
        "Alle Konflikte sind gelöst. Committe, um den Merge abzuschließen.",
    ),
    (
        "git.hint.rebase",
        "Resolve, stage, then run git rebase --continue.",
        "Lösen, vormerken, dann git rebase --continue ausführen.",
    ),
    (
        "git.hint.cherry_pick",
        "Resolve, stage, then run git cherry-pick --continue.",
        "Lösen, vormerken, dann git cherry-pick --continue ausführen.",
    ),
    (
        "git.hint.revert",
        "Resolve, stage, then run git revert --continue.",
        "Lösen, vormerken, dann git revert --continue ausführen.",
    ),
    (
        "git.hint.bisect",
        "Mark commits with git bisect good / bad.",
        "Markiere Commits mit git bisect good / bad.",
    ),
    (
        "git.hint.am",
        "Resolve, stage, then run git am --continue.",
        "Lösen, vormerken, dann git am --continue ausführen.",
    ),
    // Panel: commit box
    (
        "git.commit.placeholder",
        "Commit message",
        "Commit-Nachricht",
    ),
    ("git.commit.working", "Working…", "Wird ausgeführt …"),
    (
        "git.commit.nothing_staged",
        "Nothing staged",
        "Nichts vorgemerkt",
    ),
    (
        "git.commit.staged.one",
        "{count} file staged",
        "{count} Datei vorgemerkt",
    ),
    (
        "git.commit.staged.other",
        "{count} files staged",
        "{count} Dateien vorgemerkt",
    ),
    ("git.commit.button", "Commit", "Commit"),
    (
        "git.commit.tip.stage_first",
        "Stage changes first",
        "Merke zuerst Änderungen vor",
    ),
    (
        "git.commit.tip.message",
        "Enter a commit message",
        "Gib eine Commit-Nachricht ein",
    ),
    (
        "git.commit.tip.commit",
        "Commit staged changes (⌘↩)",
        "Vorgemerkte Änderungen committen (⌘↩)",
    ),
    // Panel: changes list
    ("git.group.conflicts", "Conflicts", "Konflikte"),
    ("git.group.staged", "Staged", "Vorgemerkt"),
    ("git.group.changes", "Changes", "Änderungen"),
    ("git.group.untracked", "Untracked", "Nicht verfolgt"),
    ("git.action.stage_all", "Stage All", "Alle vormerken"),
    ("git.action.unstage_all", "Unstage All", "Alle zurücknehmen"),
    ("git.action.stage", "Stage", "Vormerken"),
    ("git.action.unstage", "Unstage", "Vormerkung aufheben"),
    (
        "git.action.resolve",
        "Mark as resolved (stage)",
        "Als gelöst markieren (vormerken)",
    ),
    // Panel: diff preview
    ("git.diff.staged", "Staged", "Vorgemerkt"),
    ("git.diff.not_staged", "Not staged", "Nicht vorgemerkt"),
    ("git.diff.new_file", "New file", "Neue Datei"),
    ("git.diff.conflict", "Conflict", "Konflikt"),
    ("git.diff.close", "Close preview", "Vorschau schließen"),
    ("git.diff.loading", "Loading diff…", "Diff wird geladen …"),
    (
        "git.diff.binary",
        "Binary file, no preview",
        "Binärdatei, keine Vorschau",
    ),
    (
        "git.diff.empty",
        "No changes to show",
        "Keine Änderungen zum Anzeigen",
    ),
    (
        "git.diff.truncated",
        "… showing the first {n} lines",
        "… nur die ersten {n} Zeilen",
    ),
    // Panel: commits
    ("git.commits.title", "Recent Commits", "Letzte Commits"),
    ("git.commits.none", "No commits yet", "Noch keine Commits"),
    // Relative times
    ("git.time.now", "just now", "gerade eben"),
    ("git.time.min", "{n} min ago", "vor {n} Min."),
    ("git.time.hours", "{n} h ago", "vor {n} Std."),
    ("git.time.yesterday", "yesterday", "gestern"),
    ("git.time.days", "{n} days ago", "vor {n} Tagen"),
    ("git.time.weeks", "{n} weeks ago", "vor {n} Wochen"),
    ("git.time.months", "{n} months ago", "vor {n} Monaten"),
    ("git.time.year", "1 year ago", "vor 1 Jahr"),
    ("git.time.years", "{n} years ago", "vor {n} Jahren"),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::*;

    /// English strings that read the same in German.
    const SAME: &[&str] = &["Branch {name}"];

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(&CATALOG, SAME);
        assert!(p.is_empty(), "{} problems:\n{}", p.len(), p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let m = check::missing_keys(&CATALOG, &src, 60);
        assert!(m.is_empty(), "missing keys:\n{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // This crate's own drawing helpers.
        let extra = [
            "empty_state(ui, ",
            "loading(ui, \"",
            "centered_note(ui, col, \"",
            "kit::pill(ui, \"",
            "kit::button(ui, \"",
            "kit::galley(ui, \"",
            "layout_no_wrap(\"",
            "parts.push(\"",
            "tip.push(\"",
            "tip.push(format!(\"",
            "text.push_str(\"",
            "=> \"",
            "Err(format!(\"",
            "on_hover_text(format!(\"",
            "RichText::new(format!(\"",
            // icon_button / empty_state tooltips and titles follow the icon.
            "Icon::",
        ];
        // Not UI text: git arguments, ids, file names, badges.
        let allow = [
            "kit::id(",
            ".args(",
            ".arg(",
            "Command::new",
            "\"HEAD\"",
            "log::",
        ];
        let hits = check::hard_coded_strings(&src, &extra, &allow);
        assert!(
            hits.is_empty(),
            "{} hard-coded UI strings (use tr!):\n{}",
            hits.len(),
            hits.join("\n")
        );
    }

    #[test]
    fn german_lookups() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("git.group.staged"), "Vorgemerkt");
            assert_eq!(tr!("git.commit.staged", count = 3), "3 Dateien vorgemerkt");
        });
        with_lang(Lang::En, || {
            assert_eq!(tr!("git.group.untracked"), "Untracked")
        });
    }
}
