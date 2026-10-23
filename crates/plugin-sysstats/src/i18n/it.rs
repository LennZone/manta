//! Italian (it) strings for `../i18n.rs` (see `manta_plugin::i18n`).
//! Terms as in Monitoraggio Attività.

use manta_plugin::i18n::Translations;

pub const ENTRIES: Translations = &[
    ("plugin.name", "Carico di sistema"),
    ("plugin.description", "Carico della CPU, carico della GPU e memoria utilizzata nella barra di stato. Fai clic per aprire Monitoraggio Attività."),
    ("settings.show_gpu", "Mostra il carico della GPU"),
    ("settings.show_gpu.tip", "Nascosto automaticamente sui Mac senza statistiche della GPU."),
    ("label.cpu", "CPU"),
    ("label.gpu", "GPU"),
    ("label.ram", "RAM"),
    ("pct", "{n}%"),
    ("item", "{label} {value}"),
    ("ram.value", "{used} / {total}"),
    ("tip.cpu", "Carico della CPU: {pct}"),
    ("tip.cores", "Media di tutti i core: {n}"),
    ("tip.cores.split", "Core ad alte prestazioni: {perf} · Core ad alta efficienza: {eff}"),
    ("tip.busiest", "Core più carico: {pct}"),
    ("tip.gpu", "Carico della GPU: {pct}"),
    ("tip.gpu_cores", "Core GPU: {n}"),
    ("tip.ram", "Memoria utilizzata: {used} di {total}"),
    ("tip.ram.app", "Memoria app: {size}"),
    ("tip.ram.wired", "Memoria residente: {size}"),
    ("tip.ram.compressed", "Compressa: {size}"),
    ("tip.ram.swap", "Swap utilizzato: {size}"),
    ("tip.pressure", "Pressione della memoria: {level}"),
    ("pressure.normal", "normale"),
    ("pressure.warning", "elevata"),
    ("pressure.critical", "critica"),
    ("tip.click", "Fai clic per aprire Monitoraggio Attività"),
];
