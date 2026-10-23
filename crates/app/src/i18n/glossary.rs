//! `docs/i18n/glossary.md`: the UI terms (menus, commands, buttons,
//! settings labels, dialog titles, sidebar sections …) in every language,
//! generated from the catalogs so manual translators can quote the UI
//! exactly. [`glossary_is_up_to_date`] fails when the file is stale;
//! `MANTA_UPDATE_GLOSSARY=1 cargo test -p manta glossary` rewrites it.

use std::path::Path;

use manta_plugin::i18n::{Catalog, Entries, Lang};

use super::catalog;

/// The order of the columns (English first, as the source).
const COLUMNS: [Lang; 7] = [
    Lang::En,
    Lang::De,
    Lang::Zh,
    Lang::Es,
    Lang::Fr,
    Lang::It,
    Lang::Ru,
];

/// Key parts of strings that aren't UI terms (sentences, tooltips, errors,
/// search keywords, toasts).
const SKIP: &[&str] = &[
    ".kw.",
    "tip",
    "note",
    "hint",
    "body",
    "detail",
    "desc",
    "error",
    "err.",
    "toast",
    "msg",
    "intro",
    "subtitle",
    "explain",
    "why",
    "help_text",
    "placeholder",
    "status.",
    "progress",
    "summary",
    "result",
];

/// Is `(key, english)` a short UI term worth quoting?
fn is_term(key: &str, en: &str) -> bool {
    let text: String = en.split(['{', '}']).step_by(2).collect();
    !SKIP.iter().any(|s| key.contains(s))
        && en.chars().count() <= 40
        && !en.contains('\n')
        && !en.trim_end().ends_with(['.', ':', '!', '?'])
        && text.chars().filter(|c| c.is_alphabetic()).count() >= 2
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\u{a0}', '\u{202f}'], " ")
}

fn section(
    out: &mut String,
    title: &str,
    entries: Entries,
    tr: &dyn Fn(Lang, &'static str) -> &'static str,
) {
    let rows: Vec<_> = entries.iter().filter(|(k, en, _)| is_term(k, en)).collect();
    if rows.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {title}\n\n| key | en | de | zh-Hans | es | fr | it | ru |\n|---|---|---|---|---|---|---|---|\n"));
    for &(k, _, _) in rows {
        let cells: Vec<String> = COLUMNS.iter().map(|&l| cell(tr(l, k))).collect();
        out.push_str(&format!("| `{k}` | {} |\n", cells.join(" | ")));
    }
}

/// The built-in plugins' catalogs, by name.
fn plugin_catalogs() -> Vec<(&'static str, &'static Catalog)> {
    vec![
        (
            "Plugin: Duplicates",
            &manta_plugin_duplicates::i18n::CATALOG,
        ),
        (
            "Plugin: Folder Sizes",
            &manta_plugin_foldersize::i18n::CATALOG,
        ),
        ("Plugin: Git", &manta_plugin_git::i18n::CATALOG),
        (
            "Plugin: Image Tools",
            &manta_plugin_imagetools::i18n::CATALOG,
        ),
        (
            "Plugin: Markdown Preview",
            &manta_plugin_markdown::i18n::CATALOG,
        ),
        (
            "Plugin: Project Info",
            &manta_plugin_projectinfo::i18n::CATALOG,
        ),
        (
            "Plugin: Quick Actions",
            &manta_plugin_quickactions::i18n::CATALOG,
        ),
        ("Plugin: Batch Rename", &manta_plugin_rename::i18n::CATALOG),
        ("Plugin: System Load", &manta_plugin_sysstats::i18n::CATALOG),
        ("Plugin: Terminal", &manta_plugin_terminal::i18n::CATALOG),
    ]
}

/// The glossary as Markdown.
pub fn markdown() -> String {
    let mut out = String::from(
        "# UI glossary\n\n\
         Generated from the string catalogs (`crates/app/src/i18n/catalog`, `crates/plugin-*/src/i18n`) \
         by the test `i18n::glossary::glossary_is_up_to_date` — do not edit by hand; run \
         `MANTA_UPDATE_GLOSSARY=1 cargo test -p manta glossary`.\n\n\
         Short UI terms only (menus, commands, buttons, settings labels, dialog titles, sidebar sections, \
         columns); sentences, tooltips and errors are left out. Quote these exactly in the manual. \
         `{name}`-style placeholders are filled in by the app. Plural keys (`.one`, `.other`) show the \
         form for that key; Russian also has `.few` / `.many` forms in the catalog.\n",
    );
    for (title, entries) in catalog::AREAS {
        section(&mut out, &format!("App: {title}"), entries, &|l, k| {
            super::tr_in(l, k)
        });
    }
    for (title, cat) in plugin_catalogs() {
        section(&mut out, title, cat.entries(), &|l, k| cat.tr_in(l, k));
    }
    out
}

#[test]
fn glossary_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/i18n/glossary.md");
    let want = markdown();
    if std::env::var_os("MANTA_UPDATE_GLOSSARY").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "docs/i18n/glossary.md is out of date: run MANTA_UPDATE_GLOSSARY=1 cargo test -p manta glossary"
    );
}

#[test]
fn glossary_has_the_core_terms() {
    let md = markdown();
    for k in [
        "`menu.file`",
        "`cmd.move_to_trash`",
        "`settings.language`",
        "`sidebar.favorites`",
    ] {
        assert!(md.contains(k), "{k} missing");
    }
}
