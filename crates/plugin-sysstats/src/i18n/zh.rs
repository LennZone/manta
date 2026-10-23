//! Simplified Chinese (zh-Hans) strings for `../i18n.rs` (see `manta_plugin::i18n`).
//! Terms as in 活动监视器.

use manta_plugin::i18n::Translations;

pub const ENTRIES: Translations = &[
    ("plugin.name", "系统负载"),
    (
        "plugin.description",
        "在状态栏中显示 CPU 负载、GPU 负载和已使用内存。点按可打开“活动监视器”。",
    ),
    ("settings.show_gpu", "显示 GPU 负载"),
    (
        "settings.show_gpu.tip",
        "在没有 GPU 统计数据的 Mac 上会自动隐藏。",
    ),
    ("label.cpu", "CPU"),
    ("label.gpu", "GPU"),
    ("label.ram", "内存"),
    ("pct", "{n}%"),
    ("item", "{label} {value}"),
    ("ram.value", "{used} / {total}"),
    ("tip.cpu", "CPU 负载：{pct}"),
    ("tip.cores", "所有核心的平均值：{n}"),
    ("tip.cores.split", "性能核心：{perf} · 能效核心：{eff}"),
    ("tip.busiest", "负载最高的核心：{pct}"),
    ("tip.gpu", "GPU 负载：{pct}"),
    ("tip.gpu_cores", "GPU 核心：{n}"),
    ("tip.ram", "已使用内存：{used}，共 {total}"),
    ("tip.ram.app", "App 内存：{size}"),
    ("tip.ram.wired", "联动内存：{size}"),
    ("tip.ram.compressed", "已压缩：{size}"),
    ("tip.ram.swap", "已使用的交换：{size}"),
    ("tip.pressure", "内存压力：{level}"),
    ("pressure.normal", "正常"),
    ("pressure.warning", "偏高"),
    ("pressure.critical", "严重"),
    ("tip.click", "点按以打开“活动监视器”"),
];
