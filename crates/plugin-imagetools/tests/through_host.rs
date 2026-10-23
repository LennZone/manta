//! The Image Tools plugin through the real plugin host: "lo"/"XL" badges
//! and tooltips for the shown folder, the context commands while images
//! are selected, a real resize with `sips` (output naming: never
//! overwrites), lossless metadata stripping in place, and data URIs. The
//! sips parts are skipped where sips is missing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use manta_core::VfsPath;
use manta_plugin::host::{HostEvent, HostPaths, PluginHost};
use manta_plugin::{PaneId, PaneState, Plugin, ToastLevel};
use manta_plugin_imagetools::engine::{lock, SharedRef};
use manta_plugin_imagetools::headers::{self, fixtures as B};
use manta_plugin_imagetools::{pane_state, Click, ImageToolsPlugin, SheetAction, ID, SHEET};

fn until(
    host: &mut PluginHost,
    what: &str,
    mut done: impl FnMut(&mut PluginHost, &[HostEvent]) -> bool,
) -> Vec<HostEvent> {
    let start = Instant::now();
    let mut events = Vec::new();
    loop {
        events.extend(host.poll());
        if done(host, &events) {
            return events;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}; events {events:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn sips() -> bool {
    std::process::Command::new("sips")
        .arg("--help")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn click(
    host: &mut PluginHost,
    q: &Arc<Mutex<Vec<Click>>>,
    pane: &PaneState,
    c: Click,
    panel: &str,
) {
    q.lock().unwrap().push(c);
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(Default::default(), |ui| host.panel_ui(ID, panel, pane, ui));
    out.textures_delta.clear();
}

struct Setup {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    host: PluginHost,
    shared: SharedRef,
    clicks: Arc<Mutex<Vec<Click>>>,
}

fn setup() -> Setup {
    manta_plugin::i18n::set_thread_lang(Some(manta_plugin::i18n::Lang::En));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let dir = root.join("img");
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbImage::from_fn(2400, 1600, |x, y| image::Rgb([x as u8, y as u8, 90]))
        .save(dir.join("photo.png"))
        .unwrap();
    let mut seed = 7u32;
    image::RgbImage::from_fn(1400, 700, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        image::Rgb([(seed >> 8) as u8, (seed >> 16) as u8, seed as u8])
    })
    .save(dir.join("banner.png"))
    .unwrap();
    image::RgbImage::from_pixel(300, 200, image::Rgb([40, 120, 200]))
        .save(dir.join("small.png"))
        .unwrap();
    std::fs::write(dir.join("favicon-32.png"), B::png(32, 32, false, &[], 0)).unwrap();
    std::fs::write(dir.join("notes.txt"), "hi").unwrap();
    let exif = B::exif_photo(Some(B::PARIS), 6);
    std::fs::write(
        dir.join("gps.jpg"),
        B::jpeg(
            400,
            300,
            false,
            None,
            Some(&exif),
            Some("Display P3"),
            false,
        ),
    )
    .unwrap();
    let paths = HostPaths::under(&root.join("config"));
    let plugin = ImageToolsPlugin::new();
    let (shared, clicks) = (plugin.shared(), plugin.click_queue());
    let builtins: Vec<Box<dyn Plugin>> = vec![Box::new(plugin)];
    let host = PluginHost::new(egui::Context::default(), builtins, Some(paths));
    Setup {
        _tmp: tmp,
        dir,
        host,
        shared,
        clicks,
    }
}

fn select(s: &mut Setup, names: &[&str]) -> PaneState {
    let sel: Vec<PathBuf> = names.iter().map(|n| s.dir.join(n)).collect();
    let st = pane_state(PaneId::Left, Some(&s.dir), &sel);
    s.host.set_pane(st.clone(), 1);
    st
}

#[test]
fn badges_tooltips_and_commands() {
    let mut s = setup();
    select(&mut s, &[]);
    until(&mut s.host, "decorations", |h, _| {
        h.decorations(PaneId::Left).len() == 5
    });
    let deco = s.host.decorations(PaneId::Left).clone();
    let badge = |n: &str| {
        deco.get(n)
            .and_then(|d| d.badges.first().map(|b| b.text.clone()))
    };
    assert_eq!(badge("banner.png").as_deref(), Some("XL"));
    assert_eq!(badge("small.png").as_deref(), Some("lo"));
    assert_eq!(badge("photo.png"), None);
    assert_eq!(badge("favicon-32.png"), None, "small on purpose");
    assert_eq!(badge("gps.jpg").as_deref(), Some("lo"));
    assert!(!deco.contains_key("notes.txt"));
    let tip = deco["photo.png"].tooltip.clone().unwrap_or_default();
    assert!(tip.starts_with("2400×1600"), "{tip}");
    assert!(
        deco["gps.jpg"]
            .tooltip
            .clone()
            .unwrap_or_default()
            .starts_with("300×400 · Display P3"),
        "EXIF rotation shown"
    );

    let menu = |h: &PluginHost| {
        h.context_menu_commands()
            .map(|c| c.spec.id.clone())
            .collect::<Vec<_>>()
    };
    assert!(menu(&s.host).is_empty());
    select(&mut s, &["small.png"]);
    until(&mut s.host, "image commands", |h, _| {
        menu(h).contains(&"datauri".to_string())
    });
    assert_eq!(
        menu(&s.host),
        [
            "resize",
            "convert",
            "rotate-left",
            "rotate-right",
            "flip",
            "strip",
            "optimize",
            "retina",
            "datauri"
        ]
    );
    select(&mut s, &["banner.png"]);
    until(&mut s.host, "no data URI over 100 KB", |h, _| {
        !menu(h).contains(&"datauri".to_string()) && !menu(h).is_empty()
    });
    select(&mut s, &["notes.txt"]);
    until(&mut s.host, "no commands for text", |h, _| {
        menu(h).is_empty()
    });
    s.host.shutdown(Duration::from_secs(2));
}

#[test]
fn resize_with_sips_never_overwrites() {
    if !sips() {
        eprintln!("sips not found, skipping");
        return;
    }
    let mut s = setup();
    let pane = select(&mut s, &["photo.png"]);
    std::fs::write(s.dir.join("photo-resized.png"), b"taken").unwrap();
    s.host.run_command(&format!("{ID}:resize"), &pane);
    until(&mut s.host, "the sheet", |h, _| {
        h.is_panel_open(ID, SHEET, None)
    });
    assert!(lock(&s.shared).modal.is_some());
    click(
        &mut s.host,
        &s.clicks,
        &pane,
        Click::Sheet(SheetAction::Run),
        SHEET,
    );
    let ev = until(&mut s.host, "the result", |_, ev| {
        ev.iter().any(|e| {
            matches!(
                e,
                HostEvent::Toast {
                    level: ToastLevel::Success,
                    ..
                }
            )
        })
    });
    let text = ev.iter().find_map(|e| match e {
        HostEvent::Toast {
            level: ToastLevel::Success,
            text,
        } => Some(text.clone()),
        _ => None,
    });
    assert!(
        text.as_deref().unwrap().starts_with("Resized 1 image · "),
        "{text:?}"
    );
    assert!(ev
        .iter()
        .any(|e| matches!(e, HostEvent::Refresh { pane: None })));
    assert_eq!(
        std::fs::read(s.dir.join("photo-resized.png")).unwrap(),
        b"taken",
        "never overwritten"
    );
    let out = headers::probe(&s.dir.join("photo-resized-2.png"), false).unwrap();
    assert_eq!((out.width, out.height), (1920, 1280));
    let opts = std::fs::read_to_string(s.host_data().join("options.json")).unwrap();
    assert!(opts.contains("\"resize\""), "options remembered: {opts}");
    s.host.shutdown(Duration::from_secs(2));
}

impl Setup {
    fn host_data(&self) -> PathBuf {
        lock(&self.shared).data_dir.clone()
    }
}

#[test]
fn strip_in_place_asks_first_and_keeps_orientation() {
    let mut s = setup();
    let pane = select(&mut s, &["gps.jpg"]);
    let before = std::fs::read(s.dir.join("gps.jpg")).unwrap();
    s.host.run_command(&format!("{ID}:strip"), &pane);
    until(&mut s.host, "the sheet", |h, _| {
        h.is_panel_open(ID, SHEET, None)
    });
    assert_eq!(
        lock(&s.shared).modal.as_ref().unwrap().opts.output,
        "suffix",
        "never starts in overwrite mode"
    );
    lock(&s.shared).modal.as_mut().unwrap().opts.output = "overwrite".into();
    click(
        &mut s.host,
        &s.clicks,
        &pane,
        Click::Sheet(SheetAction::Run),
        SHEET,
    );
    assert!(
        lock(&s.shared).modal.as_ref().unwrap().confirm,
        "asks first"
    );
    assert_eq!(
        std::fs::read(s.dir.join("gps.jpg")).unwrap(),
        before,
        "nothing happened yet"
    );
    click(
        &mut s.host,
        &s.clicks,
        &pane,
        Click::Sheet(SheetAction::Run),
        SHEET,
    );
    until(&mut s.host, "the result", |_, ev| {
        ev.iter().any(
            |e| matches!(e, HostEvent::Toast { text, .. } if text.starts_with("Stripped 1 image")),
        )
    });
    let i = headers::probe(&s.dir.join("gps.jpg"), false).unwrap();
    assert_eq!(i.exif.gps, None);
    assert_eq!(i.orientation, Some(6));
    assert_eq!(i.profile.as_deref(), Some("Display P3"));
    let names: Vec<String> = std::fs::read_dir(&s.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !names.iter().any(|n| n.starts_with('.')),
        "no temp files: {names:?}"
    );
    s.host.shutdown(Duration::from_secs(2));
}

#[test]
fn remote_and_empty_selections_get_a_toast() {
    let mut s = setup();
    let mut remote = pane_state(PaneId::Left, None, &[]);
    remote.path = VfsPath::new("/var/www");
    s.host.run_command(&format!("{ID}:resize"), &remote);
    until(&mut s.host, "toast", |_, ev| {
        ev.iter().any(
            |e| matches!(e, HostEvent::Toast { text, .. } if text.contains("local folders only")),
        )
    });
    let pane = select(&mut s, &[]);
    s.host.run_command(&format!("{ID}:resize"), &pane);
    until(&mut s.host, "toast", |_, ev| {
        ev.iter().any(|e| matches!(e, HostEvent::Toast { text, .. } if text.contains("Select one or more images")))
    });
    s.host.shutdown(Duration::from_secs(2));
}
