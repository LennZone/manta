//! Parser for `git status --porcelain=v2 --branch -z`.
//!
//! Format reference: `git help status`, section "Porcelain Format Version 2".
//! With `-z` every header line and entry ends with NUL, paths are never
//! quoted, and a rename record (`2 …`) is followed by a second NUL-terminated
//! field holding the original path.

/// Branch headers (`# branch.*`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BranchInfo {
    /// Full commit id of HEAD; `None` before the first commit.
    pub oid: Option<String>,
    /// Branch name; `None` if HEAD is detached.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Ahead/behind are known (`# branch.ab` was present, i.e. the upstream
    /// exists).
    pub has_ab: bool,
}

impl BranchInfo {
    pub fn is_detached(&self) -> bool {
        self.head.is_none()
    }

    pub fn is_initial(&self) -> bool {
        self.oid.is_none()
    }

    pub fn short_oid(&self) -> Option<&str> {
        self.oid.as_deref().map(|o| &o[..o.len().min(7)])
    }

    /// Branch name, or the short SHA when detached.
    pub fn display_name(&self) -> String {
        match (&self.head, self.short_oid()) {
            (Some(h), _) => h.clone(),
            (None, Some(sha)) => sha.to_string(),
            (None, None) => "HEAD".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// `1`: changed tracked entry.
    Ordinary,
    /// `2`: renamed or copied (`orig` is the source path).
    Renamed { orig: String, copied: bool },
    /// `u`: unmerged (merge conflict).
    Unmerged,
    /// `?`
    Untracked,
    /// `!` (only with `--ignored`).
    Ignored,
}

/// One status entry. `x` is the index (staged) state, `y` the work-tree
/// state; `.` means unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Path relative to the work-tree root, `/`-separated.
    pub path: String,
    pub kind: EntryKind,
    pub x: char,
    pub y: char,
    /// Submodule state field (`N...` for a normal file).
    pub submodule: bool,
}

impl StatusEntry {
    pub fn is_conflict(&self) -> bool {
        self.kind == EntryKind::Unmerged
    }

    pub fn is_untracked(&self) -> bool {
        self.kind == EntryKind::Untracked
    }

    /// Has changes in the index.
    pub fn is_staged(&self) -> bool {
        matches!(self.kind, EntryKind::Ordinary | EntryKind::Renamed { .. }) && self.x != '.'
    }

    /// Has changes in the work tree that are not staged.
    pub fn is_unstaged(&self) -> bool {
        matches!(self.kind, EntryKind::Ordinary | EntryKind::Renamed { .. }) && self.y != '.'
    }

    pub fn orig_path(&self) -> Option<&str> {
        match &self.kind {
            EntryKind::Renamed { orig, .. } => Some(orig),
            _ => None,
        }
    }

    /// Last path component.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatus {
    pub branch: BranchInfo,
    pub entries: Vec<StatusEntry>,
}

impl GitStatus {
    pub fn conflicts(&self) -> usize {
        self.entries.iter().filter(|e| e.is_conflict()).count()
    }

    /// Number of changed paths (excluding ignored).
    pub fn change_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind != EntryKind::Ignored)
            .count()
    }

    pub fn is_clean(&self) -> bool {
        self.change_count() == 0
    }

    pub fn staged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_staged()).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::i18n::tr!("git.error.parse", error = self.0))
    }
}

fn xy(s: &str) -> Result<(char, char), ParseError> {
    let mut c = s.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(x), Some(y), None) => Ok((x, y)),
        _ => Err(ParseError(format!("bad XY field {s:?}"))),
    }
}

/// Parse the raw (`-z`) output. Unknown record types and headers are skipped
/// so newer git versions keep working.
pub fn parse_porcelain_v2(raw: &[u8]) -> Result<GitStatus, ParseError> {
    let text = String::from_utf8_lossy(raw);
    let mut st = GitStatus::default();
    let mut fields = text.split('\0');
    while let Some(rec) = fields.next() {
        if rec.is_empty() {
            continue;
        }
        let (tag, rest) = rec.split_at(1);
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        match tag {
            "#" => parse_header(rest, &mut st.branch),
            "1" => {
                // XY sub mH mI mW hH hI path
                let parts: Vec<&str> = rest.splitn(8, ' ').collect();
                if parts.len() != 8 {
                    return Err(ParseError(format!("short ordinary record {rec:?}")));
                }
                let (x, y) = xy(parts[0])?;
                st.entries.push(StatusEntry {
                    path: parts[7].to_string(),
                    kind: EntryKind::Ordinary,
                    x,
                    y,
                    submodule: parts[1].starts_with('S'),
                });
            }
            "2" => {
                // XY sub mH mI mW hH hI Xscore path NUL origPath
                let parts: Vec<&str> = rest.splitn(9, ' ').collect();
                if parts.len() != 9 {
                    return Err(ParseError(format!("short rename record {rec:?}")));
                }
                let (x, y) = xy(parts[0])?;
                let orig = fields
                    .next()
                    .ok_or_else(|| ParseError(format!("rename without source {rec:?}")))?;
                st.entries.push(StatusEntry {
                    path: parts[8].to_string(),
                    kind: EntryKind::Renamed {
                        orig: orig.to_string(),
                        copied: parts[7].starts_with('C'),
                    },
                    x,
                    y,
                    submodule: parts[1].starts_with('S'),
                });
            }
            "u" => {
                // XY sub m1 m2 m3 mW h1 h2 h3 path
                let parts: Vec<&str> = rest.splitn(10, ' ').collect();
                if parts.len() != 10 {
                    return Err(ParseError(format!("short unmerged record {rec:?}")));
                }
                let (x, y) = xy(parts[0])?;
                st.entries.push(StatusEntry {
                    path: parts[9].to_string(),
                    kind: EntryKind::Unmerged,
                    x,
                    y,
                    submodule: parts[1].starts_with('S'),
                });
            }
            "?" | "!" => {
                let kind = if tag == "?" {
                    EntryKind::Untracked
                } else {
                    EntryKind::Ignored
                };
                let ch = tag.chars().next().unwrap_or('?');
                st.entries.push(StatusEntry {
                    path: rest.to_string(),
                    kind,
                    x: ch,
                    y: ch,
                    submodule: false,
                });
            }
            _ => log::debug!("git status: skipping unknown record {rec:?}"),
        }
    }
    Ok(st)
}

fn parse_header(line: &str, b: &mut BranchInfo) {
    let (key, value) = line.split_once(' ').unwrap_or((line, ""));
    match key {
        "branch.oid" => b.oid = (value != "(initial)").then(|| value.to_string()),
        "branch.head" => b.head = (value != "(detached)").then(|| value.to_string()),
        "branch.upstream" => b.upstream = Some(value.to_string()),
        "branch.ab" => {
            for part in value.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    b.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    b.behind = n.parse().unwrap_or(0);
                }
            }
            b.has_ab = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H40: &str = "1111111111111111111111111111111111111111";
    const Z40: &str = "0000000000000000000000000000000000000000";

    fn z(parts: &[&str]) -> Vec<u8> {
        let mut s = String::new();
        for p in parts {
            s.push_str(p);
            s.push('\0');
        }
        s.into_bytes()
    }

    #[test]
    fn empty_output_is_clean() {
        let st = parse_porcelain_v2(b"").unwrap();
        assert!(st.is_clean());
        assert_eq!(st.branch, BranchInfo::default());
    }

    #[test]
    fn branch_with_upstream_and_ab() {
        let raw = z(&[
            &format!("# branch.oid {H40}"),
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.branch.head.as_deref(), Some("main"));
        assert_eq!(st.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!((st.branch.ahead, st.branch.behind), (2, 1));
        assert!(st.branch.has_ab);
        assert_eq!(st.branch.short_oid(), Some("1111111"));
        assert_eq!(st.branch.display_name(), "main");
        assert!(st.is_clean());
    }

    #[test]
    fn no_upstream() {
        let raw = z(&[&format!("# branch.oid {H40}"), "# branch.head feature/x"]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.branch.upstream, None);
        assert!(!st.branch.has_ab);
        assert_eq!(st.branch.display_name(), "feature/x");
    }

    #[test]
    fn detached_head_uses_short_sha() {
        let raw = z(&[
            "# branch.oid abcdef0123456789abcdef0123456789abcdef01",
            "# branch.head (detached)",
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert!(st.branch.is_detached());
        assert_eq!(st.branch.display_name(), "abcdef0");
    }

    #[test]
    fn initial_commit() {
        let raw = z(&[
            "# branch.oid (initial)",
            "# branch.head main",
            &format!("1 A. N... 000000 100644 100644 {Z40} {H40} new file.txt"),
            "? notes.md",
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert!(st.branch.is_initial());
        assert_eq!(st.branch.display_name(), "main");
        assert_eq!(st.entries.len(), 2);
        assert_eq!(st.entries[0].path, "new file.txt");
        assert!(st.entries[0].is_staged());
        assert!(!st.entries[0].is_unstaged());
        assert!(st.entries[1].is_untracked());
        assert_eq!(st.staged_count(), 1);
    }

    #[test]
    fn ordinary_staged_and_unstaged() {
        let raw = z(&[
            &format!("1 .M N... 100644 100644 100644 {H40} {H40} src/main.rs"),
            &format!("1 M. N... 100644 100644 100644 {H40} {H40} Cargo.toml"),
            &format!("1 MM N... 100644 100644 100644 {H40} {H40} README.md"),
            &format!("1 .D N... 100644 100644 000000 {H40} {H40} old.txt"),
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        let e = &st.entries;
        assert!(!e[0].is_staged() && e[0].is_unstaged());
        assert!(e[1].is_staged() && !e[1].is_unstaged());
        assert!(e[2].is_staged() && e[2].is_unstaged());
        assert_eq!(e[3].y, 'D');
        assert_eq!(e[0].file_name(), "main.rs");
    }

    #[test]
    fn rename_with_spaces() {
        let raw = z(&[
            &format!("2 R. N... 100644 100644 100644 {H40} {H40} R100 new dir/my file.txt"),
            "old dir/my  file.txt",
            &format!("1 .M N... 100644 100644 100644 {H40} {H40} after.txt"),
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.entries.len(), 2);
        let r = &st.entries[0];
        assert_eq!(r.path, "new dir/my file.txt");
        assert_eq!(r.orig_path(), Some("old dir/my  file.txt"));
        assert!(matches!(r.kind, EntryKind::Renamed { copied: false, .. }));
        assert!(r.is_staged());
        assert_eq!(st.entries[1].path, "after.txt");
    }

    #[test]
    fn copy_record() {
        let raw = z(&[
            &format!("2 C. N... 100644 100644 100644 {H40} {H40} C75 b.txt"),
            "a.txt",
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert!(matches!(
            st.entries[0].kind,
            EntryKind::Renamed { copied: true, .. }
        ));
    }

    #[test]
    fn unicode_paths() {
        let raw = z(&[
            &format!("1 .M N... 100644 100644 100644 {H40} {H40} Bücher/Grüße 🚀.md"),
            "? 日本語/ファイル.txt",
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.entries[0].path, "Bücher/Grüße 🚀.md");
        assert_eq!(st.entries[0].file_name(), "Grüße 🚀.md");
        assert_eq!(st.entries[1].path, "日本語/ファイル.txt");
    }

    #[test]
    fn conflicts() {
        let raw = z(&[
            "# branch.oid 2222222222222222222222222222222222222222",
            "# branch.head main",
            &format!(
                "u UU N... 100644 100644 100644 100644 {H40} {H40} {H40} src/lib with space.rs"
            ),
            &format!("u AA N... 000000 100644 100644 100644 {Z40} {H40} {H40} both.txt"),
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.conflicts(), 2);
        assert_eq!(st.entries[0].path, "src/lib with space.rs");
        assert!(st.entries[0].is_conflict());
        assert!(!st.entries[0].is_staged());
        assert_eq!(st.staged_count(), 0);
    }

    #[test]
    fn untracked_and_ignored() {
        let raw = z(&["? a b.txt", "! target/debug"]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.entries[0].kind, EntryKind::Untracked);
        assert_eq!(st.entries[0].path, "a b.txt");
        assert_eq!(st.entries[1].kind, EntryKind::Ignored);
        assert_eq!(st.change_count(), 1);
    }

    #[test]
    fn submodule_flag() {
        let raw = z(&[&format!(
            "1 .M S.M. 160000 160000 160000 {H40} {H40} vendor/lib"
        )]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert!(st.entries[0].submodule);
    }

    #[test]
    fn unknown_records_are_skipped() {
        let raw = z(&[
            "# stash 3",
            "# branch.future thing",
            "x something new",
            "? ok.txt",
        ]);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert_eq!(st.entries.len(), 1);
    }

    #[test]
    fn truncated_records_error() {
        assert!(parse_porcelain_v2(b"1 .M N... 100644\0").is_err());
        let raw = z(&[&format!(
            "2 R. N... 100644 100644 100644 {H40} {H40} R100 new.txt"
        )]);
        // Rename at the very end without its source path field.
        let mut raw = raw;
        raw.pop();
        assert!(parse_porcelain_v2(&raw).is_err());
    }

    #[test]
    fn invalid_utf8_is_lossy() {
        let mut raw = format!("1 .M N... 100644 100644 100644 {H40} {H40} caf").into_bytes();
        raw.push(0xE9);
        raw.push(0);
        let st = parse_porcelain_v2(&raw).unwrap();
        assert!(st.entries[0].path.starts_with("caf"));
    }
}
