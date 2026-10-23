//! Floating right prompts.
//!
//! zsh right-aligns RPROMPT only on the *current* prompt line. Once a line
//! is history, its RPROMPT is ordinary cells: when the panel gets wider it
//! stays at the old column, and when it gets narrower reflow wraps or splits
//! it into fragments. Every terminal with reflow does this.
//!
//! We do better: the zsh shim wraps RPROMPT in zero-width private OSC
//! markers ([`crate::osc::RPROMPT_START`]/[`crate::osc::RPROMPT_END`]), so
//! we know exactly which cells are the right prompt. When the prompt line
//! becomes history (the command starts, or a new prompt is drawn), those
//! cells are removed from the grid and kept here. The line is tagged with
//! an invisible marker (a private-use zero-width char on its first cell),
//! which travels with the line through scrolling and reflow and disappears
//! when the line is overwritten or cleared. The renderer draws each tagged
//! line's right prompt right-aligned at the current width, on the last row
//! of the (possibly wrapped) line, and hides it if it does not fit.

use std::collections::{HashMap, VecDeque};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::Term;

/// Supplementary Private Use Area A and B: 2 × 65534 ids.
const PLANE_A: u32 = 0xF0000;
const PLANE_B: u32 = 0x100000;
const PER_PLANE: u32 = 0xFFFE;
const MAX_IDS: u32 = 2 * PER_PLANE;

/// Marker char for a line id.
pub fn marker_char(id: u32) -> char {
    let id = id % MAX_IDS;
    let cp = if id < PER_PLANE {
        PLANE_A + id
    } else {
        PLANE_B + id - PER_PLANE
    };
    char::from_u32(cp).unwrap_or('\u{F0000}')
}

/// Line id of a marker char.
pub fn marker_id(c: char) -> Option<u32> {
    let cp = c as u32;
    if (PLANE_A..PLANE_A + PER_PLANE).contains(&cp) {
        Some(cp - PLANE_A)
    } else if (PLANE_B..PLANE_B + PER_PLANE).contains(&cp) {
        Some(cp - PLANE_B + PER_PLANE)
    } else {
        None
    }
}

/// The marker on a line's first cell, if any (the last one wins).
pub fn line_marker(cell: &Cell) -> Option<u32> {
    cell.zerowidth()?.iter().rev().find_map(|&c| marker_id(c))
}

/// Remove marker chars from copied text.
pub fn strip_markers(s: &str) -> String {
    s.chars().filter(|&c| marker_id(c).is_none()).collect()
}

/// A right prompt taken out of the grid.
#[derive(Debug, Clone)]
pub struct Float {
    pub cells: Vec<Cell>,
    /// Blank columns zsh keeps right of RPROMPT (ZLE_RPROMPT_INDENT).
    pub indent: usize,
}

/// The right prompt of the current (live) prompt line, still in the grid.
#[derive(Debug, Clone)]
pub struct Live {
    pub id: u32,
    pub start: usize,
    pub end: usize,
    pub cells: Vec<Cell>,
    pub indent: usize,
}

#[derive(Debug, Default)]
pub struct Floats {
    map: HashMap<u32, Float>,
    order: VecDeque<u32>,
    next: u32,
    /// Max number of floats kept (≈ scrollback lines).
    cap: usize,
}

impl Floats {
    pub fn new(cap: usize) -> Self {
        Floats {
            cap: cap.clamp(100, (MAX_IDS - 1) as usize),
            ..Default::default()
        }
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.clamp(100, (MAX_IDS - 1) as usize);
        self.prune();
    }

    pub fn alloc(&mut self) -> u32 {
        let id = self.next;
        self.next = (self.next + 1) % MAX_IDS;
        // A recycled id must not show an ancient float.
        self.map.remove(&id);
        id
    }

    pub fn insert(&mut self, id: u32, float: Float) {
        if self.map.insert(id, float).is_none() {
            self.order.push_back(id);
        }
        self.prune();
    }

    fn prune(&mut self) {
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    pub fn get(&self, id: u32) -> Option<&Float> {
        self.map.get(&id)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Where a float is drawn on a visible row: its first column.
pub struct Overlay<'a> {
    pub start: usize,
    pub float: &'a Float,
}

/// Last column with visible content on a viewport row, if any.
fn last_used_col<T>(term: &Term<T>, line: Line) -> Option<usize> {
    let cols = term.grid().columns();
    (0..cols).rev().find(|&c| {
        let cell = &term.grid()[Point::new(line, Column(c))];
        !(cell.c == ' ' || cell.c == '\0')
            || cell
                .flags
                .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES)
            || cell.bg != Cell::default().bg
    })
}

/// For each visible row, the float to draw on it (right-aligned at the
/// current width), or `None`.
pub fn overlays<'a, T>(term: &Term<T>, floats: &'a Floats) -> Vec<Option<Overlay<'a>>> {
    let grid = term.grid();
    let rows = grid.screen_lines();
    let cols = grid.columns();
    let offset = grid.display_offset() as i32;
    let mut out: Vec<Option<Overlay<'a>>> = (0..rows).map(|_| None).collect();
    if floats.is_empty() {
        return out;
    }
    // Lines that start above the viewport but wrap into it.
    let first = Line(-offset);
    let mut start_line = first;
    while start_line > grid.topmost_line()
        && grid[Point::new(start_line - 1i32, Column(cols - 1))]
            .flags
            .contains(Flags::WRAPLINE)
    {
        start_line -= 1i32;
    }
    let bottom = Line(rows as i32 - 1 - offset);
    let mut line = start_line;
    while line <= bottom {
        // Follow wraps to the last row of this logical line.
        let mut last = line;
        while last < grid.bottommost_line()
            && grid[Point::new(last, Column(cols - 1))]
                .flags
                .contains(Flags::WRAPLINE)
        {
            last += 1i32;
        }
        if let Some(float) =
            line_marker(&grid[Point::new(line, Column(0))]).and_then(|id| floats.get(id))
        {
            let row = last.0 + offset;
            let width = float.cells.len();
            if row >= 0 && (row as usize) < rows && width + float.indent < cols {
                let start = cols - float.indent - width;
                // Hide rather than overlap the left part (keep one space).
                let fits = last_used_col(term, last).is_none_or(|c| c + 1 < start);
                if fits {
                    out[row as usize] = Some(Overlay { start, float });
                }
            }
        }
        line = last + 1i32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_roundtrip() {
        for id in [0, 1, PER_PLANE - 1, PER_PLANE, MAX_IDS - 1] {
            assert_eq!(marker_id(marker_char(id)), Some(id), "{id}");
        }
        assert_eq!(marker_id('a'), None);
        assert_eq!(marker_id('\u{E000}'), None);
        let s = format!("a{}b", marker_char(7));
        assert_eq!(strip_markers(&s), "ab");
    }

    #[test]
    fn floats_cap_and_recycle() {
        let mut f = Floats::new(100);
        for _ in 0..150 {
            let id = f.alloc();
            f.insert(
                id,
                Float {
                    cells: vec![],
                    indent: 1,
                },
            );
        }
        assert_eq!(f.len(), 100);
        assert!(f.get(0).is_none());
        assert!(f.get(149).is_some());
    }
}
