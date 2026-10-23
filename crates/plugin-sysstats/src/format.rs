//! The status bar items: "CPU 12 %", "GPU 3 %", "RAM 12,4 / 16 GB", with
//! meters, warning colours and tooltips, in the UI language.

use manta_plugin::i18n::{format_count, format_decimal, lang, size_unit, unit_space};
use manta_plugin::{StatusAction, StatusItem, StatusLevel};

use crate::i18n::tr;
use crate::sample::{HostInfo, MemInfo, Pressure, Stats};
use crate::settings::Settings;

/// CPU and GPU loads above this are shown in amber.
pub const LOAD_WARN: f32 = 0.85;
/// Memory used above this is amber when macOS reports no memory pressure.
pub const RAM_WARN: f32 = 0.9;

pub const CPU_ID: &str = "cpu";
pub const GPU_ID: &str = "gpu";
pub const RAM_ID: &str = "ram";

/// Activity Monitor's bundle id (a click on an item opens it).
pub const ACTIVITY_MONITOR: &str = "com.apple.ActivityMonitor";

const GIB: f64 = (1u64 << 30) as f64;

/// A load 0–1 as a whole percentage: "12%", "12 %", "12 %".
pub fn percent(f: f32) -> String {
    let n = (f.clamp(0.0, 1.0) * 100.0).round() as u32;
    tr!("pct", n = n)
}

/// Bytes in GB like Activity Monitor (binary, labelled "GB"): one
/// decimal ("12,4"); whole numbers without one when `trim` ("16").
fn gb_number(bytes: u64, trim: bool) -> String {
    let v = bytes as f64 / GIB;
    if trim && (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as u64)
    } else {
        format_decimal(v, 1)
    }
}

/// "12,4 GB" / "12,4 Go" / "12,4 ГБ".
pub fn gb(bytes: u64) -> String {
    format!(
        "{}{}{}",
        gb_number(bytes, false),
        unit_space(lang()),
        size_unit(2)
    )
}

/// "16 GB" (a whole number when it is one), for the installed memory.
pub fn gb_whole(bytes: u64) -> String {
    format!(
        "{}{}{}",
        gb_number(bytes, true),
        unit_space(lang()),
        size_unit(2)
    )
}

/// "12,4 / 16 GB".
pub fn ram_value(m: &MemInfo) -> String {
    tr!(
        "ram.value",
        used = gb_number(m.used(), false),
        total = gb_whole(m.total)
    )
}

fn load_level(f: f32) -> StatusLevel {
    if f > LOAD_WARN {
        StatusLevel::Warn
    } else {
        StatusLevel::Normal
    }
}

pub fn ram_level(m: &MemInfo) -> StatusLevel {
    match m.pressure {
        Some(Pressure::Critical) => StatusLevel::Error,
        Some(Pressure::Warning) => StatusLevel::Warn,
        Some(Pressure::Normal) => StatusLevel::Normal,
        None if m.fraction() > RAM_WARN => StatusLevel::Warn,
        None => StatusLevel::Normal,
    }
}

fn open_activity_monitor() -> Option<StatusAction> {
    Some(StatusAction::OpenApp {
        bundle_id: ACTIVITY_MONITOR.into(),
    })
}

fn tooltip(lines: Vec<String>) -> Option<String> {
    let mut lines = lines;
    lines.push(String::new());
    lines.push(tr!("tip.click").to_string());
    Some(lines.join("\n"))
}

/// The items for the current loads. Priorities: CPU before memory before
/// GPU; the GPU has no compact form, so it is the first to go in narrow
/// windows, then CPU and memory shrink to icon and meter.
pub fn items(stats: &Stats, info: &HostInfo, settings: &Settings) -> Vec<StatusItem> {
    let mut v = Vec::new();
    if let Some(cpu) = &stats.cpu {
        let mut lines = vec![tr!("tip.cpu", pct = percent(cpu.total))];
        if stats.cores > 0 {
            lines.push(tr!("tip.cores", n = format_count(stats.cores as u64)));
        }
        if let (Some(p), Some(e)) = (info.perf_cores, info.eff_cores) {
            lines.push(tr!("tip.cores.split", perf = p, eff = e));
        }
        if cpu.per_core.len() > 1 {
            lines.push(tr!("tip.busiest", pct = percent(cpu.busiest())));
        }
        if let Some(chip) = &info.chip {
            lines.push(chip.clone());
        }
        v.push(StatusItem {
            compact: Some(String::new()),
            tooltip: tooltip(lines),
            icon: Some("cpu".into()),
            level: load_level(cpu.total),
            meter: Some(cpu.total),
            priority: 30,
            action: open_activity_monitor(),
            ..StatusItem::new(
                CPU_ID,
                tr!("item", label = tr!("label.cpu"), value = percent(cpu.total)),
            )
        });
    }
    if let Some(g) = stats.gpu.filter(|_| settings.show_gpu) {
        let mut lines = vec![tr!("tip.gpu", pct = percent(g))];
        if let Some(n) = info.gpu_cores {
            lines.push(tr!("tip.gpu_cores", n = n));
        }
        if let Some(chip) = &info.chip {
            lines.push(chip.clone());
        }
        v.push(StatusItem {
            compact: None,
            tooltip: tooltip(lines),
            icon: Some("gpu".into()),
            level: load_level(g),
            meter: Some(g),
            priority: 10,
            action: open_activity_monitor(),
            ..StatusItem::new(
                GPU_ID,
                tr!("item", label = tr!("label.gpu"), value = percent(g)),
            )
        });
    }
    if let Some(m) = &stats.mem {
        let mut lines = vec![
            tr!("tip.ram", used = gb(m.used()), total = gb_whole(m.total)),
            tr!("tip.ram.app", size = gb(m.app)),
            tr!("tip.ram.wired", size = gb(m.wired)),
            tr!("tip.ram.compressed", size = gb(m.compressed)),
        ];
        if let Some(s) = m.swap_used.filter(|s| *s > 0) {
            lines.push(tr!("tip.ram.swap", size = gb(s)));
        }
        if let Some(p) = m.pressure {
            let level = match p {
                Pressure::Normal => tr!("pressure.normal"),
                Pressure::Warning => tr!("pressure.warning"),
                Pressure::Critical => tr!("pressure.critical"),
            };
            lines.push(tr!("tip.pressure", level = level));
        }
        v.push(StatusItem {
            compact: Some(String::new()),
            tooltip: tooltip(lines),
            icon: Some("memory".into()),
            level: ram_level(m),
            meter: Some(m.fraction()),
            priority: 20,
            action: open_activity_monitor(),
            ..StatusItem::new(
                RAM_ID,
                tr!("item", label = tr!("label.ram"), value = ram_value(m)),
            )
        });
    }
    v
}

#[cfg(test)]
mod tests {
    use manta_plugin::i18n::{with_lang, Lang};

    use super::*;
    use crate::sample::CpuLoad;

    const G: u64 = 1 << 30;

    fn mem(used_tenths_gib: u64, pressure: Option<Pressure>) -> MemInfo {
        MemInfo {
            total: 16 * G,
            app: used_tenths_gib * G / 10 - 3 * G,
            wired: 2 * G,
            compressed: G,
            swap_used: Some(0),
            pressure,
        }
    }

    fn stats(cpu: f32, gpu: Option<f32>) -> Stats {
        Stats {
            cpu: Some(CpuLoad {
                total: cpu,
                per_core: vec![cpu, 1.0],
            }),
            cores: 10,
            mem: Some(mem(124, Some(Pressure::Normal))),
            gpu,
        }
    }

    fn info() -> HostInfo {
        HostInfo {
            chip: Some("Apple M2 Pro".into()),
            perf_cores: Some(8),
            eff_cores: Some(2),
            gpu_cores: Some(19),
        }
    }

    fn texts(v: &[StatusItem]) -> Vec<&str> {
        v.iter().map(|i| i.text.as_str()).collect()
    }

    #[test]
    fn numbers_per_language() {
        let m = mem(124, None);
        with_lang(Lang::En, || {
            assert_eq!(percent(0.123), "12%");
            assert_eq!(ram_value(&m), "12.4 / 16 GB");
        });
        with_lang(Lang::De, || {
            assert_eq!(percent(0.123), "12 %");
            assert_eq!(percent(1.7), "100 %");
            assert_eq!(ram_value(&m), "12,4 / 16 GB");
            assert_eq!(gb(G * 3 / 2), "1,5 GB");
        });
        with_lang(Lang::Fr, || {
            assert_eq!(percent(0.5), "50\u{a0}%");
            assert_eq!(ram_value(&m), "12,4 / 16\u{a0}Go");
        });
        with_lang(Lang::Ru, || assert_eq!(ram_value(&m), "12,4 / 16\u{a0}ГБ"));
        // A total that isn't whole keeps its decimal.
        let odd = MemInfo {
            total: 24 * G + G / 2,
            ..m
        };
        with_lang(Lang::En, || assert_eq!(ram_value(&odd), "12.4 / 24.5 GB"));
    }

    #[test]
    fn items_in_order_with_priorities() {
        with_lang(Lang::De, || {
            let v = items(&stats(0.12, Some(0.03)), &info(), &Settings::default());
            assert_eq!(texts(&v), ["CPU 12 %", "GPU 3 %", "RAM 12,4 / 16 GB"]);
            assert_eq!(
                v.iter().map(|i| i.priority).collect::<Vec<_>>(),
                [30, 10, 20]
            );
            assert_eq!(v[0].compact.as_deref(), Some(""));
            assert_eq!(v[1].compact, None, "the GPU goes first when narrow");
            assert!(v.iter().all(|i| i.level == StatusLevel::Normal));
            assert!(v.iter().all(|i| i.action
                == Some(StatusAction::OpenApp {
                    bundle_id: ACTIVITY_MONITOR.into()
                })));
            assert!((v[2].meter.unwrap() - 12.4 / 16.0).abs() < 1e-3);
            let tip = v[0].tooltip.as_deref().unwrap();
            assert!(
                tip.starts_with("CPU-Last: 12 %\nDurchschnitt aller Kerne: 10\n"),
                "{tip}"
            );
            assert!(
                tip.contains("Performance-Kerne: 8 · Effizienz-Kerne: 2"),
                "{tip}"
            );
            assert!(tip.contains("Am stärksten belasteter Kern: 100 %"), "{tip}");
            assert!(
                tip.ends_with("\n\nKlicken, um die Aktivitätsanzeige zu öffnen"),
                "{tip}"
            );
            let tip = v[2].tooltip.as_deref().unwrap();
            assert!(
                tip.contains("Belegter Speicher: 12,4 GB von 16 GB"),
                "{tip}"
            );
            assert!(tip.contains("Speicherdruck: normal"), "{tip}");
            assert!(!tip.contains("Swap"), "no swap line without swap: {tip}");
            assert!(v[1].tooltip.as_deref().unwrap().contains("GPU-Kerne: 19"));
        });
        with_lang(Lang::Zh, || {
            let v = items(&stats(0.5, None), &info(), &Settings::default());
            assert_eq!(texts(&v), ["CPU 50%", "内存 12.4 / 16 GB"]);
        });
        with_lang(Lang::Ru, || {
            let v = items(&stats(0.5, Some(0.2)), &info(), &Settings::default());
            assert_eq!(
                texts(&v),
                ["ЦП 50 %", "ГП 20 %", "Память 12,4 / 16\u{a0}ГБ"]
            );
        });
    }

    #[test]
    fn missing_parts_and_settings() {
        let s = Stats {
            cpu: None,
            cores: 0,
            mem: None,
            gpu: Some(0.4),
        };
        with_lang(Lang::En, || {
            assert_eq!(
                texts(&items(&s, &HostInfo::default(), &Settings::default())),
                ["GPU 40%"]
            );
            assert!(items(&s, &HostInfo::default(), &Settings { show_gpu: false }).is_empty());
            assert!(items(&Stats::default(), &info(), &Settings::default()).is_empty());
        });
    }

    #[test]
    fn warning_levels() {
        with_lang(Lang::En, || {
            let v = items(&stats(0.86, Some(0.9)), &info(), &Settings::default());
            assert_eq!(v[0].level, StatusLevel::Warn);
            assert_eq!(v[1].level, StatusLevel::Warn);
            assert_eq!(
                items(&stats(0.85, Some(0.5)), &info(), &Settings::default())[0].level,
                StatusLevel::Normal
            );
        });
        assert_eq!(
            ram_level(&mem(124, Some(Pressure::Warning))),
            StatusLevel::Warn
        );
        assert_eq!(
            ram_level(&mem(124, Some(Pressure::Critical))),
            StatusLevel::Error
        );
        // No pressure figure: by how full memory is.
        assert_eq!(ram_level(&mem(150, None)), StatusLevel::Warn);
        assert_eq!(ram_level(&mem(140, None)), StatusLevel::Normal);
        // Normal pressure wins over a full-looking memory (caches).
        assert_eq!(
            ram_level(&mem(155, Some(Pressure::Normal))),
            StatusLevel::Normal
        );
    }
}
