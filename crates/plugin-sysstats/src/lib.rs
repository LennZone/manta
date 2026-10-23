//! Built-in System Load plugin. PUBLIC API CONTRACT (used by the app):
//! `SysStatsPlugin::new()`, and `SysStatsPlugin::with_sampler` for tests.
//!
//! Shows CPU load, GPU load and memory used as status bar items
//! ([`HostRequest::SetStatusItems`]): "CPU 12 %  GPU 3 %  RAM 12,4 / 16 GB",
//! each with a small meter, amber at high load, details in the tooltip, a
//! click opens Activity Monitor.
//!
//! Threads: one background thread reads the counters every
//! [`INTERVAL`] (see [`mac`] for what is read) and pushes the items to
//! the host, which wakes the UI; nothing is sent while the window is
//! minimized or hidden, or when the items didn't change. Tests inject a
//! fake [`Sampler`], so they never read the real machine.

pub mod format;
pub mod i18n;
#[cfg(target_os = "macos")]
pub mod mac;
pub mod sample;
pub mod settings;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use manta_plugin::i18n::{lang, set_thread_lang, Lang};
use manta_plugin::{HostRequest, Plugin, PluginEvent, PluginHandle, PluginInfo, StatusItem};

use i18n::tr;
use sample::{Sampler, Tracker};
use settings::Settings;

pub use sample::{CpuTicks, HostInfo, MemInfo, Pressure, RawSample};

pub const PLUGIN_ID: &str = "manta.sysstats";

/// How often the counters are read.
pub const INTERVAL: Duration = Duration::from_secs(2);
/// The first reading after the baseline comes sooner, so the items show
/// up quickly after launch.
const FIRST: Duration = Duration::from_millis(500);

/// Makes the sampler when the plugin is switched on.
pub type SamplerFactory = Box<dyn FnMut() -> Box<dyn Sampler> + Send>;

enum Msg {
    Lang(Lang),
    Settings(Settings),
    Stop,
}

struct Worker {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

pub struct SysStatsPlugin {
    handle: Option<PluginHandle>,
    worker: Option<Worker>,
    settings: Settings,
    factory: SamplerFactory,
    interval: Duration,
}

impl SysStatsPlugin {
    /// The plugin with the real sampler (macOS); elsewhere it shows nothing.
    pub fn new() -> Self {
        Self::with_sampler(Box::new(real_sampler), INTERVAL)
    }

    /// The plugin with another sampler and interval (tests, renders).
    pub fn with_sampler(factory: SamplerFactory, interval: Duration) -> Self {
        SysStatsPlugin {
            handle: None,
            worker: None,
            settings: Settings::default(),
            factory,
            interval,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn set_settings(&mut self, s: Settings) {
        if s == self.settings {
            return;
        }
        self.settings = s;
        if let Some(h) = &self.handle {
            s.save(h.data_dir());
        }
        if let Some(w) = &self.worker {
            let _ = w.tx.send(Msg::Settings(s));
        }
    }
}

impl Default for SysStatsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
fn real_sampler() -> Box<dyn Sampler> {
    Box::new(mac::MacSampler::new())
}

/// No counters on other systems: no items.
#[cfg(not(target_os = "macos"))]
fn real_sampler() -> Box<dyn Sampler> {
    struct Nothing;
    impl Sampler for Nothing {
        fn info(&mut self) -> HostInfo {
            HostInfo::default()
        }
        fn sample(&mut self) -> RawSample {
            RawSample::default()
        }
    }
    Box::new(Nothing)
}

/// The window is minimized or fully covered: nobody sees the status bar.
fn window_hidden(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        let v = i.viewport();
        v.minimized == Some(true) || v.occluded == Some(true)
    })
}

/// The background thread: baseline reading, then one every `interval`.
fn run(
    handle: PluginHandle,
    mut sampler: Box<dyn Sampler>,
    rx: Receiver<Msg>,
    interval: Duration,
    lang: Lang,
    mut settings: Settings,
) {
    set_thread_lang(Some(lang));
    let info = sampler.info();
    let mut tracker = Tracker::default();
    let mut stats = tracker.update(sampler.sample());
    let mut sent: Option<Vec<StatusItem>> = None;
    let mut next = Instant::now() + FIRST.min(interval);
    loop {
        let mut resend = false;
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Lang(l)) => {
                set_thread_lang(Some(l));
                resend = true;
            }
            Ok(Msg::Settings(s)) => {
                settings = s;
                resend = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if !resend {
            if Instant::now() < next {
                continue;
            }
            next = Instant::now() + interval;
            if window_hidden(handle.egui_ctx()) {
                continue;
            }
            stats = tracker.update(sampler.sample());
            if stats.cpu.is_none() && stats.mem.is_none() && stats.gpu.is_none() {
                continue;
            }
        }
        let items = format::items(&stats, &info, &settings);
        if sent.as_ref() != Some(&items) {
            handle.send(HostRequest::SetStatusItems {
                items: items.clone(),
            });
            sent = Some(items);
        }
    }
}

impl Plugin for SysStatsPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: tr!("plugin.name").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: tr!("plugin.description").into(),
            author: "Manta".into(),
            builtin: true,
        }
    }

    fn enable(&mut self, handle: PluginHandle) {
        if self.worker.is_some() {
            self.disable();
        }
        self.settings = Settings::load(handle.data_dir());
        let (tx, rx) = mpsc::channel();
        let sampler = (self.factory)();
        let (h, interval, l, s) = (handle.clone(), self.interval, lang(), self.settings);
        let join = std::thread::Builder::new()
            .name("manta-sysstats".into())
            .spawn(move || run(h, sampler, rx, interval, l, s))
            .ok();
        self.worker = Some(Worker { tx, join });
        self.handle = Some(handle);
    }

    fn disable(&mut self) {
        if let Some(mut w) = self.worker.take() {
            let _ = w.tx.send(Msg::Stop);
            if let Some(j) = w.join.take() {
                // A reading takes well under a millisecond; don't hang the UI
                // if one ever stalls.
                let deadline = Instant::now() + Duration::from_millis(150);
                while !j.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                }
                if j.is_finished() {
                    let _ = j.join();
                }
            }
        }
        self.handle = None;
    }

    fn on_event(&mut self, event: &PluginEvent) {
        if let PluginEvent::LocaleChanged(_) = event {
            if let Some(w) = &self.worker {
                let _ = w.tx.send(Msg::Lang(lang()));
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut s = self.settings;
        ui.checkbox(&mut s.show_gpu, tr!("settings.show_gpu"))
            .on_hover_text(tr!("settings.show_gpu.tip"));
        self.set_settings(s);
        true
    }
}

/// A sampler that returns fixed loads (tests, renders): CPU `cpu` on
/// `cores` cores, `used` of `total` bytes of memory, GPU `gpu`.
#[doc(hidden)]
pub struct FakeSampler {
    pub cores: usize,
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub mem: Option<MemInfo>,
    pub info: HostInfo,
    ticks: u32,
}

impl FakeSampler {
    pub fn new(cpu: f32, gpu: Option<f32>, mem: Option<MemInfo>) -> Self {
        FakeSampler {
            cores: 10,
            cpu,
            gpu,
            mem,
            info: HostInfo {
                chip: Some("Apple M2 Pro".into()),
                perf_cores: Some(8),
                eff_cores: Some(2),
                gpu_cores: Some(19),
            },
            ticks: 0,
        }
    }
}

impl Sampler for FakeSampler {
    fn info(&mut self) -> HostInfo {
        self.info.clone()
    }

    fn sample(&mut self) -> RawSample {
        // 1000 ticks per reading, `cpu` of them busy.
        self.ticks += 1000;
        let busy = (self.cpu.clamp(0.0, 1.0) * self.ticks as f32).round() as u32;
        RawSample {
            cpu: vec![CpuTicks::new(busy, 0, self.ticks - busy, 0); self.cores],
            mem: self.mem,
            gpu: self.gpu,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use manta_plugin::i18n::with_lang;

    use super::*;

    const G: u64 = 1 << 30;

    fn mem() -> MemInfo {
        MemInfo {
            total: 16 * G,
            app: 9 * G,
            wired: 2 * G,
            compressed: G,
            swap_used: None,
            pressure: Some(Pressure::Normal),
        }
    }

    fn handle(dir: &std::path::Path) -> (PluginHandle, mpsc::Receiver<(String, HostRequest)>) {
        let (tx, rx) = mpsc::channel();
        (
            PluginHandle::new(
                PLUGIN_ID.into(),
                tx,
                egui::Context::default(),
                dir.to_path_buf(),
            ),
            rx,
        )
    }

    fn next_items(rx: &mpsc::Receiver<(String, HostRequest)>) -> Vec<StatusItem> {
        match rx.recv_timeout(Duration::from_secs(5)).expect("items") {
            (_, HostRequest::SetStatusItems { items }) => items,
            (_, other) => panic!("{other:?}"),
        }
    }

    #[test]
    fn pushes_items_from_the_sampler_and_stops() {
        let tmp = tempfile::tempdir().unwrap();
        let (h, rx) = handle(tmp.path());
        let mut p = SysStatsPlugin::with_sampler(
            Box::new(|| Box::new(FakeSampler::new(0.42, Some(0.03), Some(mem())))),
            Duration::from_millis(20),
        );
        with_lang(Lang::De, || p.enable(h));
        let items = next_items(&rx);
        let texts: Vec<_> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["CPU 42 %", "GPU 3 %", "RAM 12,0 / 16 GB"]);
        // Unchanged readings aren't sent again.
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        // Settings: the GPU goes (and the choice is saved).
        p.set_settings(Settings { show_gpu: false });
        let items = next_items(&rx);
        assert_eq!(
            items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["cpu", "ram"]
        );
        assert!(!Settings::load(tmp.path()).show_gpu, "saved");
        // The language follows the app.
        with_lang(Lang::Ru, || {
            p.on_event(&PluginEvent::LocaleChanged("ru".into()))
        });
        let items = next_items(&rx);
        assert_eq!(items[0].text, "ЦП 42 %");
        p.disable();
        assert!(p.worker.is_none());
        while rx.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(60));
        assert!(rx.try_recv().is_err(), "nothing after disable");
    }
}
