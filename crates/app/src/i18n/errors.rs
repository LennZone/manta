//! Translate technical error texts shown to the user (toast details).
//!
//! Errors come from other crates as English strings: `manta_core::VfsError`
//! ("not found: /x", "permission denied: …") wrapping `std::io` messages
//! ("No such file or directory (os error 2)"), and job titles from
//! `manta_ops` ("Copying 3 items to /tmp"). The known shapes are
//! translated; anything else is shown as is. Logs keep the English original.

use super::{fill, locale, tr, Locale};

/// `VfsError` display prefixes and their catalog keys (`{detail}` is the rest).
const PREFIXES: &[(&str, &str)] = &[
    ("not found: ", "error.vfs.not_found"),
    ("already exists: ", "error.vfs.already_exists"),
    ("permission denied: ", "error.vfs.permission_denied"),
    ("not a directory: ", "error.vfs.not_a_directory"),
    ("directory not empty: ", "error.vfs.directory_not_empty"),
    ("connection error: ", "error.vfs.connection"),
    ("authentication failed: ", "error.vfs.auth_failed"),
    ("unknown host key: ", "error.vfs.unknown_host_key"),
    (
        "host key changed (possible man-in-the-middle attack): ",
        "error.vfs.host_key_changed",
    ),
    ("operation not supported: ", "error.vfs.unsupported"),
    ("io error: ", "error.vfs.io"),
    ("internal error: ", "error.internal"),
];

/// `std::io::Error` messages (without the " (os error N)" suffix).
const IO: &[(&str, &str)] = &[
    ("No such file or directory", "error.io.not_found"),
    ("Permission denied", "error.io.permission_denied"),
    ("Operation not permitted", "error.io.not_permitted"),
    ("File exists", "error.io.exists"),
    ("Directory not empty", "error.io.not_empty"),
    ("Is a directory", "error.io.is_directory"),
    ("Not a directory", "error.io.not_directory"),
    ("No space left on device", "error.io.no_space"),
    ("Read-only file system", "error.io.read_only_fs"),
    ("Connection refused", "error.io.connection_refused"),
    ("Connection reset by peer", "error.io.connection_reset"),
    ("Operation timed out", "error.io.timed_out"),
    ("Network is unreachable", "error.io.network_unreachable"),
    ("Host is down", "error.io.host_down"),
    ("No route to host", "error.io.no_route"),
    ("Broken pipe", "error.io.broken_pipe"),
    ("Resource busy", "error.io.busy"),
    ("Too many open files", "error.io.too_many_files"),
    ("File name too long", "error.io.name_too_long"),
];

/// `msg` in the active language (unchanged in English and for unknown texts).
pub fn localize_error(msg: &str) -> String {
    if locale() == Locale::En {
        return msg.to_string();
    }
    translate(msg)
}

fn translate(msg: &str) -> String {
    if msg == "cancelled" {
        return tr("error.vfs.cancelled").to_string();
    }
    if msg == manta_vfs::ftp::DATA_TLS_BLOCKED {
        return tr("error.ftps.data_tls_blocked").to_string();
    }
    for &(prefix, key) in PREFIXES {
        if let Some(rest) = msg.strip_prefix(prefix) {
            return fill(tr(key), &[("detail", &translate(rest))]);
        }
    }
    let (head, code) = match msg.rfind(" (os error ") {
        Some(i) if msg.ends_with(')') => (
            &msg[..i],
            Some(&msg[i + " (os error ".len()..msg.len() - 1]),
        ),
        _ => (msg, None),
    };
    let head_tr = IO.iter().find(|(en, _)| *en == head).map(|&(_, k)| tr(k));
    match (head_tr, code) {
        (Some(h), Some(c)) => format!("{h} ({})", fill(tr("error.io.os_error"), &[("code", &c)])),
        (Some(h), None) => h.to_string(),
        (None, Some(c)) => format!(
            "{head} ({})",
            fill(tr("error.io.os_error"), &[("code", &c)])
        ),
        (None, None) => msg.to_string(),
    }
}

/// A job title from `manta_ops` ("Copying 3 items to /tmp",
/// "Deleting \"a.txt\"", "Compressing 2 items to \"x.zip\"") in the active
/// language. Unknown shapes are returned unchanged.
pub fn localize_job_title(title: &str) -> String {
    if locale() == Locale::En {
        return title.to_string();
    }
    const VERBS: &[(&str, &str, &str)] = &[
        ("Copying ", "app.job.copy_to", "app.job.copy"),
        ("Moving ", "app.job.move_to", "app.job.move"),
        ("Deleting ", "app.job.delete", "app.job.delete"),
        ("Extracting ", "app.job.extract_to", "app.job.extract"),
        ("Compressing ", "app.job.compress_to", "app.job.compress"),
    ];
    for &(verb, key_to, key) in VERBS {
        let Some(rest) = title.strip_prefix(verb) else {
            continue;
        };
        let (what, dst) = match split_to(rest) {
            Some((w, d)) => (w, Some(d)),
            None => (rest, None),
        };
        let what = what_local(what);
        return match dst {
            Some(d) if key_to != key => {
                let d = match d.strip_prefix('"').and_then(|d| d.strip_suffix('"')) {
                    Some(n) => super::quote(n),
                    None => d.to_string(),
                };
                fill(tr(key_to), &[("what", &what), ("dst", &d)])
            }
            _ => fill(tr(key), &[("what", &what)]),
        };
    }
    title.to_string()
}

/// What a job title (`Copying 3 items to /dst`, `Moving "a.txt" to /x`)
/// works on, in the UI language: `3 Objekte`, `„a.txt“` (`None`: not a
/// job title).
pub fn job_object(title: &str) -> Option<String> {
    let rest = [
        "Copying ",
        "Moving ",
        "Deleting ",
        "Extracting ",
        "Compressing ",
    ]
    .iter()
    .find_map(|v| title.strip_prefix(v))?;
    let what = split_to(rest).map_or(rest, |(w, _)| w);
    Some(if locale() == Locale::En {
        what.to_string()
    } else {
        what_local(what)
    })
}

/// Split `"a.txt" to /dst` or `3 items to /dst` at the " to " after the
/// object (a quoted name may contain " to " itself).
fn split_to(rest: &str) -> Option<(&str, &str)> {
    let start = if let Some(r) = rest.strip_prefix('"') {
        r.find('"').map(|i| i + 2)?
    } else {
        0
    };
    let i = rest[start..].find(" to ")? + start;
    Some((&rest[..i], &rest[i + 4..]))
}

/// `"a.txt"` → `„a.txt“`, `3 items` → `3 Objekte` (in the UI language).
fn what_local(what: &str) -> String {
    if let Some(name) = what.strip_prefix('"').and_then(|w| w.strip_suffix('"')) {
        return super::quote(name);
    }
    if let Some(n) = what
        .strip_suffix(" items")
        .and_then(|n| n.parse::<u64>().ok())
    {
        return super::tr_plural("app.job.items", n, &[]);
    }
    what.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{tr_key, with_locale};

    #[test]
    fn english_is_unchanged() {
        with_locale(Locale::En, || {
            assert_eq!(localize_error("not found: /x"), "not found: /x");
            assert_eq!(
                localize_job_title("Copying 3 items to /tmp"),
                "Copying 3 items to /tmp"
            );
        });
    }

    #[test]
    fn german_errors() {
        with_locale(Locale::De, || {
            assert_eq!(localize_error("not found: /x/y"), "Nicht gefunden: /x/y");
            assert_eq!(
                localize_error("permission denied: Permission denied (os error 13)"),
                "Keine Berechtigung: Zugriff verweigert (OS-Fehler 13)"
            );
            assert_eq!(
                localize_error("io error: No such file or directory (os error 2)"),
                "E/A-Fehler: Datei oder Ordner nicht gefunden (OS-Fehler 2)"
            );
            assert_eq!(localize_error("cancelled"), "Abgebrochen");
            assert_eq!(
                localize_error("Strange (os error 99)"),
                "Strange (OS-Fehler 99)"
            );
            assert_eq!(localize_error("something else"), "something else");
        });
    }

    #[test]
    fn german_job_titles() {
        with_locale(Locale::De, || {
            assert_eq!(
                localize_job_title("Copying 3 items to /tmp"),
                "3 Objekte nach /tmp kopieren"
            );
            assert_eq!(
                localize_job_title("Moving \"a to b.txt\" to /dst"),
                "„a to b.txt“ nach /dst bewegen"
            );
            assert_eq!(localize_job_title("Deleting 1 items"), "1 Objekt löschen");
            assert_eq!(localize_job_title("Deleting \"x\""), "„x“ löschen");
            assert_eq!(
                localize_job_title("Compressing 2 items to \"x.zip\""),
                "2 Objekte in „x.zip“ komprimieren"
            );
            assert_eq!(localize_job_title("Something"), "Something");
        });
    }

    #[test]
    fn table_keys_exist() {
        for (_, k) in PREFIXES.iter().chain(IO) {
            assert_ne!(tr_key(k), "", "{k}");
        }
        for k in [
            "error.vfs.cancelled",
            "error.io.os_error",
            "app.job.copy_to",
            "app.job.move_to",
            "app.job.delete",
            "app.job.extract_to",
            "app.job.compress_to",
            "app.job.copy",
            "app.job.move",
            "app.job.extract",
            "app.job.compress",
            "app.job.items.one",
            "app.job.items.other",
        ] {
            assert_ne!(tr_key(k), "", "{k}");
        }
    }
}
