//! Very large folders: benchmarks behind `docs/performance.md`. Ignored by
//! default (they create hundreds of thousands of files and take minutes):
//!
//! ```sh
//! cargo test -p manta --profile perf bench_large_folders -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `MANTA_BENCH_SIZES=100000,300000` (default) picks the local folder
//! sizes; the fixtures are kept in `$TMPDIR/manta-bench-<n>` for the next
//! run (delete them by hand). The remote case is an in-memory server
//! ([`MemFs`] behind an SFTP-looking wrapper) with 100,000 entries. The
//! real built-in plugins are loaded, as in the app. Times are wall clock
//! on the machine that runs it; frames are headless egui frames (layout and
//! tessellation input, no GPU), which is where the per-item costs are.
//!
//! Child of `ux_tests`.

use std::io::{Read, Write};
use std::sync::Arc;

use manta_core::{Protocol, SharedVfs, Vfs, VfsResult};
use manta_ops::memfs::MemFs;

use super::*;
use crate::app::{Action, Location};
use crate::util::{SortColumn, SortSpec};
use crate::views::ViewMode;

/// Resident memory of this process in MB (`ps`; good enough for deltas).
fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
        / 1024.0
}

/// The heap the left tab's listing takes: its entries (with their names
/// and paths) and the view; RSS deltas are too noisy for this.
fn listing_mb(a: &MantaApp) -> String {
    let tab = a.pane(Side::Left).tab();
    let entries: usize = tab
        .entries
        .iter()
        .map(|e| {
            std::mem::size_of::<Entry>()
                + e.name.capacity()
                + e.path.as_str().len()
                + e.link_target.as_ref().map_or(0, |l| l.capacity())
        })
        .sum::<usize>()
        + (tab.entries.capacity() - tab.entries.len()) * std::mem::size_of::<Entry>();
    let view = tab.view.capacity() * std::mem::size_of::<usize>();
    format!("{:.1} MB", (entries + view) as f64 / 1e6)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn p95(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get((v.len() * 95 / 100).min(v.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

/// A tiny valid PNG (so the Icons view makes real thumbnails).
fn png_bytes() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 5) as u8, 128, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// The name of item `i` of a benchmark folder: 1 % folders, 20 % PNGs, the
/// rest documents with mixed natural-sort names.
fn bench_name(i: usize) -> (String, bool) {
    match i % 100 {
        0 => (format!("Folder {i}"), true),
        1..=20 => (format!("IMG_{i:06}.png"), false),
        21..=40 => (format!("report-{i}.pdf"), false),
        41..=45 => (format!(".hidden-{i}"), false),
        _ => (format!("notes {i} draft.txt"), false),
    }
}

/// `$TMPDIR/manta-bench-<n>` with `big/` (n items) and a small `Design/`
/// (the harness shows it in the right pane). Made once, then reused.
fn local_fixture(n: usize) -> Fixture {
    let base = std::env::temp_dir().join(format!("manta-bench-{n}"));
    let done = base.join(".complete");
    if !done.is_file() {
        let _ = std::fs::remove_dir_all(&base);
        let big = base.join("big");
        std::fs::create_dir_all(&big).unwrap();
        std::fs::create_dir_all(base.join("Design")).unwrap();
        let png = png_bytes();
        let start = Instant::now();
        for i in 0..n {
            let (name, dir) = bench_name(i);
            let p = big.join(&name);
            if dir {
                std::fs::create_dir(&p).unwrap();
            } else if name.ends_with(".png") {
                std::fs::write(&p, &png).unwrap();
            } else {
                std::fs::write(&p, vec![b'x'; i % 2048]).unwrap();
            }
        }
        std::fs::write(&done, "").unwrap();
        eprintln!(
            "bench: made {n} files in {:.1} s",
            start.elapsed().as_secs_f64()
        );
    }
    Fixture {
        _tmp: tempfile::tempdir().unwrap(),
        root: base,
    }
}

/// An SFTP-looking server backed by memory (protocol and label only).
struct FakeRemote(MemFs);

impl Vfs for FakeRemote {
    fn label(&self) -> String {
        "bench@example.com (SFTP)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Sftp
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::new("/"))
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.0.list(dir)
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        self.0.stat(path)
    }
    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.0.create_dir(path)
    }
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.0.remove_file(path)
    }
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.0.remove_dir(path)
    }
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.0.rename(from, to)
    }
    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        self.0.open_read(path)
    }
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.0.open_write(path)
    }
}

fn bench_harness(fx: &Fixture) -> H {
    let mut h = harness_full(
        fx,
        WIDE,
        Theme::Light,
        false,
        Box::new(|ctx: &egui::Context| {
            PluginHost::new(ctx.clone(), super::super::builtin_plugins(), None)
        }),
    );
    // Panels of the built-in plugins stay closed (as on a first launch).
    let a = app(&mut h);
    for placement in [
        manta_plugin::PanelPlacement::PaneBottom,
        manta_plugin::PanelPlacement::RightSidebar,
    ] {
        for p in a.plugins.panels(placement) {
            a.plugins
                .set_panel_open(&p.plugin_id, &p.spec.id, None, false);
        }
    }
    a.activate(Side::Left);
    h.run_steps(3);
    h
}

/// One timed frame.
fn frame(h: &mut H) -> f64 {
    let t = Instant::now();
    h.step();
    ms(t.elapsed())
}

/// `n` frames, (median, p95) in ms; `input` runs before each.
fn frames(h: &mut H, n: usize, mut input: impl FnMut(&mut H, usize)) -> (f64, f64) {
    let mut v: Vec<f64> = (0..n)
        .map(|i| {
            input(h, i);
            frame(h)
        })
        .collect();
    (median(&mut v), p95(&mut v))
}

fn list_center(h: &mut H) -> egui::Pos2 {
    let _ = h;
    egui::pos2(500.0, 400.0)
}

fn wheel(h: &mut H, dy: f32) {
    let pos = list_center(h);
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
}

/// Times of opening a folder in the left pane.
struct Opened {
    /// Until the first items were on screen.
    first: f64,
    /// Until the whole listing was on screen.
    complete: f64,
    /// The longest frame meanwhile (the UI thread's share of the work).
    max_frame: f64,
}

/// Navigate the left pane and time until the listing is on screen.
fn open_and_time(h: &mut H, go: impl FnOnce(&mut MantaApp)) -> Opened {
    let t0 = Instant::now();
    go(app(h));
    let mut first = None;
    let mut max_frame: f64 = 0.0;
    loop {
        max_frame = max_frame.max(frame(h));
        let tab = app(h).pane(Side::Left).tab();
        if !tab.view.is_empty() && first.is_none() {
            first = Some(ms(t0.elapsed()));
        }
        if !tab.view.is_empty() && !tab.loading {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(300), "listing timed out");
        std::thread::sleep(Duration::from_micros(200));
    }
    let complete = ms(t0.elapsed());
    Opened {
        first: first.unwrap_or(complete),
        complete,
        max_frame,
    }
}

fn report_opened(r: &mut Report, o: &Opened) {
    r.ms("first items on screen", o.first);
    r.ms("whole listing on screen", o.complete);
    r.ms("longest frame while loading", o.max_frame);
}

fn time<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let t = Instant::now();
    let r = f();
    (ms(t.elapsed()), r)
}

struct Report(Vec<(String, String)>);

impl Report {
    fn add(&mut self, what: impl Into<String>, v: impl Into<String>) {
        let (what, v) = (what.into(), v.into());
        eprintln!("bench: {what}: {v}");
        self.0.push((what, v));
    }
    fn ms(&mut self, what: &str, v: f64) {
        self.add(what, format!("{v:.1} ms"));
    }
    fn frames(&mut self, what: &str, (med, p): (f64, f64)) {
        self.add(what, format!("{med:.1} ms (p95 {p:.1})"));
    }
}

/// Everything measured on the folder in the left pane.
fn measure_pane(h: &mut H, r: &mut Report, local: bool) {
    let n = app(h).pane(Side::Left).tab().entries.len();
    r.add("items listed", n.to_string());
    r.frames("idle frame", frames(h, 30, |_, _| {}));
    // Scrolling the list (wheel events, 60 frames).
    r.frames(
        "scroll frame (List)",
        frames(h, 60, |h, _| wheel(h, -600.0)),
    );
    let off = app(h).pane(Side::Left).tab().scroll_offset;
    assert!(off > 0.0, "the list scrolled");
    // Sorting and filtering (the work behind a header click / a key).
    for (label, column, ascending) in [
        ("sort by size", SortColumn::Size, false),
        ("sort by date", SortColumn::Modified, false),
        ("sort by name", SortColumn::Name, true),
    ] {
        let tab = app(h).pane_mut(Side::Left).tab_mut();
        tab.sort = SortSpec { column, ascending };
        let (t, _) = time(|| tab.rebuild_view());
        r.ms(label, t);
        frame(h);
    }
    for (label, f) in [
        ("filter \"9\"", "9"),
        ("filter \"99\"", "99"),
        ("filter \"999\"", "999"),
        ("filter cleared", ""),
    ] {
        let tab = app(h).pane_mut(Side::Left).tab_mut();
        tab.filter = f.to_string();
        let (t, _) = time(|| tab.rebuild_view());
        r.ms(label, t);
        frame(h);
    }
    // Type-to-select: three letters, then one with no match.
    {
        let now = Instant::now();
        let tab = app(h).pane_mut(Side::Left).tab_mut();
        let (t, _) = time(|| {
            tab.type_ahead("n", now);
            tab.type_ahead("o", now);
            tab.type_ahead("t", now);
        });
        r.ms("type-ahead (3 letters)", t / 3.0);
        tab.type_ahead.clear();
        let (t, _) = time(|| tab.type_ahead("zz", now));
        r.ms("type-ahead (no match)", t);
        tab.type_ahead.clear();
    }
    frame(h);
    // Select all, and the frames while everything is selected.
    {
        let tab = app(h).pane_mut(Side::Left).tab_mut();
        let (t, _) = time(|| tab.select_all());
        r.ms("select all", t);
    }
    r.frames("frame with all selected", frames(h, 20, |_, _| {}));
    r.frames(
        "scroll frame, all selected",
        frames(h, 20, |h, _| wheel(h, -600.0)),
    );
    {
        let tab = app(h).pane_mut(Side::Left).tab_mut();
        let (t, _) = time(|| tab.select_only(0));
        r.ms("select one again", t);
    }
    frame(h);
    // The other views: first frame, then scrolling.
    let ctx = h.ctx.clone();
    for mode in [ViewMode::Icons, ViewMode::Columns, ViewMode::Gallery] {
        app(h).handle_action(&ctx, Action::SetView(Side::Left, mode));
        let first = frame(h);
        r.add(format!("{mode:?}: first frame"), format!("{first:.1} ms"));
        r.frames(
            &format!("{mode:?}: scroll frame"),
            frames(h, 30, |h, _| wheel(h, -600.0)),
        );
        if mode == ViewMode::Icons && local {
            // Thumbnails of the visible PNGs (made on background threads):
            // scroll to the PNGs first (type-to-select "img").
            {
                let now = Instant::now();
                let tab = app(h).pane_mut(Side::Left).tab_mut();
                tab.type_ahead.clear();
                tab.type_ahead("img", now);
                tab.type_ahead.clear();
            }
            frame(h);
            let t = Instant::now();
            for _ in 0..200 {
                h.step();
                if app(h).thumbs.wait_idle(Duration::from_millis(20)) {
                    h.step();
                    if app(h).thumbs.wait_idle(Duration::from_millis(1)) {
                        break;
                    }
                }
            }
            r.ms("Icons: visible thumbnails done", ms(t.elapsed()));
            r.add("Icons: textures", app(h).thumbs.texture_count().to_string());
            r.frames(
                "Icons: scroll frame with thumbnails",
                frames(h, 30, |h, _| wheel(h, -300.0)),
            );
        }
    }
    app(h).handle_action(&ctx, Action::SetView(Side::Left, ViewMode::List));
    frame(h);
}

fn bench_local(n: usize, r: &mut Report) {
    let fx = local_fixture(n);
    let big = fx.root.join("big");
    let vbig = VfsPath::new(&big.to_string_lossy());
    let fs = manta_vfs::local::LocalFs::new();
    let mut t: Vec<f64> = (0..3)
        .map(|_| time(|| fs.list(&vbig).unwrap().len()).0)
        .collect();
    r.ms("LocalFs::list (median of 3)", median(&mut t));
    // For comparison: readdir + one lstat per file (the old LocalFs::list).
    let mut t: Vec<f64> = (0..3)
        .map(|_| {
            time(|| {
                std::fs::read_dir(&big)
                    .unwrap()
                    .filter_map(|e| std::fs::symlink_metadata(e.ok()?.path()).ok())
                    .count()
            })
            .0
        })
        .collect();
    r.ms(
        "for comparison: readdir + lstat per file, metadata only (median of 3)",
        median(&mut t),
    );
    let mut h = bench_harness(&fx);
    let rss0 = rss_mb();
    let o = open_and_time(&mut h, |a| a.navigate_path(Side::Left, vbig.clone()));
    report_opened(r, &o);
    h.run_steps(3);
    r.add(
        "listing in memory (entries + view)",
        listing_mb(app(&mut h)),
    );
    measure_pane(&mut h, r, true);
    // Reloading the same folder (⌘R / live refresh).
    let ctx = h.ctx.clone();
    let reload = open_and_time(&mut h, move |a| {
        a.handle_action(&ctx, Action::Refresh(Side::Left))
    });
    r.ms("reload (⌘R) until shown", reload.complete);
    r.ms("longest frame during the reload", reload.max_frame);
    r.add(
        "process memory growth (RSS, all of the above)",
        format!("{:.0} MB", rss_mb() - rss0),
    );
}

fn bench_remote(n: usize, r: &mut Report) {
    let mem = MemFs::new();
    mem.mkdir_p("/big");
    let now = chrono::Utc::now();
    for i in 0..n {
        let (name, dir) = bench_name(i);
        if dir {
            mem.mkdir_p(&format!("/big/{name}"));
        } else {
            mem.put(&format!("/big/{name}"), b"x", now);
        }
    }
    let fs: SharedVfs = Arc::new(FakeRemote(mem));
    let mut t: Vec<f64> = (0..3)
        .map(|_| time(|| fs.list(&VfsPath::new("/big")).unwrap().len()).0)
        .collect();
    r.ms("remote list (median of 3)", median(&mut t));
    let fx = local_fixture(0);
    let mut h = bench_harness(&fx);
    let loc = Location {
        fs,
        conn: None,
        path: VfsPath::new("/big"),
        archive: None,
    };
    let o = open_and_time(&mut h, |a| a.navigate(Side::Left, loc, false));
    report_opened(r, &o);
    h.run_steps(3);
    r.add(
        "listing in memory (entries + view)",
        listing_mb(app(&mut h)),
    );
    measure_pane(&mut h, r, false);
}

#[test]
#[ignore = "benchmark: creates 400,000 files, takes minutes (docs/performance.md)"]
fn bench_large_folders() {
    let sizes: Vec<usize> = std::env::var("MANTA_BENCH_SIZES")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![100_000, 300_000]);
    let mut all = Vec::new();
    for n in sizes {
        let mut r = Report(Vec::new());
        bench_local(n, &mut r);
        all.push((format!("local, {n} items"), r));
    }
    let mut r = Report(Vec::new());
    bench_remote(100_000, &mut r);
    all.push(("remote (in-memory SFTP), 100000 items".to_string(), r));
    // One Markdown table per case, for docs/performance.md.
    let mut md = String::new();
    for (title, r) in &all {
        md.push_str(&format!(
            "\n#### {title}\n\n| Measurement | Result |\n|---|---|\n"
        ));
        for (k, v) in &r.0 {
            md.push_str(&format!("| {k} | {v} |\n"));
        }
    }
    eprintln!("{md}");
    if let Some(out) = std::env::var_os("MANTA_BENCH_OUT") {
        std::fs::write(out, md).unwrap();
    }
}
