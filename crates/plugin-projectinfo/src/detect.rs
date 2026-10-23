//! Project detection, ported from the Python plugin's `pi_core.py`.
//!
//! [`find_projects`]`(folder, stop, home)` → projects, nearest first. A
//! project is a folder with at least one manifest (package.json,
//! composer.json, artisan, Cargo.toml, pyproject.toml, Makefile, justfile,
//! Taskfile.yml, docker-compose.yml …). Its sections hold the runnable
//! scripts, grouped by tool. Detection only walks *up* from the folder (and
//! one level into declared workspace folders), never into node_modules or
//! vendor, and caches parsed files by (mtime, size), so it stays fast in
//! monorepos. It reads files: call it on a background thread.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use manta_plugin::Tone;
use regex::Regex;

use crate::i18n::tr;
use crate::parsers;
use crate::value::{self, V};

pub const NODE_LOCKS: &[(&str, &str)] = &[
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("bun.lockb", "bun"),
    ("bun.lock", "bun"),
    ("package-lock.json", "npm"),
    ("npm-shrinkwrap.json", "npm"),
];
pub const MAKEFILES: &[&str] = &["GNUmakefile", "makefile", "Makefile"];
pub const JUSTFILES: &[&str] = &["justfile", "Justfile", ".justfile"];
pub const TASKFILES: &[&str] = &[
    "Taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yml",
    "taskfile.yaml",
];
pub const COMPOSE_FILES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];
const OTHER_MANIFESTS: &[&str] = &[
    "package.json",
    "composer.json",
    "artisan",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
];
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    ".git",
    "target",
    ".venv",
    "venv",
    "__pycache__",
];
const VENV_DIRS: &[&str] = &[".venv", "venv", "env"];

/// Every file name that makes a folder a project.
pub fn manifests() -> impl Iterator<Item = &'static str> {
    OTHER_MANIFESTS
        .iter()
        .chain(MAKEFILES)
        .chain(JUSTFILES)
        .chain(TASKFILES)
        .chain(COMPOSE_FILES)
        .copied()
}

/// Composer runs these itself on events; they aren't meant to be run by hand.
const COMPOSER_EVENTS: &[&str] = &[
    "pre-install-cmd",
    "post-install-cmd",
    "pre-update-cmd",
    "post-update-cmd",
    "pre-status-cmd",
    "post-status-cmd",
    "pre-archive-cmd",
    "post-archive-cmd",
    "pre-autoload-dump",
    "post-autoload-dump",
    "post-root-package-install",
    "post-create-project-cmd",
    "pre-operations-exec",
    "pre-pool-create",
    "pre-dependencies-solving",
    "post-dependencies-solving",
    "pre-package-install",
    "post-package-install",
    "pre-package-update",
    "post-package-update",
    "pre-package-uninstall",
    "post-package-uninstall",
    "init",
    "command",
    "pre-file-download",
    "post-file-download",
    "pre-command-run",
];

/// kind → (label, icon, tone) for tags and the chip.
pub fn kind_style(kind: &str) -> (&'static str, &'static str, Tone) {
    match kind {
        "laravel" => ("Laravel", "rocket", Tone::Danger),
        "composer" => ("Composer", "package", Tone::Info),
        "node" => ("npm", "cube", Tone::Success),
        "compose" => ("Compose", "stack", Tone::Accent),
        "cargo" => ("Cargo", "wrench", Tone::Warning),
        "python" => ("Python", "flask", Tone::Info),
        "make" => ("Make", "hammer", Tone::Neutral),
        "just" => ("just", "hammer", Tone::Neutral),
        "task" => ("Task", "hammer", Tone::Neutral),
        _ => ("", "folder", Tone::Neutral),
    }
}

/// Which kind names the project in the chip (first match wins).
const PRIMARY_ORDER: &[&str] = &[
    "laravel", "node", "composer", "cargo", "python", "compose", "make", "just", "task",
];

// ---------------------------------------------------------------- model

#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// "section/name", unique within the project.
    pub key: String,
    pub section: String,
    pub name: String,
    /// Shell text typed into the terminal.
    pub command: String,
    /// Short preview shown in the row.
    pub detail: String,
    pub desc: Option<String>,
    pub icon: String,
    /// Why it asks first (translated), if it is destructive.
    pub danger: Option<String>,
    pub needs_args: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub name: String,
    pub path: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub detail: Option<String>,
    pub scripts: Vec<Script>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub root: String,
    /// The folder name: what people call the project.
    pub name: String,
    pub names: Vec<Option<String>>,
    pub kinds: Vec<String>,
    pub sections: Vec<Section>,
    /// (kind, text)
    pub tags: Vec<(String, String)>,
    /// (label, value)
    pub info: Vec<(String, String)>,
    /// Files whose changes mean detecting again.
    pub watch: Vec<String>,
    pub problems: Vec<String>,
    pub env_file: Option<String>,
    /// The root has a `.env` (the panel offers to open it).
    pub has_env: bool,
    pub pm: Option<String>,
    pub count: usize,
    pub primary: Option<String>,
    pub primary_label: String,
    pub primary_icon: String,
    pub primary_tone: Tone,
    pub git_branch: Option<String>,
    pub git_root: Option<String>,
}

impl Project {
    fn new(root: &str) -> Project {
        Project {
            root: root.to_string(),
            name: String::new(),
            names: Vec::new(),
            kinds: Vec::new(),
            sections: Vec::new(),
            tags: Vec::new(),
            info: Vec::new(),
            watch: Vec::new(),
            problems: Vec::new(),
            env_file: None,
            has_env: false,
            pm: None,
            count: 0,
            primary: None,
            primary_label: String::new(),
            primary_icon: String::new(),
            primary_tone: Tone::Neutral,
            git_branch: None,
            git_root: None,
        }
    }

    pub fn all_scripts(&self) -> impl Iterator<Item = (&Section, &Script)> {
        self.sections
            .iter()
            .flat_map(|s| s.scripts.iter().map(move |sc| (s, sc)))
    }

    pub fn find_script(&self, key: &str) -> Option<&Script> {
        self.all_scripts().map(|(_, s)| s).find(|s| s.key == key)
    }

    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }
}

fn new_section(id: &str, title: &str, kind: &str, detail: Option<String>) -> Section {
    Section {
        id: id.into(),
        title: title.into(),
        kind: kind.into(),
        detail,
        scripts: Vec::new(),
        links: Vec::new(),
    }
}

fn script(
    section: &str,
    name: &str,
    command: String,
    detail: Option<String>,
    desc: Option<String>,
    icon: &str,
) -> Script {
    script_args(section, name, command, detail, desc, icon, false)
}

fn script_args(
    section: &str,
    name: &str,
    command: String,
    detail: Option<String>,
    desc: Option<String>,
    icon: &str,
    needs_args: bool,
) -> Script {
    let danger = destructive(
        name,
        &format!("{command} {}", detail.as_deref().unwrap_or("")),
    );
    Script {
        key: format!("{section}/{name}"),
        section: section.into(),
        name: name.into(),
        detail: detail.unwrap_or_else(|| command.clone()),
        command,
        desc,
        icon: icon.into(),
        danger,
        needs_args,
    }
}

// ---------------------------------------------------------------- paths

/// Python's `os.path.join(a, b)` for a relative `b`.
pub fn pj(a: &str, b: &str) -> String {
    if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// Python's `os.path.dirname`.
pub fn dirname(p: &str) -> String {
    let head = match p.rfind('/') {
        Some(i) => &p[..=i],
        None => "",
    };
    if !head.is_empty() && head.bytes().any(|b| b != b'/') {
        head.trim_end_matches('/').to_string()
    } else {
        head.to_string()
    }
}

/// Python's `os.path.basename`.
pub fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// Python's `os.path.normpath` (POSIX).
pub fn normpath(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let abs = p.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else if !abs {
                    parts.push("..");
                }
            }
            c => parts.push(c),
        }
    }
    let body = parts.join("/");
    match (abs, body.is_empty()) {
        (true, _) => format!("/{body}"),
        (false, true) => ".".into(),
        (false, false) => body,
    }
}

/// Python's `os.path.relpath(path, start)` for absolute paths.
pub fn relpath(path: &str, start: &str) -> String {
    let p = normpath(path);
    let s = normpath(start);
    let pv: Vec<&str> = p.split('/').filter(|x| !x.is_empty()).collect();
    let sv: Vec<&str> = s.split('/').filter(|x| !x.is_empty()).collect();
    let common = pv.iter().zip(&sv).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = vec![".."; sv.len() - common];
    out.extend(&pv[common..]);
    if out.is_empty() {
        ".".into()
    } else {
        out.join("/")
    }
}

fn is_file(p: &str) -> bool {
    Path::new(p).is_file()
}

fn is_dir(p: &str) -> bool {
    Path::new(p).is_dir()
}

fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

/// Python's `shlex.quote`.
pub fn q(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Inside node_modules or vendor (a dependency, not a project)?
pub fn is_skipped(path: &str) -> bool {
    path.split('/')
        .any(|p| p == "node_modules" || p == "vendor")
}

/// The first of `names` that exists in `folder`, with its real spelling
/// (macOS file systems ignore case, so a plain check for "makefile" would
/// find "Makefile").
pub fn has_any(folder: &str, names: &[&str]) -> Option<String> {
    let entries: HashSet<String> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names
        .iter()
        .map(|n| pj(folder, n))
        .zip(names)
        .find(|(p, n)| entries.contains(**n) && is_file(p))
        .map(|(p, _)| p)
}

/// `~` for the home folder; long paths keep their last two folders.
pub fn short_path(p: &str, home: &str, limit: usize) -> String {
    let mut p = p.to_string();
    if p == home {
        return "~".into();
    }
    let h = home.trim_end_matches('/');
    if !home.is_empty() {
        if let Some(rest) = p.strip_prefix(&format!("{h}/")) {
            p = format!("~/{rest}");
        }
    }
    if p.chars().count() > limit {
        let parts: Vec<&str> = p.trim_end_matches('/').split('/').collect();
        p = format!("…/{}", parts[parts.len().saturating_sub(2)..].join("/"));
    }
    p
}

/// Python's `fnmatch.fnmatch` (case-sensitive): `*`, `?`, `[seq]`, `[!seq]`.
pub fn fnmatch(name: &str, pat: &str) -> bool {
    fn m(n: &[char], p: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| m(&n[i..], &p[1..])),
            Some('?') => !n.is_empty() && m(&n[1..], &p[1..]),
            Some('[') => {
                let mut j = 1;
                let neg = p.get(1) == Some(&'!');
                if neg {
                    j += 1;
                }
                if p.get(j) == Some(&']') {
                    j += 1;
                }
                while j < p.len() && p[j] != ']' {
                    j += 1;
                }
                if j >= p.len() {
                    // No closing bracket: a literal "[".
                    return n.first() == Some(&'[') && m(&n[1..], &p[1..]);
                }
                let Some(&c) = n.first() else { return false };
                let set = &p[if neg { 2 } else { 1 }..j];
                let mut hit = false;
                let mut k = 0;
                while k < set.len() {
                    if k + 2 < set.len() && set[k + 1] == '-' {
                        hit |= set[k] <= c && c <= set[k + 2];
                        k += 3;
                    } else {
                        hit |= set[k] == c;
                        k += 1;
                    }
                }
                hit != neg && m(&n[1..], &p[j + 1..])
            }
            Some(&c) => n.first() == Some(&c) && m(&n[1..], &p[1..]),
        }
    }
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pat.chars().collect();
    m(&n, &p)
}

// ---------------------------------------------------------------- caching

type Sig = (i128, u64);

fn stat(path: &str) -> Option<Sig> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0);
    Some((mtime, md.len()))
}

type Entry = (Sig, Arc<dyn Any + Send + Sync>);

fn cache() -> &'static Mutex<HashMap<(String, &'static str), Entry>> {
    static C: OnceLock<Mutex<HashMap<(String, &'static str), Entry>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Why a file couldn't be used: missing (or too large), or its parse error.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadErr {
    Missing,
    Bad(String),
}

impl ReadErr {
    fn problem(&self) -> String {
        match self {
            ReadErr::Bad(e) => e.clone(),
            ReadErr::Missing => tr!("problem.unreadable").to_string(),
        }
    }
}

/// `parse(text)` of a file, cached by (mtime, size).
fn read_cached<T: Send + Sync + 'static>(
    path: &str,
    kind: &'static str,
    limit: u64,
    parse: fn(&str) -> Result<T, String>,
) -> Result<Arc<T>, ReadErr> {
    let Some(sig) = stat(path) else {
        return Err(ReadErr::Missing);
    };
    let key = (path.to_string(), kind);
    if let Some((s, v)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        if *s == sig {
            if let Ok(r) = v.clone().downcast::<Result<Arc<T>, ReadErr>>() {
                return (*r).clone();
            }
        }
    }
    let value: Result<Arc<T>, ReadErr> = if sig.1 > limit {
        Err(ReadErr::Missing)
    } else {
        match std::fs::read(path) {
            Ok(bytes) => parse(&String::from_utf8_lossy(&bytes))
                .map(Arc::new)
                .map_err(ReadErr::Bad),
            Err(e) => Err(ReadErr::Bad(e.to_string())),
        }
    };
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.len() > 500 {
        c.clear();
    }
    c.insert(key, (sig, Arc::new(value.clone())));
    value
}

const MB: u64 = 1024 * 1024;

fn read_json(path: &str) -> Result<Arc<V>, ReadErr> {
    // A JSON file that isn't an object counts as `{}`.
    read_cached(path, "json", 4 * MB, |t| {
        value::json(t).map(|v| if v.is_map() { v } else { V::Map(Vec::new()) })
    })
}

fn read_text(path: &str) -> Option<Arc<String>> {
    read_cached(path, "text", 4 * MB, |t| Ok(t.to_string())).ok()
}

fn read_toml(path: &str) -> Result<Arc<V>, ReadErr> {
    read_cached(path, "toml", 4 * MB, value::toml)
}

fn read_yaml(path: &str) -> Result<Arc<V>, ReadErr> {
    read_cached(path, "yaml", 4 * MB, parsers::parse_yaml)
}

/// {package: version} from composer.lock (only what is shown).
fn composer_lock_versions(text: &str) -> Result<Vec<(String, String)>, String> {
    let data = value::json(text)?;
    let mut out = Vec::new();
    for p in data.get("packages").and_then(V::list).unwrap_or(&[]) {
        if let Some(name) = p
            .get_str("name")
            .filter(|n| ["laravel/framework", "laravel/sail", "laravel/octane"].contains(n))
        {
            let v = p.get("version").map(V::text).unwrap_or_default();
            out.push((name.to_string(), v.trim_start_matches('v').to_string()));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- destructive

const DANGER_WORDS: &[&str] = &[
    "clean",
    "clobber",
    "reset",
    "wipe",
    "purge",
    "destroy",
    "drop",
    "nuke",
    "prune",
    "fresh",
    "deploy",
    "publish",
    "release",
    "erase",
    "uninstall",
];

fn danger_commands() -> &'static [(Regex, &'static str)] {
    static R: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    R.get_or_init(|| {
        [
            (r"\bmigrate:(fresh|reset|refresh)\b", "danger.tables"),
            (r"\bdb:wipe\b", "danger.wipe"),
            (r"\bdown\b[^&|;]*(\s-v\b|--volumes|--rmi)", "danger.volumes"),
            (r"\brm\s+-[A-Za-z]*(r[A-Za-z]*f|f[A-Za-z]*r)", "danger.rm"),
            (
                r"\bgit\s+(clean\b|reset\s+--hard|push\s+[^&|;]*(--force|-f\b))",
                "danger.git",
            ),
            (
                r"\b(system|volume|image|container)\s+prune\b|\bvolume\s+rm\b",
                "danger.docker",
            ),
            (r"\b(dropdb|dropuser)\b", "danger.dropdb"),
            (
                r"\b(npm|pnpm|yarn|cargo|twine|poetry)\s+publish\b",
                "danger.publish",
            ),
        ]
        .into_iter()
        .map(|(p, k)| (Regex::new(p).expect("valid regex"), k))
        .collect()
    })
}

/// The translated reason for a danger key.
fn danger_text(key: &str) -> String {
    match key {
        "danger.tables" => tr!("danger.tables"),
        "danger.wipe" => tr!("danger.wipe"),
        "danger.volumes" => tr!("danger.volumes"),
        "danger.rm" => tr!("danger.rm"),
        "danger.git" => tr!("danger.git"),
        "danger.docker" => tr!("danger.docker"),
        "danger.dropdb" => tr!("danger.dropdb"),
        _ => tr!("danger.publish"),
    }
    .to_string()
}

/// Why `name` / `command` should be confirmed first, or `None`.
pub fn destructive(name: &str, command: &str) -> Option<String> {
    for (rx, why) in danger_commands() {
        if rx.is_match(command) {
            return Some(danger_text(why));
        }
    }
    let lower = name.to_lowercase();
    let mut hit: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_lowercase())
        .filter(|w| !w.is_empty() && DANGER_WORDS.contains(w))
        .collect();
    hit.sort();
    let w = *hit.first()?;
    Some(match w {
        "deploy" => tr!("danger.deploy").to_string(),
        "publish" | "release" => tr!("danger.publish").to_string(),
        _ => tr!("danger.word", name = name),
    })
}

// ---------------------------------------------------------------- detectors

fn node_package_manager(pkg: &V, chain: &[String]) -> String {
    if let Some(pm) = pkg.get_str("packageManager").filter(|s| !s.is_empty()) {
        let name = pm.split('@').next().unwrap_or("");
        if ["npm", "pnpm", "yarn", "bun"].contains(&name) {
            return name.to_string();
        }
    }
    for d in chain {
        for (f, name) in NODE_LOCKS {
            if is_file(&pj(d, f)) {
                return name.to_string();
            }
        }
    }
    "npm".into()
}

fn node_version(pkg: &V, chain: &[String]) -> Option<String> {
    for d in chain {
        for f in [".nvmrc", ".node-version"] {
            if let Some(v) = read_text(&pj(d, f)) {
                if !v.trim().is_empty() {
                    return v.trim().lines().next().map(|l| l.trim().to_string());
                }
            }
        }
    }
    pkg.get_map("engines")
        .and_then(|e| e.get_str("node"))
        .map(str::to_string)
}

/// Folders below `root` matching workspace globs (one `*` level per
/// segment, no `**` descent) that contain `marker`. Never enters
/// node_modules/vendor.
pub fn expand_globs(root: &str, patterns: &[V], marker: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pat in patterns {
        let Some(pat) = pat.str() else { continue };
        if pat.starts_with('!') {
            continue;
        }
        let pat = pat.trim_matches('/').replace("/**", "/*");
        let segs: Vec<&str> = pat
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect();
        let mut cur = vec![root.to_string()];
        for seg in segs {
            let mut next = Vec::new();
            for d in &cur {
                if seg.contains(['*', '?', '[']) {
                    let Ok(rd) = std::fs::read_dir(d) else {
                        continue;
                    };
                    let mut names: Vec<String> = rd
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect();
                    names.sort();
                    for n in names {
                        if SKIP_DIRS.contains(&n.as_str()) || n.starts_with('.') {
                            continue;
                        }
                        let p = pj(d, &n);
                        if fnmatch(&n, seg) && is_dir(&p) {
                            next.push(p);
                        }
                    }
                } else {
                    let p = pj(d, seg);
                    if is_dir(&p) {
                        next.push(p);
                    }
                }
            }
            next.truncate(500);
            cur = next;
        }
        for d in cur {
            if !out.contains(&d) && is_file(&pj(&d, marker)) {
                out.push(d);
            }
        }
    }
    out.truncate(200);
    out
}

fn detect_node(p: &mut Project, chain: &[String]) {
    let path = pj(&p.root, "package.json");
    let pkg = match read_json(&path) {
        Ok(v) => v,
        Err(e) => {
            if is_file(&path) {
                p.problems.push(format!("package.json: {}", e.problem()));
            }
            return;
        }
    };
    p.watch.push(path);
    let pm = node_package_manager(&pkg, chain);
    for d in chain {
        for (f, _) in NODE_LOCKS {
            p.watch.push(pj(d, f));
        }
    }
    p.names.push(pkg.get_str("name").map(str::to_string));
    p.pm = Some(pm.clone());
    let mut sec = new_section("node", &pm, "node", Some("package.json".into()));
    for (name, body) in pkg.get_map("scripts").and_then(V::map).unwrap_or(&[]) {
        let Some(body) = body.str() else { continue };
        sec.scripts.push(script(
            "node",
            name,
            format!("{pm} run {}", q(name)),
            Some(body.to_string()),
            Some(format!("{pm} run {name}\n{body}")),
            "play",
        ));
    }
    sec.scripts.push(script(
        "node",
        "install",
        format!("{pm} install"),
        None,
        Some(tr!("desc.install_deps").into()),
        "download",
    ));
    let mut tag = pm.clone();
    if let Some(ver) = node_version(&pkg, chain) {
        tag = format!("{pm} · Node {}", ver.trim_start_matches('v'));
        p.info.push(("Node".into(), ver));
    }
    p.tags.push(("node".into(), tag));
    for d in chain {
        for f in [".nvmrc", ".node-version"] {
            p.watch.push(pj(d, f));
        }
    }
    // Workspaces: package.json "workspaces" or pnpm-workspace.yaml.
    let ws = match pkg.get("workspaces") {
        Some(m @ V::Map(_)) => m.get("packages"),
        other => other,
    };
    let mut patterns: Vec<V> = ws.and_then(V::list).map(<[V]>::to_vec).unwrap_or_default();
    let pnpm_ws = pj(&p.root, "pnpm-workspace.yaml");
    p.watch.push(pnpm_ws.clone());
    if let Ok(doc) = read_yaml(&pnpm_ws) {
        patterns.extend(parsers::yaml_list(&doc, "packages").into_iter().map(V::Str));
    }
    if !patterns.is_empty() {
        for m in expand_globs(&p.root, &patterns, "package.json") {
            let name = read_json(&pj(&m, "package.json"))
                .ok()
                .and_then(|mp| mp.get_str("name").map(str::to_string))
                .unwrap_or_else(|| basename(&m).to_string());
            let detail = relpath(&m, &p.root);
            sec.links.push(Link {
                name,
                path: m,
                detail,
            });
        }
    }
    p.sections.push(sec);
    p.kinds.push("node".into());
}

/// (name, command, description, icon)
fn laravel_commands() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        ("serve", "php artisan serve", tr!("desc.serve"), "globe"),
        (
            "migrate",
            "php artisan migrate",
            tr!("desc.migrate"),
            "database",
        ),
        (
            "migrate:fresh --seed",
            "php artisan migrate:fresh --seed",
            tr!("desc.migrate_fresh"),
            "database",
        ),
        ("test", "php artisan test", tr!("desc.tests"), "check"),
        (
            "tinker",
            "php artisan tinker",
            tr!("desc.tinker"),
            "terminal",
        ),
        (
            "route:list",
            "php artisan route:list",
            tr!("desc.routes"),
            "list",
        ),
        (
            "optimize:clear",
            "php artisan optimize:clear",
            tr!("desc.optimize_clear"),
            "refresh",
        ),
        (
            "queue:work",
            "php artisan queue:work",
            tr!("desc.queue"),
            "lightning",
        ),
    ]
}

fn detect_php(p: &mut Project, _chain: &[String]) {
    let root = p.root.clone();
    let path = pj(&root, "composer.json");
    let comp = match read_json(&path) {
        Ok(v) => Some(v),
        Err(e) => {
            if is_file(&path) {
                p.problems.push(format!("composer.json: {}", e.problem()));
            }
            None
        }
    };
    p.watch.push(path.clone());
    p.watch.push(pj(&root, "artisan"));
    let is_laravel = has_any(&root, &["artisan"]).is_some();
    if comp.is_none() && !is_laravel {
        return;
    }
    let comp: Arc<V> = comp.unwrap_or_else(|| Arc::new(V::Map(Vec::new())));
    if let Some(n) = comp.get_str("name") {
        p.names
            .push(Some(n.rsplit('/').next().unwrap_or(n).to_string()));
    }
    let lock_path = pj(&root, "composer.lock");
    p.watch.push(lock_path.clone());
    let versions = read_cached(&lock_path, "composer-lock", 16 * MB, composer_lock_versions).ok();
    let version = |name: &str| {
        versions
            .as_ref()
            .and_then(|v| v.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()))
    };
    let php = comp
        .get_map("require")
        .and_then(|r| r.get_str("php"))
        .map(str::to_string);
    if is_laravel {
        let mut sec = new_section("laravel", "Laravel", "laravel", Some("artisan".into()));
        for (name, cmd, desc, icon) in laravel_commands() {
            sec.scripts.push(script(
                "laravel",
                name,
                cmd.into(),
                None,
                Some(desc.into()),
                icon,
            ));
        }
        let sail = pj(&root, "vendor/bin/sail");
        p.watch.push(sail.clone());
        if is_file(&sail) {
            sec.scripts.push(script(
                "laravel",
                "sail up -d",
                "./vendor/bin/sail up -d".into(),
                None,
                Some(tr!("desc.sail_up").into()),
                "stack",
            ));
            sec.scripts.push(script(
                "laravel",
                "sail down",
                "./vendor/bin/sail down".into(),
                None,
                Some(tr!("desc.sail_down").into()),
                "stop",
            ));
        }
        let ver = version("laravel/framework").filter(|v| !v.is_empty());
        let tag = match &ver {
            Some(v) => format!("Laravel {}", v.split('.').next().unwrap_or("")),
            None => "Laravel".into(),
        };
        if let Some(v) = ver {
            p.info.push(("Laravel".into(), v));
        }
        p.tags.push(("laravel".into(), tag));
        p.sections.push(sec);
        p.kinds.push("laravel".into());
        let env = pj(&root, ".env");
        if is_file(&env) {
            p.env_file = Some(env);
        }
    }
    if comp.map().is_some_and(|m| !m.is_empty()) || is_file(&path) {
        let mut sec = new_section(
            "composer",
            "Composer",
            "composer",
            Some("composer.json".into()),
        );
        let descs = comp.get_map("scripts-descriptions");
        for (name, body) in comp.get_map("scripts").and_then(V::map).unwrap_or(&[]) {
            if COMPOSER_EVENTS.contains(&name.as_str()) {
                continue;
            }
            let body = match body {
                V::List(l) => l.iter().map(V::text).collect::<Vec<_>>().join(" && "),
                other => other.text(),
            };
            let desc = descs
                .and_then(|d| d.get_str(name))
                .filter(|d| !d.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| body.clone());
            sec.scripts.push(script(
                "composer",
                name,
                format!("composer run-script {}", q(name)),
                Some(body),
                Some(desc),
                "play",
            ));
        }
        sec.scripts.push(script(
            "composer",
            "install",
            "composer install".into(),
            None,
            Some(tr!("desc.install_deps").into()),
            "download",
        ));
        let tag = match &php {
            Some(v) => format!("PHP {v}"),
            None => "Composer".into(),
        };
        if let Some(v) = php {
            p.info.push(("PHP".into(), v));
        }
        p.tags.push(("composer".into(), tag));
        p.sections.push(sec);
        p.kinds.push("composer".into());
    }
}

fn detect_compose(p: &mut Project, _chain: &[String]) {
    let root = p.root.clone();
    for f in COMPOSE_FILES {
        p.watch.push(pj(&root, f));
    }
    let Some(path) = has_any(&root, COMPOSE_FILES) else {
        return;
    };
    let services: Vec<String> = match read_yaml(&path) {
        Ok(doc) => parsers::yaml_children(&doc, "services")
            .into_iter()
            .map(|(n, _)| n)
            .collect(),
        Err(ReadErr::Bad(e)) => {
            p.problems.push(format!("{}: {e}", basename(&path)));
            Vec::new()
        }
        Err(ReadErr::Missing) => Vec::new(),
    };
    let mut sec = new_section(
        "compose",
        "Docker Compose",
        "compose",
        Some(basename(&path).to_string()),
    );
    for (name, args, desc, icon) in [
        ("up -d", "up -d", tr!("desc.compose_up"), "play"),
        ("down", "down", tr!("desc.compose_down"), "stop"),
        ("ps", "ps", tr!("desc.compose_ps"), "list"),
        ("logs -f", "logs -f", tr!("desc.compose_logs"), "text"),
        ("down -v", "down -v", tr!("desc.compose_down_v"), "trash"),
    ] {
        sec.scripts.push(script(
            "compose",
            name,
            format!("docker compose {args}"),
            None,
            Some(desc.into()),
            icon,
        ));
    }
    for s in &services {
        sec.scripts.push(script(
            "compose",
            &format!("logs -f {s}"),
            format!("docker compose logs -f {}", q(s)),
            None,
            Some(tr!("desc.compose_logs_one", service = s)),
            "text",
        ));
    }
    let tag = if services.is_empty() {
        "Compose".to_string()
    } else {
        tr!("tag.compose_services", count = services.len())
    };
    if !services.is_empty() {
        p.info.push(("Services".into(), services.join(", ")));
    }
    p.tags.push(("compose".into(), tag));
    p.sections.push(sec);
    p.kinds.push("compose".into());
}

fn cargo_bins(folder: &str, manifest: &V) -> Vec<String> {
    let Some(pkg) = manifest.get_map("package") else {
        return Vec::new();
    };
    let pname = pkg.get_str("name");
    let mut bins: Vec<String> = Vec::new();
    // Explicit targets replace the auto-discovered ones.
    let mut paths: HashSet<String> = HashSet::new();
    for b in manifest.get("bin").and_then(V::list).unwrap_or(&[]) {
        if let Some(name) = b.get_str("name") {
            bins.push(name.to_string());
            let default = if Some(name) == pname {
                "src/main.rs".to_string()
            } else {
                format!("src/bin/{name}.rs")
            };
            let path = match b.get("path") {
                Some(V::Str(s)) if !s.is_empty() => s.clone(),
                Some(v @ (V::Int(_) | V::Float(_) | V::Bool(true))) => v.text(),
                _ => default,
            };
            paths.insert(normpath(&path));
        }
    }
    if pkg.get("autobins") != Some(&V::Bool(false)) {
        if let Some(pname) = pname {
            if is_file(&pj(folder, "src/main.rs"))
                && !paths.contains("src/main.rs")
                && !bins.iter().any(|b| b == pname)
            {
                bins.insert(0, pname.to_string());
            }
        }
        let bindir = pj(folder, "src/bin");
        if let Ok(rd) = std::fs::read_dir(&bindir) {
            let mut names: Vec<String> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            for n in names {
                let (name, rel) = if let Some(stem) = n.strip_suffix(".rs") {
                    (stem.to_string(), format!("src/bin/{n}"))
                } else if is_file(&pj(&pj(&bindir, &n), "main.rs")) {
                    (n.clone(), format!("src/bin/{n}/main.rs"))
                } else {
                    continue;
                };
                if !bins.contains(&name) && !paths.contains(&rel) {
                    bins.push(name);
                }
            }
        }
    }
    bins
}

fn detect_cargo(p: &mut Project, _chain: &[String]) {
    let root = p.root.clone();
    let path = pj(&root, "Cargo.toml");
    p.watch.push(path.clone());
    if !is_file(&path) {
        return;
    }
    let man = match read_toml(&path) {
        Ok(m) => m,
        Err(e) => {
            p.problems.push(format!("Cargo.toml: {}", e.problem()));
            return;
        }
    };
    let pkg = man.get_map("package");
    if let Some(n) = pkg.and_then(|p| p.get_str("name")) {
        p.names.push(Some(n.to_string()));
    }
    let mut sec = new_section("cargo", "Cargo", "cargo", Some("Cargo.toml".into()));
    let mut bins: Vec<(String, Option<String>)> = cargo_bins(&root, &man)
        .into_iter()
        .map(|b| (b, None))
        .collect();
    let ws = man.get_map("workspace");
    if let Some(ws) = ws {
        let members: Vec<V> = ws
            .get("members")
            .and_then(V::list)
            .unwrap_or(&[])
            .iter()
            .filter(|m| m.str().is_some())
            .cloned()
            .collect();
        let excluded: HashSet<String> = ws
            .get("exclude")
            .and_then(V::list)
            .unwrap_or(&[])
            .iter()
            .filter_map(V::str)
            .map(|e| normpath(&pj(&root, e)))
            .collect();
        for m in expand_globs(&root, &members, "Cargo.toml") {
            if excluded.contains(&m) {
                continue;
            }
            let mpath = pj(&m, "Cargo.toml");
            p.watch.push(mpath.clone());
            let mm = read_toml(&mpath)
                .ok()
                .unwrap_or_else(|| Arc::new(V::Map(Vec::new())));
            let name = mm
                .get_map("package")
                .and_then(|p| p.get_str("name"))
                .map(str::to_string)
                .unwrap_or_else(|| basename(&m).to_string());
            for b in cargo_bins(&m, &mm) {
                bins.push((b, Some(name.clone())));
            }
            let detail = relpath(&m, &root);
            sec.links.push(Link {
                name,
                path: m,
                detail,
            });
        }
    }
    sec.scripts.push(script(
        "cargo",
        "build",
        "cargo build".into(),
        None,
        Some(tr!("desc.compile").into()),
        "hammer",
    ));
    sec.scripts.push(script(
        "cargo",
        "test",
        "cargo test".into(),
        None,
        Some(tr!("desc.tests").into()),
        "check",
    ));
    if bins.len() == 1 {
        sec.scripts.push(script(
            "cargo",
            "run",
            "cargo run".into(),
            None,
            Some(tr!("desc.run_bin", name = bins[0].0)),
            "play",
        ));
    }
    sec.scripts.push(script(
        "cargo",
        "clippy",
        "cargo clippy".into(),
        None,
        Some(tr!("desc.lint").into()),
        "search",
    ));
    if bins.len() > 1 {
        for (b, _member) in &bins {
            sec.scripts.push(script(
                "cargo",
                &format!("run --bin {b}"),
                format!("cargo run --bin {}", q(b)),
                None,
                Some(tr!("desc.run_bin", name = b)),
                "play",
            ));
        }
    }
    if ws.is_some() {
        p.info.push((
            "Workspace".into(),
            tr!("info.members", count = sec.links.len()),
        ));
    }
    let pkg_empty = pkg.and_then(V::map).is_none_or(<[_]>::is_empty);
    let tag = if ws.is_some() && pkg_empty {
        tr!("tag.cargo_workspace").to_string()
    } else {
        match pkg.and_then(|p| p.get_str("edition")) {
            Some(e) => format!("Rust {e}"),
            None => "Rust".into(),
        }
    };
    p.tags.push(("cargo".into(), tag));
    p.sections.push(sec);
    p.kinds.push("cargo".into());
}

fn find_venv(root: &str) -> (Option<&'static str>, Option<String>) {
    static R: OnceLock<Regex> = OnceLock::new();
    let rx = R.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:version|version_info)\s*=\s*([\d.]+)").expect("valid regex")
    });
    for d in VENV_DIRS {
        let cfg = pj(&pj(root, d), "pyvenv.cfg");
        if is_file(&cfg) {
            let text = read_text(&cfg).map(|t| (*t).clone()).unwrap_or_default();
            let ver = rx.captures(&text).map(|c| c[1].to_string());
            return (Some(d), ver);
        }
    }
    (None, None)
}

fn detect_python(p: &mut Project, _chain: &[String]) {
    let root = p.root.clone();
    let path = pj(&root, "pyproject.toml");
    let req = pj(&root, "requirements.txt");
    p.watch.push(path.clone());
    p.watch.push(req.clone());
    let has_py = is_file(&path);
    let has_req = is_file(&req);
    if !(has_py || has_req) {
        return;
    }
    let empty = Arc::new(V::Map(Vec::new()));
    let data = if has_py {
        match read_toml(&path) {
            Ok(d) => d,
            Err(e) => {
                p.problems.push(format!("pyproject.toml: {}", e.problem()));
                empty.clone()
            }
        }
    } else {
        empty.clone()
    };
    let none = V::Map(Vec::new());
    let project = data.get_map("project").unwrap_or(&none);
    let tool = data.get_map("tool").unwrap_or(&none);
    let poetry = tool.get_map("poetry");
    if let Some(n) = project.get_str("name") {
        p.names.push(Some(n.to_string()));
    } else if let Some(n) = poetry.and_then(|po| po.get_str("name")) {
        p.names.push(Some(n.to_string()));
    }
    let (venv, venv_ver) = find_venv(&root);
    for d in VENV_DIRS {
        p.watch.push(pj(&pj(&root, d), "pyvenv.cfg"));
    }
    let (runner, label) = if is_file(&pj(&root, "uv.lock")) {
        (Some("uv run"), "uv")
    } else if poetry.is_some() || is_file(&pj(&root, "poetry.lock")) {
        (Some("poetry run"), "Poetry")
    } else if is_file(&pj(&root, "pdm.lock")) || tool.get("pdm").is_some() {
        (Some("pdm run"), "PDM")
    } else if tool.get("hatch").is_some() {
        (Some("hatch run"), "Hatch")
    } else {
        (None, "Python")
    };
    let detail = if label != "Python" {
        label
    } else if has_py {
        "pyproject.toml"
    } else {
        "requirements.txt"
    };
    let mut sec = new_section("python", "Python", "python", Some(detail.into()));
    let entry = project.get_map("scripts").and_then(V::map).unwrap_or(&[]);
    for (name, target) in entry {
        let cmd = match (runner, venv) {
            (Some(r), _) => format!("{r} {}", q(name)),
            (None, Some(v)) => format!("{v}/bin/{}", q(name)),
            (None, None) => q(name),
        };
        sec.scripts.push(script(
            "python",
            name,
            cmd,
            None,
            Some(format!("{name} ({})", target.text())),
            "play",
        ));
    }
    if let Some(ps) = poetry.and_then(|po| po.get_map("scripts")).and_then(V::map) {
        for (name, target) in ps {
            if entry.iter().any(|(n, _)| n == name) {
                continue;
            }
            let t = target.str().unwrap_or("script");
            sec.scripts.push(script(
                "python",
                name,
                format!("poetry run {}", q(name)),
                None,
                Some(format!("{name} ({t})")),
                "play",
            ));
        }
    }
    let envs = tool
        .get_map("hatch")
        .and_then(|h| h.get_map("envs"))
        .and_then(V::map)
        .unwrap_or(&[]);
    for (env, conf) in envs {
        let Some(scripts) = conf.get_map("scripts").and_then(V::map) else {
            continue;
        };
        for (name, body) in scripts {
            let label_name = if env == "default" {
                name.clone()
            } else {
                format!("{env}:{name}")
            };
            let body = match body {
                V::List(l) => l.iter().map(V::text).collect::<Vec<_>>().join(" && "),
                other => other.text(),
            };
            sec.scripts.push(script(
                "python",
                &label_name,
                format!("hatch run {}", q(&label_name)),
                Some(body.clone()),
                Some(body),
                "play",
            ));
        }
    }
    if let Some(pdm) = tool
        .get_map("pdm")
        .and_then(|t| t.get_map("scripts"))
        .and_then(V::map)
    {
        for (name, body) in pdm {
            if name.starts_with('_') {
                continue;
            }
            let body = match body {
                V::Map(_) => ["cmd", "shell", "call"]
                    .iter()
                    .filter_map(|k| body.get(k))
                    .find(|v| {
                        !matches!(v, V::Str(s) if s.is_empty())
                            && !matches!(v, V::List(l) if l.is_empty())
                    })
                    .cloned()
                    .unwrap_or(V::Str(String::new())),
                other => other.clone(),
            };
            let body = match body {
                V::List(l) => l.iter().map(V::text).collect::<Vec<_>>().join(" "),
                other => other.text(),
            };
            sec.scripts.push(script(
                "python",
                name,
                format!("pdm run {}", q(name)),
                Some(body.clone()),
                Some(body),
                "play",
            ));
        }
    }
    if has_req {
        let pip = venv
            .map(|v| format!("{v}/bin/pip"))
            .unwrap_or_else(|| "python3 -m pip".into());
        let desc = match venv {
            Some(v) => tr!("desc.pip_install_into", venv = v),
            None => tr!("desc.pip_install").to_string(),
        };
        sec.scripts.push(script(
            "python",
            "pip install -r requirements.txt",
            format!("{pip} install -r requirements.txt"),
            None,
            Some(desc),
            "download",
        ));
    }
    if let Some(v) = venv {
        sec.scripts.push(script(
            "python",
            &format!("activate {v}"),
            format!("source {v}/bin/activate"),
            None,
            Some(tr!("desc.activate").into()),
            "terminal",
        ));
        let val = match &venv_ver {
            Some(ver) => format!("{v} (Python {ver})"),
            None => v.to_string(),
        };
        p.info.push(("venv".into(), val));
    }
    let rp = project.get_str("requires-python");
    if let Some(rp) = rp {
        p.info.push(("Python".into(), rp.to_string()));
    }
    let tag = match rp {
        Some(rp) => format!("Python {rp}"),
        None => "Python".into(),
    };
    p.tags.push((
        "python".into(),
        if label == "Python" {
            tag
        } else {
            format!("{label} · {tag}")
        },
    ));
    p.sections.push(sec);
    p.kinds.push("python".into());
}

fn detect_make(p: &mut Project, _chain: &[String]) {
    for f in MAKEFILES {
        p.watch.push(pj(&p.root, f));
    }
    let Some(path) = has_any(&p.root, MAKEFILES) else {
        return;
    };
    let targets = read_cached(&path, "make", 4 * MB, |t| Ok(parsers::parse_makefile(t))).ok();
    let mut sec = new_section("make", "Make", "make", Some(basename(&path).to_string()));
    for (t, desc) in targets.as_deref().map(Vec::as_slice).unwrap_or(&[]) {
        let detail = desc.clone().unwrap_or_else(|| format!("make {t}"));
        sec.scripts.push(script(
            "make",
            t,
            format!("make {}", q(t)),
            Some(detail),
            desc.clone(),
            "hammer",
        ));
    }
    p.tags.push(("make".into(), "Make".into()));
    p.sections.push(sec);
    p.kinds.push("make".into());
}

fn detect_just(p: &mut Project, _chain: &[String]) {
    for f in JUSTFILES {
        p.watch.push(pj(&p.root, f));
    }
    let Some(path) = has_any(&p.root, JUSTFILES) else {
        return;
    };
    let recipes = read_cached(&path, "just", 4 * MB, |t| Ok(parsers::parse_justfile(t))).ok();
    let mut sec = new_section("just", "just", "just", Some(basename(&path).to_string()));
    for (name, desc, needs) in recipes.as_deref().map(Vec::as_slice).unwrap_or(&[]) {
        let detail = desc.clone().unwrap_or_else(|| format!("just {name}"));
        let detail = if *needs {
            tr!("detail.needs_args", detail = detail)
        } else {
            detail
        };
        sec.scripts.push(script_args(
            "just",
            name,
            format!("just {}", q(name)),
            Some(detail),
            desc.clone(),
            "hammer",
            *needs,
        ));
    }
    p.tags.push(("just".into(), "just".into()));
    p.sections.push(sec);
    p.kinds.push("just".into());
}

fn detect_task(p: &mut Project, _chain: &[String]) {
    for f in TASKFILES {
        p.watch.push(pj(&p.root, f));
    }
    let Some(path) = has_any(&p.root, TASKFILES) else {
        return;
    };
    let tasks = match read_yaml(&path) {
        Ok(doc) => parsers::yaml_children(&doc, "tasks"),
        Err(ReadErr::Bad(e)) => {
            p.problems.push(format!("{}: {e}", basename(&path)));
            Vec::new()
        }
        Err(ReadErr::Missing) => Vec::new(),
    };
    let mut sec = new_section("task", "Task", "task", Some(basename(&path).to_string()));
    for (name, fields) in tasks {
        let field = |k: &str| {
            fields
                .iter()
                .find(|(f, _)| f == k)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };
        if field("internal").as_deref() == Some("true") {
            continue;
        }
        let desc = field("desc").or_else(|| field("summary"));
        let detail = desc.clone().unwrap_or_else(|| format!("task {name}"));
        sec.scripts.push(script(
            "task",
            &name,
            format!("task {}", q(&name)),
            Some(detail),
            desc,
            "hammer",
        ));
    }
    p.tags.push(("task".into(), "Task".into()));
    p.sections.push(sec);
    p.kinds.push("task".into());
}

type Detector = fn(&mut Project, &[String]);
const DETECTORS: &[Detector] = &[
    detect_php,
    detect_node,
    detect_compose,
    detect_cargo,
    detect_python,
    detect_make,
    detect_just,
    detect_task,
];

/// (repo root, branch, HEAD file) of the Git repository containing
/// `folder`. Reads `.git/HEAD` only.
pub fn git_info(folder: &str, stop: &str) -> Option<(String, Option<String>, String)> {
    let mut d = folder.to_string();
    loop {
        let dot = pj(&d, ".git");
        if exists(&dot) {
            let mut gitdir = dot.clone();
            if is_file(&dot) {
                let text = read_text(&dot).map(|t| (*t).clone()).unwrap_or_default();
                if let Some(rest) = text.trim().strip_prefix("gitdir:") {
                    gitdir = normpath(&pj(&d, rest.trim()));
                }
            }
            let head_path = pj(&gitdir, "HEAD");
            let head = read_text(&head_path)
                .map(|t| t.trim().to_string())
                .unwrap_or_default();
            let branch = if let Some(r) = head.strip_prefix("ref:") {
                Some(match head.strip_prefix("ref: refs/heads/") {
                    Some(b) => b.to_string(),
                    None => r.trim().to_string(),
                })
            } else if head.is_empty() {
                None
            } else {
                Some(tr!(
                    "header.detached",
                    commit = head.chars().take(7).collect::<String>()
                ))
            };
            return Some((d, branch, head_path));
        }
        if d == stop || d == "/" || d.is_empty() || dirname(&d) == d {
            return None;
        }
        d = dirname(&d);
    }
}

/// Everything about one project folder (`chain`: the folders from root up
/// to where the walk stops, for lockfiles and .nvmrc further up).
pub fn detect_project(root: &str, chain: &[String]) -> Project {
    let mut p = Project::new(root);
    for det in DETECTORS {
        det(&mut p, chain);
    }
    let trimmed = root.trim_end_matches('/');
    p.name = match basename(trimmed) {
        "" => root.to_string(),
        n => n.to_string(),
    };
    p.count = p.sections.iter().map(|s| s.scripts.len()).sum();
    p.has_env = p.env_file.is_some() || is_file(&pj(root, ".env"));
    p.primary = PRIMARY_ORDER
        .iter()
        .find(|k| p.kinds.iter().any(|x| x == *k))
        .map(|k| k.to_string());
    match &p.primary {
        Some(kind) => {
            let (label, icon, tone) = kind_style(kind);
            p.primary_label = if kind == "node" {
                p.pm.clone().unwrap_or_else(|| "npm".into())
            } else {
                label.into()
            };
            p.primary_icon = icon.into();
            p.primary_tone = tone;
        }
        None => {
            p.primary_label = tr!("kind.project").into();
            p.primary_icon = "folder".into();
            p.primary_tone = Tone::Neutral;
        }
    }
    p
}

pub fn is_project_dir(d: &str) -> bool {
    manifests().any(|n| is_file(&pj(d, n)))
}

/// Projects containing `folder`, nearest first (at most 4). Walks up to
/// (not including) `home` or `/`, stops after the Git repository root and
/// at `stop`, and skips folders inside node_modules or vendor.
pub fn find_projects(folder: &str, stop: Option<&str>, home: &str) -> Vec<Project> {
    let home = normpath(home);
    let folder = normpath(folder);
    let stop = stop.map(normpath);
    let mut dirs: Vec<String> = Vec::new();
    let mut d = folder.clone();
    for _ in 0..64 {
        if d == home || dirname(&d) == d {
            break;
        }
        dirs.push(d.clone());
        if stop.as_deref() == Some(d.as_str()) || exists(&pj(&d, ".git")) {
            break;
        }
        d = dirname(&d);
    }
    let mut projects = Vec::new();
    for (i, d) in dirs.iter().enumerate() {
        if is_skipped(d) || !is_project_dir(d) {
            continue;
        }
        projects.push(detect_project(d, &dirs[i..]));
        if projects.len() >= 4 {
            break;
        }
    }
    let git = git_info(&folder, stop.as_deref().unwrap_or(&home));
    for p in &mut projects {
        match &git {
            Some((repo, branch, head))
                if format!("{}/", p.root)
                    .starts_with(&format!("{}/", repo.trim_end_matches('/'))) =>
            {
                p.git_branch = branch.clone();
                p.git_root = Some(repo.clone());
                p.watch.push(head.clone());
            }
            _ => {
                p.git_branch = None;
                p.git_root = None;
            }
        }
    }
    projects
}

/// Files whose changes mean the detection must run again: every possible
/// manifest in the folders walked plus what detection read.
pub fn watch_list(
    folder: &str,
    projects: &[Project],
    stop: Option<&str>,
    home: &str,
) -> Vec<String> {
    let home = normpath(home);
    let stop = stop.map(normpath);
    let mut out: Vec<String> = Vec::new();
    let mut d = normpath(folder);
    for _ in 0..64 {
        if d == home || dirname(&d) == d {
            break;
        }
        out.extend(manifests().map(|n| pj(&d, n)));
        if stop.as_deref() == Some(d.as_str()) || exists(&pj(&d, ".git")) {
            break;
        }
        d = dirname(&d);
    }
    for p in projects {
        out.extend(p.watch.iter().cloned());
    }
    let mut seen = HashSet::new();
    out.retain(|x| seen.insert(x.clone()));
    out
}

/// (mtime, size) of each watched file (`None` = missing).
pub fn signature(paths: &[String]) -> Vec<Option<(i128, u64)>> {
    paths.iter().map(|p| stat(p)).collect()
}

pub fn chip_text(p: &Project) -> String {
    tr!("chip.scripts", count = p.count, label = p.primary_label)
}
