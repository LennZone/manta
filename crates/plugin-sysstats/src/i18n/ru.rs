//! Russian (ru) strings for `../i18n.rs` (see `manta_plugin::i18n`).
//! Terms as in «Мониторинг системы».

use manta_plugin::i18n::Translations;

pub const ENTRIES: Translations = &[
    ("plugin.name", "Загрузка системы"),
    ("plugin.description", "Загрузка ЦП, загрузка ГП и использованная память в строке состояния. Нажатие открывает «Мониторинг системы»."),
    ("settings.show_gpu", "Показывать загрузку ГП"),
    ("settings.show_gpu.tip", "Автоматически скрывается на Mac без статистики ГП."),
    ("label.cpu", "ЦП"),
    ("label.gpu", "ГП"),
    ("label.ram", "Память"),
    ("pct", "{n} %"),
    ("item", "{label} {value}"),
    ("ram.value", "{used} / {total}"),
    ("tip.cpu", "Загрузка ЦП: {pct}"),
    ("tip.cores", "Среднее по всем ядрам: {n}"),
    ("tip.cores.split", "Производительные ядра: {perf} · Энергоэффективные ядра: {eff}"),
    ("tip.busiest", "Самое загруженное ядро: {pct}"),
    ("tip.gpu", "Загрузка ГП: {pct}"),
    ("tip.gpu_cores", "Ядра ГП: {n}"),
    ("tip.ram", "Использовано памяти: {used} из {total}"),
    ("tip.ram.app", "Память приложений: {size}"),
    ("tip.ram.wired", "Зарезервированная память: {size}"),
    ("tip.ram.compressed", "Сжатая: {size}"),
    ("tip.ram.swap", "Использовано подкачки: {size}"),
    ("tip.pressure", "Нагрузка на память: {level}"),
    ("pressure.normal", "нормальная"),
    ("pressure.warning", "повышенная"),
    ("pressure.critical", "критическая"),
    ("tip.click", "Нажмите, чтобы открыть «Мониторинг системы»"),
];
