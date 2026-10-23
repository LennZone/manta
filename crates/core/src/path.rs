//! Backend-independent, always `/`-separated absolute paths.

use std::fmt;

use serde::{Deserialize, Serialize};

/// An absolute, normalized, `/`-separated path inside a VFS backend.
/// The root is `/`. Never has a trailing slash (except the root).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VfsPath(String);

impl VfsPath {
    pub fn root() -> Self {
        VfsPath("/".into())
    }

    /// Parse and normalize (`.`/`..`/duplicate slashes). Relative input is
    /// treated as relative to the root.
    pub fn new(s: &str) -> Self {
        let mut parts: Vec<&str> = Vec::new();
        for comp in s.split('/') {
            match comp {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                c => parts.push(c),
            }
        }
        if parts.is_empty() {
            Self::root()
        } else {
            VfsPath(format!("/{}", parts.join("/")))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    pub fn parent(&self) -> Option<VfsPath> {
        if self.is_root() {
            return None;
        }
        let idx = self.0.rfind('/').unwrap_or(0);
        Some(if idx == 0 {
            Self::root()
        } else {
            VfsPath(self.0[..idx].to_string())
        })
    }

    /// Last component, or `""` for the root.
    pub fn file_name(&self) -> &str {
        if self.is_root() {
            ""
        } else {
            &self.0[self.0.rfind('/').map_or(0, |i| i + 1)..]
        }
    }

    pub fn join(&self, name: &str) -> VfsPath {
        if name.starts_with('/') {
            VfsPath::new(name)
        } else {
            VfsPath::new(&format!("{}/{}", self.0, name))
        }
    }

    /// Components from the root, e.g. `/a/b` -> `["a", "b"]`.
    pub fn components(&self) -> Vec<&str> {
        self.0.split('/').filter(|c| !c.is_empty()).collect()
    }

    /// True if `self` is `other` or lies below it.
    pub fn starts_with(&self, other: &VfsPath) -> bool {
        other.is_root() || self.0 == other.0 || self.0.starts_with(&format!("{}/", other.0))
    }

    /// Path relative to `base`, without leading slash. `None` if not below it.
    pub fn strip_prefix(&self, base: &VfsPath) -> Option<String> {
        if !self.starts_with(base) {
            return None;
        }
        let rest = if base.is_root() {
            &self.0[1..]
        } else {
            &self.0[base.0.len()..]
        };
        Some(rest.trim_start_matches('/').to_string())
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for VfsPath {
    fn from(s: &str) -> Self {
        VfsPath::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        assert_eq!(VfsPath::new("a//b/./c/../d/").as_str(), "/a/b/d");
        assert_eq!(VfsPath::new("/..").as_str(), "/");
        assert_eq!(VfsPath::new("").as_str(), "/");
    }

    #[test]
    fn parent_and_name() {
        let p = VfsPath::new("/a/b");
        assert_eq!(p.parent().unwrap().as_str(), "/a");
        assert_eq!(p.parent().unwrap().parent().unwrap().as_str(), "/");
        assert_eq!(VfsPath::root().parent(), None);
        assert_eq!(p.file_name(), "b");
    }

    #[test]
    fn join_and_strip() {
        let base = VfsPath::new("/a");
        assert_eq!(base.join("b/c").as_str(), "/a/b/c");
        assert_eq!(VfsPath::new("/a/b/c").strip_prefix(&base).unwrap(), "b/c");
        assert_eq!(VfsPath::new("/ab").strip_prefix(&base), None);
        assert_eq!(
            VfsPath::new("/a/b").strip_prefix(&VfsPath::root()).unwrap(),
            "a/b"
        );
    }
}
