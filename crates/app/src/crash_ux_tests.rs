//! "Manta quit unexpectedly last time": a crash report from an earlier run
//! shows a notice once, with Report on GitHub / Show Report / Ignore.
//! Reports live in a temporary folder here, never `~/Library/Logs`.
//! `render_crash_notice` writes German light / dark PNGs to
//! `$MANTA_RENDER_DIR/crash`.

use std::path::Path;

use eframe::egui::Theme;
use egui_kittest::kittest::Queryable;
use manta_plugin::host::PluginHost;

use super::{app, fixture, harness_init, render_dir, save_to, wgpu_available, Fixture, H, WIDE};

const TITLE: &str = "Manta quit unexpectedly last time";

const REPORT: &str = "Manta crash report\n==================\n\n\
    Manta: 0.3.6\nmacOS: macOS 15.6.1\nArchitecture: Apple silicon (arm64)\n\
    Time: 2026-10-24 12:05:01 UTC\nKind: panic\nThread: main\n\
    Message: index out of bounds: the len is 3 but the index is 7\n\
    Location: crates/app/src/pane.rs:12:5\n\n\
    Binary: manta\nUUID: 2F9C3A1E-0B5D-3C7E-9A11-5E2F3D4C5B6A\nLoad address: 0x100000000\n\n\
    Backtrace:\n  0  0x0000000100004f20  manta + 0x4f20\n\
      1  0x000000018a2b3c4d  libsystem_kernel.dylib + 0x3c4d  __pthread_kill\n";

fn crash_harness(fx: &Fixture, dir: &Path, gpu: bool) -> H {
    let dir = dir.to_path_buf();
    harness_init(
        fx,
        WIDE,
        Theme::Light,
        gpu,
        Box::new(|ctx| PluginHost::new(ctx.clone(), Vec::new(), None)),
        move |init| init.crash_dir = Some(dir),
    )
}

fn shown(h: &mut H) -> bool {
    h.run_steps(2);
    h.query_by_label(TITLE).is_some()
}

#[test]
fn no_report_no_notice() {
    let fx = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let mut h = crash_harness(&fx, tmp.path(), false);
    assert!(!shown(&mut h));
}

#[test]
fn a_report_shows_the_notice_once_and_reports_on_github() {
    let fx = fixture();
    let tmp = tempfile::tempdir().unwrap();
    let report = crate::crash::write_report(tmp.path(), 1_792_843_501, REPORT).unwrap();
    let mut h = crash_harness(&fx, tmp.path(), false);
    assert!(shown(&mut h), "the notice");

    // Show Report: the file in the text editor (`open -t`), the notice stays.
    h.get_by_label("Show Report").click();
    h.run_steps(2);
    assert_eq!(
        app(&mut h).launched.last().cloned(),
        Some(vec![
            "-t".to_string(),
            report.to_string_lossy().into_owned()
        ])
    );
    assert!(shown(&mut h));

    // Report on GitHub: the prefilled bug form; the notice goes.
    h.get_by_label("Report on GitHub").click();
    h.run_steps(2);
    let url = app(&mut h).opened_links.last().cloned().expect("a link");
    assert!(url.starts_with(
        "https://github.com/web-workers-com/manta/issues/new?template=bug_report.yml&manta-version="
    ));
    assert!(url.contains("&title=Crash%3A%20index%20out%20of%20bounds"));
    assert!(url.contains("manta%20%2B%200x4f20"), "the first frames");
    assert!(
        url.contains("crash-2026-10-24-120501.txt"),
        "names the file"
    );
    assert!(url.len() <= crate::feedback::MAX_URL);
    assert!(!shown(&mut h), "gone");

    // The next launch doesn't ask again.
    let mut h = crash_harness(&fx, tmp.path(), false);
    assert!(!shown(&mut h), "once");

    // A newer crash does.
    crate::crash::write_report(tmp.path(), 1_792_900_000, REPORT).unwrap();
    let mut h = crash_harness(&fx, tmp.path(), false);
    assert!(shown(&mut h));
    h.get_by_label("Ignore").click();
    assert!(!shown(&mut h), "Ignore hides it");
    assert!(app(&mut h).opened_links.is_empty(), "nothing opened");
    let mut h = crash_harness(&fx, tmp.path(), false);
    assert!(!shown(&mut h), "and it stays away");
}

#[test]
fn render_crash_notice() {
    if !wgpu_available() {
        return;
    }
    let fx = fixture();
    let out = render_dir("crash");
    for (theme, tname) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let tmp = tempfile::tempdir().unwrap();
        crate::crash::write_report(tmp.path(), 1_792_843_501, REPORT).unwrap();
        let dir = tmp.path().to_path_buf();
        let mut h = harness_init(
            &fx,
            WIDE,
            theme,
            true,
            Box::new(|ctx| PluginHost::new(ctx.clone(), Vec::new(), None)),
            move |init| init.crash_dir = Some(dir),
        );
        crate::i18n::set_locale(crate::i18n::Locale::De);
        h.run_steps(3);
        save_to(&mut h, &out, &format!("crash_notice_{tname}"));
    }
    crate::i18n::set_locale(crate::i18n::Locale::En);
}
