//! The "Project" side panel, the script picker and the confirm sheet, as
//! declarative blocks (drawn by the host's block renderer, like the former
//! Python plugin's panels). Built from the detected state; no IO.

use manta_plugin::protocol::{Block, ButtonSpec, ListRow, Tag};
use manta_plugin::Tone;

use crate::detect::{self, kind_style, short_path, Project, Section};
use crate::i18n::tr;
use crate::{now_ts, Inner, SEP};

fn text(t: impl Into<String>, tone: Tone) -> Block {
    Block::Text {
        text: t.into(),
        tone,
    }
}

fn heading(t: impl Into<String>) -> Block {
    Block::Heading { text: t.into() }
}

fn section(t: impl Into<String>, detail: Option<String>) -> Block {
    Block::Section {
        text: t.into(),
        detail,
    }
}

fn button(label: impl Into<String>, action: impl Into<String>, primary: bool) -> ButtonSpec {
    ButtonSpec {
        label: label.into(),
        action: action.into(),
        primary,
    }
}

fn act(parts: &[&str]) -> String {
    parts.join(&SEP.to_string())
}

fn script_rows(project: &Project, sec: &Section) -> Vec<ListRow> {
    sec.scripts
        .iter()
        .map(|sc| {
            let mut tip = Vec::new();
            if let Some(d) = sc.desc.as_ref().filter(|d| **d != sc.detail) {
                tip.push(d.clone());
            }
            tip.push(format!("$ {}", sc.command));
            if let Some(why) = &sc.danger {
                tip.push(tr!("tip.asks_first", why = why));
            }
            if sc.needs_args {
                tip.push(tr!("tip.needs_args").to_string());
            }
            ListRow {
                icon: Some(if sc.danger.is_some() {
                    "warning".into()
                } else {
                    sc.icon.clone()
                }),
                text: sc.name.clone(),
                detail: Some(sc.detail.clone()),
                tone: if sc.danger.is_some() {
                    Tone::Warning
                } else {
                    Tone::Neutral
                },
                action: Some(act(&["run", &project.root, &sc.key])),
                tooltip: Some(tip.join("\n")),
            }
        })
        .collect()
}

fn section_blocks(st: &Inner, project: &Project) -> Vec<Block> {
    let mut blocks = Vec::new();
    for sec in &project.sections {
        if !sec.scripts.is_empty() {
            blocks.push(section(sec.title.clone(), sec.detail.clone()));
            blocks.push(Block::List {
                rows: script_rows(project, sec),
            });
        }
        if !sec.links.is_empty() {
            blocks.push(section(
                tr!("section.workspace"),
                Some(tr!("ws.packages", count = sec.links.len())),
            ));
            let rows = sec
                .links
                .iter()
                .take(60)
                .map(|l| ListRow {
                    icon: Some("folder".into()),
                    text: l.name.clone(),
                    detail: Some(l.detail.clone()),
                    tone: Tone::Neutral,
                    action: Some(act(&["goto", &l.path])),
                    tooltip: Some(tr!(
                        "tip.open_in_pane",
                        path = short_path(&l.path, &st.home, 80)
                    )),
                })
                .collect();
            blocks.push(Block::List { rows });
        }
    }
    blocks
}

fn ago(ts: f64) -> String {
    let secs = (now_ts() - ts).max(0.0);
    if secs < 60.0 {
        tr!("ago.now").into()
    } else if secs < 3600.0 {
        tr!("ago.min", n = (secs / 60.0) as u64)
    } else if secs < 86400.0 {
        tr!("ago.hours", n = (secs / 3600.0) as u64)
    } else {
        tr!("ago.days", n = (secs / 86400.0) as u64)
    }
}

fn recent_rows(st: &Inner, project: &Project) -> Vec<ListRow> {
    let Some(list) = st.recent.get(&project.root) else {
        return Vec::new();
    };
    list.iter()
        .enumerate()
        .map(|(i, r)| {
            let when = ago(r.ts.unwrap_or(0.0));
            ListRow {
                icon: Some("clock".into()),
                text: if r.title.is_empty() {
                    r.command.clone()
                } else {
                    r.title.clone()
                },
                detail: Some(if r.section.is_empty() {
                    when
                } else {
                    format!("{} · {when}", r.section)
                }),
                tone: Tone::Neutral,
                action: Some(act(&["recent", &project.root, &i.to_string()])),
                tooltip: Some(format!("$ {}", r.command)),
            }
        })
        .collect()
}

fn tags(project: &Project) -> Vec<Tag> {
    project
        .tags
        .iter()
        .map(|(kind, t)| {
            let (_, icon, tone) = kind_style(kind);
            Tag {
                text: t.clone(),
                tone,
                icon: Some(icon.into()),
            }
        })
        .collect()
}

fn header_blocks(st: &Inner, project: &Project, folder: &str) -> Vec<Block> {
    let mut where_ = short_path(&project.root, &st.home, 60);
    if let Some(b) = &project.git_branch {
        where_ = format!("{where_}  ·  {}", tr!("header.on_branch", branch = b));
    }
    let mut blocks = vec![heading(project.name.clone()), text(where_, Tone::Muted)];
    let t = tags(project);
    if !t.is_empty() {
        blocks.push(Block::Tags { tags: t });
    }
    for prob in &project.problems {
        blocks.push(text(prob.clone(), Tone::Warning));
    }
    let mut buttons = vec![
        button(
            tr!("btn.vscode"),
            act(&["open", "code", &project.root]),
            false,
        ),
        button(
            tr!("btn.finder"),
            act(&["open", "finder", &project.root]),
            false,
        ),
    ];
    if project.has_env {
        buttons.push(button(".env", act(&["open", "env", &project.root]), false));
    }
    if detect::normpath(folder) != project.root {
        buttons.push(button(
            tr!("btn.go_root"),
            act(&["goto", &project.root]),
            false,
        ));
    }
    blocks.push(Block::Buttons { buttons });
    blocks
}

pub fn panel_blocks(st: &Inner) -> Vec<Block> {
    let pane = st.active_pane();
    let state = st.panes.get(&pane);
    let det = st.det.get(&pane);
    let (Some(state), Some(d)) = (state, det) else {
        return vec![text(tr!("panel.waiting"), Tone::Muted)];
    };
    let Some(folder) = d.folder.as_deref().filter(|_| !d.remote) else {
        if state.backend_label.contains("archive") {
            return vec![
                heading(tr!("panel.archive")),
                text(tr!("panel.archive.note"), Tone::Muted),
            ];
        }
        return vec![
            heading(tr!("panel.remote")),
            text(tr!("panel.remote.note"), Tone::Muted),
        ];
    };
    let Some(p) = d.projects.first() else {
        return vec![
            heading(tr!("panel.no_project")),
            text(tr!("panel.no_project.note"), Tone::Muted),
        ];
    };
    let mut blocks = header_blocks(st, p, folder);
    let rec = recent_rows(st, p);
    if !rec.is_empty() {
        blocks.push(section(tr!("section.recent"), None));
        blocks.push(Block::List { rows: rec });
    }
    blocks.extend(section_blocks(st, p));
    if p.count == 0 {
        blocks.push(text(tr!("panel.no_scripts"), Tone::Muted));
    }
    for outer in d.projects.iter().skip(1).take(2) {
        blocks.push(Block::Separator);
        blocks.push(section(
            tr!("section.inside", name = outer.name),
            Some(short_path(&outer.root, &st.home, 40)),
        ));
        blocks.push(Block::Tags { tags: tags(outer) });
        blocks.extend(section_blocks(st, outer));
    }
    blocks
}

pub fn picker_blocks(st: &Inner) -> Vec<Block> {
    let projects = st.projects_of(st.active_pane());
    let Some(p) = projects.first() else {
        return vec![
            heading(tr!("panel.picker")),
            text(tr!("picker.no_project"), Tone::Muted),
            Block::Buttons {
                buttons: vec![button(tr!("btn.close"), "close", true)],
            },
        ];
    };
    let mut blocks = vec![
        heading(tr!("picker.heading", name = p.name)),
        text(
            tr!("picker.runs_in", path = short_path(&p.root, &st.home, 60)),
            Tone::Muted,
        ),
    ];
    let rec = recent_rows(st, p);
    if !rec.is_empty() {
        blocks.push(section(tr!("section.recent"), None));
        blocks.push(Block::List { rows: rec });
    }
    for (i, proj) in projects.iter().take(3).enumerate() {
        if i > 0 {
            blocks.push(section(
                tr!("section.inside", name = proj.name),
                Some(short_path(&proj.root, &st.home, 40)),
            ));
        }
        for sec in proj.sections.iter().filter(|s| !s.scripts.is_empty()) {
            blocks.push(section(sec.title.clone(), sec.detail.clone()));
            blocks.push(Block::List {
                rows: script_rows(proj, sec),
            });
        }
    }
    blocks.push(Block::Buttons {
        buttons: vec![button(tr!("btn.cancel"), "close", false)],
    });
    blocks
}

pub fn confirm_blocks(st: &Inner) -> Vec<Block> {
    let Some((run, _)) = &st.pending else {
        return Vec::new();
    };
    vec![
        heading(tr!("confirm.heading", name = run.title)),
        text(run.danger.clone().unwrap_or_default(), Tone::Warning),
        Block::Code {
            text: format!("$ {}", run.command),
        },
        text(
            tr!("confirm.in", path = short_path(&run.cwd, &st.home, 60)),
            Tone::Muted,
        ),
        Block::Buttons {
            buttons: vec![
                button(tr!("btn.run"), "confirm", true),
                button(tr!("btn.cancel"), "cancel", false),
            ],
        },
    ]
}
