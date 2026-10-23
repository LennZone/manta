//! French (fr) strings for `../i18n.rs` (see `manta_plugin::i18n`).
//! Terms as in Moniteur d’activité.

use manta_plugin::i18n::Translations;

pub const ENTRIES: Translations = &[
    ("plugin.name", "Charge du système"),
    ("plugin.description", "Charge du processeur, charge de la carte graphique et mémoire utilisée dans la barre d’état. Cliquez pour ouvrir Moniteur d’activité."),
    ("settings.show_gpu", "Afficher la charge de la carte graphique"),
    ("settings.show_gpu.tip", "Masquée automatiquement sur les Mac sans statistiques de carte graphique."),
    ("label.cpu", "CPU"),
    ("label.gpu", "GPU"),
    ("label.ram", "RAM"),
    ("pct", "{n}\u{a0}%"),
    ("item", "{label} {value}"),
    ("ram.value", "{used} / {total}"),
    ("tip.cpu", "Charge du processeur\u{a0}: {pct}"),
    ("tip.cores", "Moyenne de tous les cœurs\u{a0}: {n}"),
    ("tip.cores.split", "Cœurs de performance\u{a0}: {perf} · Cœurs d’efficacité\u{a0}: {eff}"),
    ("tip.busiest", "Cœur le plus chargé\u{a0}: {pct}"),
    ("tip.gpu", "Charge de la carte graphique\u{a0}: {pct}"),
    ("tip.gpu_cores", "Cœurs graphiques\u{a0}: {n}"),
    ("tip.ram", "Mémoire utilisée\u{a0}: {used} sur {total}"),
    ("tip.ram.app", "Mémoire de l’app\u{a0}: {size}"),
    ("tip.ram.wired", "Mémoire résidente\u{a0}: {size}"),
    ("tip.ram.compressed", "Compressée\u{a0}: {size}"),
    ("tip.ram.swap", "Swap utilisé\u{a0}: {size}"),
    ("tip.pressure", "Pression sur la mémoire\u{a0}: {level}"),
    ("pressure.normal", "normale"),
    ("pressure.warning", "élevée"),
    ("pressure.critical", "critique"),
    ("tip.click", "Cliquez pour ouvrir Moniteur d’activité"),
];
