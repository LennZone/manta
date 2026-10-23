//! English and German strings of the System Load plugin (see
//! `manta_plugin::i18n`): `(key, English, German)`. Terms follow Activity
//! Monitor in each language.

use manta_plugin::i18n::{Catalog, Entries, Lang};

mod es;
mod fr;
mod it;
mod ru;
mod zh;

pub static CATALOG: Catalog = Catalog::with(
    ENTRIES,
    &[
        (Lang::Zh, zh::ENTRIES),
        (Lang::Es, es::ENTRIES),
        (Lang::Fr, fr::ENTRIES),
        (Lang::It, it::ENTRIES),
        (Lang::Ru, ru::ENTRIES),
    ],
);

macro_rules! tr {
    ($($t:tt)*) => { manta_plugin::tr_with!(crate::i18n::CATALOG, $($t)*) };
}
pub(crate) use tr;

const ENTRIES: Entries = &[
    // Plugin
    ("plugin.name", "System Load", "Systemauslastung"),
    ("plugin.description", "CPU load, GPU load and memory used in the status bar. Click to open Activity Monitor.", "CPU-Last, GPU-Last und belegten Speicher in der Statusleiste anzeigen. Ein Klick öffnet die Aktivitätsanzeige."),
    // Settings
    ("settings.show_gpu", "Show GPU load", "GPU-Last anzeigen"),
    ("settings.show_gpu.tip", "Hidden automatically on Macs without GPU statistics.", "Wird auf Macs ohne GPU-Statistik automatisch ausgeblendet."),
    // Status bar
    ("label.cpu", "CPU", "CPU"),
    ("label.gpu", "GPU", "GPU"),
    ("label.ram", "RAM", "RAM"),
    ("pct", "{n}%", "{n} %"),
    ("item", "{label} {value}", "{label} {value}"),
    ("ram.value", "{used} / {total}", "{used} / {total}"),
    // Tooltips
    ("tip.cpu", "CPU load: {pct}", "CPU-Last: {pct}"),
    ("tip.cores", "Average of all cores: {n}", "Durchschnitt aller Kerne: {n}"),
    ("tip.cores.split", "Performance cores: {perf} · Efficiency cores: {eff}", "Performance-Kerne: {perf} · Effizienz-Kerne: {eff}"),
    ("tip.busiest", "Busiest core: {pct}", "Am stärksten belasteter Kern: {pct}"),
    ("tip.gpu", "GPU load: {pct}", "GPU-Last: {pct}"),
    ("tip.gpu_cores", "GPU cores: {n}", "GPU-Kerne: {n}"),
    ("tip.ram", "Memory Used: {used} of {total}", "Belegter Speicher: {used} von {total}"),
    ("tip.ram.app", "App Memory: {size}", "App-Speicher: {size}"),
    ("tip.ram.wired", "Wired Memory: {size}", "Reservierter Speicher: {size}"),
    ("tip.ram.compressed", "Compressed: {size}", "Komprimiert: {size}"),
    ("tip.ram.swap", "Swap Used: {size}", "Belegter Swap-Speicher: {size}"),
    ("tip.pressure", "Memory Pressure: {level}", "Speicherdruck: {level}"),
    ("pressure.normal", "normal", "normal"),
    ("pressure.warning", "elevated", "erhöht"),
    ("pressure.critical", "critical", "kritisch"),
    ("tip.click", "Click to open Activity Monitor", "Klicken, um die Aktivitätsanzeige zu öffnen"),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use manta_plugin::i18n::{check, with_lang, Lang};

    use super::*;

    #[test]
    fn catalog_is_complete() {
        let p = check::catalog_problems(
            &CATALOG,
            &[
                "CPU",
                "GPU",
                "RAM",
                "{label} {value}",
                "{used} / {total}",
                "{n}%",
                "{n} %",
                "normal",
            ],
        );
        assert!(p.is_empty(), "{}", p.join("\n"));
    }

    #[test]
    fn every_used_key_exists() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let m = check::missing_keys(&CATALOG, &src, 25);
        assert!(m.is_empty(), "{}", m.join("\n"));
    }

    #[test]
    fn no_hard_coded_ui_strings() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let hits = check::hard_coded_strings(&src, &[], &[]);
        assert!(hits.is_empty(), "{}", hits.join("\n"));
    }

    #[test]
    fn names() {
        with_lang(Lang::De, || {
            assert_eq!(tr!("plugin.name"), "Systemauslastung")
        });
        with_lang(Lang::Zh, || assert_eq!(tr!("label.ram"), "内存"));
        with_lang(Lang::Ru, || assert_eq!(tr!("label.cpu"), "ЦП"));
    }
}
