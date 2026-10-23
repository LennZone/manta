//! The Duplicates side panel, the "Move to Trash" sheet and the settings
//! area, drawn natively with the [`crate::kit`] building blocks. Drawing
//! only reads the shared state; clicks come back as [`Action`]s.

use egui::Ui;
use manta_plugin::{PaneState, Tone};

use crate::i18n::tr;
use crate::kit::{self, icon, ButtonKind, Row};
use crate::scan::{self, folder_name, human_size, thousands, Phase};
use crate::{lock, Action, Message, SharedRef, Target, MAX_GROUPS_SHOWN};

fn duration(secs: f64) -> String {
    if secs < 0.1 {
        manta_plugin::i18n::localize_decimal("<0.1 s")
    } else if secs < 60.0 {
        manta_plugin::i18n::localize_decimal(&format!("{secs:.1} s"))
    } else {
        format!("{} min {} s", (secs / 60.0) as u64, (secs % 60.0) as u64)
    }
}

fn message_text(m: &Message) -> (String, Tone) {
    match m {
        Message::Cancelled => (tr!("msg.cancelled").into(), Tone::Muted),
        Message::Failed(e) => (tr!("msg.failed", error = e), Tone::Danger),
    }
}

/// The Options section: the keep rule, minimum size, hidden files and what
/// is skipped, with buttons that cycle them.
fn options(ui: &mut Ui, st: &crate::Shared, out: &mut Vec<Action>) {
    let s = &st.settings;
    let mut skipped: Vec<String> = s.exclude.clone();
    if s.skip_packages {
        skipped.push(tr!("opt.packages").into());
    }
    let skipped = if skipped.is_empty() {
        tr!("opt.nothing").to_string()
    } else {
        skipped.join(", ")
    };
    kit::section(ui, tr!("opt.section"), None, false);
    kit::key_values(
        ui,
        &[
            (
                tr!("opt.keep").into(),
                scan::rule_label(&s.keep_rule, &s.keep_folder),
                Tone::Neutral,
            ),
            (
                tr!("opt.min_size").into(),
                human_size(s.min_size.max(1)),
                Tone::Neutral,
            ),
            (
                tr!("opt.hidden").into(),
                if s.include_hidden {
                    tr!("opt.included")
                } else {
                    tr!("opt.skipped")
                }
                .into(),
                Tone::Neutral,
            ),
            (tr!("opt.skipped_key").into(), skipped, Tone::Muted),
        ],
    );
    let clicked = kit::buttons(
        ui,
        &[
            (tr!("btn.keep_rule"), ButtonKind::Secondary),
            (tr!("btn.min_size"), ButtonKind::Secondary),
            (tr!("btn.hidden"), ButtonKind::Secondary),
            (tr!("btn.settings"), ButtonKind::Secondary),
        ],
    );
    match clicked {
        Some(0) => out.push(Action::CycleRule),
        Some(1) => out.push(Action::CycleMinSize),
        Some(2) => out.push(Action::ToggleHidden),
        Some(3) => out.push(Action::EditSettings),
        _ => {}
    }
    for e in st.settings_errors.iter().take(3) {
        kit::callout(ui, icon::WARNING, e, Tone::Warning);
    }
}

/// The side panel. `active` is the active pane, `now` the current time
/// (seconds since the epoch, for "this year" dates).
pub fn panel_ui(ui: &mut Ui, shared: &SharedRef, active: &PaneState, now: i64) -> Vec<Action> {
    let mut out = Vec::new();
    let st = lock(shared);
    kit::body(ui, "duplicates-panel", kit::sidebar_margin(), |ui| {
        if let Some(sc) = &st.scan {
            let root = sc.root.to_string_lossy();
            let secs = sc.started.elapsed().as_secs();
            kit::section(
                ui,
                &tr!("scan.title", name = folder_name(&root)),
                Some(&format!("{secs} s")),
                true,
            );
            let (done, total) = (
                sc.progress.done.load(std::sync::atomic::Ordering::Relaxed),
                sc.progress.total.load(std::sync::atomic::Ordering::Relaxed),
            );
            let (line, frac) = match sc.progress.phase() {
                Phase::Listing => (tr!("scan.listing", n = thousands(done as u64)), None),
                Phase::Hashing => (
                    tr!(
                        "scan.hashing",
                        done = thousands(done as u64),
                        total = thousands(total as u64)
                    ),
                    Some(done as f32 / total.max(1) as f32),
                ),
                Phase::Verifying => (
                    tr!(
                        "scan.verifying",
                        done = thousands(done as u64),
                        total = thousands(total as u64)
                    ),
                    Some(done as f32 / total.max(1) as f32),
                ),
            };
            kit::note(ui, &line, Tone::Info);
            kit::progress(ui, frac);
            if kit::buttons(ui, &[(tr!("btn.cancel"), ButtonKind::Secondary)]).is_some() {
                out.push(Action::Cancel);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            kit::separator(ui);
            options(ui, &st, &mut out);
            return;
        }
        if let Some((done, total)) = st.trashing {
            kit::note(
                ui,
                &tr!(
                    "trash.progress",
                    done = thousands(done as u64),
                    total = thousands(total as u64)
                ),
                Tone::Info,
            );
            kit::progress(ui, Some(done as f32 / total.max(1) as f32));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        let Some(r) = &st.result else {
            match &active.local_path {
                Some(local) => {
                    kit::note(
                        ui,
                        &tr!("intro", name = folder_name(&local.to_string_lossy())),
                        Tone::Neutral,
                    );
                    if kit::buttons(ui, &[(tr!("btn.find"), ButtonKind::Primary)]).is_some() {
                        out.push(Action::Scan);
                    }
                }
                None => {
                    let label = if active.backend_label.is_empty() {
                        tr!("remote.server").to_string()
                    } else {
                        active.backend_label.clone()
                    };
                    kit::callout(
                        ui,
                        icon::CLOUD,
                        &tr!("remote", label = label),
                        Tone::Neutral,
                    );
                }
            }
            if let Some(m) = &st.message {
                let (t, tone) = message_text(m);
                kit::note(ui, &t, tone);
            }
            kit::separator(ui);
            options(ui, &st, &mut out);
            return;
        };

        let (groups, files, size) = r.totals();
        let root = r.root.to_string_lossy();
        let detail = format!(
            "{} · {}",
            tr!("files", count = r.stats.files),
            duration(r.elapsed.as_secs_f64())
        );
        kit::section(
            ui,
            &tr!("result.title", name = folder_name(&root)),
            Some(&detail),
            true,
        );
        if let Some(m) = &st.message {
            let (t, tone) = message_text(m);
            kit::note(ui, &t, tone);
        }
        if groups == 0 {
            kit::callout(ui, icon::CHECK_CIRCLE, tr!("none_found"), Tone::Success);
        } else {
            kit::key_values(
                ui,
                &[
                    (
                        tr!("key.groups").into(),
                        thousands(groups as u64),
                        Tone::Neutral,
                    ),
                    (
                        tr!("key.files").into(),
                        thousands(files as u64),
                        Tone::Neutral,
                    ),
                    (tr!("key.reclaimable").into(), human_size(size), Tone::Info),
                ],
            );
            match kit::buttons(
                ui,
                &[
                    (tr!("btn.trash_all"), ButtonKind::Primary),
                    (tr!("btn.reveal"), ButtonKind::Secondary),
                    (tr!("btn.select"), ButtonKind::Secondary),
                ],
            ) {
                Some(0) => out.push(Action::AskTrash(Target::All)),
                Some(1) => out.push(Action::Reveal(Target::All)),
                Some(2) => out.push(Action::Select(Target::All)),
                _ => {}
            }
        }
        let mut notes = Vec::new();
        if !r.stats.skipped_dirs.is_empty() {
            let n = r.stats.skipped_dirs.len();
            notes.push(tr!(
                "note.skipped",
                count = n,
                folders = tr!("folders", count = n)
            ));
        }
        if r.stats.hard_links > 0 {
            notes.push(tr!(
                "note.hard_links",
                links = tr!("links", count = r.stats.hard_links)
            ));
        }
        if r.stats.unreadable > 0 {
            notes.push(tr!("note.unreadable", n = thousands(r.stats.unreadable)));
        }
        if !notes.is_empty() {
            let joined = notes.join(tr!("note.sep"));
            let mut chars = joined.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            kit::note(
                ui,
                &format!("{first}{}{}", chars.as_str(), tr!("note.end")),
                Tone::Muted,
            );
        }
        if r.stale {
            kit::callout(ui, icon::INFO, tr!("stale"), Tone::Warning);
        }
        let scan_label = match &active.local_path {
            Some(l) if *l != r.root => {
                tr!("btn.scan_folder", name = folder_name(&l.to_string_lossy()))
            }
            _ => tr!("btn.scan_again").to_string(),
        };
        match kit::buttons(
            ui,
            &[
                (&scan_label, ButtonKind::Secondary),
                (tr!("btn.clear"), ButtonKind::Secondary),
            ],
        ) {
            Some(0) => out.push(Action::Scan),
            Some(1) => out.push(Action::Clear),
            _ => {}
        }

        let shown = r.groups.len().min(st.limit);
        for (gi, g) in r.groups.iter().take(shown).enumerate() {
            ui.push_id(("group", gi), |ui| {
                kit::separator(ui);
                let k = st.kept(g);
                let manual = r.keep.get(&g.key) == Some(&g.files[k].path);
                let rule = scan::rule_label(&st.settings.keep_rule, &st.settings.keep_folder)
                    .to_lowercase();
                let rows: Vec<Row> = g
                    .files
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let date = scan::short_date(f.mtime(), now);
                        if i == k {
                            let why = if manual {
                                tr!("group.chosen").to_string()
                            } else {
                                rule.clone()
                            };
                            Row::new(Some(icon::CHECK_CIRCLE), f.rel.clone())
                                .detail(date.clone())
                                .tone(Tone::Success)
                                .tooltip(tr!("group.kept", why = why, date = date))
                        } else {
                            Row::new(Some(icon::COPY), f.rel.clone())
                                .detail(date.clone())
                                .tone(Tone::Muted)
                                .clickable()
                                .tooltip(tr!("group.goes", date = date))
                        }
                    })
                    .collect();
                let name = g.files[k].rel.rsplit('/').next().unwrap_or_default();
                kit::section(
                    ui,
                    &format!("{name}  ×{}", g.files.len()),
                    Some(&tr!(
                        "group.each",
                        size = human_size(g.size),
                        total = human_size(g.reclaimable())
                    )),
                    false,
                );
                if let Some(i) = kit::list(ui, &rows) {
                    if i != k {
                        out.push(Action::Keep(gi, i));
                    }
                }
                match kit::buttons_small(
                    ui,
                    &[
                        (tr!("btn.trash_extras"), ButtonKind::Secondary),
                        (tr!("btn.reveal"), ButtonKind::Secondary),
                        (tr!("btn.select"), ButtonKind::Secondary),
                    ],
                ) {
                    Some(0) => out.push(Action::AskTrash(Target::Group(gi))),
                    Some(1) => out.push(Action::Reveal(Target::Group(gi))),
                    Some(2) => out.push(Action::Select(Target::Group(gi))),
                    _ => {}
                }
            });
        }
        let rest = r.groups.len() - shown;
        if rest > 0 {
            kit::separator(ui);
            kit::note(
                ui,
                &tr!("more", groups = tr!("groups", count = rest)),
                Tone::Muted,
            );
            if st.limit < MAX_GROUPS_SHOWN
                && kit::buttons(ui, &[(tr!("btn.more"), ButtonKind::Secondary)]).is_some()
            {
                out.push(Action::More);
            }
        }
        kit::separator(ui);
        options(ui, &st, &mut out);
    });
    out
}

/// The "Move to Trash" sheet.
pub fn confirm_ui(ui: &mut Ui, shared: &SharedRef) -> Vec<Action> {
    let mut out = Vec::new();
    let st = lock(shared);
    let Some(work) = &st.pending else { return out };
    let files: Vec<&scan::FileInfo> = work.iter().flat_map(|(_, ex)| ex).collect();
    let size: u64 = files.iter().map(|f| f.size).sum();
    let full = ui.max_rect();
    let footer_h = 24.0 + 2.0 * kit::space::LG;
    let body =
        egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - footer_h));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        kit::body(ui, "duplicates-confirm", kit::sheet_margin(), |ui| {
            kit::heading(
                ui,
                &tr!("confirm.title", files = tr!("files", count = files.len())),
            );
            kit::note(
                ui,
                &tr!("confirm.body", size = human_size(size)),
                Tone::Muted,
            );
            let mut rows: Vec<Row> = files
                .iter()
                .take(5)
                .map(|f| Row::new(Some(icon::TRASH), f.rel.clone()).detail(human_size(f.size)))
                .collect();
            if files.len() > 5 {
                rows.push(
                    Row::new(
                        None,
                        tr!("confirm.more", n = thousands(files.len() as u64 - 5)),
                    )
                    .tone(Tone::Muted),
                );
            }
            kit::list(ui, &rows);
        });
    });
    // Footer: buttons on the right, the default one last.
    let p = kit::pal(ui);
    let footer = egui::Rect::from_min_max(egui::pos2(full.left(), body.bottom()), full.max);
    ui.painter().hline(
        footer.x_range(),
        footer.top(),
        egui::Stroke::new(1.0, p.separator),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(footer.shrink2(egui::vec2(kit::space::XL, kit::space::LG)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = kit::space::SM;
            if kit::button(ui, tr!("btn.move"), ButtonKind::Primary, true).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                out.push(Action::ConfirmTrash);
            }
            if kit::button(ui, tr!("btn.cancel"), ButtonKind::Secondary, true).clicked() {
                out.push(Action::CancelTrash);
            }
        },
    );
    out
}

/// Settings → Plugins → Duplicates. Returns what changed (the caller saves
/// the settings) or [`Action::EditSettings`].
pub fn settings_ui(ui: &mut Ui, shared: &SharedRef) -> Option<Action> {
    let mut st = lock(shared);
    let before = st.settings.clone();
    let mut action = None;
    egui::Grid::new("duplicates-settings")
        .num_columns(2)
        .spacing([kit::space::MD, kit::space::SM])
        .show(ui, |ui| {
            ui.label(tr!("opt.keep"));
            let current = scan::rule_label(&st.settings.keep_rule, &st.settings.keep_folder);
            egui::ComboBox::from_id_salt("dup-keep-rule")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for r in scan::KEEP_RULES {
                        let label = scan::rule_label(
                            r,
                            if r == "folder" {
                                &before.keep_folder
                            } else {
                                ""
                            },
                        );
                        ui.selectable_value(&mut st.settings.keep_rule, r.to_string(), label);
                    }
                });
            ui.end_row();
            ui.label(tr!("opt.min_size"));
            egui::ComboBox::from_id_salt("dup-min-size")
                .selected_text(human_size(st.settings.min_size.max(1)))
                .show_ui(ui, |ui| {
                    for m in scan::MIN_SIZES {
                        ui.selectable_value(&mut st.settings.min_size, m, human_size(m));
                    }
                });
            ui.end_row();
        });
    ui.checkbox(
        &mut st.settings.include_hidden,
        tr!("settings.include_hidden"),
    );
    ui.checkbox(
        &mut st.settings.skip_packages,
        tr!("settings.skip_packages"),
    );
    if ui
        .button(tr!("settings.edit_json"))
        .on_hover_text(tr!("settings.edit_json.tip"))
        .clicked()
    {
        action = Some(Action::EditSettings);
    }
    if action.is_none() && st.settings != before {
        action = Some(Action::CycleRule);
    }
    action
}
