//! Drive files as Manta entries: kinds, Google-native export formats, and
//! deterministic, stable names for Drive's duplicate names.
//!
//! Drive allows several files with the same name in one folder and `/` in
//! names. Manta paths need unique names without `/`, so:
//! - `/` in a name is shown as `∕` (U+2215 DIVISION SLASH) and mapped back
//!   when creating or renaming;
//! - Google Docs/Sheets/Slides/Drawings get the extension of their export
//!   format (`Report` -> `Report.docx`);
//! - among files with the same (display) name the oldest one (createdTime,
//!   then id) keeps the name, the next get `name (2).ext`, `name (3).ext`,
//!   ... skipping names that already exist. The mapping only changes when an
//!   older duplicate is removed.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::api::FileMeta;

pub(crate) const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub(crate) const SHORTCUT_MIME: &str = "application/vnd.google-apps.shortcut";
const GOOGLE_APPS_PREFIX: &str = "application/vnd.google-apps.";

/// How a Google-native file is exported for reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportFormat {
    pub google_mime: &'static str,
    pub export_mime: &'static str,
    pub extension: &'static str,
}

/// Google Docs -> .docx, Sheets -> .xlsx, Slides -> .pptx, Drawings -> .pdf.
pub const EXPORT_FORMATS: &[ExportFormat] = &[
    ExportFormat {
        google_mime: "application/vnd.google-apps.document",
        export_mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        extension: "docx",
    },
    ExportFormat {
        google_mime: "application/vnd.google-apps.spreadsheet",
        export_mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        extension: "xlsx",
    },
    ExportFormat {
        google_mime: "application/vnd.google-apps.presentation",
        export_mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        extension: "pptx",
    },
    ExportFormat {
        google_mime: "application/vnd.google-apps.drawing",
        export_mime: "application/pdf",
        extension: "pdf",
    },
];

pub(crate) fn export_format(mime: &str) -> Option<&'static ExportFormat> {
    EXPORT_FORMATS.iter().find(|f| f.google_mime == mime)
}

/// What a node is, as far as Manta is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Folder,
    /// Binary content, downloadable with `alt=media`.
    File,
    /// Google Docs etc.: read-only export.
    Native(&'static ExportFormat),
    /// Forms, Sites, Maps, ...: listed, not readable.
    Unsupported,
}

/// A resolved Drive item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    /// File id (the target for shortcuts; `root` for My Drive; the drive
    /// id for a shared drive's root).
    pub id: String,
    pub kind: NodeKind,
    /// The name on Drive (not the display name).
    pub drive_name: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub created: Option<DateTime<Utc>>,
    /// Shared drive the item lives in.
    pub drive_id: Option<String>,
}

impl Node {
    pub fn folder(id: &str, name: &str, drive_id: Option<String>) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Folder,
            drive_name: name.into(),
            size: 0,
            modified: None,
            created: None,
            drive_id,
        }
    }

    pub fn is_folder(&self) -> bool {
        self.kind == NodeKind::Folder
    }

    pub fn from_meta(m: &FileMeta) -> Node {
        let (id, mime) = match (&m.shortcut_details, m.mime_type.as_str()) {
            (Some(s), SHORTCUT_MIME) if !s.target_id.is_empty() => {
                (s.target_id.clone(), s.target_mime_type.as_str())
            }
            _ => (m.id.clone(), m.mime_type.as_str()),
        };
        let kind = if mime == FOLDER_MIME {
            NodeKind::Folder
        } else if let Some(f) = export_format(mime) {
            NodeKind::Native(f)
        } else if mime.starts_with(GOOGLE_APPS_PREFIX) {
            NodeKind::Unsupported
        } else {
            NodeKind::File
        };
        Node {
            id,
            kind,
            drive_name: m.name.clone(),
            size: m.size.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0),
            modified: m.modified_time.as_deref().and_then(parse_time),
            created: m.created_time.as_deref().and_then(parse_time),
            drive_id: m.drive_id.clone(),
        }
    }
}

pub(crate) fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Drive name -> path component (`/` -> `∕`).
pub(crate) fn display_component(name: &str) -> String {
    let n = name.replace('/', "\u{2215}");
    match n.as_str() {
        "" => "(unnamed)".into(),
        "." | ".." => format!("{n}\u{2024}"),
        _ => n,
    }
}

/// Path component -> Drive name (`∕` -> `/`).
pub(crate) fn drive_component(name: &str) -> String {
    name.replace('\u{2215}', "/")
}

/// The name shown before disambiguation.
pub(crate) fn base_display_name(node: &Node) -> String {
    let name = display_component(&node.drive_name);
    match node.kind {
        NodeKind::Native(f) => {
            let suffix = format!(".{}", f.extension);
            if name.to_ascii_lowercase().ends_with(&suffix) {
                name
            } else {
                name + &suffix
            }
        }
        _ => name,
    }
}

/// The Drive name for a new display name of an existing node (strips the
/// export extension of Google-native files again).
pub(crate) fn drive_name_for(node_kind: &NodeKind, display: &str) -> String {
    let name = drive_component(display);
    if let NodeKind::Native(f) = node_kind {
        let suffix = format!(".{}", f.extension);
        if name.to_ascii_lowercase().ends_with(&suffix) && name.len() > suffix.len() {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name
}

/// `name (n).ext` (the extension stays at the end; folders have none).
fn numbered(name: &str, n: usize, is_folder: bool) -> String {
    match name.rfind('.') {
        Some(i) if !is_folder && i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// Unique, deterministic display names for one folder's children. The
/// result is sorted by display name.
pub(crate) fn disambiguate(nodes: Vec<Node>) -> Vec<(String, Node)> {
    let mut groups: HashMap<String, Vec<Node>> = HashMap::new();
    for n in nodes {
        groups.entry(base_display_name(&n)).or_default().push(n);
    }
    let mut taken: HashSet<String> = groups.keys().cloned().collect();
    let mut keys: Vec<String> = groups.keys().cloned().collect();
    keys.sort();
    let mut out = Vec::new();
    for key in keys {
        let mut group = groups.remove(&key).unwrap_or_default();
        // Oldest first; files without createdTime last; id breaks ties.
        group.sort_by(|a, b| {
            (a.created.is_none(), a.created, &a.id).cmp(&(b.created.is_none(), b.created, &b.id))
        });
        let mut next = 2;
        for (i, node) in group.into_iter().enumerate() {
            if i == 0 {
                out.push((key.clone(), node));
                continue;
            }
            let name = loop {
                let candidate = numbered(&key, next, node.is_folder());
                next += 1;
                if taken.insert(candidate.clone()) {
                    break candidate;
                }
            };
            out.push((name, node));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str, created: &str) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::File,
            drive_name: name.into(),
            size: 1,
            modified: None,
            created: parse_time(created),
            drive_id: None,
        }
    }

    fn names(v: &[(String, Node)]) -> Vec<(&str, &str)> {
        v.iter()
            .map(|(n, node)| (n.as_str(), node.id.as_str()))
            .collect()
    }

    #[test]
    fn duplicates_get_numbers_oldest_keeps_name() {
        let out = disambiguate(vec![
            file("c", "a.txt", "2024-03-01T00:00:00Z"),
            file("a", "a.txt", "2024-01-01T00:00:00Z"),
            file("b", "a.txt", "2024-02-01T00:00:00Z"),
            file("x", "b", "2024-01-01T00:00:00Z"),
        ]);
        assert_eq!(
            names(&out),
            vec![
                ("a (2).txt", "b"),
                ("a (3).txt", "c"),
                ("a.txt", "a"),
                ("b", "x")
            ]
        );
        // Input order doesn't matter.
        let again = disambiguate(vec![
            file("b", "a.txt", "2024-02-01T00:00:00Z"),
            file("x", "b", "2024-01-01T00:00:00Z"),
            file("a", "a.txt", "2024-01-01T00:00:00Z"),
            file("c", "a.txt", "2024-03-01T00:00:00Z"),
        ]);
        assert_eq!(names(&out), names(&again));
    }

    #[test]
    fn numbered_names_skip_existing_ones() {
        let out = disambiguate(vec![
            file("1", "r.pdf", "2024-01-01T00:00:00Z"),
            file("2", "r.pdf", "2024-01-02T00:00:00Z"),
            file("3", "r (2).pdf", "2024-01-03T00:00:00Z"),
        ]);
        assert_eq!(
            names(&out),
            vec![("r (2).pdf", "3"), ("r (3).pdf", "2"), ("r.pdf", "1")]
        );
        let mut d = file("4", "dir.v2", "2024-01-01T00:00:00Z");
        d.kind = NodeKind::Folder;
        let mut d2 = d.clone();
        d2.id = "5".into();
        d2.created = parse_time("2024-01-02T00:00:00Z");
        assert_eq!(
            names(&disambiguate(vec![d, d2])),
            vec![("dir.v2", "4"), ("dir.v2 (2)", "5")]
        );
    }

    #[test]
    fn natives_get_export_extension() {
        let mut doc = file("d", "Report", "2024-01-01T00:00:00Z");
        doc.kind = NodeKind::Native(export_format("application/vnd.google-apps.document").unwrap());
        assert_eq!(base_display_name(&doc), "Report.docx");
        assert_eq!(drive_name_for(&doc.kind, "Final.docx"), "Final");
        assert_eq!(drive_name_for(&NodeKind::File, "Final.docx"), "Final.docx");
        // A Doc next to an uploaded Report.docx: both visible.
        let up = file("u", "Report.docx", "2024-01-02T00:00:00Z");
        assert_eq!(
            names(&disambiguate(vec![up, doc])),
            vec![("Report (2).docx", "u"), ("Report.docx", "d")]
        );
    }

    #[test]
    fn slashes_and_shortcuts() {
        assert_eq!(display_component("a/b"), "a\u{2215}b");
        assert_eq!(drive_component("a\u{2215}b"), "a/b");
        let m = FileMeta {
            id: "s".into(),
            name: "Link".into(),
            mime_type: SHORTCUT_MIME.into(),
            shortcut_details: Some(super::super::api::ShortcutDetails {
                target_id: "t".into(),
                target_mime_type: FOLDER_MIME.into(),
            }),
            ..Default::default()
        };
        let n = Node::from_meta(&m);
        assert_eq!((n.id.as_str(), n.is_folder()), ("t", true));
        let form = FileMeta {
            id: "f".into(),
            mime_type: "application/vnd.google-apps.form".into(),
            ..Default::default()
        };
        assert_eq!(Node::from_meta(&form).kind, NodeKind::Unsupported);
    }
}
