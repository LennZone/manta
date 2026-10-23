//! `MANTA_DRAG_SELFTEST` — the native drag on a real Mac.
//!
//! * `MANTA_DRAG_SELFTEST=1`: a small window opens, two temp files (plus one
//!   promised "remote" file) are handed to [`super::begin_external`]
//!   programmatically, and the AppKit callbacks are logged (session started,
//!   operation mask requested, session ended). Quits after ~4 s; exit code 0
//!   when the session started and the source callbacks fired.
//!   Nobody holds the mouse button, so AppKit ends the session right away
//!   (operation None): that is expected.
//! * `MANTA_DRAG_SELFTEST=manual`: the same window shows draggable rows that
//!   use the real hook ([`super::drag_out_when_leaving`]). Drag one out of the
//!   window onto the Desktop / Finder / another app; drop files from Finder
//!   onto the window to see the drag-in path. Closes after 60 s.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use super::{
    begin_external, drag_out_when_leaving, external_hover, poll_events, take_external_drop,
    DragEvent, DragItem, RemoteItem,
};

#[derive(Clone)]
struct RowDrag(Vec<PathBuf>);

pub struct SelfTest {
    manual: bool,
    dir: tempfile::TempDir,
    files: Vec<PathBuf>,
    first_frame: Option<Instant>,
    begun: bool,
    started: bool,
    mask: bool,
    ended: Option<Instant>,
    promise_ok: bool,
    dropped_self: bool,
    centred: bool,
    released: bool,
    reported: bool,
    log: Vec<String>,
}

impl SelfTest {
    fn new(manual: bool) -> std::io::Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("manta-drag-selftest")
            .tempdir()?;
        let files = vec![
            dir.path().join("Manta drag test 1.txt"),
            dir.path().join("Manta drag test 2.md"),
        ];
        for (i, f) in files.iter().enumerate() {
            std::fs::write(f, format!("Dragged out of Manta (file {}).\n", i + 1))?;
        }
        Ok(SelfTest {
            manual,
            dir,
            files,
            first_frame: None,
            begun: false,
            started: false,
            mask: false,
            ended: None,
            promise_ok: false,
            dropped_self: false,
            centred: false,
            released: false,
            reported: false,
            log: Vec::new(),
        })
    }

    fn items(&self) -> Vec<DragItem> {
        let mut items: Vec<DragItem> = self.files.iter().cloned().map(DragItem::Local).collect();
        items.push(DragItem::Remote(RemoteItem {
            backend: "self-test (promise)".into(),
            name: "Manta promised file.txt".into(),
            size: Some(32),
            is_dir: false,
            fetch: std::sync::Arc::new(|dest| {
                std::fs::write(dest, b"Written by a Manta file promise.\n")
                    .map_err(|e| e.to_string())
            }),
        }));
        items
    }

    /// Run the self-test window; returns the process exit code.
    pub fn run(spec: &str) -> i32 {
        let manual = spec.eq_ignore_ascii_case("manual");
        let test = match SelfTest::new(manual) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("MANTA_DRAG_SELFTEST: couldn't create temp files: {e}");
                return 2;
            }
        };
        let ok = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(manual));
        let ok2 = ok.clone();
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Manta drag self-test")
                .with_inner_size([460.0, 240.0]),
            ..Default::default()
        };
        let res = eframe::run_native(
            "Manta drag self-test",
            options,
            Box::new(move |_| Ok(Box::new(Runner { test, ok: ok2 }))),
        );
        if let Err(e) = res {
            eprintln!("MANTA_DRAG_SELFTEST: {e}");
            return 2;
        }
        if ok.load(std::sync::atomic::Ordering::SeqCst) {
            0
        } else {
            1
        }
    }
}

struct Runner {
    test: SelfTest,
    ok: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl eframe::App for Runner {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t = &mut self.test;
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(30));
        let first = *t.first_frame.get_or_insert_with(Instant::now);

        for ev in poll_events() {
            match &ev {
                DragEvent::Started { .. } => t.started = true,
                DragEvent::MaskRequested { .. } => t.mask = true,
                DragEvent::Ended { .. } => t.ended = Some(Instant::now()),
                DragEvent::PromiseDone { result: Ok(()), .. } => t.promise_ok = true,
                _ => {}
            }
            log::info!("drag self-test: {ev:?}");
            t.log.push(format!("{ev:?}"));
        }

        ui.heading(if t.manual {
            "Drag a row out of this window"
        } else {
            "Native drag self-test"
        });
        for f in t.files.clone() {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ui.dnd_drag_source(egui::Id::unique(&f), RowDrag(vec![f.clone()]), |ui| {
                ui.label(format!("📄 {name}"));
            });
        }
        if let Some(p) = egui::DragAndDrop::payload::<RowDrag>(&ctx) {
            let paths = p.0.clone();
            drag_out_when_leaving(&ctx, || paths.into_iter().map(DragItem::Local).collect());
        }
        let (hov, dro) = ctx.input(|i| (i.raw.hovered_files.len(), i.raw.dropped_files.len()));
        if hov + dro > 0 {
            log::debug!("drag self-test: raw hovered={hov} dropped={dro}");
        }
        if let Some(h) = external_hover(&ctx) {
            ui.label(format!(
                "Hovering: {} file(s) at {:?}",
                h.paths.len(),
                h.pos
            ));
        }
        if let Some(d) = take_external_drop(&ctx) {
            log::info!(
                "drag self-test: dropped in {:?} at {:?} (from_self={})",
                d.paths,
                d.pos,
                d.from_self
            );
            t.log.push(format!("dropped {} file(s)", d.paths.len()));
            t.dropped_self |= d.from_self;
        }
        for l in t.log.iter().rev().take(5) {
            ui.small(l);
        }

        if t.manual {
            if first.elapsed() > Duration::from_secs(60) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        // Put the window under the mouse first, so the synthetic drop below
        // lands on Manta itself and never on another app.
        if !t.centred && first.elapsed() > Duration::from_millis(300) {
            t.centred = true;
            super::mac::self_test("center");
        }
        if t.begun && !t.released && t.started && first.elapsed() > Duration::from_millis(1500) {
            t.released = true;
            super::mac::self_test("drop");
        }
        if !t.begun && first.elapsed() > Duration::from_millis(900) {
            t.begun = true;
            match super::mac::self_test_promises(t.dir.path()) {
                Ok(s) => {
                    t.promise_ok = true;
                    log::info!("drag self-test: promise delegate: {s}");
                }
                Err(e) => log::warn!("drag self-test: promise delegate FAILED: {e}"),
            }
            let ok = begin_external(&ctx, t.items());
            log::info!(
                "drag self-test: begin_external({} items in {}) = {ok}",
                t.files.len() + 1,
                t.dir.path().display()
            );
        }
        let done = t
            .ended
            .is_some_and(|e| e.elapsed() > Duration::from_millis(700));
        if !t.reported && (done || first.elapsed() > Duration::from_secs(5)) {
            t.reported = true;
            let pass = t.started && t.mask && t.ended.is_some() && t.promise_ok;
            log::info!(
                "drag self-test: RESULT {} (session started={}, operation mask requested={}, ended={}, dropped back into Manta={}, promise written={})",
                if pass { "PASS" } else { "FAIL" },
                t.started,
                t.mask,
                t.ended.is_some(),
                t.dropped_self,
                t.promise_ok
            );
            self.ok.store(pass, std::sync::atomic::Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
