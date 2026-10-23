//! Renders of the transfers panel's resume states (paused, waiting for the
//! network, resuming, needs a connection, offline banner) and of the launch
//! prompt, light and dark, English and German. PNGs go to
//! `$MANTA_RENDER_DIR/transfers-resume` (default `target/ux-renders`).
//! Skipped without a wgpu adapter.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eframe::egui::{self, Theme};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use manta_core::{Auth, ConnectionConfig, Protocol};
use manta_ops::{Endpoint, JobKind, JobState, JobStatus, ResumeInfo, TransferManager};

use super::{PromptRow, ResumePrompt, TransfersView};
use crate::i18n::Locale;

fn wgpu_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let ok = std::panic::catch_unwind(|| {
            egui_kittest::wgpu::create_render_state(
                egui_kittest::wgpu::default_wgpu_setup(),
                egui_wgpu::RendererOptions::PREDICTABLE,
            )
        })
        .is_ok();
        if !ok {
            eprintln!("transfers ux: no wgpu adapter available, skipping renders");
        }
        ok
    })
}

fn out_dir() -> PathBuf {
    let base = std::env::var_os("MANTA_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ux-renders"));
    let dir = base.join("transfers-resume");
    std::fs::create_dir_all(&dir).expect("render dir");
    dir
}

fn status(
    id: u64,
    kind: JobKind,
    title: &str,
    state: JobState,
    done: u64,
    total: u64,
    files: (u64, u64),
) -> JobStatus {
    JobStatus {
        id,
        kind,
        title: title.into(),
        state,
        current_file: Some("/pub/releases/Manta-0.1.3.zip".into()),
        bytes_done: done,
        bytes_total: total,
        files_done: files.0,
        files_total: files.1,
        speed: if done > 0 { 8_400_000.0 } else { 0.0 },
        errors: Vec::new(),
        resume: ResumeInfo {
            can_pause: true,
            ..Default::default()
        },
    }
}

fn server() -> Endpoint {
    Endpoint::remote(&ConnectionConfig {
        name: "Media".into(),
        protocol: Protocol::Sftp,
        host: "media.example.com".into(),
        port: 22,
        username: "anna".into(),
        auth: Auth::Agent,
        initial_path: None,
        s3: None,
        container: None,
        ftp: None,
    })
}

fn jobs() -> Vec<JobStatus> {
    let mut resumed = status(
        1,
        JobKind::Copy,
        "Copying \"Manta-0.1.3.zip\" to /Users/anna/Downloads",
        JobState::Running,
        1_480_000_000,
        3_000_000_000,
        (0, 1),
    );
    resumed.resume.resumed_from = Some(1_200_000_000);
    let waiting = status(
        2,
        JobKind::Copy,
        "Copying 3 items to deploy@web-01.example.com:/var/www",
        JobState::WaitingForNetwork {
            attempt: 3,
            next_retry: Some(Instant::now() + Duration::from_millis(11_600)),
            error: "connection error: web-01.example.com: Connection reset by peer".into(),
        },
        31_200_000,
        52_000_000,
        (14, 22),
    );
    let paused = status(
        3,
        JobKind::Move,
        "Moving 12 items to /Volumes/Backup/photos",
        JobState::Paused,
        412_000_000,
        980_000_000,
        (5, 12),
    );
    let mut needs = status(
        4,
        JobKind::Copy,
        "Copying \"raw-footage\" to /Users/anna/Movies",
        JobState::NeedsConnection(server().label()),
        6_100_000_000,
        14_800_000_000,
        (31, 77),
    );
    needs.resume.needs = vec![server()];
    needs.resume.restored = true;
    let queued = status(
        5,
        JobKind::Copy,
        "Copying \"backup-2026-10-01.zip\" to /Volumes/Backup",
        JobState::Queued,
        0,
        48_300_000,
        (0, 1),
    );
    let mut done = status(
        6,
        JobKind::Copy,
        "Copying \"invoice.pdf\" to /Users/anna/Documents",
        JobState::Done,
        2_400_000,
        2_400_000,
        (1, 1),
    );
    done.resume.log = vec![
        "/Users/anna/invoice.pdf: the source changed since the interruption; copying it again from the start".into(),
    ];
    vec![resumed, waiting, paused, needs, queued, done]
}

fn setup(ctx: &egui::Context) {
    ctx.add_font_provider(std::sync::Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
    crate::theme::install(ctx);
}

#[test]
fn render_transfer_resume_states() {
    if !wgpu_available() {
        return;
    }
    let dir = out_dir();
    for (locale, lname) in [(Locale::En, "en"), (Locale::De, "de")] {
        for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            for offline in [false, true] {
                let manager = TransferManager::new(1);
                crate::i18n::set_locale(locale);
                let list = jobs();
                let mut view = TransfersView {
                    force_offline: offline,
                    ..Default::default()
                };
                let mut h = Harness::builder()
                    .with_size(egui::vec2(980.0, if offline { 400.0 } else { 372.0 }))
                    .with_pixels_per_point(2.0)
                    .with_theme(theme)
                    .with_context_setup(setup)
                    .wgpu()
                    .build_ui(move |ui| {
                        let p = crate::theme::pal(ui);
                        ui.painter().rect_filled(ui.max_rect(), 0.0, p.content);
                        view.ui(ui, &manager, &list);
                    });
                h.run_steps(3);
                let img = h.render().expect("render");
                let name = if offline { "offline" } else { "states" };
                let path = dir.join(format!("transfers_{name}_{lname}_{tname}.png"));
                img.save(&path).expect("save png");
                eprintln!("transfers ux: wrote {}", path.display());
            }
        }
    }
    crate::i18n::set_locale(Locale::En);
}

/// Three interrupted jobs, one needing a server.
fn resume_prompt() -> ResumePrompt {
    ResumePrompt {
        rows: vec![
            PromptRow {
                kind: JobKind::Copy,
                title: "Copying \"Manta-0.1.3.zip\" to /Users/anna/Downloads".into(),
                bytes_done: 1_200_000_000,
                bytes_total: 3_000_000_000,
                files_total: 1,
                needs: None,
            },
            PromptRow {
                kind: JobKind::Move,
                title: "Moving 12 items to /Volumes/Backup/photos".into(),
                bytes_done: 412_000_000,
                bytes_total: 980_000_000,
                files_total: 12,
                needs: None,
            },
            PromptRow {
                kind: JobKind::Copy,
                title: "Copying \"raw-footage\" to /Users/anna/Movies".into(),
                bytes_done: 6_100_000_000,
                bytes_total: 14_800_000_000,
                files_total: 77,
                needs: Some(server().label()),
            },
        ],
    }
}

#[test]
fn render_resume_prompt() {
    if !wgpu_available() {
        return;
    }
    let dir = out_dir();
    for (locale, lname) in [(Locale::En, "en"), (Locale::De, "de")] {
        for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
            crate::i18n::set_locale(locale);
            let mut prompt = resume_prompt();
            let mut h = Harness::builder()
                .with_size(egui::vec2(760.0, 520.0))
                .with_pixels_per_point(2.0)
                .with_theme(theme)
                .with_context_setup(setup)
                .wgpu()
                .build_ui(move |ui| {
                    let p = crate::theme::pal(ui);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, p.content);
                    let ctx = ui.ctx().clone();
                    let _ = prompt.ui(&ctx);
                });
            h.run_steps(4);
            let img = h.render().expect("render");
            let path = dir.join(format!("resume_prompt_{lname}_{tname}.png"));
            img.save(&path).expect("save png");
            eprintln!("transfers ux: wrote {}", path.display());
        }
    }
    crate::i18n::set_locale(Locale::En);
}

/// Behaviour (no GPU): the Reconnect chip hands the endpoint to the app.
#[test]
fn reconnect_chip_queues_a_request() {
    let manager = TransferManager::new(1);
    let list = jobs();
    let view = std::rc::Rc::new(std::cell::RefCell::new(TransfersView::default()));
    let v = view.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(980.0, 372.0))
        .with_context_setup(setup)
        .build_ui(move |ui| {
            v.borrow_mut().ui(ui, &manager, &list);
        });
    h.run_steps(2);
    let label = crate::i18n::tr("transfers.reconnect");
    h.get_by_label(label).click();
    h.run_steps(2);
    let requests = view.borrow_mut().take_reconnect_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].same_connection(&server()));
}

/// The manual's screenshots of these states in every language (see
/// `manual_shots`): the offline Transfers panel and the resume prompt.
#[test]
fn render_manual_images_transfers() {
    if !crate::manual_shots::enabled() || !wgpu_available() {
        return;
    }
    for (locale, code) in crate::manual_shots::langs() {
        crate::i18n::set_locale(locale);
        let manager = TransferManager::new(1);
        let list = jobs();
        let mut view = TransfersView {
            force_offline: true,
            ..Default::default()
        };
        let mut h = Harness::builder()
            .with_size(egui::vec2(980.0, 412.0))
            .with_pixels_per_point(2.0)
            .with_theme(Theme::Light)
            .with_context_setup(setup)
            .wgpu()
            .build_ui(move |ui| {
                let p = crate::theme::pal(ui);
                ui.painter().rect_filled(ui.max_rect(), 0.0, p.content);
                view.ui(ui, &manager, &list);
            });
        h.run_steps(3);
        let img = h.render().expect("render");
        crate::manual_shots::save(&img, 2.0, None, code, "transfers-paused-waiting");

        let mut prompt = resume_prompt();
        let mut h = Harness::builder()
            .with_size(egui::vec2(760.0, 520.0))
            .with_pixels_per_point(2.0)
            .with_theme(Theme::Light)
            .with_context_setup(setup)
            .wgpu()
            .build_ui(move |ui| {
                let p = crate::theme::pal(ui);
                ui.painter().rect_filled(ui.max_rect(), 0.0, p.content);
                let ctx = ui.ctx().clone();
                let _ = prompt.ui(&ctx);
            });
        h.run_steps(4);
        let img = h.render().expect("render");
        crate::manual_shots::save(&img, 2.0, None, code, "resume-prompt");
    }
    crate::i18n::set_locale(Locale::En);
}
