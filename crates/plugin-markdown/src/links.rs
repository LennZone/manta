//! What a link or image source in a document points to, resolved against
//! the document's folder.

use manta_core::VfsPath;

/// File extensions the preview shows.
pub const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "mdx"];

/// True for `.md`, `.markdown` and `.mdx` (any case).
pub fn is_markdown(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            MARKDOWN_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `http(s)://`, `mailto:` and other URLs: the browser / default app.
    External(String),
    /// `#section` in the same document.
    Anchor(String),
    /// Another Markdown file (and optionally a section in it).
    Markdown {
        path: VfsPath,
        anchor: Option<String>,
    },
    /// Any other file or folder.
    File(VfsPath),
}

/// Percent-decoding (`%20` → space); invalid sequences are kept as they are.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `scheme:` prefix (letters, digits, `+-.`, starting with a letter, at
/// least two characters so `C:` style drive letters don't count).
fn has_scheme(href: &str) -> bool {
    let Some(i) = href.find(':') else {
        return false;
    };
    let s = &href[..i];
    s.len() >= 2
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

/// Resolve a path written in a document (relative to `dir`, or absolute).
pub fn resolve_path(dir: &VfsPath, rel: &str) -> VfsPath {
    let rel = percent_decode(rel);
    dir.join(&rel)
}

/// Classify the link `href` of a document in `dir`.
pub fn classify(href: &str, dir: &VfsPath) -> Target {
    let href = href.trim();
    if let Some(a) = href.strip_prefix('#') {
        return Target::Anchor(percent_decode(a));
    }
    if has_scheme(href) {
        if let Some(p) = href.strip_prefix("file://") {
            return classify(p, &VfsPath::root());
        }
        return Target::External(href.to_string());
    }
    let (path_part, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, Some(percent_decode(a)).filter(|a| !a.is_empty())),
        None => (href, None),
    };
    let path_part = path_part.split('?').next().unwrap_or("");
    if path_part.is_empty() {
        return Target::Anchor(anchor.unwrap_or_default());
    }
    let path = resolve_path(dir, path_part);
    if is_markdown(path.file_name()) {
        Target::Markdown { path, anchor }
    } else {
        Target::File(path)
    }
}

/// Where an image comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageSource {
    /// A file in the document's backend.
    File(VfsPath),
    /// `http(s)://` (loaded only if remote images are switched on).
    Web(String),
    /// `data:` URIs and other schemes: not shown.
    Unsupported(String),
}

pub fn image_source(src: &str, dir: &VfsPath) -> ImageSource {
    let src = src.trim();
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return ImageSource::Web(src.to_string());
    }
    if let Some(p) = src.strip_prefix("file://") {
        return ImageSource::File(VfsPath::new(&percent_decode(p)));
    }
    if has_scheme(src) {
        return ImageSource::Unsupported(src.to_string());
    }
    let path = src.split(['?', '#']).next().unwrap_or("");
    ImageSource::File(resolve_path(dir, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d() -> VfsPath {
        VfsPath::new("/Users/anna/project/docs")
    }

    #[test]
    fn external_and_anchor_links() {
        assert_eq!(
            classify("https://example.com/a#b", &d()),
            Target::External("https://example.com/a#b".into())
        );
        assert_eq!(
            classify("mailto:anna@example.com", &d()),
            Target::External("mailto:anna@example.com".into())
        );
        assert_eq!(
            classify("#getting-started", &d()),
            Target::Anchor("getting-started".into())
        );
        assert_eq!(classify("#caf%C3%A9", &d()), Target::Anchor("café".into()));
    }

    #[test]
    fn relative_markdown_links() {
        assert_eq!(
            classify("guide.md", &d()),
            Target::Markdown {
                path: VfsPath::new("/Users/anna/project/docs/guide.md"),
                anchor: None
            }
        );
        assert_eq!(
            classify("../README.md#install", &d()),
            Target::Markdown {
                path: VfsPath::new("/Users/anna/project/README.md"),
                anchor: Some("install".into())
            }
        );
        assert_eq!(
            classify("./My%20Notes.markdown", &d()),
            Target::Markdown {
                path: VfsPath::new("/Users/anna/project/docs/My Notes.markdown"),
                anchor: None
            }
        );
        assert_eq!(
            classify("/etc/motd.MDX", &d()),
            Target::Markdown {
                path: VfsPath::new("/etc/motd.MDX"),
                anchor: None
            }
        );
        assert_eq!(
            classify("../src/main.rs", &d()),
            Target::File(VfsPath::new("/Users/anna/project/src/main.rs"))
        );
        assert_eq!(
            classify("assets/", &d()),
            Target::File(VfsPath::new("/Users/anna/project/docs/assets"))
        );
    }

    #[test]
    fn image_sources() {
        assert_eq!(
            image_source("img/shot.png", &d()),
            ImageSource::File(VfsPath::new("/Users/anna/project/docs/img/shot.png"))
        );
        assert_eq!(
            image_source("../logo.png?raw=true", &d()),
            ImageSource::File(VfsPath::new("/Users/anna/project/logo.png"))
        );
        assert_eq!(
            image_source("https://img.shields.io/x.svg", &d()),
            ImageSource::Web("https://img.shields.io/x.svg".into())
        );
        assert!(matches!(
            image_source("data:image/png;base64,AAAA", &d()),
            ImageSource::Unsupported(_)
        ));
    }

    #[test]
    fn markdown_extensions() {
        for n in ["README.md", "a.MARKDOWN", "page.mdx"] {
            assert!(is_markdown(n), "{n}");
        }
        for n in ["md", ".md", "notes.txt", "x.mdown.bak"] {
            assert!(!is_markdown(n), "{n}");
        }
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
