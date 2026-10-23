//! The rename rules and the pipeline that applies them, in order, to one
//! file name.
//!
//! A name is split into a hidden-file dot, a stem and an extension
//! (`.env` → dot + `env`, no extension; `archive.tar.gz` → `archive.tar` +
//! `gz`; folders never have an extension). Text rules work on the stem, the
//! extension or both ([`Target`]); the leading dot of a dotfile is kept out
//! of every rule so a hidden file stays hidden.

use chrono::{Datelike, NaiveDateTime, Timelike};
use regex::{NoExpand, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::i18n::tr;

/// Which part of the name a text rule changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    #[default]
    Name,
    Extension,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertAt {
    #[default]
    Start,
    End,
    /// At character position `position` (0 = before the first character).
    Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveMode {
    #[default]
    First,
    Last,
    /// `count` characters starting at character `from` (0-based).
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseMode {
    #[default]
    Lower,
    Upper,
    Title,
    Sentence,
}

impl CaseMode {
    pub fn label(self) -> &'static str {
        match self {
            CaseMode::Lower => tr!("case.lower"),
            CaseMode::Upper => tr!("case.upper"),
            CaseMode::Title => tr!("case.title"),
            CaseMode::Sentence => tr!("case.sentence"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    #[default]
    Prefix,
    Suffix,
    /// The number replaces the name (numbering only).
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    #[default]
    Modified,
    Today,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtMode {
    #[default]
    Keep,
    Change,
    Lower,
    Upper,
    Remove,
}

impl ExtMode {
    pub fn label(self) -> &'static str {
        match self {
            ExtMode::Keep => tr!("ext.keep"),
            ExtMode::Change => tr!("ext.change"),
            ExtMode::Lower => tr!("case.lower"),
            ExtMode::Upper => tr!("case.upper"),
            ExtMode::Remove => tr!("ext.remove"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMode {
    #[default]
    Keep,
    Dash,
    Underscore,
}

/// Date format presets (tokens: `YYYY YY MM DD hh mm ss`).
pub const DATE_PRESETS: &[&str] = &[
    "YYYY-MM-DD",
    "YYYYMMDD",
    "DD.MM.YYYY",
    "YYYY-MM-DD hh.mm",
    "YYMMDD",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleKind {
    Replace {
        find: String,
        #[serde(default)]
        replace: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        case_sensitive: bool,
        /// All occurrences (else only the first).
        #[serde(default = "yes")]
        all: bool,
        #[serde(default)]
        target: Target,
    },
    Insert {
        text: String,
        #[serde(default)]
        at: InsertAt,
        #[serde(default)]
        position: usize,
        #[serde(default)]
        target: Target,
    },
    Remove {
        #[serde(default)]
        mode: RemoveMode,
        #[serde(default = "one")]
        count: usize,
        #[serde(default)]
        from: usize,
        #[serde(default)]
        target: Target,
    },
    Case {
        #[serde(default)]
        mode: CaseMode,
        #[serde(default)]
        target: Target,
    },
    Number {
        #[serde(default = "one_i")]
        start: i64,
        #[serde(default = "one_i")]
        step: i64,
        #[serde(default)]
        pad: usize,
        #[serde(default)]
        place: Place,
        #[serde(default = "space")]
        sep: String,
    },
    Date {
        #[serde(default)]
        source: DateSource,
        #[serde(default = "date_fmt")]
        format: String,
        #[serde(default)]
        place: Place,
        #[serde(default = "space")]
        sep: String,
    },
    Extension {
        #[serde(default)]
        mode: ExtMode,
        #[serde(default)]
        new_ext: String,
    },
    Clean {
        #[serde(default = "yes")]
        trim: bool,
        #[serde(default = "yes")]
        collapse: bool,
        #[serde(default)]
        spaces: SpaceMode,
        #[serde(default)]
        umlauts: bool,
        #[serde(default)]
        target: Target,
    },
}

fn yes() -> bool {
    true
}
fn one() -> usize {
    1
}
fn one_i() -> i64 {
    1
}
fn space() -> String {
    " ".into()
}
fn date_fmt() -> String {
    DATE_PRESETS[0].into()
}

/// One step of the pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(flatten)]
    pub kind: RuleKind,
}

/// The rule types, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleType {
    Replace,
    Insert,
    Remove,
    Case,
    Number,
    Date,
    Extension,
    Clean,
}

impl RuleType {
    pub const ALL: [RuleType; 8] = [
        RuleType::Replace,
        RuleType::Insert,
        RuleType::Remove,
        RuleType::Case,
        RuleType::Number,
        RuleType::Date,
        RuleType::Extension,
        RuleType::Clean,
    ];

    pub fn title(self) -> &'static str {
        match self {
            RuleType::Replace => tr!("rule.replace"),
            RuleType::Insert => tr!("rule.insert"),
            RuleType::Remove => tr!("rule.remove"),
            RuleType::Case => tr!("rule.case"),
            RuleType::Number => tr!("rule.number"),
            RuleType::Date => tr!("rule.date"),
            RuleType::Extension => tr!("rule.extension"),
            RuleType::Clean => tr!("rule.clean"),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            RuleType::Replace => tr!("rule.replace.hint"),
            RuleType::Insert => tr!("rule.insert.hint"),
            RuleType::Remove => tr!("rule.remove.hint"),
            RuleType::Case => tr!("rule.case.hint"),
            RuleType::Number => tr!("rule.number.hint"),
            RuleType::Date => tr!("rule.date.hint"),
            RuleType::Extension => tr!("rule.extension.hint"),
            RuleType::Clean => tr!("rule.clean.hint"),
        }
    }

    /// A new rule of this type with sensible defaults.
    pub fn new_rule(self) -> Rule {
        let kind = match self {
            RuleType::Replace => RuleKind::Replace {
                find: String::new(),
                replace: String::new(),
                regex: false,
                case_sensitive: false,
                all: true,
                target: Target::Name,
            },
            RuleType::Insert => RuleKind::Insert {
                text: String::new(),
                at: InsertAt::Start,
                position: 0,
                target: Target::Name,
            },
            RuleType::Remove => RuleKind::Remove {
                mode: RemoveMode::First,
                count: 1,
                from: 0,
                target: Target::Name,
            },
            RuleType::Case => RuleKind::Case {
                mode: CaseMode::Lower,
                target: Target::Name,
            },
            RuleType::Number => RuleKind::Number {
                start: 1,
                step: 1,
                pad: 2,
                place: Place::Prefix,
                sep: " ".into(),
            },
            RuleType::Date => RuleKind::Date {
                source: DateSource::Modified,
                format: DATE_PRESETS[0].into(),
                place: Place::Prefix,
                sep: " ".into(),
            },
            RuleType::Extension => RuleKind::Extension {
                mode: ExtMode::Lower,
                new_ext: String::new(),
            },
            RuleType::Clean => RuleKind::Clean {
                trim: true,
                collapse: true,
                spaces: SpaceMode::Keep,
                umlauts: false,
                target: Target::Name,
            },
        };
        Rule {
            enabled: true,
            kind,
        }
    }
}

impl RuleKind {
    pub fn rule_type(&self) -> RuleType {
        match self {
            RuleKind::Replace { .. } => RuleType::Replace,
            RuleKind::Insert { .. } => RuleType::Insert,
            RuleKind::Remove { .. } => RuleType::Remove,
            RuleKind::Case { .. } => RuleType::Case,
            RuleKind::Number { .. } => RuleType::Number,
            RuleKind::Date { .. } => RuleType::Date,
            RuleKind::Extension { .. } => RuleType::Extension,
            RuleKind::Clean { .. } => RuleType::Clean,
        }
    }
}

// ------------------------------------------------------------------ names

/// A file name in parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameParts {
    /// The name starts with "." (a hidden file); not part of `stem`.
    pub dot: bool,
    pub stem: String,
    pub ext: Option<String>,
}

impl NameParts {
    pub fn split(name: &str, is_dir: bool) -> NameParts {
        let (dot, rest) = match name.strip_prefix('.') {
            Some(r) if !r.is_empty() => (true, r),
            _ => (false, name),
        };
        if is_dir {
            return NameParts {
                dot,
                stem: rest.to_string(),
                ext: None,
            };
        }
        match rest.rfind('.') {
            Some(i) if i > 0 && i + 1 < rest.len() => NameParts {
                dot,
                stem: rest[..i].to_string(),
                ext: Some(rest[i + 1..].to_string()),
            },
            _ => NameParts {
                dot,
                stem: rest.to_string(),
                ext: None,
            },
        }
    }

    pub fn join(&self) -> String {
        let mut s = String::new();
        if self.dot {
            s.push('.');
        }
        s.push_str(&self.stem);
        if let Some(e) = &self.ext {
            s.push('.');
            s.push_str(e);
        }
        s
    }

    /// Stem plus extension, without the leading dot.
    fn body(&self) -> String {
        match &self.ext {
            Some(e) => format!("{}.{e}", self.stem),
            None => self.stem.clone(),
        }
    }
}

/// What a rule needs to know about the item it renames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemCtx {
    /// Position in the list (0-based), for numbering.
    pub index: usize,
    pub is_dir: bool,
    /// Modification time in local time, if known.
    pub modified: Option<NaiveDateTime>,
    /// "Now" in local time.
    pub today: NaiveDateTime,
}

/// Rules ready to run: regexes compiled once per change of the rules.
pub struct Compiled<'a> {
    rules: &'a [Rule],
    regexes: Vec<Option<Regex>>,
    /// (rule index, message) for rules that can't run (bad regex).
    pub errors: Vec<(usize, String)>,
}

/// `$1x` means group "1x" to the regex crate; make it `${1}x` like most
/// tools do. `$$` stays a literal dollar.
fn fix_group_refs(rep: &str) -> String {
    let mut out = String::with_capacity(rep.len() + 4);
    let mut chars = rep.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push_str("$$");
            }
            Some(d) if d.is_ascii_digit() => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str(&format!("${{{num}}}"));
            }
            _ => out.push('$'),
        }
    }
    out
}

/// Short, readable regex error (the crate's message is multi-line).
fn regex_error(e: &regex::Error) -> String {
    let msg = e.to_string();
    let last = msg
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("invalid regex");
    let last = last.trim().trim_start_matches("error: ");
    let mut s = last.to_string();
    if let Some(f) = s.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    s
}

pub fn compile(rules: &[Rule]) -> Compiled<'_> {
    let mut regexes = Vec::with_capacity(rules.len());
    let mut errors = Vec::new();
    for (i, r) in rules.iter().enumerate() {
        let re = match &r.kind {
            RuleKind::Replace {
                find,
                regex,
                case_sensitive,
                ..
            } if !find.is_empty() => {
                let pat = if *regex {
                    find.clone()
                } else {
                    regex::escape(find)
                };
                match RegexBuilder::new(&pat)
                    .case_insensitive(!case_sensitive)
                    .build()
                {
                    Ok(re) => Some(re),
                    Err(e) => {
                        if r.enabled {
                            errors.push((i, regex_error(&e)));
                        }
                        None
                    }
                }
            }
            _ => None,
        };
        regexes.push(re);
    }
    Compiled {
        rules,
        regexes,
        errors,
    }
}

impl Compiled<'_> {
    /// The new name of `name` after every enabled rule, in order.
    pub fn apply(&self, name: &str, ctx: &ItemCtx) -> String {
        let mut parts = NameParts::split(name, ctx.is_dir);
        for (i, r) in self.rules.iter().enumerate() {
            if r.enabled {
                apply_rule(&mut parts, &r.kind, self.regexes[i].as_ref(), ctx);
            }
        }
        parts.join()
    }
}

/// Run `f` on the part(s) of the name `target` selects.
fn on_target(parts: &mut NameParts, target: Target, is_dir: bool, f: impl Fn(&str) -> String) {
    match target {
        Target::Name => parts.stem = f(&parts.stem),
        Target::Extension => {
            if let Some(e) = &parts.ext {
                parts.ext = Some(f(e));
            }
        }
        Target::Both => {
            let new = f(&parts.body());
            let dot = parts.dot;
            *parts = NameParts::split(&new, is_dir);
            parts.dot |= dot;
        }
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of char index `i` (clamped).
fn byte_at(s: &str, i: usize) -> usize {
    s.char_indices().nth(i).map_or(s.len(), |(b, _)| b)
}

fn apply_rule(parts: &mut NameParts, kind: &RuleKind, re: Option<&Regex>, ctx: &ItemCtx) {
    let dir = ctx.is_dir;
    match kind {
        RuleKind::Replace {
            replace,
            regex,
            all,
            target,
            ..
        } => {
            let Some(re) = re else { return };
            let rep = fix_group_refs(replace);
            on_target(parts, *target, dir, |s| {
                let limit = if *all { 0 } else { 1 };
                if *regex {
                    re.replacen(s, limit, rep.as_str()).into_owned()
                } else {
                    re.replacen(s, limit, NoExpand(replace)).into_owned()
                }
            });
        }
        RuleKind::Insert {
            text,
            at,
            position,
            target,
        } => {
            if text.is_empty() {
                return;
            }
            on_target(parts, *target, dir, |s| match at {
                InsertAt::Start => format!("{text}{s}"),
                InsertAt::End => format!("{s}{text}"),
                InsertAt::Position => {
                    let b = byte_at(s, *position);
                    format!("{}{text}{}", &s[..b], &s[b..])
                }
            });
        }
        RuleKind::Remove {
            mode,
            count,
            from,
            target,
        } => {
            on_target(parts, *target, dir, |s| {
                let n = char_len(s);
                let (a, b) = match mode {
                    RemoveMode::First => (0, (*count).min(n)),
                    RemoveMode::Last => (n.saturating_sub(*count), n),
                    RemoveMode::Range => ((*from).min(n), from.saturating_add(*count).min(n)),
                };
                let (a, b) = (byte_at(s, a), byte_at(s, b));
                format!("{}{}", &s[..a], &s[b..])
            });
        }
        RuleKind::Case { mode, target } => {
            on_target(parts, *target, dir, |s| change_case(s, *mode))
        }
        RuleKind::Number {
            start,
            step,
            pad,
            place,
            sep,
        } => {
            let n = start.saturating_add(step.saturating_mul(ctx.index as i64));
            let num = format_number(n, *pad);
            parts.stem = match place {
                Place::Prefix => format!("{num}{sep}{}", parts.stem),
                Place::Suffix => format!("{}{sep}{num}", parts.stem),
                Place::Replace => num,
            };
        }
        RuleKind::Date {
            source,
            format,
            place,
            sep,
        } => {
            let when = match source {
                DateSource::Modified => ctx.modified,
                DateSource::Today => Some(ctx.today),
            };
            let Some(when) = when else { return };
            let d = format_date(&when, format);
            if d.is_empty() {
                return;
            }
            parts.stem = match place {
                Place::Prefix => format!("{d}{sep}{}", parts.stem),
                Place::Suffix | Place::Replace => format!("{}{sep}{d}", parts.stem),
            };
        }
        RuleKind::Extension { mode, new_ext } => {
            if dir {
                return;
            }
            match mode {
                ExtMode::Keep => {}
                ExtMode::Change => {
                    let e = new_ext.trim().trim_start_matches('.');
                    parts.ext = (!e.is_empty()).then(|| e.to_string());
                }
                ExtMode::Lower => parts.ext = parts.ext.as_ref().map(|e| e.to_lowercase()),
                ExtMode::Upper => parts.ext = parts.ext.as_ref().map(|e| e.to_uppercase()),
                ExtMode::Remove => parts.ext = None,
            }
        }
        RuleKind::Clean {
            trim,
            collapse,
            spaces,
            umlauts,
            target,
        } => {
            on_target(parts, *target, dir, |s| {
                clean(s, *trim, *collapse, *spaces, *umlauts)
            });
        }
    }
}

pub fn format_number(n: i64, pad: usize) -> String {
    let digits = format!("{:0>pad$}", n.unsigned_abs(), pad = pad.min(12));
    if n < 0 {
        format!("-{digits}")
    } else {
        digits
    }
}

/// Format with the tokens `YYYY YY MM DD hh mm ss`; everything else is
/// literal.
pub fn format_date(t: &NaiveDateTime, fmt: &str) -> String {
    let mut out = String::new();
    let mut rest = fmt;
    while !rest.is_empty() {
        let (tok, val) = if rest.starts_with("YYYY") {
            ("YYYY", format!("{:04}", t.year()))
        } else if rest.starts_with("YY") {
            ("YY", format!("{:02}", t.year().rem_euclid(100)))
        } else if rest.starts_with("MM") {
            ("MM", format!("{:02}", t.month()))
        } else if rest.starts_with("DD") {
            ("DD", format!("{:02}", t.day()))
        } else if rest.starts_with("hh") {
            ("hh", format!("{:02}", t.hour()))
        } else if rest.starts_with("mm") {
            ("mm", format!("{:02}", t.minute()))
        } else if rest.starts_with("ss") {
            ("ss", format!("{:02}", t.second()))
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        };
        out.push_str(&val);
        rest = &rest[tok.len()..];
    }
    out
}

fn is_word_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | '.' | '(' | '[' | '{' | '/' | '+' | ',')
}

pub fn change_case(s: &str, mode: CaseMode) -> String {
    match mode {
        CaseMode::Lower => s.to_lowercase(),
        CaseMode::Upper => s.to_uppercase(),
        CaseMode::Title => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    out.extend(c.to_lowercase());
                    if is_word_sep(c) {
                        start = true;
                    } else if c.is_alphanumeric() {
                        start = false;
                    }
                }
            }
            out
        }
        CaseMode::Sentence => {
            let mut out = String::with_capacity(s.len());
            let mut done = false;
            for c in s.chars() {
                if !done && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    done = true;
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            out
        }
    }
}

/// German umlauts become two letters (ä → ae, ß → ss); other accented
/// Latin letters lose the accent (é → e). Handles decomposed (NFD) input.
pub fn transliterate(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        // Decomposed: base letter + combining mark.
        if let Some(&m) = chars.peek() {
            if m == '\u{308}' && matches!(c, 'a' | 'o' | 'u' | 'A' | 'O' | 'U') {
                chars.next();
                out.push(c);
                out.push(
                    if c.is_uppercase() && chars.peek().is_some_and(|n| n.is_uppercase()) {
                        'E'
                    } else {
                        'e'
                    },
                );
                continue;
            }
        }
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue; // other combining marks
        }
        let next_upper = chars.peek().is_some_and(|n| n.is_uppercase());
        let rep: Option<&str> = match c {
            'ä' => Some("ae"),
            'ö' => Some("oe"),
            'ü' => Some("ue"),
            'Ä' => Some(if next_upper { "AE" } else { "Ae" }),
            'Ö' => Some(if next_upper { "OE" } else { "Oe" }),
            'Ü' => Some(if next_upper { "UE" } else { "Ue" }),
            'ß' => Some("ss"),
            'ẞ' => Some("SS"),
            'æ' => Some("ae"),
            'Æ' => Some("AE"),
            'œ' => Some("oe"),
            'Œ' => Some("OE"),
            'ø' => Some("o"),
            'Ø' => Some("O"),
            'å' | 'á' | 'à' | 'â' | 'ã' | 'ā' => Some("a"),
            'Å' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ā' => Some("A"),
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ę' => Some("e"),
            'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ę' => Some("E"),
            'í' | 'ì' | 'î' | 'ï' => Some("i"),
            'Í' | 'Ì' | 'Î' | 'Ï' => Some("I"),
            'ó' | 'ò' | 'ô' | 'õ' => Some("o"),
            'Ó' | 'Ò' | 'Ô' | 'Õ' => Some("O"),
            'ú' | 'ù' | 'û' => Some("u"),
            'Ú' | 'Ù' | 'Û' => Some("U"),
            'ñ' | 'ń' => Some("n"),
            'Ñ' | 'Ń' => Some("N"),
            'ç' | 'ć' | 'č' => Some("c"),
            'Ç' | 'Ć' | 'Č' => Some("C"),
            'š' | 'ś' => Some("s"),
            'Š' | 'Ś' => Some("S"),
            'ž' | 'ź' | 'ż' => Some("z"),
            'Ž' | 'Ź' | 'Ż' => Some("Z"),
            'ý' | 'ÿ' => Some("y"),
            'Ý' => Some("Y"),
            'ł' => Some("l"),
            'Ł' => Some("L"),
            _ => None,
        };
        match rep {
            Some(r) => out.push_str(r),
            None => out.push(c),
        }
    }
    out
}

pub fn clean(s: &str, trim: bool, collapse: bool, spaces: SpaceMode, umlauts: bool) -> String {
    let mut t = if umlauts {
        transliterate(s)
    } else {
        s.to_string()
    };
    if trim {
        t = t.trim().to_string();
    }
    if collapse {
        let mut out = String::with_capacity(t.len());
        let mut prev_ws = false;
        for c in t.chars() {
            if c.is_whitespace() {
                if !prev_ws {
                    out.push(' ');
                }
                prev_ws = true;
            } else {
                out.push(c);
                prev_ws = false;
            }
        }
        t = out;
    }
    match spaces {
        SpaceMode::Keep => t,
        SpaceMode::Dash => t.replace(' ', "-"),
        SpaceMode::Underscore => t.replace(' ', "_"),
    }
}

/// A short description of a rule for its card when collapsed / tooltips.
pub fn summary(rule: &Rule) -> String {
    match &rule.kind {
        RuleKind::Replace { find, replace, .. } if !find.is_empty() => {
            tr!("summary.replace", find = find, replace = replace)
        }
        RuleKind::Replace { .. } => tr!("summary.nothing_to_find").into(),
        RuleKind::Insert { text, .. } => tr!("summary.insert", text = text),
        RuleKind::Remove { count, .. } => tr!("summary.remove", count = *count),
        RuleKind::Case { mode, .. } => tr!("summary.case", mode = mode.label()),
        RuleKind::Number { start, step, .. } => tr!("summary.number", start = start, step = step),
        RuleKind::Date { format, .. } => format.clone(),
        RuleKind::Extension { mode, .. } => tr!("summary.extension", mode = mode.label()),
        RuleKind::Clean { .. } => tr!("summary.clean").into(),
    }
}
