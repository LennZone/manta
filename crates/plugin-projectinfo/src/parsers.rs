//! Build-file scanners: Makefile targets and justfile recipes (ported from
//! the Python plugin's `pi_parsers.py`), and the YAML lookups for compose
//! `services:`, Taskfile `tasks:` and pnpm-workspace `packages:` (a real
//! YAML parser instead of the old line scanner).

use std::sync::OnceLock;

use regex::Regex;

use crate::value::{self, V};

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("valid regex"))
}

// ---------------------------------------------------------------- Makefile

fn make_assign() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(
        &R,
        r"^\s*(?:export\s+|override\s+)?[A-Za-z_][\w.]*\s*(?:[:+?!]|::)?=",
    )
}

const MAKE_DIRECTIVES: &[&str] = &[
    "ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "-include", "sinclude",
    "vpath", "export", "unexport", "override", "undefine", "$(",
];

/// Join backslash continuations.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf: Option<String> = None;
    for raw in text.replace("\r\n", "\n").split('\n') {
        let mut line = match buf.take() {
            Some(b) => format!("{b} {}", raw.trim()),
            None => raw.to_string(),
        };
        if line.ends_with('\\') && !line.ends_with("\\\\") {
            line.pop();
            buf = Some(line.trim_end().to_string());
            continue;
        }
        out.push(line);
    }
    if let Some(b) = buf {
        out.push(b);
    }
    out
}

/// `^([^\s#:=][^:=#]*?)\s*(::|:)(?![=:])(.*)$` → (names, rest).
fn make_rule(line: &str) -> Option<(&str, &str)> {
    let first = line.chars().next()?;
    if first.is_whitespace() || matches!(first, '#' | ':' | '=') {
        return None;
    }
    let i = line.find([':', '=', '#'])?;
    if line.as_bytes()[i] != b':' {
        return None;
    }
    let after = &line[i + 1..];
    let rest = if let Some(r) = after.strip_prefix(':') {
        if r.starts_with(['=', ':']) {
            return None;
        }
        r
    } else {
        if after.starts_with('=') {
            return None;
        }
        after
    };
    let names = line[..i].trim_end();
    (!names.is_empty()).then_some((names, rest))
}

/// [(target, description)] in file order. Skips special targets (.PHONY,
/// .DEFAULT …), pattern rules (%), targets with variables, file paths and
/// recipe lines. A description is a `## text` after the prerequisites or a
/// `## text` line right above.
pub fn parse_makefile(text: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut doc: Option<String> = None;
    let mut in_define = false;
    for line in logical_lines(text) {
        if in_define {
            if line.trim().starts_with("endef") {
                in_define = false;
            }
            continue;
        }
        if line.starts_with('\t') {
            doc = None;
            continue;
        }
        let stripped = line.trim();
        if stripped.is_empty() {
            doc = None;
            continue;
        }
        if stripped.starts_with("define ") || stripped == "define" {
            in_define = true;
            doc = None;
            continue;
        }
        if stripped.starts_with("##") {
            let d = stripped.trim_start_matches('#').trim();
            doc = (!d.is_empty()).then(|| d.to_string());
            continue;
        }
        if stripped.starts_with('#') {
            continue;
        }
        let first_word = stripped.split_whitespace().next().unwrap_or("");
        if MAKE_DIRECTIVES.contains(&first_word) || make_assign().is_match(&line) {
            doc = None;
            continue;
        }
        let Some((names, rest)) = make_rule(&line) else {
            doc = None;
            continue;
        };
        let mut desc = doc.take();
        if make_assign().is_match(rest) {
            continue; // target-specific variable
        }
        if let Some((_, d)) = rest.split_once("##") {
            let d = d.trim();
            if !d.is_empty() {
                desc = Some(d.to_string());
            }
        }
        for t in names.split_whitespace() {
            if t.starts_with('.')
                || t.contains(['%', '$', '/', '(', ')'])
                || out.iter().any(|(x, _)| x == t)
            {
                continue;
            }
            out.push((t.to_string(), desc.clone()));
        }
    }
    out
}

// ---------------------------------------------------------------- justfile

const JUST_KEYWORDS: &[&str] = &["set", "alias", "export", "import", "mod", "if", "else"];

/// [(recipe, description, needs_args)] in file order (private recipes,
/// `_name` or `[private]`, are left out).
pub fn parse_justfile(text: &str) -> Vec<(String, Option<String>, bool)> {
    static RECIPE: OnceLock<Regex> = OnceLock::new();
    static DOC: OnceLock<Regex> = OnceLock::new();
    let recipe = re(
        &RECIPE,
        r"^(@?)([A-Za-z_][A-Za-z0-9_-]*)((?:\s+[^:]*?)?)\s*:",
    );
    let doc_attr = re(&DOC, r#"doc\(\s*(?:"([^"]*)"|'([^']*)')\s*\)"#);
    let mut out = Vec::new();
    let mut doc: Option<String> = None;
    let mut private = false;
    for line in text.replace("\r\n", "\n").split('\n') {
        if line.trim().is_empty() {
            doc = None;
            private = false;
            continue;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let s = line.trim();
        if s.starts_with('#') {
            if !s.starts_with("#!") {
                let d = s.trim_start_matches('#').trim();
                doc = (!d.is_empty()).then(|| d.to_string());
            }
            continue;
        }
        if s.starts_with('[') {
            if s.contains("private") {
                private = true;
            }
            if let Some(c) = doc_attr.captures(s) {
                doc = c
                    .get(1)
                    .or_else(|| c.get(2))
                    .map(|m| m.as_str().to_string());
            }
            continue;
        }
        let m = recipe.captures(line).filter(|c| {
            // `:` but not `:=` / `::` (the regex crate has no lookahead).
            let end = c.get(0).map_or(0, |m| m.end());
            !line[end..].starts_with(['=', ':'])
        });
        let Some(c) = m.filter(|c| !JUST_KEYWORDS.contains(&&c[2])) else {
            doc = None;
            private = false;
            continue;
        };
        let name = c[2].to_string();
        let needs = c[3].split_whitespace().any(|p| {
            let p = p.strip_prefix('$').unwrap_or(p);
            !(p.contains('=') || p.starts_with('*'))
        });
        if !(private || name.starts_with('_')) {
            out.push((name, doc.take(), needs));
        }
        doc = None;
        private = false;
    }
    out
}

// ---------------------------------------------------------------- YAML

/// A YAML scalar as text ("true" for booleans), `None` for anything else.
fn scalar(v: &V) -> Option<String> {
    match v {
        V::Str(s) => Some(s.clone()),
        V::Bool(b) => Some(b.to_string()),
        V::Int(i) => Some(i.to_string()),
        V::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

/// Children of a top-level mapping key: [(name, [(field, scalar)])], the
/// scalar fields directly below each child (e.g. `desc:`). A block scalar
/// (`desc: |`) counts with its first line.
pub fn yaml_children(doc: &V, key: &str) -> Vec<(String, Vec<(String, String)>)> {
    let Some(children) = doc.get(key).and_then(V::map) else {
        return Vec::new();
    };
    children
        .iter()
        .map(|(name, body)| {
            let fields = body
                .map()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| {
                            let s = scalar(v)?;
                            let first = s
                                .lines()
                                .map(str::trim)
                                .find(|l| !l.is_empty())?
                                .to_string();
                            Some((k.clone(), first))
                        })
                        .collect()
                })
                .unwrap_or_default();
            (name.clone(), fields)
        })
        .collect()
}

/// Scalar items of a top-level list key (`packages:\n  - 'a/*'`).
pub fn yaml_list(doc: &V, key: &str) -> Vec<String> {
    doc.get(key)
        .and_then(V::list)
        .map(|l| l.iter().filter_map(scalar).collect())
        .unwrap_or_default()
}

/// Parse YAML text (compose, Taskfile, pnpm-workspace).
pub fn parse_yaml(text: &str) -> Result<V, String> {
    value::yaml(text)
}
