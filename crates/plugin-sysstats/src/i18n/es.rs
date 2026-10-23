//! Spanish (es) strings for `../i18n.rs` (see `manta_plugin::i18n`).
//! Terms as in Monitor de Actividad.

use manta_plugin::i18n::Translations;

pub const ENTRIES: Translations = &[
    ("plugin.name", "Carga del sistema"),
    ("plugin.description", "Carga de la CPU, carga de la GPU y memoria usada en la barra de estado. Haz clic para abrir Monitor de Actividad."),
    ("settings.show_gpu", "Mostrar la carga de la GPU"),
    ("settings.show_gpu.tip", "Se oculta automáticamente en los Mac sin estadísticas de GPU."),
    ("label.cpu", "CPU"),
    ("label.gpu", "GPU"),
    ("label.ram", "RAM"),
    ("pct", "{n} %"),
    ("item", "{label} {value}"),
    ("ram.value", "{used} / {total}"),
    ("tip.cpu", "Carga de la CPU: {pct}"),
    ("tip.cores", "Promedio de todos los núcleos: {n}"),
    ("tip.cores.split", "Núcleos de rendimiento: {perf} · Núcleos de eficiencia: {eff}"),
    ("tip.busiest", "Núcleo más cargado: {pct}"),
    ("tip.gpu", "Carga de la GPU: {pct}"),
    ("tip.gpu_cores", "Núcleos de GPU: {n}"),
    ("tip.ram", "Memoria usada: {used} de {total}"),
    ("tip.ram.app", "Memoria de apps: {size}"),
    ("tip.ram.wired", "Memoria fija: {size}"),
    ("tip.ram.compressed", "Comprimida: {size}"),
    ("tip.ram.swap", "Memoria de intercambio usada: {size}"),
    ("tip.pressure", "Presión de memoria: {level}"),
    ("pressure.normal", "normal"),
    ("pressure.warning", "elevada"),
    ("pressure.critical", "crítica"),
    ("tip.click", "Haz clic para abrir Monitor de Actividad"),
];
