//! Localisation: a small, embedded string catalog.
//!
//! Why home-grown instead of Fluent: the app needs `{name}` placeholders,
//! CLDR plurals for a handful of languages and nothing else. Static tables
//! give `&'static str` for every plain string (no allocation per frame),
//! need no parsing at startup and no extra dependencies.
//!
//! Layout (see `catalog/mod.rs`):
//! - `catalog/<area>.rs`: the source tables, `(key, English, German)` —
//!   English and German side by side, as before.
//! - `catalog/<lang>/<area>.rs`: one translation table `(key, text)` per
//!   further language and area (zh = Simplified Chinese, es, fr, it, ru),
//!   same keys. A missing key falls back to English.
//!
//! All tables are merged once into one hash map `key → [text; languages]`,
//! so a lookup is one hash plus an array index.
//!
//! Plurals follow CLDR ([`manta_plugin::i18n::Lang::plural`]): the source
//! has `key.one` / `key.other`; Russian adds `key.few` / `key.many`,
//! Chinese only has `key.other`, French uses `key.one` for 0 and 1.
//!
//! Adding a language: a variant in `manta_plugin::i18n::Lang` (code,
//! native name, plural rule, number format), a [`Language`] variant, the
//! language in `scripts/i18n/catalog.py`, then `scripts/i18n/skeleton.py`
//! creates the tables to translate. Tests catch missing keys, untranslated
//! strings and placeholder mismatches (see `tests.rs`).
//!
//! Call sites:
//! - `t!("toolbar.connect")` → `&'static str`
//! - `t!("status.items", count = n)` → `String`, picks the plural form,
//!   `{count}` is replaced
//! - `t!("dialog.delete.title", name = file)` → `String`
//!
//! The active locale is global ([`set_locale`]); tests can override it per
//! thread ([`with_locale`]) so English assertions and German renders can run
//! in parallel. Log messages stay English and don't go through the catalog.

/// Translate a string: `t!("key")`, `t!("key", count = n)`,
/// `t!("key", name = value, …)`. See the module docs.
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, count = $n:expr $(, $name:ident = $val:expr)* $(,)?) => {
        $crate::i18n::tr_plural($key, ($n) as u64, &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),*])
    };
    ($key:literal $(, $name:ident = $val:expr)+ $(,)?) => {
        $crate::i18n::tr_fmt($key, &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+])
    };
}

mod catalog;
mod errors;
#[cfg(test)]
mod glossary;

pub use errors::{job_object, localize_error, localize_job_title};

use std::cell::Cell;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use manta_plugin::i18n::{build_table, lookup, lookup_plural, Table};
use serde::{Deserialize, Serialize};

/// A supported UI language (shared with the plugins).
pub use manta_plugin::i18n::Lang as Locale;

/// The Language setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// The first supported language in macOS's preferred languages.
    #[default]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "zh-Hans")]
    Chinese,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "ru")]
    Russian,
}

impl Language {
    /// The picker's order: System, then like macOS's language list.
    pub const ALL: [Language; 8] = [
        Language::System,
        Language::German,
        Language::English,
        Language::Chinese,
        Language::Spanish,
        Language::French,
        Language::Italian,
        Language::Russian,
    ];

    /// Shown in the picker. Language names are in their own language, like
    /// macOS does; "System" is translated.
    pub fn label(self) -> &'static str {
        match self.locale() {
            None => t!("language.system"),
            Some(l) => l.native_name(),
        }
    }

    /// The fixed language, `None` for System.
    pub fn locale(self) -> Option<Locale> {
        Some(match self {
            Language::System => return None,
            Language::English => Locale::En,
            Language::German => Locale::De,
            Language::Chinese => Locale::Zh,
            Language::Spanish => Locale::Es,
            Language::French => Locale::Fr,
            Language::Italian => Locale::It,
            Language::Russian => Locale::Ru,
        })
    }

    pub fn resolve(self) -> Locale {
        self.locale().unwrap_or_else(system_locale)
    }
}

/// The first supported language in the user's preferred languages
/// (macOS: System Settings → General → Language & Region), else English.
/// "zh-Hans"/"zh-CN" pick Chinese; Traditional Chinese isn't supported and
/// is skipped. Read once; the call is a cheap CoreFoundation lookup.
pub fn system_locale() -> Locale {
    static SYS: OnceLock<Locale> = OnceLock::new();
    if cfg!(test) {
        // Tests assert English strings whatever the machine's language.
        return Locale::En;
    }
    *SYS.get_or_init(|| {
        if let Ok(v) = std::env::var("MANTA_LANG") {
            if let Some(l) = Locale::parse(&v) {
                return l;
            }
        }
        best_match(sys_locale::get_locales())
    })
}

/// The first supported language in `preferred` (BCP 47 tags in order of
/// preference), else English.
pub fn best_match(preferred: impl IntoIterator<Item = String>) -> Locale {
    preferred
        .into_iter()
        .find_map(|t| Locale::parse(&t))
        .unwrap_or(Locale::En)
}

static LOCALE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static OVERRIDE: Cell<Option<Locale>> = const { Cell::new(None) };
}

/// The active locale.
pub fn locale() -> Locale {
    if let Some(l) = OVERRIDE.with(Cell::get) {
        return l;
    }
    Locale::from_index(LOCALE.load(Ordering::Relaxed))
}

/// Switch the app's language (takes effect on the next frame). Under
/// `cargo test` this only affects the calling thread, so tests in other
/// languages can't leak into each other.
pub fn set_locale(l: Locale) {
    if cfg!(test) {
        OVERRIDE.with(|o| o.set(Some(l)));
        // Built-in plugins drawn on this thread follow along.
        manta_plugin::i18n::set_thread_lang(Some(l));
    } else {
        LOCALE.store(l as u8, Ordering::Relaxed);
    }
}

/// Run `f` with `l` as this thread's locale (tests, renders).
#[allow(dead_code)]
pub fn with_locale<R>(l: Locale, f: impl FnOnce() -> R) -> R {
    let prev = OVERRIDE.with(|o| o.replace(Some(l)));
    struct Reset(Option<Locale>);
    impl Drop for Reset {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _reset = Reset(prev);
    manta_plugin::i18n::with_lang(l, f)
}

/// Set this thread's locale until [`clear_thread_locale`] (render tests
/// whose closures run on the test thread).
#[allow(dead_code)]
pub fn set_thread_locale(l: Option<Locale>) {
    OVERRIDE.with(|o| o.set(l));
    manta_plugin::i18n::set_thread_lang(l);
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        build_table(
            catalog::ALL.iter().flat_map(|part| part.iter()),
            catalog::translations(),
        )
    })
}

/// The string for `key` in the active locale. Unknown keys return the key
/// itself (tests make sure there are none).
pub fn tr(key: &'static str) -> &'static str {
    tr_in(locale(), key)
}

pub fn tr_in(l: Locale, key: &'static str) -> &'static str {
    lookup(table(), l, key).unwrap_or(key)
}

/// `key` with `{name}` placeholders filled in.
pub fn tr_fmt(key: &'static str, args: &[(&str, &dyn Display)]) -> String {
    fill(tr(key), args)
}

/// Plural: the CLDR form of `key` for `n` in the active locale
/// (`key.one`, `key.few`, `key.many`, `key.other`). `{count}` is `n`,
/// formatted for the locale.
pub fn tr_plural(key: &'static str, n: u64, args: &[(&str, &dyn Display)]) -> String {
    tr_plural_dyn(key, n, args)
}

/// [`tr_plural`] for a key built at runtime.
pub fn tr_plural_dyn(key: &str, n: u64, args: &[(&str, &dyn Display)]) -> String {
    let count = format_count(n);
    let mut all: Vec<(&str, &dyn Display)> = vec![("count", &count)];
    all.extend_from_slice(args);
    fill(lookup_plural(table(), locale(), key, n), &all)
}

/// Look up a key that isn't a literal (e.g. built at runtime). Unknown keys
/// give `""`.
#[allow(dead_code)]
pub fn tr_key(key: &str) -> &'static str {
    lookup(table(), locale(), key).unwrap_or("")
}

fn fill(s: &str, args: &[(&str, &dyn Display)]) -> String {
    manta_plugin::i18n::fill(s, args)
}

/// A backend label for display: the local file system's "Local" is
/// translated, connection labels ("user@host (SFTP)") stay.
pub fn fs_label(label: &str) -> String {
    if label == "Local" {
        tr("app.local").to_string()
    } else {
        label.to_string()
    }
}

/// `name` in the active locale's quotation marks: “a”, „a“, « a », «a».
pub fn quote(name: &str) -> String {
    locale().quote(name)
}

// ------------------------------------------------------------------ numbers

/// Decimal separator of the active locale.
pub fn decimal_sep() -> char {
    locale().decimal_sep()
}

/// Thousands separators: "12,345" / "12.345" / "12 345".
pub fn format_count(n: u64) -> String {
    manta_plugin::i18n::format_count_in(locale(), n)
}

/// `value` with `decimals` places and the locale's decimal separator.
pub fn format_decimal(value: f64, decimals: usize) -> String {
    let s = format!("{value:.decimals$}");
    match decimal_sep() {
        '.' => s,
        c => s.replace('.', &c.to_string()),
    }
}

// -------------------------------------------------------------------- dates

/// A short date like Finder's list: "12 Oct 2024", "12. Okt. 2024",
/// "2024年10月12日", "12 oct. 2024"; without the year when `year` is
/// `None`. `time` ("14:02") is appended when given.
pub fn format_day(day: u32, month0: u32, year: Option<i32>, time: Option<&str>) -> String {
    manta_plugin::i18n::format_day(locale(), day, month0, year, time)
}

#[cfg(test)]
mod tests;
