//! What a sampler reads, and turning two readings into loads.
//!
//! A [`Sampler`] returns raw counters ([`RawSample`]); the [`Tracker`]
//! keeps the previous reading and computes the CPU load from the tick
//! deltas. Tests inject fake samplers, so no real sampling happens there.

/// Cumulative CPU ticks of one core since boot (`host_processor_info`,
/// `PROCESSOR_CPU_LOAD_INFO`). 32-bit counters that wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuTicks {
    pub user: u32,
    pub system: u32,
    pub idle: u32,
    pub nice: u32,
}

impl CpuTicks {
    pub fn new(user: u32, system: u32, idle: u32, nice: u32) -> Self {
        CpuTicks {
            user,
            system,
            idle,
            nice,
        }
    }
}

/// How hard macOS works to provide memory (Activity Monitor's "Memory
/// Pressure").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

/// Memory figures in bytes, as Activity Monitor's Memory tab shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemInfo {
    /// Physical memory (`hw.memsize`).
    pub total: u64,
    /// "App Memory": anonymous pages minus purgeable ones.
    pub app: u64,
    /// "Wired Memory".
    pub wired: u64,
    /// "Compressed": what the compressor occupies.
    pub compressed: u64,
    /// "Swap Used", if known.
    pub swap_used: Option<u64>,
    pub pressure: Option<Pressure>,
}

impl MemInfo {
    /// "Memory Used" = app memory + wired + compressed.
    pub fn used(&self) -> u64 {
        self.app
            .saturating_add(self.wired)
            .saturating_add(self.compressed)
            .min(self.total.max(1))
    }

    /// Used / total, 0–1.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.used() as f64 / self.total as f64).clamp(0.0, 1.0) as f32
    }
}

/// One reading. Missing parts (`None` / empty) are hidden in the UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawSample {
    /// Ticks per core.
    pub cpu: Vec<CpuTicks>,
    pub mem: Option<MemInfo>,
    /// GPU utilization 0–1 (`None`: no GPU statistics on this Mac).
    pub gpu: Option<f32>,
}

/// Facts that don't change while the app runs (read once).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HostInfo {
    /// e.g. "Apple M2 Pro".
    pub chip: Option<String>,
    /// Performance and efficiency cores (Apple silicon), if known.
    pub perf_cores: Option<u32>,
    pub eff_cores: Option<u32>,
    pub gpu_cores: Option<u32>,
}

/// Reads the counters. Implemented by the real macOS sampler and by fakes
/// in tests. Runs on the plugin's background thread.
pub trait Sampler: Send {
    fn info(&mut self) -> HostInfo;
    fn sample(&mut self) -> RawSample;
}

/// CPU load between two readings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CpuLoad {
    /// All cores together, 0–1 (Activity Monitor's "% CPU" ÷ cores).
    pub total: f32,
    /// Each core, 0–1.
    pub per_core: Vec<f32>,
}

impl CpuLoad {
    /// The busiest core's load.
    pub fn busiest(&self) -> f32 {
        self.per_core.iter().copied().fold(0.0, f32::max)
    }
}

/// The CPU load from two tick readings. `None` if the core count changed
/// or nothing is known.
pub fn cpu_load(prev: &[CpuTicks], cur: &[CpuTicks]) -> Option<CpuLoad> {
    if prev.is_empty() || prev.len() != cur.len() {
        return None;
    }
    let mut busy_all = 0u64;
    let mut total_all = 0u64;
    let mut per_core = Vec::with_capacity(cur.len());
    for (a, b) in prev.iter().zip(cur) {
        let d = |x: u32, y: u32| y.wrapping_sub(x) as u64;
        let busy = d(a.user, b.user) + d(a.system, b.system) + d(a.nice, b.nice);
        let total = busy + d(a.idle, b.idle);
        busy_all += busy;
        total_all += total;
        per_core.push(if total == 0 {
            0.0
        } else {
            busy as f32 / total as f32
        });
    }
    let total = if total_all == 0 {
        0.0
    } else {
        busy_all as f32 / total_all as f32
    };
    Some(CpuLoad {
        total: total.clamp(0.0, 1.0),
        per_core,
    })
}

/// The loads to show.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub cpu: Option<CpuLoad>,
    /// Cores (from the last reading).
    pub cores: usize,
    pub mem: Option<MemInfo>,
    pub gpu: Option<f32>,
}

/// Keeps the previous CPU reading.
#[derive(Debug, Default)]
pub struct Tracker {
    prev: Vec<CpuTicks>,
}

impl Tracker {
    /// Take a reading. The first one has no CPU load yet (nothing to
    /// compare with).
    pub fn update(&mut self, raw: RawSample) -> Stats {
        let cpu = cpu_load(&self.prev, &raw.cpu);
        let cores = raw.cpu.len();
        self.prev = raw.cpu;
        Stats {
            cpu,
            cores,
            mem: raw.mem,
            gpu: raw.gpu.map(|g| g.clamp(0.0, 1.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_deltas() {
        let prev = [
            CpuTicks::new(100, 50, 850, 0),
            CpuTicks::new(10, 10, 980, 0),
        ];
        // Core 0: 90 busy of 100 → 0.9; core 1: 10 of 100 → 0.1.
        let cur = [
            CpuTicks::new(170, 70, 860, 0),
            CpuTicks::new(15, 13, 1070, 2),
        ];
        let l = cpu_load(&prev, &cur).unwrap();
        assert!((l.per_core[0] - 0.9).abs() < 1e-6, "{l:?}");
        assert!((l.per_core[1] - 0.1).abs() < 1e-6, "{l:?}");
        assert!((l.total - 0.5).abs() < 1e-6, "{l:?}");
        assert!((l.busiest() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn idle_wraps_and_mismatches() {
        // Counters wrap around u32::MAX.
        let prev = [CpuTicks::new(u32::MAX - 9, 0, u32::MAX - 9, 0)];
        let cur = [CpuTicks::new(10, 0, 10, 0)];
        let l = cpu_load(&prev, &cur).unwrap();
        assert!((l.total - 0.5).abs() < 1e-6, "{l:?}");
        // No time passed: 0, not NaN.
        assert_eq!(cpu_load(&cur, &cur).unwrap().total, 0.0);
        assert_eq!(cpu_load(&[], &cur), None);
        assert_eq!(cpu_load(&cur, &[cur[0], cur[0]]), None);
    }

    #[test]
    fn tracker_needs_two_readings() {
        let mut t = Tracker::default();
        let raw = |busy: u32, idle: u32| RawSample {
            cpu: vec![CpuTicks::new(busy, 0, idle, 0); 4],
            mem: None,
            gpu: Some(1.7),
        };
        let s = t.update(raw(0, 0));
        assert_eq!(s.cpu, None);
        assert_eq!(s.cores, 4);
        assert_eq!(s.gpu, Some(1.0), "clamped");
        let s = t.update(raw(25, 75));
        assert!((s.cpu.unwrap().total - 0.25).abs() < 1e-6);
        let s = t.update(raw(125, 75));
        assert!((s.cpu.unwrap().total - 1.0).abs() < 1e-6);
    }

    #[test]
    fn memory_used_like_activity_monitor() {
        const GIB: u64 = 1 << 30;
        let m = MemInfo {
            total: 16 * GIB,
            app: 8 * GIB,
            wired: 3 * GIB,
            compressed: GIB,
            ..Default::default()
        };
        assert_eq!(m.used(), 12 * GIB);
        assert!((m.fraction() - 0.75).abs() < 1e-6);
        let over = MemInfo {
            total: GIB,
            app: 2 * GIB,
            ..Default::default()
        };
        assert_eq!(over.used(), GIB);
        assert_eq!(MemInfo::default().fraction(), 0.0);
    }
}
