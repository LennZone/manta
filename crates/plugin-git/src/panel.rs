//! The Git sidebar panel. Pure drawing: reads the shared snapshot (cheap
//! `Arc` clones under a short lock) and returns [`PanelAction`]s that the
//! plugin forwards to the background threads.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{CornerRadius, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use manta_plugin::{PaneState, Tone};

use crate::git::{Commit, Diff, DiffRequest, DiffSource, LineKind, MAX_DIFF_LINES};
use crate::i18n::tr;
use crate::kit::{self, space, text, ButtonKind, Colors, Icon, ROW_H};
use crate::model::{ahead_behind_text, classify};
use crate::shared::{lock, PaneRepo, RepoSnapshot, SharedRef};
use crate::status::{GitStatus, StatusEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Conflicts,
    Staged,
    Changes,
    Untracked,
}

impl Group {
    pub const ALL: [Group; 4] = [
        Group::Conflicts,
        Group::Staged,
        Group::Changes,
        Group::Untracked,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Conflicts => tr!("git.group.conflicts"),
            Group::Staged => tr!("git.group.staged"),
            Group::Changes => tr!("git.group.changes"),
            Group::Untracked => tr!("git.group.untracked"),
        }
    }

    pub fn contains(self, e: &StatusEntry) -> bool {
        match self {
            Group::Conflicts => e.is_conflict(),
            Group::Staged => e.is_staged(),
            Group::Changes => e.is_unstaged(),
            Group::Untracked => e.is_untracked(),
        }
    }

    fn source(self) -> DiffSource {
        match self {
            Group::Conflicts => DiffSource::Conflict,
            Group::Staged => DiffSource::Staged,
            Group::Changes => DiffSource::Unstaged,
            Group::Untracked => DiffSource::Untracked,
        }
    }

    /// Badge for an entry shown in this group.
    fn badge(self, e: &StatusEntry) -> (String, Tone) {
        match self {
            Group::Conflicts => ("U".into(), Tone::Danger),
            Group::Untracked => ("?".into(), Tone::Info),
            Group::Staged => {
                let t = if e.orig_path().is_some() {
                    "R"
                } else {
                    letter(e.x)
                };
                let tone = if e.orig_path().is_some() {
                    Tone::Accent
                } else {
                    Tone::Success
                };
                (t.into(), tone)
            }
            Group::Changes => (letter(e.y).into(), Tone::Warning),
        }
    }

    fn describe(self, e: &StatusEntry) -> String {
        match self {
            Group::Conflicts | Group::Untracked => classify(e).2,
            Group::Staged => match (e.x, e.orig_path()) {
                (_, Some(o)) => tr!("git.state.renamed_short", orig = o),
                ('A', _) => tr!("git.state.added_staged").into(),
                ('D', _) => tr!("git.state.deletion_staged").into(),
                _ => tr!("git.state.staged").into(),
            },
            Group::Changes => match e.y {
                'D' => tr!("git.state.deleted_unstaged").into(),
                _ => tr!("git.state.modified_unstaged").into(),
            },
        }
    }
}

fn letter(c: char) -> &'static str {
    match c {
        'A' => "A",
        'D' => "D",
        'R' => "R",
        'C' => "C",
        'T' => "T",
        _ => "M",
    }
}

/// The selected file whose diff is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub root: PathBuf,
    pub group: Group,
    pub path: String,
    pub orig: Option<String>,
}

impl Selection {
    pub fn request(&self) -> DiffRequest {
        DiffRequest {
            path: self.path.clone(),
            orig: if self.group == Group::Staged {
                self.orig.clone()
            } else {
                None
            },
            source: self.group.source(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    Select(Option<Selection>),
    Stage(PathBuf, Vec<String>),
    Unstage(PathBuf, Vec<String>),
    StageAll(PathBuf),
    Commit(PathBuf, String),
    Refresh,
}

#[derive(Default)]
pub struct PanelState {
    pub commit_msg: String,
    pub selection: Option<Selection>,
    seen_commit_seq: u64,
    last_root: Option<PathBuf>,
    /// Layout of the changes list in the last frame (for tests).
    #[doc(hidden)]
    pub layout: ChangesLayout,
    /// Scroll the changes list to its end on the next frame (for tests).
    #[doc(hidden)]
    pub scroll_changes_to_end: bool,
}

/// Where the changes list put things in the last frame (screen coords).
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct ChangesLayout {
    /// Visible part of the scroll area.
    pub viewport: Rect,
    pub headers: Vec<(Group, Rect)>,
    pub rows: Vec<(Group, String, Rect)>,
    /// More content below the visible part.
    pub more_below: bool,
}

impl Default for ChangesLayout {
    fn default() -> Self {
        ChangesLayout {
            viewport: Rect::NOTHING,
            headers: Vec::new(),
            rows: Vec::new(),
            more_below: false,
        }
    }
}

const GROUP_HEADER_H: f32 = 22.0;

/// Height for the changes list: everything if it fits in `max_h`, else cut
/// at a row boundary so a group header is never shown without its rows.
pub fn changes_height(group_sizes: &[usize], max_h: f32) -> f32 {
    let mut y = 0.0;
    let mut cuts = Vec::new();
    for (i, n) in group_sizes.iter().enumerate() {
        if i > 0 {
            y += space::SM;
        }
        y += GROUP_HEADER_H;
        for _ in 0..*n {
            y += ROW_H;
            cuts.push(y);
        }
    }
    if y <= max_h {
        return y;
    }
    // At least the first header with one row.
    let first = cuts.first().copied().unwrap_or(y);
    cuts.into_iter().rfind(|c| *c <= max_h).unwrap_or(first)
}

const DIFF_LINE_H: f32 = 16.0;

type DiffResult = Result<Arc<Diff>, String>;

/// Everything the panel needs, copied out of the shared state.
struct View {
    repo: Option<PaneRepo>,
    snap: Option<Arc<RepoSnapshot>>,
    diff: Option<(DiffRequest, Option<DiffResult>)>,
    commit_seq: u64,
    busy: bool,
}

fn read_view(shared: &SharedRef, pane: &PaneState) -> View {
    let sh = lock(shared);
    let repo = sh.panes.get(&pane.pane).cloned();
    let root = match &repo {
        Some(PaneRepo::Repo { root, .. }) => Some(root.clone()),
        _ => None,
    };
    let snap = root.as_ref().and_then(|r| sh.repos.get(r).cloned());
    let diff = sh
        .diff
        .as_ref()
        .filter(|d| Some(&d.root) == root.as_ref())
        .map(|d| (d.req.clone(), d.result.clone()));
    View {
        repo,
        snap,
        diff,
        commit_seq: sh.commit_seq,
        busy: root.as_ref().is_some_and(|r| sh.busy.contains(r)),
    }
}

pub fn panel_ui(
    st: &mut PanelState,
    shared: &SharedRef,
    pane: &PaneState,
    ui: &mut Ui,
) -> Vec<PanelAction> {
    let mut actions = Vec::new();
    let view = read_view(shared, pane);
    let col = Colors::of(ui);
    ui.spacing_mut().item_spacing = Vec2::new(space::MD, space::SM);

    if pane.local_path.is_none() || view.repo == Some(PaneRepo::NotLocal) {
        empty_state(
            ui,
            Icon::Cloud,
            tr!("git.empty.not_local"),
            tr!("git.empty.not_local.body"),
        );
        return actions;
    }
    let (root, rel) = match &view.repo {
        Some(PaneRepo::Repo { root, rel }) => (root.clone(), rel.clone()),
        Some(PaneRepo::NotRepo) => {
            empty_state(
                ui,
                Icon::Folder,
                tr!("git.empty.not_repo"),
                tr!("git.empty.not_repo.body"),
            );
            return actions;
        }
        _ => {
            loading(ui, tr!("git.loading.repo"));
            return actions;
        }
    };

    if st.last_root.as_ref() != Some(&root) {
        st.last_root = Some(root.clone());
        st.commit_msg.clear();
        if st.selection.take().is_some() {
            actions.push(PanelAction::Select(None));
        }
    }
    if view.commit_seq != st.seen_commit_seq {
        if st.seen_commit_seq != 0 || view.commit_seq > 0 {
            st.commit_msg.clear();
        }
        st.seen_commit_seq = view.commit_seq;
    }

    let Some(snap) = view.snap.clone() else {
        loading(ui, tr!("git.loading.status"));
        return actions;
    };
    let Some(status) = snap.status.as_ref() else {
        match &snap.error {
            Some(e) => empty_state(ui, Icon::Warning, tr!("git.error.status"), e),
            None => loading(ui, tr!("git.loading.status")),
        }
        return actions;
    };

    // Keep the selection valid: a staged file moves from "Changes" to
    // "Staged", follow it there.
    if let Some(sel) = st.selection.clone() {
        let entry = status.entries.iter().find(|e| e.path == sel.path);
        let still = entry.is_some_and(|e| sel.group.contains(e));
        if !still {
            let moved = entry.and_then(|e| {
                [
                    Group::Staged,
                    Group::Changes,
                    Group::Conflicts,
                    Group::Untracked,
                ]
                .into_iter()
                .find(|g| g.contains(e))
                .map(|g| Selection {
                    root: root.clone(),
                    group: g,
                    path: e.path.clone(),
                    orig: e.orig_path().map(String::from),
                })
            });
            st.selection = moved.clone();
            actions.push(PanelAction::Select(moved));
        }
    }

    let margin = Margin {
        left: 12,
        right: 12,
        top: 10,
        bottom: 8,
    };
    egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
        ui.set_width(ui.available_width());
        header(ui, &col, &snap, status, &mut actions);
        ui.add_space(space::SM);
        commit_box(ui, &col, st, &root, status, view.busy, &mut actions);
    });
    kit::separator(ui);

    let avail = ui.available_height();
    let has_diff = st.selection.is_some();
    let changes_max = if has_diff {
        (avail * 0.34).max(84.0)
    } else {
        (avail * 0.52).max(96.0)
    };

    egui::Frame::NONE
        .inner_margin(Margin {
            left: 6,
            right: 6,
            top: 6,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            changes(ui, &col, st, &root, &rel, status, changes_max, &mut actions);
        });

    if let Some(sel) = st.selection.clone() {
        kit::separator(ui);
        let diff = view
            .diff
            .as_ref()
            .filter(|(req, _)| *req == sel.request())
            .and_then(|(_, r)| r.clone());
        let max_h = (avail * 0.42)
            .max(120.0)
            .min(ui.available_height() - 90.0)
            .max(60.0);
        // Fit short diffs; long ones scroll.
        let want = match &diff {
            Some(Ok(d)) if !d.binary && !d.lines.is_empty() => {
                (d.lines.len() + usize::from(d.truncated)) as f32 * DIFF_LINE_H + 8.0
            }
            _ => 64.0,
        };
        let diff_h = want.clamp(64.0, max_h);
        diff_view(ui, &col, &sel, status, diff, diff_h, &mut actions, st);
    }

    kit::separator(ui);
    commits(ui, &col, &snap.commits);
    actions
}

// ------------------------------------------------------------------ header

fn header(
    ui: &mut Ui,
    col: &Colors,
    snap: &RepoSnapshot,
    status: &GitStatus,
    actions: &mut Vec<PanelAction>,
) {
    let b = &status.branch;
    // Row 1: repo name + refresh.
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let btn = Rect::from_min_size(egui::pos2(row.right() - 22.0, row.top()), Vec2::splat(22.0));
    let g = kit::galley(
        ui,
        &snap.name(),
        kit::semibold(ui, text::TITLE),
        col.text,
        row.width() - 30.0,
    );
    kit::paint_galley(ui, g, row.left(), row, col.text);
    if kit::icon_button(
        ui,
        kit::id("git-refresh"),
        btn,
        Icon::Refresh,
        tr!("git.header.refresh"),
        col.text2,
    )
    .clicked()
    {
        actions.push(PanelAction::Refresh);
    }
    let path_tip = snap.loc.root.display().to_string();
    ui.interact(
        row.with_max_x(btn.left() - 4.0),
        kit::id("git-repo-name"),
        Sense::hover(),
    )
    .on_hover_text(path_tip);

    // Row 2: branch + ahead/behind.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        kit::paint_icon(ui, ir, Icon::Branch, col.text2);
        ui.add_space(space::XXS);
        let name = b.display_name();
        ui.label(
            RichText::new(name)
                .font(kit::medium(ui, text::BODY))
                .color(col.text),
        );
        if b.is_detached() {
            kit::pill(
                ui,
                tr!("git.header.detached"),
                col.warning,
                col.warning_soft,
            );
        }
        ui.add_space(space::XS);
        if b.ahead > 0 {
            kit::pill(
                ui,
                &ahead_behind_text(b.ahead, 0),
                col.accent_text,
                col.accent_soft,
            )
            .on_hover_text(tr!("git.header.to_push", count = b.ahead));
        }
        if b.behind > 0 {
            kit::pill(
                ui,
                &ahead_behind_text(0, b.behind),
                col.warning,
                col.warning_soft,
            )
            .on_hover_text(tr!("git.header.to_pull", count = b.behind));
        }
    });
    // Row 3: upstream in words.
    let up = match (&b.upstream, b.has_ab, b.ahead, b.behind) {
        (Some(u), false, _, _) => tr!("git.header.tracking_gone", up = u),
        (Some(u), true, 0, 0) => tr!("git.header.tracking_current", up = u),
        (Some(u), true, a, 0) => tr!("git.header.tracking_ahead", up = u, a = a),
        (Some(u), true, 0, d) => tr!("git.header.tracking_behind", up = u, d = d),
        (Some(u), true, a, d) => tr!("git.header.tracking_both", up = u, a = a, d = d),
        (None, ..) if b.is_initial() => tr!("git.chip.no_commits").to_string(),
        (None, ..) if b.is_detached() => tr!("git.header.detached_head").to_string(),
        (None, ..) => tr!("git.chip.no_upstream").to_string(),
    };
    ui.add_space(-space::XXS);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.label(RichText::new(up).size(text::CAPTION).color(col.text2));
    });

    if let Some(p) = &snap.in_progress {
        ui.add_space(space::XS);
        let tone = if p.is_blocking() {
            (col.danger, col.danger_soft)
        } else {
            (col.accent_text, col.accent_soft)
        };
        egui::Frame::NONE
            .fill(tone.1)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::SM;
                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    kit::paint_icon(ui, ir, Icon::Warning, tone.0);
                    ui.label(
                        RichText::new(p.describe())
                            .font(kit::medium(ui, text::SMALL))
                            .color(tone.0),
                    );
                });
                let hint = match p {
                    crate::model::InProgress::Merge if status.conflicts() > 0 => {
                        tr!("git.hint.merge_conflicts")
                    }
                    crate::model::InProgress::Merge => tr!("git.hint.merge_done"),
                    crate::model::InProgress::Rebase { .. } => tr!("git.hint.rebase"),
                    crate::model::InProgress::CherryPick => tr!("git.hint.cherry_pick"),
                    crate::model::InProgress::Revert => tr!("git.hint.revert"),
                    crate::model::InProgress::Bisect => tr!("git.hint.bisect"),
                    crate::model::InProgress::ApplyMailbox => tr!("git.hint.am"),
                };
                ui.label(RichText::new(hint).size(text::CAPTION).color(col.text2));
            });
    }
    if let Some(e) = &snap.error {
        ui.label(RichText::new(e).size(text::CAPTION).color(col.danger));
    }
}

fn commit_box(
    ui: &mut Ui,
    col: &Colors,
    st: &mut PanelState,
    root: &std::path::Path,
    status: &GitStatus,
    busy: bool,
    actions: &mut Vec<PanelAction>,
) {
    let staged = status.staged_count();
    let field_id = kit::id("git-commit-msg");
    let focused = ui.memory(|m| m.has_focus(field_id));
    let stroke_col = if focused {
        col.accent
    } else {
        ui.visuals().widgets.inactive.bg_stroke.color
    };
    let resp = egui::Frame::NONE
        .fill(col.field)
        .stroke(Stroke::new(1.0, stroke_col))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut st.commit_msg)
                    .id(field_id)
                    .frame(egui::Frame::NONE)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::proportional(text::BODY))
                    .hint_text(RichText::new(tr!("git.commit.placeholder")).color(col.text3)),
            )
        })
        .inner;
    let can_commit = !st.commit_msg.trim().is_empty() && staged > 0 && !busy;
    let shortcut =
        resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));

    let mut clicked = false;
    ui.horizontal(|ui| {
        let info = if busy {
            tr!("git.commit.working").to_string()
        } else if staged == 0 {
            tr!("git.commit.nothing_staged").to_string()
        } else {
            tr!("git.commit.staged", count = staged)
        };
        if busy {
            ui.add(egui::Spinner::new().size(12.0).color(col.text2));
        }
        ui.label(RichText::new(info).size(text::CAPTION).color(col.text2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = kit::button(
                ui,
                tr!("git.commit.button"),
                ButtonKind::Primary,
                can_commit,
                24.0,
            );
            let tip = if staged == 0 {
                tr!("git.commit.tip.stage_first")
            } else if st.commit_msg.trim().is_empty() {
                tr!("git.commit.tip.message")
            } else {
                tr!("git.commit.tip.commit")
            };
            clicked = r.on_hover_text(tip).clicked();
        });
    });
    if (clicked || shortcut) && can_commit {
        let msg = st.commit_msg.trim().to_string();
        actions.push(PanelAction::Commit(root.to_path_buf(), msg));
    }
}

// ------------------------------------------------------------------ changes

/// `path` relative to the shown folder where possible.
pub fn display_path<'a>(path: &'a str, rel: &str) -> &'a str {
    if rel.is_empty() {
        return path;
    }
    path.strip_prefix(rel)
        .and_then(|r| r.strip_prefix('/'))
        .unwrap_or(path)
}

#[allow(clippy::too_many_arguments)]
fn changes(
    ui: &mut Ui,
    col: &Colors,
    st: &mut PanelState,
    root: &std::path::Path,
    rel: &str,
    status: &GitStatus,
    max_h: f32,
    actions: &mut Vec<PanelAction>,
) {
    if status.is_clean() {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
        let font = kit::medium(ui, text::BODY);
        let g = ui
            .painter()
            .layout_no_wrap(tr!("git.summary.clean").into(), font, col.text2);
        let total = 16.0 + space::SM + g.size().x;
        let x0 = rect.center().x - total / 2.0;
        let ir = Rect::from_center_size(egui::pos2(x0 + 8.0, rect.center().y), Vec2::splat(16.0));
        ui.painter()
            .circle_filled(ir.center(), 9.0, col.success_soft);
        kit::paint_icon(ui, ir.shrink(2.5), Icon::Check, col.success);
        kit::paint_galley(ui, g, x0 + 16.0 + space::SM, rect, col.text2);
        return;
    }

    let groups: Vec<(Group, Vec<&StatusEntry>)> = Group::ALL
        .into_iter()
        .map(|g| {
            (
                g,
                status
                    .entries
                    .iter()
                    .filter(|e| g.contains(e))
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let stage_all_on = groups
        .iter()
        .find(|(g, _)| matches!(g, Group::Changes | Group::Untracked))
        .map(|(g, _)| *g);

    let sizes: Vec<usize> = groups.iter().map(|(_, v)| v.len()).collect();
    let height = changes_height(&sizes, max_h);
    let mut layout = ChangesLayout::default();
    // A real (non-floating) scrollbar, so it is obvious the list scrolls.
    let mut scroll_style = egui::style::ScrollStyle::solid();
    scroll_style.bar_width = 6.0;
    scroll_style.bar_inner_margin = 2.0;
    ui.spacing_mut().scroll = scroll_style;
    let mut area = egui::ScrollArea::vertical()
        .id_salt("git-changes")
        .min_scrolled_height(height)
        .max_height(height)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
    if std::mem::take(&mut st.scroll_changes_to_end) {
        area =
            area.vertical_scroll_offset((changes_height(&sizes, f32::INFINITY) - height).max(0.0));
    }
    let out = area.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, (group, entries)) in groups.iter().enumerate() {
            if i > 0 {
                ui.add_space(space::SM);
            }
            let top = ui.cursor().top();
            group_header(
                ui,
                col,
                *group,
                entries.len(),
                root,
                stage_all_on == Some(*group),
                entries,
                actions,
            );
            let r = Rect::from_min_size(
                egui::pos2(ui.max_rect().left(), top),
                Vec2::new(ui.available_width(), GROUP_HEADER_H),
            );
            layout.headers.push((*group, r));
            for e in entries {
                let top = ui.cursor().top();
                file_row(ui, col, st, root, rel, *group, e, actions);
                let r = Rect::from_min_size(
                    egui::pos2(ui.max_rect().left(), top),
                    Vec2::new(ui.available_width(), ROW_H),
                );
                layout.rows.push((*group, e.path.clone(), r));
            }
        }
    });
    layout.viewport = out.inner_rect;
    layout.more_below =
        out.state.clamped_offset().y + out.inner_rect.height() < out.content_size.y - 1.0;
    if layout.more_below {
        // Fade the bottom edge as a hint that the list continues.
        let vp = out.inner_rect;
        let bg = ui.visuals().panel_fill;
        let fade = Rect::from_min_max(
            egui::pos2(vp.left(), vp.bottom() - 18.0),
            egui::pos2(vp.right() - 10.0, vp.bottom()),
        );
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(fade.left_top(), bg.gamma_multiply(0.0));
        mesh.colored_vertex(fade.right_top(), bg.gamma_multiply(0.0));
        mesh.colored_vertex(fade.left_bottom(), bg);
        mesh.colored_vertex(fade.right_bottom(), bg);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 2, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
    }
    st.layout = layout;
}

#[allow(clippy::too_many_arguments)]
fn group_header(
    ui: &mut Ui,
    col: &Colors,
    group: Group,
    count: usize,
    root: &std::path::Path,
    show_stage_all: bool,
    entries: &[&StatusEntry],
    actions: &mut Vec<PanelAction>,
) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), GROUP_HEADER_H),
        Sense::hover(),
    );
    let x = rect.left() + space::SM;
    let title = group.title().to_uppercase();
    let font = kit::semibold(ui, 10.5);
    let g = ui.painter().layout_no_wrap(title, font, col.text2);
    let x = kit::paint_galley(ui, g, x, rect, col.text2);
    let cg = ui
        .painter()
        .layout_no_wrap(count.to_string(), kit::medium(ui, 10.5), col.text3);
    kit::paint_galley(ui, cg, x + space::SM, rect, col.text3);

    let action = match group {
        Group::Staged => Some((tr!("git.action.unstage_all"), false)),
        _ if show_stage_all => Some((tr!("git.action.stage_all"), true)),
        _ => None,
    };
    if let Some((label, stage)) = action {
        let font = kit::medium(ui, text::CAPTION);
        let g = ui
            .painter()
            .layout_no_wrap(label.into(), font, col.accent_text);
        let br = Rect::from_min_max(
            egui::pos2(
                rect.right() - g.size().x - 2.0 * space::SM,
                rect.top() + 2.0,
            ),
            egui::pos2(rect.right(), rect.bottom() - 2.0),
        );
        let r = ui.interact(br, kit::id(("git-group-action", group)), Sense::click());
        if r.hovered() {
            ui.painter()
                .rect_filled(br, CornerRadius::same(5), col.hover);
        }
        ui.painter()
            .galley(br.center() - g.size() / 2.0, g, col.accent_text);
        if r.clicked() {
            if stage {
                actions.push(PanelAction::StageAll(root.to_path_buf()));
            } else {
                let paths = entries
                    .iter()
                    .flat_map(|e| {
                        std::iter::once(e.path.clone()).chain(e.orig_path().map(String::from))
                    })
                    .collect();
                actions.push(PanelAction::Unstage(root.to_path_buf(), paths));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn file_row(
    ui: &mut Ui,
    col: &Colors,
    st: &mut PanelState,
    root: &std::path::Path,
    rel: &str,
    group: Group,
    e: &StatusEntry,
    actions: &mut Vec<PanelAction>,
) {
    let id = kit::id(("git-row", group, &e.path));
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    let selected = st
        .selection
        .as_ref()
        .is_some_and(|s| s.group == group && s.path == e.path);
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), col.selected);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), col.hover);
    }

    let (letter, tone) = group.badge(e);
    let (fg, bg) = col.tone(tone);
    let br = Rect::from_center_size(
        egui::pos2(rect.left() + space::SM + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    kit::paint_badge(ui, br, &letter, fg, bg);

    let shown = display_path(&e.path, rel);
    let (dir, name) = match shown.rsplit_once('/') {
        Some((d, n)) => (Some(d), n),
        None => (None, shown),
    };
    let action_w = if hovered || selected { 24.0 } else { 0.0 };
    let x = br.right() + space::SM + 2.0;
    let max_w = rect.right() - x - action_w - space::XS;
    let name_g = kit::galley(ui, name, kit::regular(text::BODY), col.text, max_w);
    let name_w = name_g.size().x;
    let mut end = kit::paint_galley(ui, name_g, x, rect, col.text);
    let detail = match (group, e.orig_path()) {
        (Group::Staged, Some(o)) => Some(format!("← {}", display_path(o, rel))),
        _ => dir.map(String::from),
    };
    if let Some(d) = detail {
        let left = max_w - name_w - space::SM;
        if left > 24.0 {
            let g = kit::galley(ui, &d, kit::regular(text::CAPTION), col.text3, left);
            end = kit::paint_galley(ui, g, end + space::SM, rect, col.text3);
        }
    }
    let _ = end;

    let mut action_clicked = false;
    if hovered || selected {
        let ar = Rect::from_center_size(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            Vec2::splat(20.0),
        );
        let (icon, hint) = match group {
            Group::Staged => (Icon::Minus, tr!("git.action.unstage")),
            Group::Conflicts => (Icon::Plus, tr!("git.action.resolve")),
            _ => (Icon::Plus, tr!("git.action.stage")),
        };
        let r = kit::icon_button(ui, id.with("action"), ar, icon, hint, col.text2);
        if r.clicked() {
            action_clicked = true;
            let paths = vec![e.path.clone()];
            actions.push(match group {
                Group::Staged => {
                    let mut p = paths;
                    p.extend(e.orig_path().map(String::from));
                    PanelAction::Unstage(root.to_path_buf(), p)
                }
                _ => PanelAction::Stage(root.to_path_buf(), paths),
            });
        }
    }

    let resp = resp.on_hover_text(format!("{}\n{}", e.path, group.describe(e)));
    if resp.clicked() && !action_clicked {
        let sel = Selection {
            root: root.to_path_buf(),
            group,
            path: e.path.clone(),
            orig: e.orig_path().map(String::from),
        };
        if selected {
            st.selection = None;
            actions.push(PanelAction::Select(None));
        } else {
            st.selection = Some(sel.clone());
            actions.push(PanelAction::Select(Some(sel)));
        }
    }
}

// ------------------------------------------------------------------ diff

#[allow(clippy::too_many_arguments)]
fn diff_view(
    ui: &mut Ui,
    col: &Colors,
    sel: &Selection,
    status: &GitStatus,
    diff: Option<Result<Arc<Diff>, String>>,
    height: f32,
    actions: &mut Vec<PanelAction>,
    st: &mut PanelState,
) {
    // Header.
    egui::Frame::NONE
        .inner_margin(Margin {
            left: 12,
            right: 8,
            top: 6,
            bottom: 4,
        })
        .show(ui, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            let entry = status.entries.iter().find(|e| e.path == sel.path);
            let mut x = rect.left();
            if let Some(e) = entry {
                let (letter, tone) = sel.group.badge(e);
                let (fg, bg) = col.tone(tone);
                let br =
                    Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
                kit::paint_badge(ui, br, &letter, fg, bg);
                x = br.right() + space::SM;
            }
            let close = Rect::from_center_size(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                Vec2::splat(20.0),
            );
            // +/- counts.
            let mut right = close.left() - space::SM;
            if let Some(Ok(d)) = &diff {
                for (txt, c) in [
                    (format!("−{}", d.dels), col.danger),
                    (format!("+{}", d.adds), col.success),
                ] {
                    let g = ui
                        .painter()
                        .layout_no_wrap(txt, kit::medium(ui, text::CAPTION), c);
                    right -= g.size().x;
                    ui.painter().galley(
                        egui::pos2(right, rect.center().y - g.size().y / 2.0),
                        g,
                        c,
                    );
                    right -= space::XS;
                }
            }
            let name = sel.path.rsplit('/').next().unwrap_or(&sel.path);
            let src = match sel.group {
                Group::Staged => tr!("git.diff.staged"),
                Group::Changes => tr!("git.diff.not_staged"),
                Group::Untracked => tr!("git.diff.new_file"),
                Group::Conflicts => tr!("git.diff.conflict"),
            };
            let g = kit::galley(
                ui,
                name,
                kit::semibold(ui, text::SMALL),
                col.text,
                (right - x - 70.0).max(40.0),
            );
            let end = kit::paint_galley(ui, g, x, rect, col.text);
            let g = kit::galley(
                ui,
                src,
                kit::regular(text::CAPTION),
                col.text3,
                (right - end - space::SM).max(10.0),
            );
            kit::paint_galley(ui, g, end + space::SM, rect, col.text3);
            if kit::icon_button(
                ui,
                kit::id("git-diff-close"),
                close,
                Icon::Close,
                tr!("git.diff.close"),
                col.text2,
            )
            .clicked()
            {
                st.selection = None;
                actions.push(PanelAction::Select(None));
            }
        });

    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let inner = rect.shrink2(Vec2::new(8.0, 2.0));
    ui.painter().rect(
        inner,
        CornerRadius::same(6),
        col.field,
        Stroke::new(1.0, col.separator),
        StrokeKind::Inside,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner.shrink(1.0)));
    let ui = &mut child;
    match diff {
        None => {
            ui.add_space(space::LG);
            ui.horizontal(|ui| {
                ui.add_space(space::LG);
                ui.add(egui::Spinner::new().size(12.0).color(col.text2));
                ui.label(
                    RichText::new(tr!("git.diff.loading"))
                        .size(text::SMALL)
                        .color(col.text2),
                );
            });
        }
        Some(Err(e)) => {
            ui.add_space(space::MD);
            ui.horizontal_wrapped(|ui| {
                ui.add_space(space::MD);
                ui.label(RichText::new(e).size(text::SMALL).color(col.danger));
            });
        }
        Some(Ok(d)) if d.binary => centered_note(ui, col, tr!("git.diff.binary")),
        Some(Ok(d)) if d.lines.is_empty() => centered_note(ui, col, tr!("git.diff.empty")),
        Some(Ok(d)) => diff_lines(ui, col, &d),
    }
}

fn centered_note(ui: &mut Ui, col: &Colors, msg: &str) {
    let rect = ui.max_rect();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        msg,
        egui::FontId::proportional(text::SMALL),
        col.text2,
    );
}

fn diff_lines(ui: &mut Ui, col: &Colors, d: &Diff) {
    let font = kit::mono(11.0);
    let line_h = DIFF_LINE_H;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    let max_no = d
        .lines
        .iter()
        .filter_map(|l| l.new.max(l.old))
        .max()
        .unwrap_or(1);
    let digits = max_no.to_string().len().max(2) as f32;
    let has_old = d.lines.iter().any(|l| l.old.is_some());
    let num_w = digits * char_w + 8.0;
    let gutter = if has_old { 2.0 * num_w } else { num_w } + 4.0;
    let max_chars = d
        .lines
        .iter()
        .map(|l| l.text.chars().count())
        .max()
        .unwrap_or(0) as f32;
    let content_w = (gutter + 16.0 + max_chars * char_w + 16.0).max(ui.available_width());
    let rows = d.lines.len() + usize::from(d.truncated);

    // `show_rows` sizes rows with the current item spacing: zero it first.
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both()
        .id_salt("git-diff")
        .auto_shrink([false, false])
        .show_rows(ui, line_h, rows, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(content_w, line_h), Sense::hover());
                let Some(line) = d.lines.get(i) else {
                    ui.painter().text(
                        egui::pos2(rect.left() + gutter + 16.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        tr!(
                            "git.diff.truncated",
                            n = manta_plugin::i18n::format_count(MAX_DIFF_LINES as u64)
                        ),
                        egui::FontId::proportional(text::CAPTION),
                        col.text2,
                    );
                    continue;
                };
                let (bg, marker, mark_col, text_col) = match line.kind {
                    LineKind::Add => (Some(col.add_bg), "+", col.success, col.text),
                    LineKind::Del => (Some(col.del_bg), "−", col.danger, col.text),
                    LineKind::Hunk => (Some(col.hunk_bg), "", col.text2, col.accent_text),
                    LineKind::Meta => (None, "", col.text3, col.text3),
                    LineKind::Context => (None, "", col.text3, col.text2),
                };
                if let Some(bg) = bg {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
                }
                let p = ui.painter();
                let cy = rect.center().y;
                if line.kind != LineKind::Hunk {
                    if has_old {
                        if let Some(o) = line.old {
                            p.text(
                                egui::pos2(rect.left() + num_w - 4.0, cy),
                                egui::Align2::RIGHT_CENTER,
                                o.to_string(),
                                font.clone(),
                                col.text3,
                            );
                        }
                    }
                    if let Some(n) = line.new {
                        let x = rect.left() + if has_old { 2.0 * num_w } else { num_w } - 4.0;
                        p.text(
                            egui::pos2(x, cy),
                            egui::Align2::RIGHT_CENTER,
                            n.to_string(),
                            font.clone(),
                            col.text3,
                        );
                    }
                    p.text(
                        egui::pos2(rect.left() + gutter + 2.0, cy),
                        egui::Align2::LEFT_CENTER,
                        marker,
                        font.clone(),
                        mark_col,
                    );
                    p.text(
                        egui::pos2(rect.left() + gutter + 14.0, cy),
                        egui::Align2::LEFT_CENTER,
                        &line.text,
                        font.clone(),
                        text_col,
                    );
                } else {
                    p.text(
                        egui::pos2(rect.left() + 8.0, cy),
                        egui::Align2::LEFT_CENTER,
                        &line.text,
                        font.clone(),
                        text_col,
                    );
                }
            }
        });
}

// ------------------------------------------------------------------ commits

fn commits(ui: &mut Ui, col: &Colors, commits: &[Commit]) {
    egui::Frame::NONE
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(tr!("git.commits.title").to_uppercase())
                        .font(kit::semibold(ui, 10.5))
                        .color(col.text2),
                );
            });
        });
    if commits.is_empty() {
        egui::Frame::NONE
            .inner_margin(Margin::symmetric(12, 4))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr!("git.commits.none"))
                        .size(text::SMALL)
                        .color(col.text3),
                );
            });
        return;
    }
    let now = kit::now_unix();
    egui::ScrollArea::vertical()
        .id_salt("git-log")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(Margin {
                    left: 6,
                    right: 6,
                    top: 0,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for c in commits {
                        commit_row(ui, col, c, now);
                    }
                });
        });
}

fn commit_row(ui: &mut Ui, col: &Colors, c: &Commit, now: i64) {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), col.hover);
    }
    let x = rect.left() + space::SM;
    let top = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + 21.0));
    let bottom = Rect::from_min_max(egui::pos2(rect.left(), rect.top() + 19.0), rect.max);
    let sha = ui
        .painter()
        .layout_no_wrap(c.short.clone(), kit::mono(11.0), col.accent_text);
    let sha_w = sha.size().x;
    kit::paint_galley(ui, sha, x, top, col.accent_text);
    let sx = x + sha_w.max(52.0) + space::SM;
    let g = kit::galley(
        ui,
        &c.subject,
        kit::regular(text::BODY),
        col.text,
        rect.right() - sx - space::SM,
    );
    kit::paint_galley(ui, g, sx, top, col.text);
    let meta = format!("{} · {}", c.author, kit::relative_time(now, c.time));
    let g = kit::galley(
        ui,
        &meta,
        kit::regular(text::CAPTION),
        col.text2,
        rect.right() - sx - space::SM,
    );
    kit::paint_galley(ui, g, sx, bottom, col.text2);

    resp.on_hover_ui(|ui| {
        ui.set_max_width(360.0);
        ui.label(RichText::new(&c.subject).font(kit::semibold(ui, text::BODY)));
        if !c.body.is_empty() {
            ui.add_space(space::XS);
            ui.label(RichText::new(&c.body).size(text::SMALL));
        }
        ui.add_space(space::XS);
        ui.label(
            RichText::new(format!("{} <{}>", c.author, c.email))
                .size(text::CAPTION)
                .weak(),
        );
        ui.label(
            RichText::new(format!("{} · {}", c.oid, kit::relative_time(now, c.time)))
                .font(kit::mono(10.5))
                .weak(),
        );
    });
}

// ------------------------------------------------------------------ empty states

fn empty_state(ui: &mut Ui, icon: Icon, title: &str, body: &str) {
    let col = Colors::of(ui);
    let rect = ui.available_rect_before_wrap();
    let width = (rect.width() - 2.0 * space::XL).clamp(120.0, 300.0);
    let inner = Rect::from_center_size(
        rect.center() - Vec2::new(0.0, 40.0),
        Vec2::new(width, 160.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 22.0, col.hover);
            kit::paint_icon(ui, r.shrink(11.0), icon, col.text2);
            ui.add_space(space::MD);
            ui.label(
                RichText::new(title)
                    .font(kit::semibold(ui, text::TITLE))
                    .color(col.text),
            );
            ui.add_space(space::XXS);
            ui.add(
                egui::Label::new(RichText::new(body).size(text::SMALL).color(col.text2))
                    .wrap()
                    .halign(egui::Align::Center),
            );
        },
    );
}

fn loading(ui: &mut Ui, msg: &str) {
    let col = Colors::of(ui);
    ui.add_space(space::XL);
    ui.horizontal(|ui| {
        ui.add_space(space::LG);
        ui.add(egui::Spinner::new().size(12.0).color(col.text2));
        ui.label(RichText::new(msg).size(text::SMALL).color(col.text2));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_height_never_orphans_a_header() {
        // Fits: full height (22+24*2 + 6+22+24).
        assert_eq!(
            changes_height(&[2, 1], 500.0),
            22.0 + 48.0 + 6.0 + 22.0 + 24.0
        );
        // Cut right after the second header would orphan it: cut before.
        let h = changes_height(&[2, 2, 2], 22.0 + 48.0 + 6.0 + 22.0 + 5.0);
        assert_eq!(h, 22.0 + 48.0);
        // Cut lands inside the second group's rows: after a full row.
        let h = changes_height(&[2, 2], 22.0 + 48.0 + 6.0 + 22.0 + 30.0);
        assert_eq!(h, 22.0 + 48.0 + 6.0 + 22.0 + 24.0);
        // Tiny space: still one header + one row.
        assert_eq!(changes_height(&[3], 10.0), 22.0 + 24.0);
    }

    #[test]
    fn display_paths() {
        assert_eq!(display_path("src/a.rs", ""), "src/a.rs");
        assert_eq!(display_path("src/a.rs", "src"), "a.rs");
        assert_eq!(display_path("srcx/a.rs", "src"), "srcx/a.rs");
        assert_eq!(display_path("docs/x.md", "src"), "docs/x.md");
    }
}
