//! iCloud Drive as Finder shows it: the top folder lists, besides the files
//! in `com~apple~CloudDocs`, the apps' folders ("App Library"): "Obsidian"
//! is `~/Library/Mobile Documents/iCloud~md~obsidian/Documents`.
//!
//! Which apps show is iCloud's decision (the app made its documents public
//! and has some). There is no public API for it; `brctl dump -i` lists the
//! app libraries with their flags and obfuscated ids (`i{4}d.md.o{6}n` for
//! `iCloud.md.obsidian`), which are matched against the folder names. If
//! that fails, no app folders show (the top folder is as before). The names
//! are the ones Finder uses (`NSFileManager displayNameAtPath:`).
//!
//! App folders can't be renamed, moved or deleted, like in Finder
//! ([`is_app_library`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// iCloud Drive's own container.
pub const CLOUD_DOCS: &str = "com~apple~CloudDocs";
/// `~/Library/Mobile Documents`, relative to the home folder.
pub const MOBILE_DOCUMENTS: &str = "Library/Mobile Documents";

/// One app's folder in iCloud Drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLibrary {
    /// "Obsidian".
    pub name: String,
    /// `…/Mobile Documents/iCloud~md~obsidian/Documents`.
    pub docs: PathBuf,
}

/// `~/Library/Mobile Documents` of `home`.
pub fn mobile_documents(home: &Path) -> PathBuf {
    home.join(MOBILE_DOCUMENTS)
}

/// The `Documents` folder of an app's container that `path` is in (or
/// is), with the container's folder name: `(…/iCloud~md~obsidian/Documents,
/// "iCloud~md~obsidian")`. Not for iCloud Drive's own container.
pub fn app_library_of(path: &Path) -> Option<(PathBuf, String)> {
    let home = crate::cloud_local::cloud_home()?;
    app_library_in(&mobile_documents(&home), path)
}

/// [`app_library_of`] for another `Mobile Documents` folder (tests).
pub fn app_library_in(mobile: &Path, path: &Path) -> Option<(PathBuf, String)> {
    let rel = path.strip_prefix(mobile).ok()?;
    let mut comps = rel.components();
    let container = comps.next()?.as_os_str().to_str()?.to_string();
    if container == CLOUD_DOCS || container.starts_with('.') {
        return None;
    }
    (comps.next()?.as_os_str() == "Documents")
        .then(|| (mobile.join(&container).join("Documents"), container))
}

/// An app's folder itself (not something inside it): it can't be renamed,
/// moved or deleted.
pub fn is_app_library(path: &Path) -> bool {
    app_library_of(path).is_some_and(|(docs, _)| docs == path)
}

/// The name Finder shows for an app's container ("Obsidian"), else the
/// last part of its id (`iCloud~md~obsidian` -> "obsidian").
/// Remembered: the path bar and tab titles ask every frame.
pub fn display_name(container_dir: &Path) -> String {
    static NAMES: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = names.get_or_insert_with(HashMap::new);
    if let Some(n) = names.get(container_dir) {
        return n.clone();
    }
    #[cfg(target_os = "macos")]
    let native = native::display_name(container_dir);
    #[cfg(not(target_os = "macos"))]
    let native: Option<String> = None;
    let name = native.unwrap_or_else(|| {
        let id = container_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        id.rsplit('~').next().unwrap_or(&id).to_string()
    });
    names.insert(container_dir.to_path_buf(), name.clone());
    name
}

/// The name a folder in iCloud Drive is shown under, like in Finder:
/// "iCloud Drive" for the top folder, "Obsidian" for an app's folder.
pub fn folder_label(path: &Path) -> Option<String> {
    let home = crate::cloud_local::cloud_home()?;
    let mobile = mobile_documents(&home);
    if path == mobile.join(CLOUD_DOCS) {
        return Some("iCloud Drive".into());
    }
    match app_library_in(&mobile, path) {
        Some((docs, container)) if docs == path => Some(display_name(&mobile.join(container))),
        _ => None,
    }
}

/// Where the path bar starts inside iCloud Drive: after the top folder
/// (the location chip says "iCloud Drive"), or at an app's folder (shown
/// under its name). `None` outside iCloud Drive.
pub fn crumb_base(path: &Path) -> Option<PathBuf> {
    let home = crate::cloud_local::cloud_home()?;
    let mobile = mobile_documents(&home);
    let top = mobile.join(CLOUD_DOCS);
    if path.starts_with(&top) {
        return Some(top);
    }
    app_library_in(&mobile, path).map(|(_, container)| mobile.join(container))
}

/// How long the list of app folders is reused.
const TTL: Duration = Duration::from_secs(30);

type Cached = (Instant, Vec<AppLibrary>);
static CACHE: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);

/// The app folders iCloud Drive's top folder shows, sorted by name
/// (cached for a while). With a fake home
/// ([`crate::cloud_local::HOME_OVERRIDE_ENV`]): every container with
/// something in `Documents`.
pub fn app_libraries() -> Vec<AppLibrary> {
    let Some(home) = crate::cloud_local::cloud_home() else {
        return Vec::new();
    };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some((at, libs)) = map.get(&home) {
        if at.elapsed() < TTL {
            return libs.clone();
        }
    }
    let fake =
        std::env::var_os(crate::cloud_local::HOME_OVERRIDE_ENV).is_some_and(|v| !v.is_empty());
    let libs = if fake {
        app_libraries_in(&mobile_documents(&home), |dir| {
            std::fs::read_dir(dir.join("Documents"))
                .map(|mut r| r.next().is_some())
                .unwrap_or(false)
        })
    } else {
        match public_ids() {
            Some(ids) => app_libraries_in(&mobile_documents(&home), |dir| {
                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                ids.iter().any(|p| id_matches(p, name))
            }),
            None => Vec::new(),
        }
    };
    map.insert(home, (Instant::now(), libs.clone()));
    libs
}

/// The containers in `mobile` that `shown` accepts and that have a
/// `Documents` folder, as app folders.
pub fn app_libraries_in(mobile: &Path, shown: impl Fn(&Path) -> bool) -> Vec<AppLibrary> {
    let mut out: Vec<AppLibrary> = std::fs::read_dir(mobile)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == CLOUD_DOCS || name.starts_with('.') {
                return None;
            }
            let dir = e.path();
            let docs = dir.join("Documents");
            (docs.is_dir() && shown(&dir)).then(|| AppLibrary {
                name: display_name(&dir),
                docs,
            })
        })
        .collect();
    out.sort_by_key(|l| l.name.to_lowercase());
    out
}

/// The obfuscated ids of the app libraries iCloud shows in iCloud Drive
/// (`brctl dump -i`: public and with documents).
fn public_ids() -> Option<Vec<String>> {
    let out = std::process::Command::new("/usr/bin/brctl")
        .args(["dump", "-i"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(parse_dump(&String::from_utf8_lossy(&out.stdout)))
}

/// The ids of `+ app library: <id[n] … DocumentScopePublic …>` lines
/// without the `no-documents` state.
pub fn parse_dump(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim_start().strip_prefix("+ app library: <")?;
            if !l.contains("DocumentScopePublic") || l.contains("no-documents") {
                return None;
            }
            let id = rest.split(['[', ' ']).next()?;
            (!id.is_empty()).then(|| id.to_string())
        })
        .collect()
}

/// Does the obfuscated `pattern` (`i{4}d.md.o{6}n`) stand for the
/// container folder `folder` (`iCloud~md~obsidian`)? Each part is either
/// written out or "first letter, {hidden count}, last letter".
pub fn id_matches(pattern: &str, folder: &str) -> bool {
    let parts: Vec<&str> = pattern.split('.').collect();
    let names: Vec<&str> = folder.split('~').collect();
    parts.len() == names.len() && parts.iter().zip(&names).all(|(p, n)| part_matches(p, n))
}

fn part_matches(pattern: &str, name: &str) -> bool {
    let Some((first, rest)) = pattern.split_once('{') else {
        return pattern == name;
    };
    let Some((count, last)) = rest.split_once('}') else {
        return pattern == name;
    };
    let Ok(count) = count.parse::<usize>() else {
        return pattern == name;
    };
    name.chars().count() == count + first.chars().count() + last.chars().count()
        && name.starts_with(first)
        && name.ends_with(last)
}

#[cfg(target_os = "macos")]
mod native {
    use std::path::Path;

    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSFileManager, NSString};

    pub fn display_name(path: &Path) -> Option<String> {
        autoreleasepool(|_| {
            let p = NSString::from_str(path.to_str()?);
            let name = NSFileManager::defaultManager().displayNameAtPath(&p);
            let name = name.to_string().trim_matches('\u{200e}').to_string();
            // Without a name of its own, Foundation returns the folder name.
            (!name.is_empty() && Some(name.as_str()) != path.file_name()?.to_str()).then_some(name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscated_ids() {
        assert!(id_matches("i{4}d.md.o{6}n", "iCloud~md~obsidian"));
        assert!(id_matches(
            "i{4}d.is.w{6}w.my.w{7}s",
            "iCloud~is~workflow~my~workflows"
        ));
        assert!(id_matches("c{1}m.a{3}e.N{5}s", "com~apple~Numbers"));
        assert!(id_matches(
            "F{8}7.c{1}m.a{3}e.m{14}d",
            "F3LWYJ7GM7~com~apple~mobilegarageband"
        ));
        assert!(!id_matches("c{1}m.a{3}e.N{5}s", "com~apple~Pages"));
        assert!(!id_matches("c{1}m.a{3}e.P{3}s", "com~apple~Pages~extra"));
        assert!(!id_matches("i{4}d.md.o{6}n", "iCloud~md~obsidia"));
    }

    #[test]
    fn dump_lines() {
        let text = "\
    + app library: <i{4}d.md.o{6}n[32] \x1b[0;1;30mNA\x1b[0m {s:root-created|l-docs} apps:{md.o{6}n:387} DocumentScopePublic|InCloudDocsZone>
    + app library: <c{1}m.a{3}e.K{5}e[48] NA {s:no-documents|root-created} DocumentScopePublic|InCloudDocsZone>
    + app library: <i{4}d.c{1}m.8{3}t.b{7}n[9] NA {s:root-created} InCloudDocsZone>
    other line";
        assert_eq!(parse_dump(text), vec!["i{4}d.md.o{6}n".to_string()]);
    }

    #[test]
    fn app_folders_and_their_contents() {
        let mobile = Path::new("/Users/a/Library/Mobile Documents");
        let docs = mobile.join("iCloud~md~obsidian/Documents");
        assert_eq!(
            app_library_in(mobile, &docs.join("Vault/note.md")),
            Some((docs.clone(), "iCloud~md~obsidian".into()))
        );
        assert_eq!(
            app_library_in(mobile, &docs),
            Some((docs, "iCloud~md~obsidian".into()))
        );
        assert_eq!(
            app_library_in(mobile, &mobile.join("iCloud~md~obsidian")),
            None
        );
        assert_eq!(
            app_library_in(mobile, &mobile.join("com~apple~CloudDocs/Documents")),
            None
        );
        assert_eq!(app_library_in(mobile, Path::new("/tmp/x")), None);
    }

    #[test]
    fn lists_the_shown_containers_with_documents() {
        let tmp = tempfile::tempdir().unwrap();
        let mobile = tmp.path();
        for c in [
            "iCloud~md~obsidian",
            "com~apple~Numbers",
            "iCloud~x~hidden",
            CLOUD_DOCS,
        ] {
            std::fs::create_dir_all(mobile.join(c).join("Documents")).unwrap();
        }
        std::fs::create_dir_all(mobile.join("com~apple~notes")).unwrap();
        let libs = app_libraries_in(mobile, |d| !d.ends_with("iCloud~x~hidden"));
        let docs: Vec<PathBuf> = libs.iter().map(|l| l.docs.clone()).collect();
        assert_eq!(docs.len(), 2, "{libs:?}");
        assert!(docs.contains(&mobile.join("iCloud~md~obsidian/Documents")));
        assert!(docs.contains(&mobile.join("com~apple~Numbers/Documents")));
    }
}
