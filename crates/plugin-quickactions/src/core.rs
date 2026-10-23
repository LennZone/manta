//! Quick Actions core: reading actions.json, deciding which actions apply
//! to a selection, and turning a command template into safe shell text.
//! Pure functions (no threads, no IO), ported 1:1 from the Python plugin's
//! `qa_core.py`, including its exact quoting rules.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

use crate::i18n::{self, tr};

/// The default actions.json, written on first run (JSON with comments).
pub const DEFAULT_ACTIONS: &str = include_str!("defaults.jsonc");

pub const MODES: &[&str] = &["background", "terminal", "silent"];
pub const OUTPUTS: &[&str] = &["none", "toast", "panel"];
/// Placeholders that make an action run once per selected item.
pub const PER_ITEM: &[&str] = &["file", "name", "stem", "ext"];
pub const PLACEHOLDERS: &[&str] = &[
    "files", "file", "names", "name", "stem", "ext", "dir", "dest", "count",
];
const KINDS: &[&str] = &["any", "files", "folders"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Background,
    Terminal,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    None,
    Toast,
    Panel,
}

/// One checked action from actions.json, with defaults filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub command: String,
    /// Normalised targets: "any", "files", "folders" or an extension
    /// without the dot, lower case.
    pub applies_to: Vec<String>,
    pub min: u64,
    pub max: Option<u64>,
    pub mode: Mode,
    pub output: Output,
    pub confirm: Option<String>,
    pub refresh: bool,
    pub message: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub title_de: Option<String>,
    pub message_de: Option<String>,
    pub confirm_de: Option<String>,
}

/// A problem in actions.json (already in the UI language).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// --------------------------------------------------------------- config

/// JSON5-lite: drop `//` and `/* */` comments outside strings and trailing
/// commas before `}` or `]`, so actions.json can be commented.
pub fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut i = 0;
    let mut in_str = false;
    while i < n {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < n {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if b[i..].starts_with(b"//") {
            i = b[i..].iter().position(|&x| x == b'\n').map_or(n, |j| i + j);
        } else if b[i..].starts_with(b"/*") {
            i = find(&b[i + 2..], b"*/").map_or(n, |j| i + 2 + j + 2);
        } else {
            out.push(c);
            i += 1;
        }
    }
    // Trailing commas: `,` + whitespace + `}`/`]` (strings were copied
    // verbatim above; this pass skips them too).
    let mut res: Vec<u8> = Vec::with_capacity(out.len());
    let mut i = 0;
    let mut in_str = false;
    while i < out.len() {
        let c = out[i];
        if in_str {
            res.push(c);
            if c == b'\\' && i + 1 < out.len() {
                res.push(out[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
        } else if c == b',' {
            let mut j = i + 1;
            while j < out.len() && out[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < out.len() && (out[j] == b'}' || out[j] == b']') {
                i += 1; // drop the comma, keep the whitespace
                continue;
            }
        }
        res.push(c);
        i += 1;
    }
    // Only ASCII bytes were removed, so this stays valid UTF-8.
    String::from_utf8(res).unwrap_or_default()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Python's `repr()` of a JSON value, as the error messages show ids.
pub fn py_repr(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => py_repr_str(s),
        Some(other) => other.to_string(),
    }
}

fn py_repr_str(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

/// Python's `str()` of a value inside `applies_to`.
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(Some(other)),
    }
}

/// Python truthiness.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `raw.get(k) or None` for text fields (only non-empty strings count).
fn opt_text(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Optional German texts: stripped, `None` when empty.
fn opt_str(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn norm_target(t: &str) -> String {
    let t = t.trim().to_lowercase();
    match t.as_str() {
        "file" => "files".into(),
        "folder" | "dirs" | "directories" => "folders".into(),
        _ if KINDS.contains(&t.as_str()) => t,
        _ => t.trim_start_matches('.').to_string(),
    }
}

fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)),
        _ => None,
    }
}

/// Check one action from actions.json and fill in defaults.
pub fn normalize_action(raw: &Value) -> Result<Action, ConfigError> {
    let Some(obj) = raw.as_object() else {
        return Err(ConfigError(tr!("err.object").to_string()));
    };
    let rid = py_repr(obj.get("id"));
    let id = match obj.get("id") {
        Some(Value::String(s)) if valid_id(s) => s.clone(),
        _ => return Err(ConfigError(tr!("err.id", id = rid))),
    };
    let title = match obj.get("title") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err(ConfigError(tr!("err.title", id = rid))),
    };
    let command = match obj.get("command") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err(ConfigError(tr!("err.command", id = rid))),
    };
    let applies: Vec<Value> = match obj.get("applies_to") {
        None => vec![Value::String("any".into())],
        Some(Value::String(s)) => vec![Value::String(s.clone())],
        Some(Value::Array(a)) if !a.is_empty() => a.clone(),
        _ => return Err(ConfigError(tr!("err.applies", id = rid))),
    };
    let applies_to = applies
        .iter()
        .map(py_str)
        .filter(|t| !t.trim().is_empty())
        .map(|t| norm_target(&t))
        .collect();
    let mode = match obj.get("mode") {
        None => Mode::Background,
        Some(Value::String(m)) if m == "background" => Mode::Background,
        Some(Value::String(m)) if m == "terminal" => Mode::Terminal,
        Some(Value::String(m)) if m == "silent" => Mode::Silent,
        _ => {
            return Err(ConfigError(tr!(
                "err.mode",
                id = rid,
                values = MODES.join(", ")
            )))
        }
    };
    let output = match obj.get("output") {
        None => Output::None,
        Some(Value::String(m)) if m == "none" => Output::None,
        Some(Value::String(m)) if m == "toast" => Output::Toast,
        Some(Value::String(m)) if m == "panel" => Output::Panel,
        _ => {
            return Err(ConfigError(tr!(
                "err.output",
                id = rid,
                values = OUTPUTS.join(", ")
            )))
        }
    };
    let min = match obj.get("min") {
        None => 1,
        Some(v) => match as_int(v) {
            Some(n) if n >= 0 => n as u64,
            _ => return Err(ConfigError(tr!("err.min", id = rid))),
        },
    };
    let max = match obj.get("max") {
        None | Some(Value::Null) => None,
        Some(v) => match as_int(v) {
            Some(n) if n >= 0 && n as u64 >= min.max(1) => Some(n as u64),
            _ => return Err(ConfigError(tr!("err.max", id = rid))),
        },
    };
    if uses(&command, PER_ITEM) && uses(&command, &["files", "names"]) {
        return Err(ConfigError(tr!("err.mix", id = rid)));
    }
    Ok(Action {
        id,
        title,
        command,
        applies_to,
        min,
        max,
        mode,
        output,
        confirm: opt_text(obj.get("confirm")),
        refresh: truthy(obj.get("refresh")),
        message: opt_text(obj.get("message")),
        icon: opt_text(obj.get("icon")),
        shortcut: opt_text(obj.get("shortcut")),
        title_de: opt_str(obj.get("title_de")),
        message_de: opt_str(obj.get("message_de")),
        confirm_de: opt_str(obj.get("confirm_de")),
    })
}

/// Parse actions.json text. Returns (actions, errors): broken actions are
/// skipped with a message, a broken file is an error.
pub fn parse_actions(text: &str) -> Result<(Vec<Action>, Vec<String>), ConfigError> {
    let data: Value = serde_json::from_str(&strip_comments(text))
        .map_err(|e| ConfigError(tr!("err.json", error = e)))?;
    let data = match data {
        Value::Object(mut o) => o.remove("actions").unwrap_or(Value::Array(Vec::new())),
        other => other,
    };
    let Value::Array(list) = data else {
        return Err(ConfigError(tr!("err.shape").to_string()));
    };
    let mut actions: Vec<Action> = Vec::new();
    let mut errors = Vec::new();
    for raw in &list {
        if raw.as_object().is_some_and(|o| o.keys().all(|k| k == "//")) {
            continue; // a comment-only entry
        }
        match normalize_action(raw) {
            Err(e) => errors.push(e.0),
            Ok(a) if actions.iter().any(|x| x.id == a.id) => {
                errors.push(tr!("err.dup", id = py_repr_str(&a.id)));
            }
            Ok(a) => actions.push(a),
        }
    }
    Ok((actions, errors))
}

// --------------------------------------------------------------- selection

/// The `{name}`s in a template (`\{([a-z]+)\}`).
fn placeholders_in(template: &str) -> Vec<&str> {
    let b = template.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            if let Some(len) = placeholder_at(b, i) {
                out.push(&template[i + 1..i + 1 + len]);
                i += len + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Length of the name if `b[i..]` starts with `{[a-z]+}`.
fn placeholder_at(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < b.len() && b[j].is_ascii_lowercase() {
        j += 1;
    }
    (j > i + 1 && j < b.len() && b[j] == b'}').then_some(j - i - 1)
}

pub fn uses(template: &str, names: &[&str]) -> bool {
    placeholders_in(template).iter().any(|n| names.contains(n))
}

/// Python's `os.path.basename`.
fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// Python's `os.path.splitext` on a name: (stem, ext with the dot).
fn splitext(name: &str) -> (&str, &str) {
    if let Some(dot) = name.rfind('.') {
        if name[..dot].bytes().any(|c| c != b'.') {
            return (&name[..dot], &name[dot..]);
        }
    }
    (name, "")
}

/// (name, stem, ext) of a path; ext without the dot, "" if none.
pub fn split_name(path: &str) -> (String, String, String) {
    let trimmed = path.trim_end_matches('/');
    let name = match basename(trimmed) {
        "" => path,
        n => n,
    };
    let (stem, ext) = splitext(name);
    (
        name.to_string(),
        stem.to_string(),
        ext.get(1..).unwrap_or("").to_string(),
    )
}

/// Python's `os.path.dirname(p.rstrip("/")) or "/"`.
fn dir_of(path: &str) -> String {
    let t = path.trim_end_matches('/');
    match t.rfind('/') {
        Some(0) => "/".into(),
        Some(i) => {
            let d = t[..i].trim_end_matches('/');
            if d.is_empty() {
                "/".into()
            } else {
                d.to_string()
            }
        }
        None => "/".into(),
    }
}

pub fn item_matches(target: &str, path: &str, is_dir: bool) -> bool {
    match target {
        "any" => true,
        "files" => !is_dir,
        "folders" => is_dir,
        ext => !is_dir && basename(path).to_lowercase().ends_with(&format!(".{ext}")),
    }
}

/// Whether `action` applies to `items` ([(path, is_dir)]): `Ok(())` or
/// `Err(why)`.
pub fn applies(
    action: &Action,
    items: &[(String, bool)],
    dest: Option<&str>,
) -> Result<(), String> {
    let n = items.len() as u64;
    if n < action.min {
        return Err(tr!("why.min", count = action.min));
    }
    if action.max.is_some_and(|m| n > m) {
        return Err(tr!("why.max", count = action.max.unwrap_or(0)));
    }
    for (path, is_dir) in items {
        if !action
            .applies_to
            .iter()
            .any(|t| item_matches(t, path, *is_dir))
        {
            if *is_dir {
                return Err(tr!("why.folders").to_string());
            }
            let ext = split_name(path).2;
            return Err(if ext.is_empty() {
                tr!("why.these").to_string()
            } else {
                tr!("why.ext", ext = ext)
            });
        }
    }
    if uses(&action.command, &["dest"]) && dest.is_none_or(str::is_empty) {
        return Err(tr!("why.dest").to_string());
    }
    Ok(())
}

// --------------------------------------------------------------- templates

/// Python's `shlex.quote`.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if s.chars().all(safe) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// A placeholder's value.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    One(String),
    Many(Vec<String>),
    Count(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum Quote {
    None,
    Single,
    Double,
}

/// Text for one placeholder in a quoting context.
fn render(values: &[String], state: Quote) -> String {
    match state {
        Quote::Double => values
            .iter()
            .map(|v| {
                let mut s = String::with_capacity(v.len());
                for c in v.chars() {
                    if matches!(c, '\\' | '"' | '$' | '`') {
                        s.push('\\');
                    }
                    s.push(c);
                }
                s
            })
            .collect::<Vec<_>>()
            .join(" "),
        Quote::None | Quote::Single => {
            let quoted = values
                .iter()
                .map(|v| shell_quote(v))
                .collect::<Vec<_>>()
                .join(" ");
            if state == Quote::Single {
                // Inside '…' (e.g. sh -c '…'): the quoted words as literal
                // text, so the inner shell sees them quoted.
                quoted.replace('\'', "'\\''")
            } else {
                quoted
            }
        }
    }
}

/// Replace `{placeholders}` in a shell template. Values are inserted
/// quoted for where they stand: bare → shell-quoted words, inside "…" →
/// escaped literal text, inside '…' → quoted words for an inner shell.
/// Unknown `{words}` (awk programs, shell braces) are left alone. A known
/// placeholder without a value is an error.
pub fn expand(template: &str, values: &HashMap<&str, Val>) -> Result<String, ConfigError> {
    let b = template.as_bytes();
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(n + 64);
    let mut i = 0;
    let mut state = Quote::None;
    while i < n {
        let c = b[i];
        match state {
            Quote::None => {
                if c == b'\\' && i + 1 < n {
                    out.extend_from_slice(&b[i..i + 2]);
                    i += 2;
                    continue;
                }
                if c == b'\'' || c == b'"' {
                    state = if c == b'\'' {
                        Quote::Single
                    } else {
                        Quote::Double
                    };
                    out.push(c);
                    i += 1;
                    continue;
                }
            }
            Quote::Single => {
                if c == b'\'' {
                    state = Quote::None;
                    out.push(c);
                    i += 1;
                    continue;
                }
            }
            Quote::Double => {
                if c == b'\\' && i + 1 < n {
                    out.extend_from_slice(&b[i..i + 2]);
                    i += 2;
                    continue;
                }
                if c == b'"' {
                    state = Quote::None;
                    out.push(c);
                    i += 1;
                    continue;
                }
            }
        }
        if c == b'{' {
            if let Some(len) = placeholder_at(b, i) {
                let name = &template[i + 1..i + 1 + len];
                if PLACEHOLDERS.contains(&name) {
                    let text = match values.get(name) {
                        None => {
                            return Err(ConfigError(tr!(
                                "err.no_value",
                                placeholder = format!("{{{name}}}")
                            )));
                        }
                        Some(Val::Count(c)) => c.to_string(),
                        Some(Val::One(v)) => render(std::slice::from_ref(v), state),
                        Some(Val::Many(v)) => render(v, state),
                    };
                    out.extend_from_slice(text.as_bytes());
                    i += len + 2;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    // Only whole ASCII bytes and complete strings were inserted.
    Ok(String::from_utf8(out).unwrap_or_default())
}

/// The shell commands to run for `action` on `items` in `folder`: one per
/// item if the template uses {file}, {name}, {stem} or {ext}, else one.
/// Returns [(command, cwd)].
pub fn plan(
    action: &Action,
    items: &[(String, bool)],
    folder: &str,
    dest: Option<&str>,
) -> Result<Vec<(String, String)>, ConfigError> {
    let paths: Vec<String> = items.iter().map(|(p, _)| p.clone()).collect();
    let mut common: HashMap<&str, Val> = HashMap::new();
    common.insert("files", Val::Many(paths.clone()));
    common.insert(
        "names",
        Val::Many(paths.iter().map(|p| split_name(p).0).collect()),
    );
    common.insert("dir", Val::One(folder.to_string()));
    if let Some(d) = dest.filter(|d| !d.is_empty()) {
        common.insert("dest", Val::One(d.to_string()));
    }
    common.insert("count", Val::Count(paths.len()));
    let template = &action.command;
    if !uses(template, PER_ITEM) {
        return Ok(vec![(expand(template, &common)?, folder.to_string())]);
    }
    let mut out = Vec::new();
    for p in &paths {
        let (name, stem, ext) = split_name(p);
        let mut v = common.clone();
        v.insert("file", Val::One(p.clone()));
        v.insert("name", Val::One(name));
        v.insert("stem", Val::One(stem));
        v.insert("ext", Val::One(ext));
        v.insert("dir", Val::One(dir_of(p)));
        out.push((expand(template, &v)?, folder.to_string()));
    }
    Ok(out)
}

/// Placeholders in a `message` / `confirm` text (plain text, not shell).
pub fn fill_message(text: &str, count: usize, title: &str) -> String {
    let s = if count == 1 { "" } else { "s" };
    text.replace("{count}", &count.to_string())
        .replace("{s}", s)
        .replace("{title}", title)
}

// --------------------------------------------------------------- language

/// (title, message, confirm) of the shipped default actions by id, in
/// English as written in [`DEFAULT_ACTIONS`].
type DefaultTexts = HashMap<String, (String, Option<String>, Option<String>)>;

pub fn defaults() -> &'static DefaultTexts {
    static D: OnceLock<DefaultTexts> = OnceLock::new();
    D.get_or_init(|| {
        let actions = manta_plugin::i18n::with_lang(manta_plugin::i18n::Lang::En, || {
            parse_actions(DEFAULT_ACTIONS)
                .map(|(a, _)| a)
                .unwrap_or_default()
        });
        actions
            .into_iter()
            .map(|a| (a.id, (a.title, a.message, a.confirm)))
            .collect()
    })
}

#[derive(Clone, Copy)]
enum Field {
    Title,
    Message,
    Confirm,
}

fn field(action: &Action, f: Field) -> Option<&str> {
    match f {
        Field::Title => Some(action.title.as_str()),
        Field::Message => action.message.as_deref(),
        Field::Confirm => action.confirm.as_deref(),
    }
}

fn is_default(action: &Action, f: Field) -> bool {
    let Some(d) = defaults().get(&action.id) else {
        return false;
    };
    let want = match f {
        Field::Title => Some(d.0.as_str()),
        Field::Message => d.1.as_deref(),
        Field::Confirm => d.2.as_deref(),
    };
    field(action, f).is_some() && field(action, f) == want
}

/// The action's title in the current language: `title_de` in German if
/// set; a shipped default title the user hasn't changed is translated;
/// anything the user wrote stays as it is.
pub fn title(action: &Action) -> String {
    if i18n::is_german() {
        if let Some(t) = &action.title_de {
            return t.clone();
        }
    }
    if is_default(action, Field::Title) {
        if let Some(t) = i18n::lookup(&format!("default.{}", action.id)) {
            return t.to_string();
        }
    }
    action.title.clone()
}

fn localized_text(action: &Action, f: Field, count: usize) -> Option<String> {
    let text = field(action, f)?;
    let (de, name) = match f {
        Field::Message => (&action.message_de, "message"),
        Field::Confirm => (&action.confirm_de, "confirm"),
        Field::Title => (&action.title_de, "title"),
    };
    if i18n::is_german() {
        if let Some(d) = de {
            return Some(fill_message(d, count, &title(action)));
        }
    }
    if is_default(action, f) {
        if let Some(t) = i18n::plural_key(&format!("default.{}.{name}", action.id), count) {
            return Some(t);
        }
    }
    Some(fill_message(text, count, &title(action)))
}

/// The success toast for `count` items, or `None`.
pub fn message(action: &Action, count: usize) -> Option<String> {
    localized_text(action, Field::Message, count)
}

/// The confirmation question for `count` items, or `None`.
pub fn confirm_text(action: &Action, count: usize) -> Option<String> {
    localized_text(action, Field::Confirm, count)
}
