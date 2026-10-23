//! Localisation helpers shared by the app and the built-in plugins.
//!
//! Each catalog has a source table `(key, English, German)` plus one
//! translation table `(key, text)` per further language (Chinese, Spanish,
//! French, Italian, Russian). Plugins wrap theirs in a [`Catalog`]; a
//! three-line `tr!` macro in the plugin forwards to
//! [`tr_with!`](crate::tr_with):
//!
//! ```ignore
//! // src/i18n.rs of a plugin (translations in src/i18n/<lang>.rs)
//! use manta_plugin::i18n::{Catalog, Entries, Lang};
//!
//! mod es; mod fr; mod it; mod ru; mod zh;
//!
//! pub static CATALOG: Catalog = Catalog::with(ENTRIES, &[
//!     (Lang::Zh, zh::ENTRIES), (Lang::Es, es::ENTRIES), (Lang::Fr, fr::ENTRIES),
//!     (Lang::It, it::ENTRIES), (Lang::Ru, ru::ENTRIES),
//! ]);
//! const ENTRIES: Entries = &[
//!     ("panel.title", "Disk Usage", "Speicherbelegung"),
//!     ("files.one", "{count} file", "{count} Datei"),
//!     ("files.other", "{count} files", "{count} Dateien"),
//! ];
//!
//! macro_rules! tr {
//!     ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
//! }
//! pub(crate) use tr;
//!
//! // anywhere in the plugin:
//! tr!("panel.title")                   // → &'static str
//! tr!("files", count = n)              // → String, CLDR plural form
//! tr!("error", name = file_name)       // → String
//! ```
//!
//! Plurals use the CLDR categories of each language ([`Lang::plural`]):
//! the source table has `key.one` / `key.other`; Russian uses `key.one`,
//! `key.few` and `key.many` (and `key.other` for fractions), Chinese only
//! `key.other`, French uses `key.one` for 0 and 1. Translations still fill
//! every form ([`Lang::plural_forms`]: Chinese `key.one` too). A key
//! missing in a translation falls back to English.
//!
//! The language is the app's ([`crate::locale`]), read through a lock-free
//! cache so calling `tr!` every frame is cheap. Tests and renders can force
//! a language for the current thread with [`with_lang`] / [`set_thread_lang`].
//!
//! Style: Apple's macOS terminology in every language. German: informal
//! "du", „…“ quotes, a space before "…" ("Öffnen …"), decimal comma.
//!
//! [`check`] has the catalog checks every plugin runs in its tests: keys
//! unique, every language translated, placeholders equal, plurals complete,
//! every key used in the source present, and no hard-coded UI string
//! literals.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// The source table format: `(key, English, German)`. Plurals are two
/// entries, `key.one` and `key.other`; placeholders are `{name}`.
pub type Entries = &'static [(&'static str, &'static str, &'static str)];

/// One language's translations: `(key, text)`, keys as in the source
/// table (plural keys per [`Lang::plural_forms`]).
pub type Translations = &'static [(&'static str, &'static str)];

/// Number of supported languages.
pub const LANG_COUNT: usize = 7;

/// A supported UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    En = 0,
    De = 1,
    /// Simplified Chinese.
    Zh = 2,
    Es = 3,
    Fr = 4,
    It = 5,
    Ru = 6,
}

/// A CLDR plural category (integers only; "zero" and "two" are unused by
/// the supported languages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Plural {
    One,
    Few,
    Many,
    Other,
}

impl Plural {
    /// The key suffix: `"one"`, `"few"`, `"many"`, `"other"`.
    pub fn suffix(self) -> &'static str {
        match self {
            Plural::One => "one",
            Plural::Few => "few",
            Plural::Many => "many",
            Plural::Other => "other",
        }
    }
}

impl Lang {
    /// All languages, in the order of the Language picker after "System".
    pub const ALL: [Lang; LANG_COUNT] = [
        Lang::De,
        Lang::En,
        Lang::Zh,
        Lang::Es,
        Lang::Fr,
        Lang::It,
        Lang::Ru,
    ];

    /// The languages with separate translation tables (English and German
    /// are the source table).
    pub const TRANSLATED: [Lang; 5] = [Lang::Zh, Lang::Es, Lang::Fr, Lang::It, Lang::Ru];

    /// BCP 47 code: "en", "de", "zh-Hans", "es", "fr", "it", "ru". Sent to
    /// plugins and used for the manual.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Zh => "zh-Hans",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::It => "it",
            Lang::Ru => "ru",
        }
    }

    /// The language's name in itself ("Deutsch", "简体中文"), like the
    /// macOS language list.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Zh => "简体中文",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::It => "Italiano",
            Lang::Ru => "Русский",
        }
    }

    /// Best supported match for a BCP 47 / POSIX tag ("de-DE", "en_US",
    /// "zh-Hans-CN", "zh_CN"). Traditional Chinese (zh-Hant, zh-TW, zh-HK,
    /// zh-MO) and unknown languages give `None`.
    pub fn parse(tag: &str) -> Option<Lang> {
        let mut parts = tag.split(['-', '_', '.', '@']).map(str::to_ascii_lowercase);
        let lang = parts.next()?;
        Some(match lang.as_str() {
            "en" => Lang::En,
            "de" => Lang::De,
            "es" => Lang::Es,
            "fr" => Lang::Fr,
            "it" => Lang::It,
            "ru" => Lang::Ru,
            "zh" => {
                if parts.any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo")) {
                    return None;
                }
                Lang::Zh
            }
            _ => return None,
        })
    }

    /// [`Lang::parse`], English for anything unsupported.
    pub fn from_code(code: &str) -> Lang {
        Lang::parse(code).unwrap_or(Lang::En)
    }

    /// The language with discriminant `i` (English when out of range).
    pub fn from_index(i: u8) -> Lang {
        match i {
            1 => Lang::De,
            2 => Lang::Zh,
            3 => Lang::Es,
            4 => Lang::Fr,
            5 => Lang::It,
            6 => Lang::Ru,
            _ => Lang::En,
        }
    }

    /// CLDR plural category of the integer `n`.
    pub fn plural(self, n: u64) -> Plural {
        match self {
            Lang::Zh => Plural::Other,
            // French: 0 and 1 are singular ("0 élément").
            Lang::Fr if n <= 1 => Plural::One,
            Lang::Ru => {
                let (m10, m100) = (n % 10, n % 100);
                if m10 == 1 && m100 != 11 {
                    Plural::One
                } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                    Plural::Few
                } else {
                    Plural::Many
                }
            }
            _ if n == 1 => Plural::One,
            _ => Plural::Other,
        }
    }

    /// The plural keys a translation must have for every plural entry: the
    /// forms [`Lang::plural`] picks, plus the source's `one` / `other`
    /// where the language doesn't use them for whole numbers (Chinese
    /// `one`, Russian `other` for fractions), so no form is ever English.
    pub fn plural_forms(self) -> &'static [Plural] {
        match self {
            Lang::Zh => &[Plural::One, Plural::Other],
            Lang::Ru => &[Plural::One, Plural::Few, Plural::Many, Plural::Other],
            _ => &[Plural::One, Plural::Other],
        }
    }

    /// Decimal separator: "1.5" / "1,5".
    pub fn decimal_sep(self) -> char {
        match self {
            Lang::En | Lang::Zh => '.',
            _ => ',',
        }
    }

    /// Thousands separator: "12,345", "12.345", "12 345" (French: narrow
    /// no-break space, Russian: no-break space).
    pub fn group_sep(self) -> char {
        match self {
            Lang::En | Lang::Zh => ',',
            Lang::De | Lang::Es | Lang::It => '.',
            Lang::Fr => '\u{202F}',
            Lang::Ru => '\u{A0}',
        }
    }

    /// `name` in the language's quotation marks: “a”, „a“, « a », «a».
    pub fn quote(self, name: &str) -> String {
        match self {
            Lang::De => format!("„{name}“"),
            Lang::Fr => format!("«\u{A0}{name}\u{A0}»"),
            Lang::Ru => format!("«{name}»"),
            _ => format!("“{name}”"),
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// Called by [`crate::set_global_locale`].
pub(crate) fn set_global(code: &str) {
    LANG.store(Lang::from_code(code) as u8, Ordering::Relaxed);
}

/// The active language: this thread's override, else the app's.
pub fn lang() -> Lang {
    if let Some(l) = OVERRIDE.with(Cell::get) {
        return l;
    }
    Lang::from_index(LANG.load(Ordering::Relaxed))
}

/// This thread's forced language, if any (see [`set_thread_lang`]).
pub fn thread_lang() -> Option<Lang> {
    OVERRIDE.with(Cell::get)
}

/// Run `f` with `l` as this thread's language (tests, renders).
pub fn with_lang<R>(l: Lang, f: impl FnOnce() -> R) -> R {
    let prev = OVERRIDE.with(|o| o.replace(Some(l)));
    struct Reset(Option<Lang>);
    impl Drop for Reset {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _reset = Reset(prev);
    f()
}

/// Force this thread's language until called again with `None`.
pub fn set_thread_lang(l: Option<Lang>) {
    OVERRIDE.with(|o| o.set(l));
}

// -------------------------------------------------------------------- table

/// A key's text per language (`""`: not translated).
pub type Row = [&'static str; LANG_COUNT];

/// A lookup table built from a source table and translation tables.
pub type Table = HashMap<&'static str, Row>;

/// Build the lookup table: every source entry, plus every translation
/// (also keys only translations have, like Russian `key.few`).
pub fn build_table<'a>(
    source: impl IntoIterator<Item = &'a (&'static str, &'static str, &'static str)>,
    translations: impl IntoIterator<Item = (Lang, Translations)>,
) -> Table {
    let mut map: Table = HashMap::new();
    for &(k, en, de) in source {
        let row = map.entry(k).or_insert([""; LANG_COUNT]);
        row[Lang::En as usize] = en;
        row[Lang::De as usize] = de;
    }
    for (l, t) in translations {
        for &(k, s) in t {
            map.entry(k).or_insert([""; LANG_COUNT])[l as usize] = s;
        }
    }
    map
}

/// `key` in `l`, falling back to English; `None` for unknown keys.
pub fn lookup(table: &Table, l: Lang, key: &str) -> Option<&'static str> {
    let row = table.get(key)?;
    match row[l as usize] {
        "" => match row[Lang::En as usize] {
            "" => None,
            en => Some(en),
        },
        s => Some(s),
    }
}

/// The plural form of `key` for `n` in `l`: `key.<category>`, else
/// `key.other` in `l`, else English (`key.one` / `key.other`). `""` when
/// the key is unknown.
pub fn lookup_plural(table: &Table, l: Lang, key: &str, n: u64) -> &'static str {
    let get = |suffix: &str, l: Lang| -> Option<&'static str> {
        let s = table.get(format!("{key}.{suffix}").as_str())?[l as usize];
        (!s.is_empty()).then_some(s)
    };
    get(l.plural(n).suffix(), l)
        .or_else(|| get("other", l))
        .or_else(|| get(Lang::En.plural(n).suffix(), Lang::En))
        .unwrap_or("")
}

/// A plugin's string table. Build it in a `static` with [`Catalog::new`]
/// (English and German only) or [`Catalog::with`].
pub struct Catalog {
    entries: Entries,
    translations: &'static [(Lang, Translations)],
    map: OnceLock<Table>,
}

impl Catalog {
    pub const fn new(entries: Entries) -> Catalog {
        Catalog {
            entries,
            translations: &[],
            map: OnceLock::new(),
        }
    }

    /// A catalog with translation tables for further languages.
    pub const fn with(entries: Entries, translations: &'static [(Lang, Translations)]) -> Catalog {
        Catalog {
            entries,
            translations,
            map: OnceLock::new(),
        }
    }

    pub fn entries(&self) -> Entries {
        self.entries
    }

    pub fn translations(&self) -> &'static [(Lang, Translations)] {
        self.translations
    }

    fn map(&self) -> &Table {
        self.map
            .get_or_init(|| build_table(self.entries.iter(), self.translations.iter().copied()))
    }

    /// The string for `key` in the active language; the key itself when
    /// it's missing (the checks make sure none is).
    pub fn tr(&self, key: &'static str) -> &'static str {
        self.tr_in(lang(), key)
    }

    pub fn tr_in(&self, l: Lang, key: &'static str) -> &'static str {
        lookup(self.map(), l, key).unwrap_or(key)
    }

    /// Look up a key built at runtime; `""` when unknown.
    pub fn tr_key(&self, key: &str) -> &'static str {
        lookup(self.map(), lang(), key).unwrap_or("")
    }

    pub fn has(&self, key: &str) -> bool {
        self.map().contains_key(key)
    }

    /// `key` with `{name}` placeholders filled in.
    pub fn fmt(&self, key: &'static str, args: &[(&str, &dyn Display)]) -> String {
        fill(self.tr(key), args)
    }

    /// [`Catalog::plural`] for a key built at runtime; `None` when unknown.
    pub fn plural_key(&self, key: &str, n: u64) -> Option<String> {
        let s = lookup_plural(self.map(), lang(), key, n);
        (!s.is_empty()).then(|| fill(s, &[("count", &format_count(n))]))
    }

    /// The plural form of `key` for `n` (see [`lookup_plural`]); `{count}`
    /// is `n` with the language's thousands separator.
    pub fn plural(&self, key: &'static str, n: u64, args: &[(&str, &dyn Display)]) -> String {
        let count = format_count(n);
        let mut all: Vec<(&str, &dyn Display)> = vec![("count", &count)];
        all.extend_from_slice(args);
        fill(lookup_plural(self.map(), lang(), key, n), &all)
    }
}

/// Translate with a plugin's catalog. Plugins wrap this in their own `tr!`
/// (see the module docs):
/// `tr_with!(CAT, "key")`, `tr_with!(CAT, "key", count = n)`,
/// `tr_with!(CAT, "key", name = value, …)`.
#[macro_export]
macro_rules! tr_with {
    ($cat:path, $key:literal) => {
        $cat.tr($key)
    };
    ($cat:path, $key:literal, count = $n:expr $(, $name:ident = $val:expr)* $(,)?) => {
        $cat.plural($key, ($n) as u64, &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),*])
    };
    ($cat:path, $key:literal $(, $name:ident = $val:expr)+ $(,)?) => {
        $cat.fmt($key, &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+])
    };
}

/// Replace `{name}` placeholders. Unknown ones stay as they are.
pub fn fill(s: &str, args: &[(&str, &dyn Display)]) -> String {
    if args.is_empty() || !s.contains('{') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 16);
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, v)) => {
                        use std::fmt::Write;
                        let _ = write!(out, "{v}");
                    }
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------ numbers

/// Decimal separator of the active language.
pub fn decimal_sep() -> char {
    lang().decimal_sep()
}

/// `n` with thousands separators: "12,345" / "12.345" / "12 345".
pub fn format_count(n: u64) -> String {
    format_count_in(lang(), n)
}

/// [`format_count`] for language `l`. Spanish leaves four-digit numbers
/// alone ("1234", "12.345"), like CLDR.
pub fn format_count_in(l: Lang, n: u64) -> String {
    let s = n.to_string();
    if l == Lang::Es && s.len() <= 4 {
        return s;
    }
    let sep = l.group_sep();
    let mut out = String::with_capacity(s.len() + s.len() / 3 * 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// `value` with `decimals` places and the language's decimal separator.
pub fn format_decimal(value: f64, decimals: usize) -> String {
    let s = format!("{value:.decimals$}");
    match decimal_sep() {
        '.' => s,
        c => s.replace('.', &c.to_string()),
    }
}

/// Swap the decimal point of an already formatted number ("12.4 MB") for
/// the language's separator. Only touches dots between two digits.
pub fn localize_decimal(s: &str) -> String {
    let sep = decimal_sep();
    if sep == '.' {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let digit = |j: Option<usize>| {
                j.and_then(|j| chars.get(j))
                    .is_some_and(|c| c.is_ascii_digit())
            };
            if c == '.' && digit(i.checked_sub(1)) && digit(Some(i + 1)) {
                sep
            } else {
                c
            }
        })
        .collect()
}

/// The size unit `i` (0 = KB … 5 = EB) like Finder in the active language:
/// "MB", French "Mo", Russian "МБ".
pub fn size_unit(i: usize) -> &'static str {
    const LATIN: [&str; 6] = ["KB", "MB", "GB", "TB", "PB", "EB"];
    const FR: [&str; 6] = ["Ko", "Mo", "Go", "To", "Po", "Eo"];
    const RU: [&str; 6] = ["КБ", "МБ", "ГБ", "ТБ", "ПБ", "ЭБ"];
    let i = i.min(5);
    match lang() {
        Lang::Fr => FR[i],
        Lang::Ru => RU[i],
        _ => LATIN[i],
    }
}

/// Localize the units of an already formatted size ("12.4 MB" →
/// "12,4 Mo"): the decimal separator and KB…EB.
pub fn localize_size(s: &str) -> String {
    let s = localize_decimal(s);
    if !matches!(lang(), Lang::Fr | Lang::Ru) {
        return s;
    }
    const LATIN: [&str; 6] = ["KB", "MB", "GB", "TB", "PB", "EB"];
    let mut out = s;
    for (i, u) in LATIN.iter().enumerate() {
        if out.contains(u) {
            out = out.replace(u, size_unit(i));
        }
    }
    out
}

/// "{n} bytes" in the active language ("1 byte", "2 octets", "3 байта").
pub fn format_bytes(n: u64) -> String {
    let l = lang();
    let word = match (l, l.plural(n)) {
        (Lang::En | Lang::Es, Plural::One) => "byte",
        (Lang::En | Lang::Es, _) => "bytes",
        (Lang::De, _) => "Byte",
        (Lang::Zh, _) => "字节",
        (Lang::Fr, Plural::One) => "octet",
        (Lang::Fr, _) => "octets",
        (Lang::It, _) => "byte",
        (Lang::Ru, Plural::Few) => "байта",
        (Lang::Ru, _) => "байт",
    };
    format!("{}{}{word}", format_count_in(l, n), unit_space(l))
}

/// The space between a number and its unit: a no-break space in French
/// and Russian (so "12 Mo" never wraps), else a normal one.
pub fn unit_space(l: Lang) -> char {
    match l {
        Lang::Fr | Lang::Ru => '\u{A0}',
        _ => ' ',
    }
}

/// Human readable size with decimal units like Finder: "1.2 MB", German
/// "1,2 MB", French "1,2 Mo", Russian "1,2 МБ"; below 1000 bytes
/// [`format_bytes`]. One decimal below 10, none above.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1000 {
        return format_bytes(bytes);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    for i in 0..6 {
        value /= 1000.0;
        unit = i;
        if value < 1000.0 {
            break;
        }
    }
    let decimals = if value < 10.0 { 1 } else { 0 };
    format!(
        "{}{}{}",
        format_decimal(value, decimals),
        unit_space(lang()),
        size_unit(unit)
    )
}

// -------------------------------------------------------------------- dates

/// Abbreviated month name (`month0` 0…11) like macOS shows in Finder's
/// list: "Oct", "Okt.", "oct", "oct.", "ott", "окт."; Chinese "10月".
pub fn month_abbr(l: Lang, month0: u32) -> &'static str {
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const DE: [&str; 12] = [
        "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.",
        "Dez.",
    ];
    const ZH: [&str; 12] = [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ];
    const ES: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
    ];
    const FR: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    const IT: [&str; 12] = [
        "gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic",
    ];
    // Genitive, as in "12 окт.".
    const RU: [&str; 12] = [
        "янв.",
        "февр.",
        "мар.",
        "апр.",
        "мая",
        "июн.",
        "июл.",
        "авг.",
        "сент.",
        "окт.",
        "нояб.",
        "дек.",
    ];
    let i = (month0 as usize).min(11);
    match l {
        Lang::En => EN[i],
        Lang::De => DE[i],
        Lang::Zh => ZH[i],
        Lang::Es => ES[i],
        Lang::Fr => FR[i],
        Lang::It => IT[i],
        Lang::Ru => RU[i],
    }
}

/// A short date in `l` (`month0` 0…11): "12 Oct 2024", "12. Okt. 2024",
/// "2024年10月12日", "12 oct. 2024", "12 окт. 2024 г."; without the year
/// when `year` is `None`. `time` ("14:02") follows after a space.
pub fn format_day(l: Lang, day: u32, month0: u32, year: Option<i32>, time: Option<&str>) -> String {
    let m = month_abbr(l, month0);
    let mut s = match (l, year) {
        (Lang::Zh, Some(y)) => format!("{y}年{m}{day}日"),
        (Lang::Zh, None) => format!("{m}{day}日"),
        (Lang::De, Some(y)) => format!("{day}. {m} {y}"),
        (Lang::De, None) => format!("{day}. {m}"),
        (Lang::Ru, Some(y)) => format!("{day} {m} {y} г."),
        (_, Some(y)) => format!("{day} {m} {y}"),
        (_, None) => format!("{day} {m}"),
    };
    if let Some(t) = time {
        s.push(' ');
        s.push_str(t);
    }
    s
}

// ------------------------------------------------------------------- checks

/// Catalog and source checks for plugin tests.
pub mod check {
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::path::{Path, PathBuf};

    use super::{Catalog, Lang};

    /// English words that stay the same in German. Plugins pass their own
    /// extra list to [`catalog_problems`].
    pub const SAME_IN_GERMAN: &[&str] = &[
        "OK", "Git", "Name", "Status", "Tab", "System", "Plugin", "Plugins", "Terminal", "Shell",
        "Log", "Details", "Info", "Filter", "Start", "Stop", "Pause", "Format", "Version",
        "Commit", "Branch", "Markdown", "URL", "JSON", "ID", "PID", "Readme", "Total", "Debug",
        "Error",
    ];

    pub fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            let after = &rest[i + 1..];
            match after.find('}') {
                Some(j) => {
                    out.insert(after[..j].to_string());
                    rest = &after[j + 1..];
                }
                None => break,
            }
        }
        out
    }

    fn letters_outside_placeholders(s: &str) -> bool {
        s.split(['{', '}'])
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .any(|(_, s)| s.chars().any(char::is_alphabetic))
    }

    /// Duplicate keys, empty or untranslated German (unless the English is
    /// in [`SAME_IN_GERMAN`] or `same`), placeholder mismatches and
    /// unpaired plurals. Empty means fine.
    pub fn catalog_problems(cat: &Catalog, same: &[&str]) -> Vec<String> {
        let mut bad = Vec::new();
        let mut seen = HashSet::new();
        for &(k, en, de) in cat.entries() {
            if !seen.insert(k) {
                bad.push(format!("{k}: duplicate key"));
            }
            if de.trim().is_empty() && !en.trim().is_empty() {
                bad.push(format!("{k}: empty German"));
            }
            if en == de
                && letters_outside_placeholders(en)
                && !SAME_IN_GERMAN.contains(&en)
                && !same.contains(&en)
            {
                bad.push(format!("{k}: German is a copy of English: {en:?}"));
            }
            if placeholders(en) != placeholders(de) {
                bad.push(format!("{k}: placeholders differ: {en:?} vs {de:?}"));
            }
            if de.contains('"') && en.contains('“') {
                bad.push(format!("{k}: use „…“ quotes in German: {de:?}"));
            }
            if de.contains("...") || (de.contains('…') && !de.contains(" …") && en.ends_with('…'))
            {
                bad.push(format!("{k}: German ellipsis is \" …\": {de:?}"));
            }
        }
        for k in &seen {
            if let Some(base) = k.strip_suffix(".one") {
                if !seen.contains(format!("{base}.other").as_str()) {
                    bad.push(format!("{base}.other missing"));
                }
            }
            if let Some(base) = k.strip_suffix(".other") {
                if !seen.contains(format!("{base}.one").as_str()) {
                    bad.push(format!("{base}.one missing"));
                }
            }
        }
        bad.extend(translation_problems(
            cat.entries().iter(),
            cat.translations().iter().copied(),
            same,
        ));
        bad
    }

    /// Words that stay English in every language: product, protocol and
    /// format names.
    pub const SAME_EVERYWHERE: &[&str] = &[
        "OK",
        "Manta",
        "Git",
        "SFTP",
        "FTP",
        "FTPS",
        "S3",
        "SSH",
        "WebDAV",
        "Docker",
        "Kubernetes",
        "Google Drive",
        "URL",
        "JSON",
        "ID",
        "PID",
        "API",
        "Markdown",
        "Readme",
        "README",
        "zip",
        "tar",
        "Access Key ID",
        "Secret Access Key",
        "Session Token",
        "Debug",
        "Trace",
        "PNG",
        "JPEG",
        "HEIC",
        "WebP",
        "TIFF",
        "GIF",
        "PDF",
        "SVG",
        "EXIF",
        "GPS",
        "Pod",
        "Pods",
        "Token",
        "Bucket",
        "Endpoint",
        "Shell",
        "Commit",
        "Branch",
        "Compose",
        "Namespace",
        "Cluster",
        "Beta",
    ];

    /// Texts that are the same as English on purpose in one language (the
    /// word is native there too). Keep in sync with `scripts/i18n/check.py`.
    pub const SAME_IN: &[(Lang, &[&str])] = &[
        (
            Lang::Es,
            &[
                "Local",
                "Personal",
                "Error",
                "General",
                "Original",
                "original",
                "No",
                "E",
                "{count} byte",
                "{count} bytes",
                "{size} ({bytes} bytes)",
                "{count} original",
                "{label} · {count} script",
                "{label} · {count} scripts",
            ],
        ),
        (
            Lang::Fr,
            &[
                "Local",
                "Services",
                "Graphite",
                "Archive",
                "Archives",
                "Public",
                "port",
                "Action",
                "Actions",
                "Audio",
                "Document",
                "Documents",
                "Image",
                "Images",
                "{count} images",
                "Stable",
                "Original",
                "original",
                "Options",
                "Dimensions",
                "Formats",
                "Important",
                "Date",
                "Extension",
                "E",
                "{label} · {count} scripts",
                "Compose · {count} services",
                "Licence",
            ],
        ),
        (
            Lang::It,
            &[
                "File",
                "{count} file",
                "Password",
                "Account",
                "Video",
                "Audio",
                "{count} container",
                "In “{folder}”",
                "In “{name}”",
                "in “{name}”",
                "{count} byte",
                "{count} hard link",
                "No",
                "E",
                "{label} · {count} script",
            ],
        ),
    ];

    /// Problems of the translation tables against the source table: a
    /// language without a table, keys missing in a language (plurals: the
    /// forms of [`Lang::plural_forms`]), keys the source doesn't have,
    /// empty texts, placeholder mismatches, and texts copied from English.
    /// A copy is fine when it has no letters outside placeholders, when
    /// German kept the English too (borrowed words like "Terminal"), or
    /// when the English is in [`SAME_EVERYWHERE`] or `same`.
    pub fn translation_problems<'a>(
        source: impl Iterator<Item = &'a (&'static str, &'static str, &'static str)>,
        translations: impl Iterator<Item = (Lang, super::Translations)>,
        same: &[&str],
    ) -> Vec<String> {
        let mut bad = Vec::new();
        let source: Vec<_> = source.copied().collect();
        let src: HashMap<&str, (&str, &str)> =
            source.iter().map(|&(k, en, de)| (k, (en, de))).collect();
        let mut per_lang: HashMap<Lang, HashMap<&str, &str>> = HashMap::new();
        for (l, t) in translations {
            let m = per_lang.entry(l).or_default();
            for &(k, s) in t {
                if m.insert(k, s).is_some() {
                    bad.push(format!("{}: {k}: duplicate key", l.code()));
                }
            }
        }
        let is_plural = |base: &str| {
            src.contains_key(format!("{base}.one").as_str())
                && src.contains_key(format!("{base}.other").as_str())
        };
        for l in Lang::TRANSLATED {
            let code = l.code();
            let Some(m) = per_lang.get(&l) else {
                bad.push(format!("{code}: no translation table"));
                continue;
            };
            // The keys this language needs: (English, German, placeholders
            // required, placeholders allowed, a plural form).
            type Need<'b> = (&'b str, &'b str, BTreeSet<String>, BTreeSet<String>, bool);
            let mut needed: HashMap<String, Need> = HashMap::new();
            for &(k, e, d) in &source {
                if let Some(base) = k.strip_suffix(".one").or_else(|| k.strip_suffix(".other")) {
                    if is_plural(base) {
                        let (one, one_de) = src[format!("{base}.one").as_str()];
                        let (other, other_de) = src[format!("{base}.other").as_str()];
                        let allowed: BTreeSet<String> = placeholders(one)
                            .union(&placeholders(other))
                            .cloned()
                            .collect();
                        for f in l.plural_forms() {
                            // "one" means exactly 1 only where 1 is the
                            // only number in it (not French 0, Russian 21).
                            let only_1 =
                                *f == super::Plural::One && !matches!(l, Lang::Fr | Lang::Ru);
                            let (e, d, req) = if only_1 {
                                (one, one_de, placeholders(one))
                            } else {
                                (other, other_de, placeholders(other))
                            };
                            needed.entry(format!("{base}.{}", f.suffix())).or_insert((
                                e,
                                d,
                                req,
                                allowed.clone(),
                                true,
                            ));
                        }
                        continue;
                    }
                }
                needed.insert(
                    k.to_string(),
                    (e, d, placeholders(e), placeholders(e), false),
                );
            }
            for (k, (e, d, req, allowed, plural)) in &needed {
                let (e, d) = (*e, *d);
                match m.get(k.as_str()) {
                    None => bad.push(format!("{code}: {k}: missing")),
                    Some(s) => {
                        if s.trim().is_empty() && !e.trim().is_empty() {
                            bad.push(format!("{code}: {k}: empty"));
                        }
                        let p = placeholders(s);
                        if !req.is_subset(&p) || !p.is_subset(allowed) {
                            bad.push(format!("{code}: {k}: placeholders differ: {e:?} vs {s:?}"));
                        }
                        // A plural form equal to English is always a miss,
                        // even where German keeps the English.
                        if *s == e
                            && letters_outside_placeholders(e)
                            && (d != e || *plural)
                            && !SAME_EVERYWHERE.contains(&e)
                            && !same.contains(&e)
                            && !SAME_IN.iter().any(|(sl, w)| *sl == l && w.contains(&e))
                        {
                            bad.push(format!("{code}: {k}: copy of English: {e:?}"));
                        }
                    }
                }
            }
            for k in m.keys() {
                if !needed.contains_key(*k) {
                    bad.push(format!(
                        "{code}: {k}: not in the source table (or not a plural form of {code})"
                    ));
                }
            }
        }
        bad.sort();
        bad
    }

    pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    /// Keys used as `tr!("…")` in the `.rs` files under `src` that the
    /// catalog lacks (for `count = ` calls: both plural forms). Also fails
    /// when fewer than `min_used` keys were found (the scanner broke).
    pub fn missing_keys(cat: &Catalog, src: &Path, min_used: usize) -> Vec<String> {
        let mut missing = Vec::new();
        let mut used = 0;
        for f in rust_files(src) {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            let mut rest = text.as_str();
            let pat = "tr!(\"";
            while let Some(i) = rest.find(pat) {
                let before = rest[..i].chars().next_back();
                rest = &rest[i + pat.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some(end) = rest.find('"') else { break };
                let key = &rest[..end];
                let tail = &rest[end..rest.floor_char_boundary(end + 200)];
                let plural = tail
                    .trim_start_matches('"')
                    .trim_start()
                    .starts_with(", count");
                used += 1;
                let ok = if plural {
                    cat.has(&format!("{key}.one")) && cat.has(&format!("{key}.other"))
                } else {
                    cat.has(key)
                };
                if !ok && key != "key" {
                    let rel = f.strip_prefix(src).unwrap_or(&f).display();
                    missing.push(format!("{rel}: {key}"));
                }
            }
        }
        if used < min_used {
            missing.push(format!("scanner found only {used} tr!() calls"));
        }
        missing
    }

    /// Calls that put text on screen; a literal with letters right after
    /// one of them is a hard-coded UI string.
    pub const UI_CALLS: &[&str] = &[
        "ui.label(\"",
        "ui.heading(\"",
        "ui.small(\"",
        "ui.button(\"",
        "ui.weak(\"",
        "ui.strong(\"",
        "ui.hyperlink_to(\"",
        "ui.checkbox(",
        "ui.radio_value(",
        "ui.selectable_label(",
        "selectable_value(",
        "RichText::new(\"",
        "Label::new(\"",
        "Button::new(\"",
        "on_hover_text(\"",
        "on_disabled_hover_text(\"",
        "hint_text(\"",
        "selected_text(\"",
        "Window::new(\"",
        "title: \"",
        "tooltip: \"",
        "title: Some(\"",
        "tooltip: Some(\"",
        "text: \"",
        "label: \"",
        "message: \"",
    ];

    fn literal_after(line: &str, pat: &str) -> Option<String> {
        let i = line.find(pat)?;
        let rest = &line[i + pat.len()..];
        let window = &rest[..rest.len().min(120)];
        let q = if pat.ends_with('"') {
            Some(0)
        } else {
            window.find('"').map(|q| q + 1)
        }?;
        let before = &window[..q.saturating_sub(1)];
        if before.contains("tr!(") || before.contains("tr_key(") || before.contains("tr_with!(") {
            return None;
        }
        let body = &window[q..];
        let end = body.find('"')?;
        Some(body[..end].to_string())
    }

    /// Lines in the `.rs` files under `src` that pass a literal with words
    /// to a UI call (see [`UI_CALLS`] plus `extra_calls`). Lines containing
    /// any of `allow` are skipped, as are test modules, comments and files
    /// whose path contains "test" or "i18n".
    pub fn hard_coded_strings(src: &Path, extra_calls: &[&str], allow: &[&str]) -> Vec<String> {
        let mut hits = Vec::new();
        for f in rust_files(src) {
            let rel = f.strip_prefix(src).unwrap_or(&f).display().to_string();
            if rel.contains("test") || rel.contains("i18n") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            for (n, line) in text.lines().enumerate() {
                let l = line.trim_start();
                if l.starts_with("//") || l.starts_with("#[") && l.contains("doc") {
                    continue;
                }
                if l.starts_with("#[cfg(test)]") || l.starts_with("mod tests") {
                    break;
                }
                if allow.iter().any(|a| line.contains(a)) {
                    continue;
                }
                for pat in UI_CALLS.iter().chain(extra_calls) {
                    if let Some(lit) = literal_after(line, pat) {
                        let text: String = lit
                            .split(['{', '}'])
                            .enumerate()
                            .filter(|(i, _)| i % 2 == 0)
                            .map(|(_, s)| s)
                            .collect();
                        if text.chars().filter(|c| c.is_alphabetic()).count() >= 2 {
                            hits.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                            break;
                        }
                    }
                }
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static CAT: Catalog = Catalog::with(
        &[
            ("hello", "Hello", "Hallo"),
            ("files.one", "{count} file", "{count} Datei"),
            ("files.other", "{count} files", "{count} Dateien"),
            ("open", "Open “{name}”", "„{name}“ öffnen"),
        ],
        &[
            (
                Lang::Zh,
                &[
                    ("hello", "你好"),
                    ("files.one", "{count} 个文件"),
                    ("files.other", "{count} 个文件"),
                    ("open", "打开“{name}”"),
                ],
            ),
            (
                Lang::Es,
                &[
                    ("hello", "Hola"),
                    ("files.one", "{count} archivo"),
                    ("files.other", "{count} archivos"),
                    ("open", "Abrir “{name}”"),
                ],
            ),
            (
                Lang::Fr,
                &[
                    ("hello", "Bonjour"),
                    ("files.one", "{count} fichier"),
                    ("files.other", "{count} fichiers"),
                    ("open", "Ouvrir « {name} »"),
                ],
            ),
            (
                Lang::It,
                &[
                    ("hello", "Ciao"),
                    ("files.one", "{count} file"),
                    ("files.other", "{count} file"),
                    ("open", "Apri “{name}”"),
                ],
            ),
            (
                Lang::Ru,
                &[
                    ("hello", "Привет"),
                    ("files.one", "{count} файл"),
                    ("files.few", "{count} файла"),
                    ("files.many", "{count} файлов"),
                    ("files.other", "{count} файла"),
                    ("open", "Открыть «{name}»"),
                ],
            ),
        ],
    );

    #[test]
    fn lookups_and_plurals() {
        with_lang(Lang::De, || {
            assert_eq!(crate::tr_with!(CAT, "hello"), "Hallo");
            assert_eq!(crate::tr_with!(CAT, "files", count = 1), "1 Datei");
            assert_eq!(
                crate::tr_with!(CAT, "files", count = 12345),
                "12.345 Dateien"
            );
            assert_eq!(
                crate::tr_with!(CAT, "open", name = "a.txt"),
                "„a.txt“ öffnen"
            );
            assert_eq!(format_decimal(12.43, 1), "12,4");
            assert_eq!(localize_decimal("✓ 12.4 MB · v1.2"), "✓ 12,4 MB · v1,2");
            assert_eq!(localize_decimal("a. b"), "a. b");
        });
        with_lang(Lang::En, || {
            assert_eq!(crate::tr_with!(CAT, "files", count = 2), "2 files");
            assert_eq!(CAT.tr("missing"), "missing");
            assert_eq!(CAT.tr_key("nope"), "");
        });
        with_lang(Lang::Ru, || {
            let f = |n: u64| crate::tr_with!(CAT, "files", count = n);
            assert_eq!(f(1), "1 файл");
            assert_eq!(f(21), "21 файл");
            assert_eq!(f(3), "3 файла");
            assert_eq!(f(11), "11 файлов");
            assert_eq!(f(5), "5 файлов");
            assert_eq!(f(12345), "12\u{A0}345 файлов");
        });
        with_lang(Lang::Fr, || {
            assert_eq!(crate::tr_with!(CAT, "files", count = 0), "0 fichier");
            assert_eq!(crate::tr_with!(CAT, "files", count = 2), "2 fichiers");
        });
        with_lang(Lang::Zh, || {
            assert_eq!(crate::tr_with!(CAT, "files", count = 1), "1 个文件");
            assert_eq!(crate::tr_with!(CAT, "open", name = "a"), "打开“a”");
        });
        assert_eq!(Lang::from_code("de-DE"), Lang::De);
        assert_eq!(Lang::from_code("pt"), Lang::En);
        let problems = check::catalog_problems(&CAT, &["{count} file"]);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn missing_translations_fall_back_to_english() {
        static PART: Catalog = Catalog::with(
            &[("a", "Apple", "Apfel"), ("b", "Pear", "Birne")],
            &[(Lang::Es, &[("a", "Manzana")])],
        );
        with_lang(Lang::Es, || {
            assert_eq!(PART.tr("a"), "Manzana");
            assert_eq!(PART.tr("b"), "Pear");
        });
        let p = check::catalog_problems(&PART, &[]);
        assert!(p.iter().any(|p| p == "es: b: missing"), "{p:?}");
        assert!(p.iter().any(|p| p == "ru: no translation table"), "{p:?}");
    }

    #[test]
    fn tags() {
        for (tag, l) in [
            ("zh-Hans", Some(Lang::Zh)),
            ("zh-Hans-CN", Some(Lang::Zh)),
            ("zh_CN", Some(Lang::Zh)),
            ("zh-CN", Some(Lang::Zh)),
            ("zh-SG", Some(Lang::Zh)),
            ("zh-Hant", None),
            ("zh-TW", None),
            ("zh-HK", None),
            ("zh-Hant-HK", None),
            ("fr-CA", Some(Lang::Fr)),
            ("es-419", Some(Lang::Es)),
            ("it_IT.UTF-8", Some(Lang::It)),
            ("ru-RU", Some(Lang::Ru)),
            ("pt-BR", None),
        ] {
            assert_eq!(Lang::parse(tag), l, "{tag}");
        }
        for l in Lang::ALL {
            assert_eq!(Lang::parse(l.code()), Some(l));
            assert_eq!(Lang::from_index(l as u8), l);
        }
    }

    #[test]
    fn cldr_plurals() {
        let cats =
            |l: Lang, ns: &[u64]| ns.iter().map(|&n| l.plural(n).suffix()).collect::<Vec<_>>();
        assert_eq!(cats(Lang::En, &[0, 1, 2]), ["other", "one", "other"]);
        assert_eq!(cats(Lang::Fr, &[0, 1, 2]), ["one", "one", "other"]);
        assert_eq!(cats(Lang::Zh, &[0, 1, 2]), ["other", "other", "other"]);
        assert_eq!(
            cats(
                Lang::Ru,
                &[0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 111, 112, 1000]
            ),
            [
                "many", "one", "few", "few", "many", "many", "many", "many", "one", "few", "many",
                "one", "many", "many", "many"
            ]
        );
    }

    #[test]
    fn numbers_and_sizes() {
        let size = |l: Lang, n: u64| with_lang(l, || format_size(n));
        assert_eq!(size(Lang::En, 1_234_567), "1.2 MB");
        assert_eq!(size(Lang::De, 1_234_567), "1,2 MB");
        assert_eq!(size(Lang::Fr, 1_234_567), "1,2\u{A0}Mo");
        assert_eq!(size(Lang::Ru, 12_345_678), "12\u{A0}МБ");
        assert_eq!(size(Lang::Zh, 1_234_567), "1.2 MB");
        assert_eq!(size(Lang::It, 999), "999 byte");
        assert_eq!(size(Lang::Ru, 3), "3\u{A0}байта");
        assert_eq!(size(Lang::Fr, 1), "1\u{A0}octet");
        assert_eq!(format_count_in(Lang::Es, 1234), "1234");
        assert_eq!(format_count_in(Lang::Es, 12345), "12.345");
        assert_eq!(format_count_in(Lang::Fr, 12345), "12\u{202F}345");
        assert_eq!(format_count_in(Lang::Zh, 12345), "12,345");
        assert_eq!(with_lang(Lang::Fr, || localize_size("12.4 MB")), "12,4 Mo");
        assert_eq!(Lang::Fr.quote("a"), "«\u{A0}a\u{A0}»");
    }

    #[test]
    fn checks_find_problems() {
        static BAD: Catalog = Catalog::new(&[
            ("a", "Hello", "Hello"),
            ("b", "{n} x", "{m} x"),
            ("c.one", "one", "eins"),
            ("d", "Open…", "Öffnen…"),
        ]);
        let p: Vec<_> = check::catalog_problems(&BAD, &[])
            .into_iter()
            .filter(|p| !p.contains("translation table"))
            .collect();
        assert_eq!(p.len(), 4, "{p:?}");
        static BAD_TR: Catalog = Catalog::with(
            &[
                ("a", "Hello {name}", "Hallo {name}"),
                ("n.one", "{count} file", "{count} Datei"),
                ("n.other", "{count} files", "{count} Dateien"),
            ],
            &[
                (Lang::Zh, &[("a", "你好"), ("n.other", "{count} 个文件")]),
                (
                    Lang::Es,
                    &[
                        ("a", "Hello {name}"),
                        ("n.one", "{count} archivo"),
                        ("n.other", "{count} archivos"),
                    ],
                ),
                (
                    Lang::Fr,
                    &[
                        ("a", "Bonjour {name}"),
                        ("n.one", "{count} fichier"),
                        ("n.other", "{count} fichiers"),
                    ],
                ),
                (
                    Lang::It,
                    &[
                        ("a", "Ciao {name}"),
                        ("n.one", "{count} documento"),
                        ("n.other", "{count} documenti"),
                        ("x", "?"),
                    ],
                ),
                (
                    Lang::Ru,
                    &[
                        ("a", "Привет {name}"),
                        ("n.one", "{count} файл"),
                        ("n.other", "{count} files"),
                    ],
                ),
            ],
        );
        let p = check::catalog_problems(&BAD_TR, &[]);
        for want in [
            "zh-Hans: a: placeholders differ",
            "es: a: copy of English",
            "it: x: not in the source table",
            "ru: n.few: missing",
            "ru: n.many: missing",
            "zh-Hans: n.one: missing",
            // A plural form left in English, even one Russian never picks.
            "ru: n.other: copy of English",
        ] {
            assert!(
                p.iter().any(|p| p.starts_with(want)),
                "{want} not in {p:#?}"
            );
        }
        assert_eq!(p.len(), 7, "{p:#?}");
    }
}
