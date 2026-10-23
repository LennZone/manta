//! Catalog checks: every key used in the source exists, every entry is
//! translated, placeholders match, and no UI string bypasses the catalog.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use super::*;

/// English strings that are the same in German (names, acronyms, words
/// German borrowed).
const SAME_IN_GERMAN: &[&str] = &[
    "OK",
    "SFTP",
    "FTP",
    "FTPS",
    "S3",
    "SSH",
    "WebDAV",
    "Docker",
    "Kubernetes",
    "Git",
    "Name",
    "Cloud",
    "Google Drive",
    "Status",
    "Tab",
    "Orange",
    "System",
    "Host",
    "Port",
    "Bucket",
    "Region",
    "Endpoint",
    "Plugins",
    "Plugin",
    "Terminal",
    "Container",
    "Pod",
    "Pods",
    "Namespace",
    "Context",
    "Shell",
    "Log",
    "Details",
    "Info",
    "Filter",
    "Start",
    "Stop",
    "Pause",
    "Level",
    "Workers",
    "Token",
    "Server",
    "Format",
    "zip",
    "tar",
    "Archiv",
    "Version",
    "Login",
    "Passwort",
    "Profil",
    "Rolle",
    "Account",
    "Access Key ID",
    "Secret Access Key",
    "Session Token",
    "Kind",
    "Path",
    "Readme",
    "Markdown",
    "URL",
    "API",
    "JSON",
    "PID",
    "ID",
    "Debug",
    "Trace",
    "Warn",
    "Error",
    "Total",
    "Image",
    "Images",
    "Compose",
    "Cluster",
    "Label",
    "Labels",
    "Passphrase",
    "Updates",
    "Beta",
    "Update {version}",
    "Manta {version}",
    "Downloads",
    "Website & Support",
];

fn entries() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    catalog::ALL.iter().flat_map(|p| p.iter())
}

fn placeholders(s: &str) -> BTreeSet<String> {
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

#[test]
fn keys_are_unique() {
    let mut seen = HashSet::new();
    for (k, _, _) in entries() {
        assert!(seen.insert(*k), "duplicate key {k}");
    }
}

#[test]
fn german_is_translated_and_placeholders_match() {
    let mut bad = Vec::new();
    for &(k, en, de) in entries() {
        if de.trim().is_empty() {
            bad.push(format!("{k}: empty German"));
        }
        if en == de && !SAME_IN_GERMAN.contains(&en) && en.chars().any(|c| c.is_alphabetic()) {
            // Strings that are only placeholders or symbols are fine.
            let letters: String = en
                .split(['{', '}'])
                .enumerate()
                .filter(|(i, _)| i % 2 == 0)
                .map(|(_, s)| s)
                .collect();
            if letters.chars().any(|c| c.is_alphabetic()) && !k.starts_with("cmd.kw.") {
                bad.push(format!("{k}: German is a copy of English: {en:?}"));
            }
        }
        if placeholders(en) != placeholders(de) {
            bad.push(format!("{k}: placeholders differ: {en:?} vs {de:?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} problems:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// Every key in every language (plural forms per CLDR), placeholders
/// equal, nothing copied from English by accident.
#[test]
fn every_language_is_complete() {
    // Per-language exceptions: manta_plugin::i18n::check::SAME_IN.
    let bad = manta_plugin::i18n::check::translation_problems(
        entries(),
        catalog::translations(),
        SAME_IN_GERMAN,
    );
    assert!(
        bad.is_empty(),
        "{} problems:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn plurals_come_in_pairs() {
    let keys: HashSet<&str> = entries().map(|e| e.0).collect();
    for k in &keys {
        if let Some(base) = k.strip_suffix(".one") {
            assert!(
                keys.contains(format!("{base}.other").as_str()),
                "{base}.other missing"
            );
        }
        if let Some(base) = k.strip_suffix(".other") {
            assert!(
                keys.contains(format!("{base}.one").as_str()),
                "{base}.one missing"
            );
        }
    }
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every `t!("…")`, `tr("…")` and `tr_in(_, "…")` key in the source exists
/// (for `count = ` calls: both plural forms).
#[test]
fn every_used_key_exists() {
    let keys: HashSet<&str> = entries().map(|e| e.0).collect();
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut missing = BTreeSet::new();
    let mut used = 0;
    for f in files {
        if f.starts_with(src_dir().join("i18n")) {
            continue;
        }
        let src = std::fs::read_to_string(&f).unwrap();
        for pat in ["t!(\"", "tr(\""] {
            let mut rest = src.as_str();
            while let Some(i) = rest.find(pat) {
                // Skip identifiers that merely end in `t` / `tr` (e.g. `assert!`).
                let before = rest[..i].chars().next_back();
                rest = &rest[i + pat.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some(end) = rest.find('"') else { break };
                let key = &rest[..end];
                let tail = &rest[end..rest.len().min(end + 200)];
                let plural = tail
                    .trim_start_matches('"')
                    .trim_start()
                    .starts_with(", count");
                used += 1;
                let ok = if plural {
                    keys.contains(format!("{key}.one").as_str())
                        && keys.contains(format!("{key}.other").as_str())
                } else {
                    keys.contains(key)
                };
                if !ok && key != "key" {
                    missing.insert(format!(
                        "{}: {key}",
                        f.strip_prefix(src_dir()).unwrap().display()
                    ));
                }
            }
        }
    }
    assert!(used > 50, "scanner found only {used} keys");
    assert!(
        missing.is_empty(),
        "missing catalog keys:\n{}",
        missing.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn enum_keys_exist() {
    for a in crate::theme::Accent::ALL {
        assert_ne!(tr_key(a.key()), "", "{}", a.key());
    }
    for a in crate::theme::Appearance::ALL {
        assert_ne!(tr_key(a.key()), "", "{}", a.key());
    }
}

#[test]
fn macro_and_plurals() {
    with_locale(Locale::De, || {
        assert_eq!(t!("common.cancel"), "Abbrechen");
        assert_eq!(t!("status.items", count = 1), "1 Objekt");
        assert_eq!(t!("date.today", time = "14:02"), "Heute 14:02");
    });
    with_locale(Locale::En, || {
        assert_eq!(t!("status.items", count = 2), "2 items");
        assert_eq!(t!("status.items", count = 12345), "12,345 items");
    });
    // Russian picks one/few/many, Chinese has one form, French 0 is singular.
    with_locale(Locale::Ru, || {
        let (one, few, many) = (
            t!("status.items", count = 21),
            t!("status.items", count = 3),
            t!("status.items", count = 5),
        );
        assert!(
            one.starts_with("21 ") && few.starts_with("3 ") && many.starts_with("5 "),
            "{one} {few} {many}"
        );
        assert_ne!(one[3..], few[2..]);
        assert_ne!(few[2..], many[2..]);
    });
    with_locale(Locale::Fr, || {
        assert_eq!(
            t!("status.items", count = 0)[1..],
            t!("status.items", count = 1)[1..]
        );
        assert_ne!(
            t!("status.items", count = 1)[1..],
            t!("status.items", count = 2)[1..]
        );
    });
    with_locale(Locale::Zh, || {
        assert_eq!(
            t!("status.items", count = 1)[1..],
            t!("status.items", count = 2)[1..]
        );
    });
    assert_eq!(fill("a {x} b {y}", &[("x", &1), ("y", &"z")]), "a 1 b z");
    assert_eq!(fill("{missing}", &[("x", &1)]), "{missing}");
    assert_eq!(Locale::parse("de-DE"), Some(Locale::De));
    assert_eq!(Locale::parse("en_US"), Some(Locale::En));
    assert_eq!(Locale::parse("pt"), None);
}

/// System language: the first supported one in macOS's preferred list.
#[test]
fn system_language_best_match() {
    let m = |tags: &[&str]| best_match(tags.iter().map(|t| t.to_string()));
    assert_eq!(m(&["zh-Hans-CN", "en-US"]), Locale::Zh);
    assert_eq!(m(&["zh-Hant-TW", "fr-FR"]), Locale::Fr);
    assert_eq!(m(&["zh-HK"]), Locale::En);
    assert_eq!(m(&["pt-BR", "ru-RU", "de"]), Locale::Ru);
    assert_eq!(m(&["es-419"]), Locale::Es);
    assert_eq!(m(&[]), Locale::En);
}

/// The picker lists System, then each language in its own name.
#[test]
fn language_picker() {
    with_locale(Locale::En, || {
        let labels: Vec<_> = Language::ALL.iter().map(|l| l.label()).collect();
        assert_eq!(
            labels,
            [
                "System",
                "Deutsch",
                "English",
                "简体中文",
                "Español",
                "Français",
                "Italiano",
                "Русский"
            ]
        );
    });
    for l in Language::ALL {
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<Language>(&json).unwrap(), l);
        if let Some(loc) = l.locale() {
            assert_eq!(l.resolve(), loc);
        }
    }
    assert_eq!(
        serde_json::from_str::<Language>("\"zh-Hans\"").unwrap(),
        Language::Chinese
    );
}

/// Sizes and dates per language.
#[test]
fn formats_per_language() {
    use chrono::TimeZone;
    let now = chrono::Local
        .with_ymd_and_hms(2026, 10, 22, 12, 0, 0)
        .unwrap();
    let t = |y, mo, d| {
        Some(
            chrono::Local
                .with_ymd_and_hms(y, mo, d, 14, 2, 0)
                .unwrap()
                .with_timezone(&chrono::Utc),
        )
    };
    let cases: &[(Locale, &str, &str, &str)] = &[
        (Locale::En, "1.2 MB", "12 Oct 14:02", "3 Mar 2024"),
        (Locale::De, "1,2 MB", "12. Okt. 14:02", "3. März 2024"),
        (Locale::Zh, "1.2 MB", "10月12日 14:02", "2024年3月3日"),
        (Locale::Es, "1,2 MB", "12 oct 14:02", "3 mar 2024"),
        (Locale::Fr, "1,2\u{a0}Mo", "12 oct. 14:02", "3 mars 2024"),
        (Locale::It, "1,2 MB", "12 ott 14:02", "3 mar 2024"),
        (Locale::Ru, "1,2\u{a0}МБ", "12 окт. 14:02", "3 мар. 2024 г."),
    ];
    for &(l, size, this_year, other_year) in cases {
        with_locale(l, || {
            assert_eq!(crate::util::format_size(1_234_567), size, "{l:?}");
            assert_eq!(
                crate::util::format_time_relative(t(2026, 10, 12), now),
                this_year,
                "{l:?}"
            );
            assert_eq!(
                crate::util::format_time_relative(t(2024, 3, 3), now),
                other_year,
                "{l:?}"
            );
            let today = crate::util::format_time_relative(t(2026, 10, 22), now);
            assert!(
                today.contains("14:02") && today != "14:02",
                "{l:?}: {today}"
            );
        });
    }
}

/// Calls that put text on screen. A string literal with letters right after
/// one of these is a hard-coded UI string that should go through `t!`.
const UI_CALLS: &[&str] = &[
    "ui.label(\"",
    "ui.heading(\"",
    "ui.small(\"",
    "ui.button(\"",
    "ui.weak(\"",
    "ui.strong(\"",
    "ui.monospace(\"",
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
    "tooltip(",
    "section_title(ui, \"",
    "section_header(ui, \"",
    "setting_row(ui, \"",
    "widgets::button(ui, \"",
    "button(ui, \"",
    "empty_state(",
    "Window::new(\"",
    "selected_text(\"",
    "hint_text(\"",
    "show_input(ui, ",
    "show_with_hint(",
    "dialog_header(",
    "toasts.push(",
    "set_error(\"",
    "set_error_detail(\"",
    "set_status(\"",
    "kv_line(ui, p, \"",
    "link_button(ui, ",
    "disclosure(ui, \"",
    "chip(ui, \"",
    "tb(ui, ",
    "field_label(ui, \"",
    "status_label(",
];

/// Lines that may keep a literal (with the reason).
const ALLOW: &[&str] = &[
    // Brand and format names shown as-is.
    "\"Manta\"",
    "\"SFTP\"",
    "\"FTP\"",
    "\"FTPS\"",
    "\"S3\"",
    "\"zip\"",
    "\"tar\"",
    "\"English\"",
    "\"Deutsch\"",
    "\"Español\"",
    "\"Français\"",
    "\"Italiano\"",
    // Manual chapter targets ("05-dateien.md#pausieren"), not UI text.
    "help_button(ui, \"",
];

fn literal_after(line: &str, pat: &str) -> Option<String> {
    let i = line.find(pat)?;
    let rest = &line[i + pat.len()..];
    // The first string literal within the call's first ~80 chars.
    let window = &rest[..rest.len().min(120)];
    let q = if pat.ends_with('"') {
        Some(0)
    } else {
        window.find('"').map(|q| q + 1)
    }?;
    // Reject `t!("` / `tr("` / `format!("{` as the literal's opener.
    let before = &window[..q.saturating_sub(1)];
    if before.contains("t!(") || before.contains("tr(") || before.contains("tr_key(") {
        return None;
    }
    let body = &window[q..];
    let end = body.find('"')?;
    Some(body[..end].to_string())
}

#[test]
fn no_hard_coded_ui_strings() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut hits = Vec::new();
    for f in files {
        let rel = f.strip_prefix(src_dir()).unwrap().display().to_string();
        if rel.contains("tests") || rel.starts_with("i18n") {
            continue;
        }
        let src = std::fs::read_to_string(&f).unwrap();
        for (n, line) in src.lines().enumerate() {
            let l = line.trim_start();
            if l.starts_with("//") || l.starts_with("#[cfg(test)]") {
                continue;
            }
            if l.starts_with("mod tests") {
                break;
            }
            if ALLOW.iter().any(|a| line.contains(a)) {
                continue;
            }
            for pat in UI_CALLS {
                if let Some(lit) = literal_after(line, pat) {
                    // Placeholders and symbol-only literals are fine.
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
    assert!(
        hits.is_empty(),
        "{} hard-coded UI strings (use t!):\n{}",
        hits.len(),
        hits.join("\n")
    );
}
