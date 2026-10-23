//! The "Quick Actions" side panel and the confirmation sheet, as
//! declarative blocks (drawn by the host's block renderer, like the former
//! Python plugin's panels).

use std::time::{Duration, Instant};

use manta_plugin::protocol::{Block, ButtonSpec, ListRow};
use manta_plugin::Tone;

use crate::core::{self, Action, Mode};
use crate::i18n::tr;
use crate::jobs::{Job, Status};
use crate::Inner;

pub fn human_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    let num = |v: f64| manta_plugin::i18n::format_decimal(v, 1);
    if secs < 0.1 {
        format!("<{} s", num(0.1))
    } else if secs < 10.0 {
        format!("{} s", num(secs))
    } else if secs < 60.0 {
        format!("{} s", secs as u64)
    } else {
        let s = secs as u64;
        format!("{} min {} s", s / 60, s % 60)
    }
}

fn selection_heading(selection: &[String]) -> String {
    if selection.len() == 1 {
        core::split_name(&selection[0]).0
    } else {
        tr!("items", count = selection.len())
    }
}

/// `~` for the home folder; long paths keep their last two folders.
pub fn short_path(p: &str, limit: usize) -> String {
    let home = dirs_home();
    let mut p = p.to_string();
    if !home.is_empty() {
        if p == home {
            return "~".into();
        }
        if let Some(rest) = p.strip_prefix(&format!("{home}/")) {
            p = format!("~/{rest}");
        }
    }
    if p.chars().count() > limit {
        let parts: Vec<&str> = p.trim_end_matches('/').split('/').collect();
        p = format!("…/{}", parts[parts.len().saturating_sub(2)..].join("/"));
    }
    p
}

fn dirs_home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn status_icon(s: Status) -> &'static str {
    match s {
        Status::Queued | Status::Running => "clock",
        Status::Ok => "check",
        Status::Failed => "warning",
        Status::Cancelled => "info",
        Status::Terminal => "terminal",
    }
}

fn status_tone(s: Status) -> Tone {
    match s {
        Status::Queued | Status::Cancelled => Tone::Muted,
        Status::Running => Tone::Accent,
        Status::Ok => Tone::Success,
        Status::Failed => Tone::Danger,
        Status::Terminal => Tone::Neutral,
    }
}

fn job_detail(j: &Job, now: Instant) -> String {
    match j.status {
        Status::Queued => tr!("status.waiting").into(),
        Status::Running => {
            let d = tr!("status.running", time = human_duration(now - j.started));
            if j.commands.len() > 1 {
                format!("{}/{} · {d}", j.step + 1, j.commands.len())
            } else {
                d
            }
        }
        Status::Terminal => tr!("status.in_terminal").into(),
        Status::Failed => tr!(
            "status.failed",
            time = human_duration(j.ended.unwrap_or(now) - j.started)
        ),
        Status::Cancelled => tr!("status.cancelled").into(),
        Status::Ok => human_duration(j.ended.unwrap_or(now) - j.started),
    }
}

fn text(t: impl Into<String>, tone: Tone) -> Block {
    Block::Text {
        text: t.into(),
        tone,
    }
}

fn heading(t: impl Into<String>) -> Block {
    Block::Heading { text: t.into() }
}

fn button(label: impl Into<String>, action: impl Into<String>, primary: bool) -> ButtonSpec {
    ButtonSpec {
        label: label.into(),
        action: action.into(),
        primary,
    }
}

pub fn panel_blocks(st: &Inner) -> Vec<Block> {
    let now = Instant::now();
    let mut blocks = Vec::new();
    if let Some(e) = st.errors.first() {
        blocks.push(text(format!("actions.json: {e}"), Tone::Warning));
    }
    let state = st.active.and_then(|a| st.panes.get(&a));
    match state {
        None => blocks.push(text(tr!("panel.waiting"), Tone::Muted)),
        Some(s) if s.local_path.is_none() => {
            let in_archive = s.backend_label.contains("archive");
            let sel: Vec<String> = s.selection.iter().map(|p| p.as_str().to_string()).collect();
            let fallback = if in_archive {
                tr!("panel.in_archive")
            } else {
                tr!("panel.remote")
            };
            blocks.push(heading(if sel.is_empty() {
                fallback.to_string()
            } else {
                selection_heading(&sel)
            }));
            let note = if in_archive {
                tr!("panel.local_only_archive")
            } else {
                tr!("panel.local_only")
            };
            blocks.push(text(note, Tone::Muted));
        }
        Some(s) if s.selection.is_empty() => {
            blocks.push(heading(tr!("panel.nothing_selected")));
            blocks.push(text(tr!("panel.select_hint"), Tone::Muted));
        }
        Some(s) => {
            let sel: Vec<String> = s.selection.iter().map(|p| p.as_str().to_string()).collect();
            blocks.push(heading(selection_heading(&sel)));
            let folder = s
                .local_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            blocks.push(text(
                tr!("panel.in_folder", path = short_path(&folder, 40)),
                Tone::Muted,
            ));
            let rows: Vec<ListRow> = st
                .actions
                .iter()
                .filter(|a| st.eval.ok.contains(&a.id))
                .map(|a| ListRow {
                    icon: Some(a.icon.clone().unwrap_or_else(|| "file".into())),
                    text: core::title(a),
                    detail: if a.mode == Mode::Terminal {
                        Some(tr!("status.in_terminal").to_string())
                    } else {
                        a.shortcut.clone()
                    },
                    tone: Tone::Neutral,
                    action: Some(format!("run:{}", a.id)),
                    tooltip: None,
                })
                .collect();
            if rows.is_empty() {
                blocks.push(text(tr!("panel.no_actions"), Tone::Muted));
            } else {
                blocks.push(Block::List { rows });
            }
        }
    }

    blocks.push(Block::Separator);
    blocks.push(heading(tr!("panel.last_runs")));
    if st.jobs.is_empty() {
        blocks.push(text(tr!("panel.nothing_run"), Tone::Muted));
    } else {
        let rows = st
            .jobs
            .iter()
            .map(|j| ListRow {
                icon: Some(status_icon(j.status).into()),
                text: core::title(&j.action),
                detail: Some(job_detail(j, now)),
                tone: status_tone(j.status),
                action: Some(format!("show:{}", j.id)),
                tooltip: None,
            })
            .collect();
        blocks.push(Block::List { rows });
        let running: Vec<&Job> = st.jobs.iter().filter(|j| j.status.busy()).collect();
        if !running.is_empty() {
            let buttons = running
                .iter()
                .map(|j| {
                    let label = if running.len() > 1 {
                        tr!("panel.cancel_named", title = core::title(&j.action))
                    } else {
                        tr!("cancel").to_string()
                    };
                    button(label, format!("cancel:{}", j.id), false)
                })
                .collect();
            blocks.push(Block::Buttons { buttons });
        }
        let shown = st
            .jobs
            .iter()
            .find(|j| Some(j.id) == st.shown_job)
            .unwrap_or(&st.jobs[0]);
        let tail = shown.tail(14);
        let mut code = format!("$ {}", shown.display_command());
        if !tail.is_empty() {
            code.push('\n');
            code.push_str(&tail);
        }
        blocks.push(Block::Code { text: code });
    }
    blocks.push(Block::Buttons {
        buttons: vec![
            button(tr!("panel.edit"), "edit", false),
            button(tr!("panel.reload"), "reload", false),
        ],
    });
    blocks
}

/// The confirmation sheet for `action` on `items`.
pub fn confirm_blocks(action: &Action, items: &[(String, bool)]) -> Vec<Block> {
    let names: Vec<String> = items.iter().map(|(p, _)| core::split_name(p).0).collect();
    let mut listing = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        listing = tr!("confirm.and_more", names = listing, count = names.len() - 3);
    }
    vec![
        heading(core::title(action)),
        text(
            core::confirm_text(action, items.len()).unwrap_or_default(),
            Tone::Neutral,
        ),
        text(listing, Tone::Muted),
        Block::Buttons {
            buttons: vec![
                button(tr!("confirm.run"), "confirm:run", true),
                button(tr!("cancel"), "confirm:cancel", false),
            ],
        },
    ]
}
