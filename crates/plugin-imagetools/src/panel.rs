//! The "Image" side panel and the "Image Tools" sheet, drawn natively with
//! the [`crate::kit`] building blocks from the engine's models. Clicks come
//! back as actions; option changes in the sheet are applied in place.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use egui::Ui;
use manta_plugin::Tone;

use crate::engine::{ing, lock, op_title, Details, Modal, PanelModel, SharedRef};
use crate::headers::Info;
use crate::i18n::{dec, num, tr};
use crate::kit::{self, icon, ButtonKind, Row};
use crate::meta::{self, human_size};
use crate::ops::{self, Options};

/// What a click in the side panel asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    /// Open the sheet for an operation.
    Op(String),
    DataUri,
    Maps,
    Cancel,
}

/// What a click in the sheet asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetAction {
    Run,
    Close,
}

fn yes_no(v: Option<bool>) -> String {
    match v {
        None => "–".into(),
        Some(true) => tr!("val.yes").into(),
        Some(false) => tr!("val.no").into(),
    }
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn dpi_text(x: f64, y: f64) -> String {
    let g = |v: f64| {
        if v.fract() == 0.0 {
            format!("{}", v as i64)
        } else {
            format!("{v}")
        }
    };
    dec(&if x == y {
        g(x)
    } else {
        format!("{} × {}", g(x), g(y))
    })
}

fn single(ui: &mut Ui, d: &Details, out: &mut Vec<PanelAction>) {
    let name = name_of(&d.path);
    let info = d.info.as_ref();
    if let Some(prev) = &d.preview {
        let caption = info.filter(|i| i.animated).map(|_| tr!("caption.animated"));
        let mtime = if prev == &d.path { d.mtime } else { 0 };
        crate::preview::show(ui, prev, mtime, 220.0, caption);
    }
    kit::heading(ui, &name);
    let mut tags: Vec<(String, Tone, Option<&'static str>)> = Vec::new();
    if let Some(i) = info {
        tags.push((i.format.clone(), Tone::Accent, Some(icon::FILE_IMAGE)));
        let (w, h) = i.display_size();
        let badge = meta::decoration(&name, d.size, Some(i))
            .and_then(|d| d.badge)
            .map(|b| b.text);
        if badge.as_deref() == Some("XL") || (meta::is_web(&name) && d.size > meta::XL_BYTES) {
            tags.push((tr!("tag.large").into(), Tone::Danger, None));
        }
        if w.max(h) < meta::LOW_RES_EDGE {
            tags.push((tr!("tag.low").into(), Tone::Warning, None));
        }
        if i.animated {
            let t = match i.frames {
                Some(f) => tr!("tag.animated_frames", frames = tr!("frames", count = f)),
                None => tr!("tag.animated").into(),
            };
            tags.push((t, Tone::Info, None));
        }
        if i.alpha == Some(true) {
            tags.push((tr!("tag.alpha").into(), Tone::Muted, None));
        }
        if i.progressive {
            let t = if i.format == "JPEG" {
                tr!("tag.progressive")
            } else {
                tr!("tag.interlaced")
            };
            tags.push((t.into(), Tone::Muted, None));
        }
    }
    if d.exif.gps.is_some() {
        tags.push((tr!("tag.gps").into(), Tone::Warning, Some(icon::GLOBE)));
    }
    if !tags.is_empty() {
        kit::tags(ui, &tags);
    }
    let mut rows: Vec<(String, String, Tone)> = Vec::new();
    if let Some(i) = info {
        let (w, h) = i.display_size();
        let mut dims = format!("{w} × {h} px");
        if (w, h) != (i.width, i.height) {
            dims = tr!("val.rotated", dims = dims);
        }
        rows.push((tr!("key.dimensions").into(), dims, Tone::Neutral));
        rows.push((
            tr!("key.aspect").into(),
            meta::aspect_ratio(w, h),
            Tone::Neutral,
        ));
    }
    rows.push((
        tr!("key.file_size").into(),
        tr!("val.bytes", size = human_size(d.size), bytes = num(d.size)),
        Tone::Neutral,
    ));
    if let Some(i) = info {
        let mut fmt = i.format.clone();
        if let Some(b) = i.bit_depth.filter(|_| i.format != "BMP") {
            fmt = format!("{fmt} · {b}-bit {}", i.color.clone().unwrap_or_default());
        }
        rows.push((
            tr!("key.format").into(),
            fmt.trim().to_string(),
            Tone::Neutral,
        ));
        match &i.profile {
            Some(p) => rows.push((tr!("key.profile").into(), p.clone(), Tone::Neutral)),
            None => rows.push((
                tr!("key.profile").into(),
                tr!("val.no_profile").into(),
                Tone::Muted,
            )),
        }
        if let Some((x, y)) = i.dpi {
            rows.push((tr!("key.dpi").into(), dpi_text(x, y), Tone::Neutral));
        }
        rows.push((tr!("key.alpha").into(), yes_no(i.alpha), Tone::Neutral));
        if matches!(i.format.as_str(), "GIF" | "WebP" | "PNG") {
            let anim = if !i.animated {
                tr!("val.no").to_string()
            } else if let Some(f) = i.frames {
                tr!("val.yes_frames", frames = tr!("frames", count = f))
            } else {
                tr!("val.yes").to_string()
            };
            rows.push((tr!("key.animated").into(), anim, Tone::Neutral));
        }
    } else {
        let fmt = meta::format_name(&meta::ext_of(&name)).unwrap_or("?");
        rows.push((
            tr!("key.format").into(),
            tr!("val.unreadable", format = fmt),
            Tone::Warning,
        ));
    }
    kit::key_values(ui, &rows);
    let cam = meta::exif_rows(&d.exif);
    if !cam.is_empty() {
        kit::section(ui, tr!("section.camera"), Some("EXIF"), false);
        kit::key_values(
            ui,
            &cam.into_iter()
                .map(|(k, v)| (k, v, Tone::Neutral))
                .collect::<Vec<_>>(),
        );
    }
    if let Some((lat, lon)) = d.exif.gps {
        kit::section(ui, tr!("section.location"), Some("GPS"), false);
        kit::key_values(
            ui,
            &[(
                tr!("key.coordinates").into(),
                meta::gps_text(lat, lon),
                Tone::Neutral,
            )],
        );
        kit::callout(ui, icon::WARNING, tr!("gps.warning"), Tone::Warning);
        match kit::buttons(
            ui,
            &[
                (tr!("cmd.strip"), ButtonKind::Primary),
                (tr!("btn.open_maps"), ButtonKind::Secondary),
            ],
        ) {
            Some(0) => out.push(PanelAction::Op("strip".into())),
            Some(1) => out.push(PanelAction::Maps),
            _ => {}
        }
    }
}

fn summary(ui: &mut Ui, images: &[(PathBuf, u64, Option<Info>)]) {
    kit::heading(ui, &tr!("images", count = images.len()));
    let sizes: Vec<u64> = images.iter().map(|x| x.1).collect();
    let mut rows = vec![(
        tr!("key.total_size").to_string(),
        human_size(sizes.iter().sum()),
        Tone::Neutral,
    )];
    if let (Some(min), Some(max)) = (sizes.iter().min(), sizes.iter().max()) {
        rows.push((
            tr!("key.file_sizes").into(),
            format!("{} – {}", human_size(*min), human_size(*max)),
            Tone::Neutral,
        ));
    }
    let dims: Vec<(u32, u32)> = images
        .iter()
        .filter_map(|x| x.2.as_ref().map(Info::display_size))
        .collect();
    if let (Some(s), Some(l)) = (
        dims.iter().min_by_key(|d| d.0 as u64 * d.1 as u64),
        dims.iter().max_by_key(|d| d.0 as u64 * d.1 as u64),
    ) {
        let span = if s == l {
            meta::dims(s.0, s.1)
        } else {
            format!("{} – {}", meta::dims(s.0, s.1), meta::dims(l.0, l.1))
        };
        rows.push((tr!("key.dimensions").into(), span, Tone::Neutral));
    }
    let mut fmts: Vec<(String, usize)> = Vec::new();
    for (p, _, i) in images {
        let f = i.as_ref().map(|i| i.format.clone()).unwrap_or_else(|| {
            meta::format_name(&meta::ext_of(&p.to_string_lossy()))
                .unwrap_or("?")
                .to_string()
        });
        match fmts.iter_mut().find(|x| x.0 == f) {
            Some(x) => x.1 += 1,
            None => fmts.push((f, 1)),
        }
    }
    fmts.sort_by_key(|f| std::cmp::Reverse(f.1));
    rows.push((
        tr!("key.formats").into(),
        fmts.iter()
            .map(|(f, n)| format!("{n} {f}"))
            .collect::<Vec<_>>()
            .join(", "),
        Tone::Neutral,
    ));
    let xl = images
        .iter()
        .filter(|(p, s, _)| meta::is_web(&p.to_string_lossy()) && *s > meta::XL_BYTES)
        .count();
    if xl > 0 {
        rows.push((
            tr!("key.over_2mb").into(),
            tr!("images", count = xl),
            Tone::Danger,
        ));
    }
    kit::key_values(ui, &rows);
    let mut biggest: Vec<&(PathBuf, u64, Option<Info>)> = images.iter().collect();
    biggest.sort_by_key(|b| std::cmp::Reverse(b.1));
    kit::section(ui, tr!("section.largest"), None, false);
    let rows: Vec<Row> = biggest
        .iter()
        .take(5)
        .map(|(p, size, info)| {
            let mut detail = human_size(*size);
            if let Some(i) = info {
                let (w, h) = i.display_size();
                detail = format!("{} · {detail}", meta::dims(w, h));
            }
            let tone = if *size > meta::XL_BYTES && meta::is_web(&p.to_string_lossy()) {
                Tone::Danger
            } else {
                Tone::Neutral
            };
            Row::new(Some(icon::FILE_IMAGE), name_of(p))
                .detail(detail)
                .tone(tone)
        })
        .collect();
    kit::list(ui, &rows);
}

/// The "Image" side panel.
pub fn side_panel(ui: &mut Ui, shared: &SharedRef) -> Vec<PanelAction> {
    let mut out = Vec::new();
    let st = lock(shared);
    kit::body(ui, "imagetools-panel", kit::sidebar_margin(), |ui| {
        if let Some(j) = st.job.as_ref().filter(|j| !j.done.load(Ordering::Relaxed)) {
            let i = j.index.load(Ordering::Relaxed);
            let cur = j
                .current
                .lock()
                .map(|c| c.as_deref().map(name_of).unwrap_or_default())
                .unwrap_or_default();
            kit::section(
                ui,
                tr!("section.working"),
                Some(&tr!(
                    "working.detail",
                    done = (i + 1).min(j.total),
                    total = j.total
                )),
                true,
            );
            kit::note(
                ui,
                &tr!("working.line", verb = ing(&j.op), name = cur),
                Tone::Info,
            );
            kit::progress(ui, Some(i as f32 / j.total.max(1) as f32));
            if kit::buttons(ui, &[(tr!("btn.cancel"), ButtonKind::Secondary)]).is_some() {
                out.push(PanelAction::Cancel);
            }
            kit::separator(ui);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        let images = match &st.panel {
            PanelModel::Waiting => {
                kit::note(ui, tr!("state.waiting"), Tone::Muted);
                return;
            }
            PanelModel::Remote => {
                kit::heading(ui, tr!("state.remote"));
                kit::note(ui, tr!("state.remote_note"), Tone::Muted);
                return;
            }
            PanelModel::NoImage => {
                kit::heading(ui, tr!("state.none"));
                kit::note(ui, tr!("state.none_note"), Tone::Muted);
                return;
            }
            PanelModel::CantRead(name) => {
                kit::callout(
                    ui,
                    icon::WARNING,
                    &tr!("state.cant_read", name = name),
                    Tone::Warning,
                );
                return;
            }
            PanelModel::Single(d) => {
                single(ui, d, &mut out);
                1
            }
            PanelModel::Multi(images) => {
                summary(ui, images);
                images.len()
            }
        };
        let detail = (images > 1).then(|| tr!("images", count = images));
        kit::section(ui, tr!("section.actions"), detail.as_deref(), false);
        let mut rows = vec![
            Row::new(Some(icon::WRENCH), tr!("cmd.resize")).clickable(),
            Row::new(Some(icon::REFRESH), tr!("cmd.convert")).clickable(),
            Row::new(Some(icon::LIGHTNING), tr!("cmd.optimize"))
                .detail("≤ 2560 px · JPEG 82")
                .clickable(),
            Row::new(Some(icon::COPY), tr!("cmd.retina")).clickable(),
            Row::new(Some(icon::LOCK), tr!("cmd.strip")).clickable(),
        ];
        if st.datauri {
            rows.push(
                Row::new(Some(icon::CODE), tr!("cmd.datauri"))
                    .detail("pbcopy")
                    .clickable(),
            );
        }
        match kit::list(ui, &rows) {
            Some(0) => out.push(PanelAction::Op("resize".into())),
            Some(1) => out.push(PanelAction::Op("convert".into())),
            Some(2) => out.push(PanelAction::Op("optimize".into())),
            Some(3) => out.push(PanelAction::Op("retina".into())),
            Some(4) => out.push(PanelAction::Op("strip".into())),
            Some(5) => out.push(PanelAction::DataUri),
            _ => {}
        }
    });
    out
}

// ------------------------------------------------------------------ sheet

fn choice(text: impl Into<String>, on: bool, detail: Option<String>) -> Row {
    let mut r = Row::new(
        if on {
            Some(icon::CHECK_CIRCLE)
        } else {
            Some(icon::CIRCLE)
        },
        text,
    )
    .clickable();
    if on {
        r = r.tone(Tone::Info);
    }
    if let Some(d) = detail {
        r = r.detail(d);
    }
    r
}

fn transform_title(id: &str) -> &'static str {
    match id {
        "left" => tr!("cmd.rotate_left"),
        "right" => tr!("cmd.rotate_right"),
        "180" => tr!("cmd.rotate_180"),
        "flip-h" => tr!("cmd.flip_h"),
        _ => tr!("cmd.flip_v"),
    }
}

fn format_detail(id: &str) -> &'static str {
    match id {
        "jpeg" => tr!("fmt.jpeg"),
        "png" => tr!("fmt.png"),
        "heic" => tr!("fmt.heic"),
        "tiff" => tr!("fmt.tiff"),
        _ => tr!("fmt.webp"),
    }
}

fn quality_name(q: u32) -> &'static str {
    match q {
        60 => tr!("quality.60"),
        82 => tr!("quality.82"),
        90 => tr!("quality.90"),
        _ => tr!("quality.100"),
    }
}

fn run_verb(op: &str) -> &'static str {
    match op {
        "resize" => tr!("run.resize"),
        "convert" => tr!("run.convert"),
        "rotate" => tr!("run.rotate"),
        "strip" => tr!("run.strip"),
        _ => tr!("run.optimize"),
    }
}

/// The body of the sheet for `m`; returns the primary button's label.
fn sheet_body(ui: &mut Ui, m: &mut Modal) -> String {
    let first = m.files[0].clone();
    let total: u64 = m.infos.values().map(|x| x.0).sum();
    let title = op_title(&m.op);
    let (heading, sub) = if m.files.len() == 1 {
        let sub = m
            .infos
            .get(&first)
            .map(|(s, i)| meta::tooltip(i.as_ref(), *s))
            .unwrap_or_default();
        (
            tr!("modal.heading_one", op = title, name = name_of(&first)),
            sub,
        )
    } else {
        let names: Vec<String> = m.files.iter().take(3).map(|f| name_of(f)).collect();
        let more = if m.files.len() > 3 { "…" } else { "" };
        (
            tr!(
                "modal.heading_many",
                op = title,
                images = tr!("images", count = m.files.len())
            ),
            format!("{}{more} · {}", names.join(", "), human_size(total)),
        )
    };
    kit::heading(ui, &heading);
    kit::note(ui, &sub, Tone::Muted);
    if m.confirm {
        let what = tr!("originals", count = m.files.len());
        let one = m.files.len() == 1;
        let (question, label) = if m.op == "convert" {
            (
                if one {
                    tr!("confirm.replace_one", what = what)
                } else {
                    tr!("confirm.replace", what = what)
                },
                tr!("confirm.btn_replace", what = what),
            )
        } else {
            (
                if one {
                    tr!("confirm.overwrite_one", what = what)
                } else {
                    tr!("confirm.overwrite", what = what)
                },
                tr!("confirm.btn_overwrite", what = what),
            )
        };
        kit::callout(ui, icon::WARNING, &question, Tone::Warning);
        return label;
    }
    let info1 = m.first_info().cloned();
    let all_previews = m.previews.clone();
    let o = &mut m.opts;
    let mut ext: Option<String> = None;
    match m.op.as_str() {
        "resize" => {
            kit::section(ui, tr!("resize.size"), Some(tr!("resize.keeps")), false);
            let mut rows: Vec<Row> = ops::RESIZE_PRESETS
                .iter()
                .map(|(id, title, kind, _)| {
                    let detail = match (&info1, m.files.len()) {
                        (Some(i), 1) => Some(
                            ops::target_size(
                                i.width,
                                i.height,
                                &Options {
                                    preset: id.to_string(),
                                    ..o.clone()
                                },
                            )
                            .map_or_else(
                                || tr!("resize.smaller").to_string(),
                                |(w, h)| meta::dims(w, h),
                            ),
                        ),
                        _ if *kind == "edge" => Some(tr!("resize.edge").to_string()),
                        _ => None,
                    };
                    choice(*title, o.preset == *id, detail)
                })
                .collect();
            let custom = if o.custom_axis == "width" {
                tr!("resize.width_px", px = o.custom_value)
            } else {
                tr!("resize.height_px", px = o.custom_value)
            };
            rows.push(choice(
                tr!("resize.custom"),
                o.preset == "custom",
                Some(custom),
            ));
            if let Some(i) = kit::list(ui, &rows) {
                o.preset = ops::RESIZE_PRESETS
                    .get(i)
                    .map_or("custom", |p| p.0)
                    .to_string();
            }
            if o.preset == "custom" {
                let width = o.custom_axis == "width";
                let mut kv = vec![(
                    if width {
                        tr!("key.width")
                    } else {
                        tr!("key.height")
                    }
                    .to_string(),
                    format!("{} px", o.custom_value),
                    Tone::Neutral,
                )];
                if let Some(i) = &info1 {
                    if let Some((w, h)) = ops::target_size(i.width, i.height, o) {
                        let (k, v) = if width {
                            (tr!("key.height"), h)
                        } else {
                            (tr!("key.width"), w)
                        };
                        kv.push((k.into(), format!("{v} px"), Tone::Muted));
                    }
                }
                kit::key_values(ui, &kv);
                let axis = if width {
                    tr!("resize.set_height")
                } else {
                    tr!("resize.set_width")
                };
                let clicked = kit::buttons_small(
                    ui,
                    &[
                        ("−100", ButtonKind::Secondary),
                        ("−10", ButtonKind::Secondary),
                        ("+10", ButtonKind::Secondary),
                        ("+100", ButtonKind::Secondary),
                        (axis, ButtonKind::Secondary),
                    ],
                );
                match clicked {
                    Some(i @ 0..=3) => {
                        let d = [-100i64, -10, 10, 100][i];
                        o.custom_value = (o.custom_value as i64 + d).clamp(16, 20_000) as u32;
                    }
                    Some(4) => {
                        if let Some((w, h)) = info1
                            .as_ref()
                            .and_then(|i| ops::target_size(i.width, i.height, o))
                        {
                            o.custom_value = if width { h } else { w };
                        }
                        o.custom_axis = if width { "height" } else { "width" }.into();
                    }
                    _ => {}
                }
            }
        }
        "convert" => {
            kit::section(ui, tr!("convert.format"), None, false);
            let formats: Vec<_> = ops::CONVERT_FORMATS
                .iter()
                .filter(|f| f.0 != "webp" || m.have_cwebp)
                .collect();
            let rows: Vec<Row> = formats
                .iter()
                .map(|f| choice(f.1, o.format == f.0, Some(format_detail(f.0).into())))
                .collect();
            if let Some(i) = kit::list(ui, &rows) {
                o.format = formats[i].0.into();
            }
            if !m.have_cwebp {
                kit::note(ui, tr!("convert.webp_needs"), Tone::Muted);
            }
            if matches!(o.format.as_str(), "jpeg" | "heic" | "webp") {
                kit::section(
                    ui,
                    tr!("convert.quality"),
                    Some(&o.quality.to_string()),
                    false,
                );
                let labels: Vec<String> = ops::QUALITIES
                    .iter()
                    .map(|q| format!("{} {q}", quality_name(*q)))
                    .collect();
                let items: Vec<(&str, ButtonKind)> = labels
                    .iter()
                    .zip(ops::QUALITIES)
                    .map(|(l, q)| {
                        (
                            l.as_str(),
                            if o.quality == q {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Secondary
                            },
                        )
                    })
                    .collect();
                if let Some(i) = kit::buttons(ui, &items) {
                    o.quality = ops::QUALITIES[i];
                }
            }
            ext = Some(ops::convert_ext(o, &first));
        }
        "rotate" => {
            kit::section(ui, tr!("rotate.transform"), None, false);
            let rows: Vec<Row> = ops::TRANSFORMS
                .iter()
                .map(|(id, _)| choice(transform_title(id), o.transform == *id, None))
                .collect();
            if let Some(i) = kit::list(ui, &rows) {
                o.transform = ops::TRANSFORMS[i].0.into();
            }
        }
        "strip" => {
            let how = if m.have_exiftool {
                "exiftool"
            } else {
                tr!("strip.builtin")
            };
            kit::section(ui, tr!("strip.what"), Some(how), false);
            kit::note(ui, tr!("strip.note"), Tone::Neutral);
            if !m.have_exiftool {
                kit::note(ui, tr!("strip.heic_note"), Tone::Muted);
            }
        }
        "optimize" => {
            kit::section(ui, tr!("strip.what"), None, false);
            kit::key_values(
                ui,
                &[
                    (
                        tr!("key.size").into(),
                        tr!("opt.size", px = ops::WEB_EDGE),
                        Tone::Neutral,
                    ),
                    (
                        tr!("key.format").into(),
                        tr!("opt.format", q = ops::WEB_QUALITY),
                        Tone::Neutral,
                    ),
                    (
                        tr!("key.colours").into(),
                        tr!("opt.colours").into(),
                        Tone::Neutral,
                    ),
                ],
            );
            ext = Some(ops::optimize_ext(&first, info1.as_ref()).into());
        }
        _ => {
            kit::section(ui, tr!("retina.files"), Some("srcset / image-set()"), false);
            let mut rows = Vec::new();
            for r in &m.retina {
                match r {
                    Ok((two, one, renamed)) => {
                        let d = if *renamed {
                            tr!("retina.original_renamed")
                        } else {
                            tr!("retina.original")
                        };
                        rows.push(Row::new(Some(icon::FILE_IMAGE), two.clone()).detail(d));
                        rows.push(
                            Row::new(Some(icon::FILE_IMAGE), one.clone())
                                .detail(tr!("retina.half")),
                        );
                    }
                    Err((name, why)) => rows.push(
                        Row::new(Some(icon::WARNING), name.clone())
                            .detail(why.clone())
                            .tone(Tone::Warning),
                    ),
                }
            }
            kit::list(ui, &rows);
            if m.files.len() > 3 {
                kit::note(
                    ui,
                    &tr!(
                        "retina.more",
                        images = tr!("images", count = m.files.len() - 3)
                    ),
                    Tone::Muted,
                );
            }
            return tr!("make_sets", count = m.files.len());
        }
    }
    // Save as.
    kit::section(ui, tr!("save.as"), None, false);
    let own = ops::split_ext(&name_of(&first)).1.to_string();
    let ext = ext.unwrap_or(own);
    let previews = |mode: &str| {
        all_previews
            .get(&(mode.to_string(), ext.clone()))
            .cloned()
            .unwrap_or_default()
    };
    let convert = m.op == "convert";
    let over_label = if convert {
        tr!("save.replace")
    } else {
        tr!("save.overwrite")
    };
    let over_detail = if convert {
        tr!("save.trash")
    } else {
        tr!("save.no_undo")
    };
    let mut over = choice(
        over_label,
        o.output == "overwrite",
        Some(over_detail.into()),
    )
    .tooltip(tr!("save.asks"));
    if o.output != "overwrite" {
        over = over.tone(Tone::Warning);
    }
    let (next_to, sub) = (previews("suffix"), previews("subfolder"));
    let rows = vec![
        choice(tr!("save.next_to"), o.output == "suffix", Some(next_to)),
        choice(tr!("save.subfolder"), o.output == "subfolder", Some(sub)),
        over,
    ];
    if let Some(i) = kit::list(ui, &rows) {
        o.output = ["suffix", "subfolder", "overwrite"][i].into();
    }
    let verb = run_verb(&m.op);
    if m.files.len() > 1 {
        tr!("run.n", verb = verb, n = m.files.len())
    } else {
        verb.into()
    }
}

/// The "Image Tools" sheet.
pub fn sheet(ui: &mut Ui, shared: &SharedRef) -> Vec<SheetAction> {
    let mut out = Vec::new();
    let mut st = lock(shared);
    let Some(m) = st.modal.as_mut() else {
        return out;
    };
    let before = (m.opts.clone(), m.confirm);
    let full = ui.max_rect();
    let footer_h = 24.0 + 2.0 * kit::space::LG;
    let body =
        egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - footer_h));
    let primary = ui
        .scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            kit::body(ui, "imagetools-sheet", kit::sheet_margin(), |ui| {
                sheet_body(ui, m)
            })
        })
        .inner;
    // A changed option leaves the confirmation.
    if m.opts != before.0 {
        m.confirm = false;
    }
    let p = kit::pal(ui);
    let footer = egui::Rect::from_min_max(egui::pos2(full.left(), body.bottom()), full.max);
    ui.painter().hline(
        footer.x_range(),
        footer.top(),
        egui::Stroke::new(1.0, p.separator),
    );
    let confirm = m.confirm;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(footer.shrink2(egui::vec2(kit::space::XL, kit::space::LG)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = kit::space::SM;
            if kit::button(ui, &primary, ButtonKind::Primary, true).clicked() {
                out.push(SheetAction::Run);
            }
            let back = if confirm {
                tr!("btn.back")
            } else {
                tr!("btn.cancel")
            };
            if kit::button(ui, back, ButtonKind::Secondary, true).clicked() {
                if confirm {
                    m.confirm = false;
                } else {
                    out.push(SheetAction::Close);
                }
            }
        },
    );
    out
}
