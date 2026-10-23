//! Layout logic for any number of file panes (at least one): ids, widths,
//! the "most recently active" order that picks the copy / compare target,
//! and the saved session. Pure functions, so they are easy to test; the
//! app (`app.rs`) and the pane UI (`pane.rs`) use them.

use std::path::{Path, PathBuf};

use manta_plugin::PaneId;
use serde::{Deserialize, Serialize};

/// The narrowest a pane may get. A new pane is refused when the window
/// can't give every pane this much.
pub const MIN_PANE_W: f32 = 280.0;

/// The smallest id not in use (ids of closed panes are reused, so with two
/// panes they are always 0 and 1 — "left" and "right" for plugins).
pub fn next_free_id(ids: &[PaneId]) -> PaneId {
    (0..=u16::MAX)
        .map(PaneId)
        .find(|id| !ids.contains(id))
        .unwrap_or(PaneId(u16::MAX))
}

/// Move `id` to the front of the most-recently-active list.
pub fn touch_mru(mru: &mut Vec<PaneId>, id: PaneId) {
    mru.retain(|x| *x != id);
    mru.insert(0, id);
}

/// The default target of copy / move / compare from `active`: the most
/// recently active other pane, else its right neighbour, else the left
/// one. `None` with a single pane.
pub fn target_of(active: PaneId, mru: &[PaneId], order: &[PaneId]) -> Option<PaneId> {
    if let Some(t) = mru
        .iter()
        .copied()
        .find(|p| *p != active && order.contains(p))
    {
        return Some(t);
    }
    let i = order.iter().position(|p| *p == active)?;
    order
        .get(i + 1)
        .or_else(|| i.checked_sub(1).and_then(|j| order.get(j)))
        .copied()
}

/// The pane after (`step` = 1) or before (`-1`) `active`, wrapping around.
pub fn cycle(order: &[PaneId], active: PaneId, step: isize) -> PaneId {
    let n = order.len() as isize;
    if n == 0 {
        return active;
    }
    let i = order.iter().position(|p| *p == active).unwrap_or(0) as isize;
    order[((i + step) % n + n) as usize % n as usize]
}

/// The neighbour of `active` in direction `step` (no wrapping; `None` at
/// the edge).
pub fn neighbour(order: &[PaneId], active: PaneId, step: isize) -> Option<PaneId> {
    let i = order.iter().position(|p| *p == active)? as isize + step;
    (i >= 0).then(|| order.get(i as usize).copied()).flatten()
}

/// Scale weights so they sum to 1 (equal shares if they are unusable).
pub fn normalize(weights: &mut [f32]) {
    let sum: f32 = weights.iter().filter(|w| w.is_finite() && **w > 0.0).sum();
    if weights.is_empty() {
        return;
    }
    if sum <= f32::EPSILON || weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
        let eq = 1.0 / weights.len() as f32;
        weights.iter_mut().for_each(|w| *w = eq);
        return;
    }
    weights.iter_mut().for_each(|w| *w /= sum);
}

/// Split pane `at`: it and the new pane right of it share its width.
pub fn split_weights(weights: &mut Vec<f32>, at: usize) {
    let at = at.min(weights.len().saturating_sub(1));
    match weights.get_mut(at) {
        Some(w) => {
            *w /= 2.0;
            let half = *w;
            weights.insert(at + 1, half);
        }
        None => weights.push(1.0),
    }
    normalize(weights);
}

/// Remove pane `at`; its width goes to the left neighbour (or the right
/// one for the first pane).
pub fn remove_weight(weights: &mut Vec<f32>, at: usize) {
    if at >= weights.len() {
        return;
    }
    let w = weights.remove(at);
    if let Some(n) = weights.get_mut(at.saturating_sub(1)) {
        *n += w;
    }
    normalize(weights);
}

/// Pixel widths of the panes for `total` points (gaps already taken off),
/// honouring `min` where the space allows.
pub fn widths(weights: &[f32], total: f32, min: f32) -> Vec<f32> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let mut w: Vec<f32> = weights.iter().map(|x| x * total).collect();
    let min = min.min(total / n as f32);
    // Lift panes below the minimum, taking the difference from the wider
    // ones in proportion (a few rounds settle it).
    for _ in 0..n {
        let deficit: f32 = w.iter().filter(|x| **x < min).map(|x| min - x).sum();
        if deficit <= 0.01 {
            break;
        }
        let spare: f32 = w.iter().filter(|x| **x > min).map(|x| x - min).sum();
        if spare <= 0.0 {
            break;
        }
        let k = (deficit / spare).min(1.0);
        for x in &mut w {
            if *x < min {
                *x = min;
            } else {
                *x -= (*x - min) * k;
            }
        }
    }
    w
}

/// Drag the divider after pane `i` by `dx` points (`total` = width of all
/// panes without gaps). Only the two neighbours change.
pub fn drag_divider(weights: &mut [f32], i: usize, dx: f32, total: f32, min: f32) {
    if i + 1 >= weights.len() || total <= 0.0 {
        return;
    }
    let cur = widths(weights, total, min);
    let pair = cur[i] + cur[i + 1];
    let min = min.min(pair / 2.0);
    let left = (cur[i] + dx).clamp(min, pair - min);
    let mut px = cur;
    px[i] = left;
    px[i + 1] = pair - left;
    for (w, p) in weights.iter_mut().zip(px) {
        *w = p / total;
    }
    normalize(weights);
}

/// Equal widths (double-click on a divider).
pub fn equalize(weights: &mut [f32]) {
    let n = weights.len().max(1) as f32;
    weights.iter_mut().for_each(|w| *w = 1.0 / n);
}

/// Whether `n` panes plus one more fit into `area` points with `gap`
/// between neighbours.
pub fn can_add(area: f32, n: usize, gap: f32) -> bool {
    let m = (n + 1) as f32;
    m * MIN_PANE_W + (m - 1.0) * gap <= area + 0.5
}

// ------------------------------------------------------------------ session

/// The panes as they were when the app quit (`config_dir/Manta/session.json`).
///
/// Only local folders are kept: servers, containers and archives need a
/// connection, so their tabs aren't restored (favorites keep servers).
/// Manta 0.1.x had two fixed panes and saved no session; a missing file
/// gives the classic two panes in the home folder (the plugins' saved
/// panel state uses the same keys, "left" / "right", for panes 0 and 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub panes: Vec<SavedPane>,
    /// The focused pane.
    #[serde(default)]
    pub active: Option<PaneId>,
    /// Most recently active first.
    #[serde(default)]
    pub mru: Vec<PaneId>,
}

fn version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedPane {
    pub id: PaneId,
    /// Share of the pane area.
    #[serde(default = "one")]
    pub width: f32,
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active_tab: usize,
    /// Icons, List, Columns or Gallery.
    #[serde(default)]
    pub view: crate::views::ViewMode,
    /// Icon size of the Icons view (points).
    #[serde(default)]
    pub icon_size: Option<f32>,
    /// Widths of the Columns view's columns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub column_widths: Vec<f32>,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    /// A local folder.
    pub path: String,
    #[serde(default)]
    pub show_hidden: bool,
}

impl Session {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Manta").join("session.json"))
    }

    /// The saved session, cleaned up (unique ids, no tabless panes, valid
    /// widths and indices). `None` if there is none (first launch, or the
    /// two-pane version before it) or it can't be read.
    pub fn load(path: &Path) -> Option<Session> {
        let text = std::fs::read_to_string(path).ok()?;
        let s: Session = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("session: ignoring {}: {e}", path.display());
                return None;
            }
        };
        s.sanitized()
    }

    /// Drop what can't be restored; `None` if no pane is left.
    pub fn sanitized(mut self) -> Option<Session> {
        let mut seen = Vec::new();
        self.panes.retain(|p| {
            let fresh = !seen.contains(&p.id) && !p.tabs.is_empty();
            seen.push(p.id);
            fresh
        });
        if self.panes.is_empty() {
            return None;
        }
        for p in &mut self.panes {
            p.active_tab = p.active_tab.min(p.tabs.len() - 1);
        }
        let mut w: Vec<f32> = self.panes.iter().map(|p| p.width).collect();
        normalize(&mut w);
        for (p, w) in self.panes.iter_mut().zip(w) {
            p.width = w;
        }
        let ids: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        self.mru.retain(|p| ids.contains(p));
        self.mru.dedup();
        if self.active.is_none_or(|a| !ids.contains(&a)) {
            self.active = Some(ids[0]);
        }
        Some(self)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write next to it, then rename: a crash never leaves half a file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: PaneId = PaneId::LEFT;
    const R: PaneId = PaneId::RIGHT;

    fn ids(n: u16) -> Vec<PaneId> {
        (0..n).map(PaneId).collect()
    }

    #[test]
    fn ids_are_reused_smallest_first() {
        assert_eq!(next_free_id(&[]), L);
        assert_eq!(next_free_id(&[L]), R);
        assert_eq!(next_free_id(&[R, PaneId(2)]), L);
        assert_eq!(next_free_id(&[L, R, PaneId(3)]), PaneId(2));
    }

    #[test]
    fn mru_target_with_one_two_and_four_panes() {
        // One pane: no target.
        assert_eq!(target_of(L, &[L], &[L]), None);
        // Two panes: always the other one.
        assert_eq!(target_of(L, &[L, R], &ids(2)), Some(R));
        assert_eq!(target_of(R, &[R, L], &ids(2)), Some(L));
        assert_eq!(
            target_of(R, &[], &ids(2)),
            Some(L),
            "no history: the neighbour"
        );
        // Four panes: the most recently active other one…
        let order = ids(4);
        let mut mru = Vec::new();
        for p in [L, PaneId(3), R] {
            touch_mru(&mut mru, p);
        }
        assert_eq!(mru, vec![R, PaneId(3), L]);
        assert_eq!(target_of(R, &mru, &order), Some(PaneId(3)));
        touch_mru(&mut mru, PaneId(2));
        assert_eq!(target_of(PaneId(2), &mru, &order), Some(R));
        // …skipping closed panes; with no history the right neighbour, at
        // the right edge the left one.
        assert_eq!(
            target_of(PaneId(2), &[PaneId(2), PaneId(9)], &order),
            Some(PaneId(3))
        );
        assert_eq!(target_of(PaneId(3), &[], &order), Some(PaneId(2)));
    }

    #[test]
    fn focus_cycles_and_steps() {
        let order = vec![L, PaneId(2), R];
        assert_eq!(cycle(&order, L, 1), PaneId(2));
        assert_eq!(cycle(&order, R, 1), L, "Tab wraps");
        assert_eq!(cycle(&order, L, -1), R);
        assert_eq!(cycle(&[L], L, 1), L);
        assert_eq!(neighbour(&order, PaneId(2), 1), Some(R));
        assert_eq!(neighbour(&order, PaneId(2), -1), Some(L));
        assert_eq!(neighbour(&order, R, 1), None, "⌥⌘→ stops at the edge");
        assert_eq!(neighbour(&order, L, -1), None);
    }

    #[test]
    fn split_remove_and_widths() {
        let mut w = vec![1.0];
        split_weights(&mut w, 0);
        assert_eq!(w, vec![0.5, 0.5]);
        split_weights(&mut w, 1);
        assert_eq!(w, vec![0.5, 0.25, 0.25]);
        remove_weight(&mut w, 1);
        assert_eq!(w, vec![0.75, 0.25], "the left neighbour takes the width");
        remove_weight(&mut w, 0);
        assert_eq!(w, vec![1.0]);
        remove_weight(&mut w, 5);
        assert_eq!(w, vec![1.0]);
        let mut bad = vec![0.0, f32::NAN, 2.0];
        normalize(&mut bad);
        assert!(bad.iter().all(|x| (x - 1.0 / 3.0).abs() < 1e-6));
        // The minimum is kept while there is room.
        let px = widths(&[0.9, 0.1], 1000.0, 280.0);
        assert!(
            (px[1] - 280.0).abs() < 0.5 && (px[0] + px[1] - 1000.0).abs() < 0.5,
            "{px:?}"
        );
        let px = widths(&[0.5, 0.5], 400.0, 280.0);
        assert_eq!(px, vec![200.0, 200.0], "too narrow: equal shares");
    }

    #[test]
    fn dividers_drag_and_reset() {
        let mut w = vec![1.0 / 3.0; 3];
        drag_divider(&mut w, 0, 100.0, 1200.0, 280.0);
        assert!((w[0] * 1200.0 - 500.0).abs() < 0.5, "{w:?}");
        assert!((w[1] * 1200.0 - 300.0).abs() < 0.5);
        assert!(
            (w[2] * 1200.0 - 400.0).abs() < 0.5,
            "the third pane keeps its width"
        );
        drag_divider(&mut w, 0, 1000.0, 1200.0, 280.0);
        assert!(
            (w[1] * 1200.0 - 280.0).abs() < 0.5,
            "clamped at the minimum: {w:?}"
        );
        drag_divider(&mut w, 2, 50.0, 1200.0, 280.0);
        equalize(&mut w);
        assert_eq!(w, vec![1.0 / 3.0; 3]);
    }

    #[test]
    fn adding_needs_room() {
        assert!(can_add(1000.0, 2, 8.0), "3 × 280 + 2 × 8 = 856");
        assert!(!can_add(850.0, 2, 8.0));
        assert!(can_add(MIN_PANE_W * 2.0 + 8.0, 1, 8.0));
        assert!(!can_add(500.0, 1, 8.0));
    }

    #[test]
    fn session_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        assert_eq!(Session::load(&path), None, "no file: the classic two panes");
        let s = Session {
            version: 1,
            panes: vec![
                SavedPane {
                    id: R,
                    width: 0.25,
                    tabs: vec![SavedTab {
                        path: "/tmp".into(),
                        show_hidden: true,
                    }],
                    active_tab: 0,
                    view: crate::views::ViewMode::Gallery,
                    icon_size: Some(128.0),
                    column_widths: vec![240.0],
                },
                SavedPane {
                    id: PaneId(2),
                    width: 0.75,
                    tabs: vec![
                        SavedTab {
                            path: "/a".into(),
                            show_hidden: false,
                        },
                        SavedTab {
                            path: "/b".into(),
                            show_hidden: false,
                        },
                    ],
                    active_tab: 1,
                    view: Default::default(),
                    icon_size: None,
                    column_widths: Vec::new(),
                },
            ],
            active: Some(PaneId(2)),
            mru: vec![PaneId(2), R],
        };
        s.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"id\": \"right\"") && text.contains("\"id\": 2"),
            "{text}"
        );
        assert_eq!(Session::load(&path), Some(s));
    }

    #[test]
    fn broken_sessions_are_repaired_or_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Session::load(&path), None);
        std::fs::write(&path, r#"{"panes": []}"#).unwrap();
        assert_eq!(Session::load(&path), None);
        std::fs::write(
            &path,
            r#"{"panes": [
                {"id": "left", "width": -3, "tabs": [{"path": "/x"}], "active_tab": 7},
                {"id": "left", "tabs": [{"path": "/dup"}]},
                {"id": 4, "tabs": []},
                {"id": 1, "width": 1, "tabs": [{"path": "/y"}]}
            ], "active": 9, "mru": [4, 1, 1, 0]}"#,
        )
        .unwrap();
        let s = Session::load(&path).unwrap();
        let ids: Vec<PaneId> = s.panes.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![L, R], "duplicates and empty panes dropped");
        assert_eq!(s.panes[0].active_tab, 0);
        assert_eq!(s.panes[0].width, 0.5, "unusable widths become equal shares");
        assert_eq!(s.active, Some(L));
        assert_eq!(s.mru, vec![R, L]);
    }
}
