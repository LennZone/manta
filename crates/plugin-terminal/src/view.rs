//! Drawing the emulator grid with an egui painter, and turning egui input
//! into PTY bytes.

use std::collections::HashMap;

use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use crate::keys::{self, KeyModes};
use crate::palette::{mix, Theme};
use crate::rprompt::{self, Floats, Overlay};
use crate::session::{GridSize, Listener};

/// Inner padding between the panel edge and the cell grid.
pub const PAD: Vec2 = Vec2::new(8.0, 6.0);

/// Cell metrics for one font size at one pixel density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Painted cell width. Equals `advance` until [`Metrics::fit`] stretches
    /// it so the grid fills the panel exactly.
    pub cell_w: f32,
    /// The font's glyph advance (decides the column count).
    pub advance: f32,
    pub cell_h: f32,
    /// Height of a text row of the font (≤ cell_h).
    pub row_h: f32,
    pub ppp: f32,
}

impl Metrics {
    pub fn grid_size(&self, rect: Rect) -> GridSize {
        let inner = rect.shrink2(PAD);
        let cols = (inner.width() / self.advance).floor().max(2.0) as u16;
        let rows = (inner.height() / self.cell_h).floor().max(1.0) as u16;
        GridSize { cols, rows }
    }

    /// Metrics for painting `cols` columns in `rect`: when `cols` is the
    /// column count for this width, cells are widened by the leftover
    /// fraction of a cell so the last column ends exactly at the right
    /// content edge (rect − PAD) at every width; the difference is applied
    /// as letter spacing (at most `advance / cols`, invisible). Otherwise
    /// (a resize is still being debounced) cells keep the font advance.
    pub fn fit(&self, rect: Rect, cols: usize) -> Metrics {
        let inner = rect.width() - 2.0 * PAD.x;
        let settled = cols > 0 && self.grid_size(rect).cols as usize == cols;
        let cell_w = if settled {
            inner / cols as f32
        } else {
            self.advance
        };
        Metrics { cell_w, ..*self }
    }

    /// Extra space after each glyph so text runs stay on the cell grid.
    pub fn letter_spacing(&self) -> f32 {
        (self.cell_w - self.advance).max(0.0)
    }

    pub fn cell_px(&self) -> (u16, u16) {
        (
            (self.cell_w * self.ppp).round() as u16,
            (self.cell_h * self.ppp).round() as u16,
        )
    }

    /// Top-left of the grid inside `rect`, snapped to physical pixels.
    pub fn origin(&self, rect: Rect) -> Pos2 {
        let p = rect.min + PAD;
        Pos2::new(
            (p.x * self.ppp).round() / self.ppp,
            (p.y * self.ppp).round() / self.ppp,
        )
    }
}

/// Per-panel view state that survives across frames.
#[derive(Default)]
pub struct ViewState {
    metrics_key: Option<(u32, u32)>,
    metrics: Option<Metrics>,
    /// char → glyph advance equals the cell width (safe to batch).
    regular: HashMap<char, bool>,
    /// Time (egui seconds) of the last keystroke/output, restarts blinking.
    pub blink_epoch: f64,
    scroll_accum: f32,
    skip_text: bool,
    mouse_button_down: Option<u8>,
    bold_checked: Option<(u32, u32)>,
    /// Output counter seen last frame (blinking restarts on output).
    pub last_output_seq: u64,
    bold_ok: bool,
    last_report_cell: Option<(usize, usize)>,
}

impl ViewState {
    pub fn metrics(&mut self, ctx: &egui::Context, font: &FontId) -> Metrics {
        let ppp = ctx.pixels_per_point();
        let key = (font.size.to_bits(), ppp.to_bits());
        if self.metrics_key != Some(key) || self.metrics.is_none() {
            let (w, h) = ctx.fonts_mut(|f| (f.glyph_width(font, 'M'), f.row_height(font)));
            let row_h = h;
            // Whole physical pixels per row keep every baseline crisp; a
            // little extra leading reads better than the font's tight box.
            let cell_h = ((h * 1.12) * ppp).round() / ppp;
            self.metrics = Some(Metrics {
                cell_w: w,
                advance: w,
                cell_h,
                row_h,
                ppp,
            });
            self.metrics_key = Some(key);
            self.regular.clear();
        }
        self.metrics.unwrap_or(Metrics {
            cell_w: 8.0,
            advance: 8.0,
            cell_h: 16.0,
            row_h: 14.0,
            ppp,
        })
    }

    /// `bold` if it is installed and has exactly the regular advance.
    pub fn usable_bold(
        &mut self,
        ctx: &egui::Context,
        regular: &FontId,
        bold: &FontId,
    ) -> Option<FontId> {
        let installed = ctx.fonts(|f| f.definitions().families.contains_key(&bold.family));
        if !installed {
            return None;
        }
        let key = (regular.size.to_bits(), ctx.pixels_per_point().to_bits());
        if self.bold_checked != Some(key) {
            let (a, b) = ctx.fonts_mut(|f| (f.glyph_width(regular, 'M'), f.glyph_width(bold, 'M')));
            self.bold_ok = (a - b).abs() < 0.01;
            self.bold_checked = Some(key);
        }
        self.bold_ok.then(|| bold.clone())
    }

    fn is_regular(&mut self, ctx: &egui::Context, font: &FontId, cell_w: f32, c: char) -> bool {
        if c.is_ascii_graphic() || c == ' ' {
            // ASCII is in every monospace font; skip the lookup.
            return true;
        }
        *self.regular.entry(c).or_insert_with(|| {
            let w = ctx.fonts_mut(|f| f.glyph_width(font, c));
            (w - cell_w).abs() < 0.01
        })
    }
}

/// Everything that decides how one frame looks.
pub struct PaintOptions {
    pub theme: Theme,
    pub font: FontId,
    /// A real bold face with the same advance as `font` (ASCII only), if
    /// installed; otherwise bold is drawn twice with a 1 px offset.
    pub bold_font: Option<FontId>,
    pub focused: bool,
    /// Cursor visible in the blink cycle.
    pub blink_on: bool,
    /// Draw the whole grid dimmed (remote pane).
    pub dimmed: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Color32,
    italic: bool,
    underline: bool,
    strike: bool,
}

fn resolve(color: AnsiColor, colors: &Colors, theme: &Theme, bold: bool) -> Color32 {
    let pick = |idx: usize, fallback: Color32| -> Color32 {
        match colors[idx] {
            Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
            None => fallback,
        }
    };
    match color {
        AnsiColor::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => {
            let i = if bold && i < 8 { i + 8 } else { i };
            pick(i as usize, theme.indexed(i))
        }
        AnsiColor::Named(n) => {
            let idx = n as usize;
            if idx < 16 {
                let idx = if bold && idx < 8 { idx + 8 } else { idx };
                return pick(idx, theme.ansi[idx]);
            }
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground => {
                    pick(NamedColor::Foreground as usize, theme.fg)
                }
                NamedColor::Background => pick(NamedColor::Background as usize, theme.bg),
                NamedColor::Cursor => pick(NamedColor::Cursor as usize, theme.cursor),
                NamedColor::DimForeground => mix(theme.fg, theme.bg, 0.35),
                other => {
                    // Dim variants of the 8 base colours.
                    let base = (other as usize)
                        .saturating_sub(NamedColor::DimBlack as usize)
                        .min(7);
                    mix(theme.ansi[base], theme.bg, 0.35)
                }
            }
        }
    }
}

/// Colours of a cell after inverse/dim/hidden.
fn cell_colors(cell: &Cell, colors: &Colors, theme: &Theme) -> (Color32, Color32) {
    let bold = cell.flags.contains(Flags::BOLD);
    let mut fg = resolve(cell.fg, colors, theme, bold);
    let default_bg = colors[NamedColor::Background as usize]
        .map(|c| Color32::from_rgb(c.r, c.g, c.b))
        .unwrap_or(theme.bg);
    let mut bg = match cell.bg {
        AnsiColor::Named(NamedColor::Background) => default_bg,
        c => resolve(c, colors, theme, false),
    };
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags.contains(Flags::DIM) {
        fg = mix(fg, bg, 0.4);
    }
    if cell.flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    (fg, bg)
}

/// Characters drawn by [`draw_box_char`] instead of the font.
fn is_box_char(c: char) -> bool {
    matches!(
        c,
        '─' | '━'
            | '│'
            | '┃'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '╭'
            | '╮'
            | '╯'
            | '╰'
            | '┏'
            | '┓'
            | '┗'
            | '┛'
            | '┣'
            | '┫'
            | '┳'
            | '┻'
            | '╋'
            | '═'
            | '║'
            | '█'
            | '▀'
            | '▄'
            | '▌'
            | '▐'
            | '▁'
            | '▂'
            | '▃'
            | '▅'
            | '▆'
            | '▇'
            | '░'
            | '▒'
            | '▓'
    )
}

/// Lines and blocks drawn as rectangles so borders join seamlessly whatever
/// the font has. Returns false for characters it does not handle.
fn draw_box_char(p: &Painter, r: Rect, c: char, color: Color32, ppp: f32) -> bool {
    let snap = |v: f32| (v * ppp).round() / ppp;
    let cx = snap(r.center().x);
    let cy = snap(r.center().y);
    let thin = (1.0 / ppp).max(1.0);
    let heavy = thin * 2.0;
    // (left, right, up, down) arm weights: 0 none, 1 light, 2 heavy, 3 double
    let arms: Option<[u8; 4]> = match c {
        '─' => Some([1, 1, 0, 0]),
        '━' => Some([2, 2, 0, 0]),
        '│' => Some([0, 0, 1, 1]),
        '┃' => Some([0, 0, 2, 2]),
        '┌' | '╭' => Some([0, 1, 0, 1]),
        '┐' | '╮' => Some([1, 0, 0, 1]),
        '└' | '╰' => Some([0, 1, 1, 0]),
        '┘' | '╯' => Some([1, 0, 1, 0]),
        '├' => Some([0, 1, 1, 1]),
        '┤' => Some([1, 0, 1, 1]),
        '┬' => Some([1, 1, 0, 1]),
        '┴' => Some([1, 1, 1, 0]),
        '┼' => Some([1, 1, 1, 1]),
        '┏' => Some([0, 2, 0, 2]),
        '┓' => Some([2, 0, 0, 2]),
        '┗' => Some([0, 2, 2, 0]),
        '┛' => Some([2, 0, 2, 0]),
        '┣' => Some([0, 2, 2, 2]),
        '┫' => Some([2, 0, 2, 2]),
        '┳' => Some([2, 2, 0, 2]),
        '┻' => Some([2, 2, 2, 0]),
        '╋' => Some([2, 2, 2, 2]),
        '═' => Some([3, 3, 0, 0]),
        '║' => Some([0, 0, 3, 3]),
        _ => None,
    };
    if let Some([l, rt, u, d]) = arms {
        let w = |k: u8| if k == 2 { heavy } else { thin };
        let gap = snap(thin * 2.0);
        // horizontal arms
        for (k, x0, x1) in [(l, r.left(), cx), (rt, cx, r.right())] {
            if k == 0 {
                continue;
            }
            let (x0, x1) = (x0.min(x1), x0.max(x1));
            let ext = if u > 0 || d > 0 {
                w(u.max(d)) / 2.0
            } else {
                0.0
            };
            let (x0, x1) = (
                if x0 < cx { x0 } else { x0 - ext },
                if x1 > cx { x1 } else { x1 + ext },
            );
            if k == 3 {
                for dy in [-gap, gap] {
                    p.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(x0, cy + dy - thin / 2.0),
                            Pos2::new(x1, cy + dy + thin / 2.0),
                        ),
                        0.0,
                        color,
                    );
                }
            } else {
                let t = w(k);
                p.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, cy - t / 2.0), Pos2::new(x1, cy + t / 2.0)),
                    0.0,
                    color,
                );
            }
        }
        for (k, y0, y1) in [(u, r.top(), cy), (d, cy, r.bottom())] {
            if k == 0 {
                continue;
            }
            let ext = if l > 0 || rt > 0 {
                w(l.max(rt)) / 2.0
            } else {
                0.0
            };
            let (y0, y1) = (
                if y0 < cy { y0 } else { y0 - ext },
                if y1 > cy { y1 } else { y1 + ext },
            );
            if k == 3 {
                for dx in [-gap, gap] {
                    p.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(cx + dx - thin / 2.0, y0),
                            Pos2::new(cx + dx + thin / 2.0, y1),
                        ),
                        0.0,
                        color,
                    );
                }
            } else {
                let t = w(k);
                p.rect_filled(
                    Rect::from_min_max(Pos2::new(cx - t / 2.0, y0), Pos2::new(cx + t / 2.0, y1)),
                    0.0,
                    color,
                );
            }
        }
        return true;
    }
    let block = |x0: f32, y0: f32, x1: f32, y1: f32, col: Color32| {
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(
                    snap(r.left() + r.width() * x0),
                    snap(r.top() + r.height() * y0),
                ),
                Pos2::new(
                    snap(r.left() + r.width() * x1),
                    snap(r.top() + r.height() * y1),
                ),
            ),
            0.0,
            col,
        );
    };
    match c {
        '█' => block(0.0, 0.0, 1.0, 1.0, color),
        '▀' => block(0.0, 0.0, 1.0, 0.5, color),
        '▄' => block(0.0, 0.5, 1.0, 1.0, color),
        '▌' => block(0.0, 0.0, 0.5, 1.0, color),
        '▐' => block(0.5, 0.0, 1.0, 1.0, color),
        '▁' => block(0.0, 0.875, 1.0, 1.0, color),
        '▂' => block(0.0, 0.75, 1.0, 1.0, color),
        '▃' => block(0.0, 0.625, 1.0, 1.0, color),
        '▅' => block(0.0, 0.375, 1.0, 1.0, color),
        '▆' => block(0.0, 0.25, 1.0, 1.0, color),
        '▇' => block(0.0, 0.125, 1.0, 1.0, color),
        '░' => block(0.0, 0.0, 1.0, 1.0, color.gamma_multiply(0.25)),
        '▒' => block(0.0, 0.0, 1.0, 1.0, color.gamma_multiply(0.5)),
        '▓' => block(0.0, 0.0, 1.0, 1.0, color.gamma_multiply(0.75)),
        _ => return false,
    }
    true
}

/// A run of batchable cells sharing boldness.
struct Segment {
    col: usize,
    bold: bool,
    job: LayoutJob,
}

/// Paint the visible grid of `term` with its top-left cell at `origin`.
/// Cell to draw at (`row`, `col`): a floating right prompt cell if one is
/// placed there, else the grid cell.
fn cell_ref<'a>(
    grid: &'a Grid<Cell>,
    overlays: &'a [Option<Overlay<'a>>],
    row: usize,
    line: Line,
    col: usize,
) -> &'a Cell {
    if let Some(Some(o)) = overlays.get(row) {
        if col >= o.start && col < o.start + o.float.cells.len() {
            return &o.float.cells[col - o.start];
        }
    }
    &grid[Point::new(line, Column(col))]
}

#[allow(clippy::too_many_arguments)]
pub fn paint(
    ui: &egui::Ui,
    painter: &Painter,
    rect: Rect,
    term: &Term<Listener>,
    floats: &Floats,
    view: &mut ViewState,
    m: &Metrics,
    opts: &PaintOptions,
) {
    let overlays = rprompt::overlays(term, floats);
    let theme = &opts.theme;
    let colors = term.colors();
    let default_bg = colors[NamedColor::Background as usize]
        .map(|c| Color32::from_rgb(c.r, c.g, c.b))
        .unwrap_or(theme.bg);
    painter.rect_filled(rect, 0.0, default_bg);

    let origin = m.origin(rect);
    let grid = term.grid();
    let rows = grid.screen_lines();
    let cols = grid.columns();
    let offset = grid.display_offset() as i32;
    let content = term.renderable_content();
    let selection = content.selection;
    let cursor = content.cursor;
    let ctx = ui.ctx();
    let font = &opts.font;
    let text_dy = ((m.cell_h - m.row_h) / 2.0 * m.ppp).round() / m.ppp;
    let bold_dx = (1.0 / m.ppp).max(0.5);

    let cell_rect = |row: usize, col: usize, width: usize| -> Rect {
        Rect::from_min_size(
            Pos2::new(
                origin.x + col as f32 * m.cell_w,
                origin.y + row as f32 * m.cell_h,
            ),
            Vec2::new(m.cell_w * width as f32, m.cell_h),
        )
    };
    // Snap x edges to pixels so adjacent background rects never leave gaps.
    let snap_rect = |r: Rect| -> Rect {
        let s = |v: f32| (v * m.ppp).round() / m.ppp;
        Rect::from_min_max(
            Pos2::new(s(r.min.x), s(r.min.y)),
            Pos2::new(s(r.max.x), s(r.max.y)),
        )
    };

    let cursor_row = cursor.point.line.0 + offset;
    let cursor_visible =
        cursor.shape != CursorShape::Hidden && cursor_row >= 0 && (cursor_row as usize) < rows;
    let draw_block_cursor =
        cursor_visible && opts.focused && opts.blink_on && cursor.shape == CursorShape::Block;

    for row in 0..rows {
        let line = Line(row as i32 - offset);
        let y = origin.y + row as f32 * m.cell_h;

        // Pass 1: backgrounds and selection, merged into runs.
        let mut bg_run: Option<(usize, Color32)> = None;
        let mut sel_run: Option<usize> = None;
        for col in 0..=cols {
            let (bg, selected) = if col < cols {
                let cell = cell_ref(grid, &overlays, row, line, col);
                let (_, bg) = cell_colors(cell, colors, theme);
                let sel = selection.is_some_and(|s| s.contains(Point::new(line, Column(col))));
                (bg, sel)
            } else {
                (default_bg, false)
            };
            match bg_run {
                Some((start, c)) if c != bg => {
                    if c != default_bg {
                        painter.rect_filled(snap_rect(cell_rect(row, start, col - start)), 0.0, c);
                    }
                    bg_run = Some((col, bg));
                }
                None => bg_run = Some((col, bg)),
                _ => {}
            }
            match (sel_run, selected) {
                (None, true) => sel_run = Some(col),
                (Some(start), false) => {
                    painter.rect_filled(
                        snap_rect(cell_rect(row, start, col - start)),
                        0.0,
                        theme.selection,
                    );
                    sel_run = None;
                }
                _ => {}
            }
        }

        // Pass 2: text.
        let mut seg: Option<Segment> = None;
        let flush = |seg: &mut Option<Segment>| {
            if let Some(s) = seg.take() {
                if s.job.text.trim_end().is_empty()
                    && s.job.sections.iter().all(|x| {
                        x.format.underline == Stroke::NONE && x.format.strikethrough == Stroke::NONE
                    })
                {
                    return;
                }
                let pos = Pos2::new(origin.x + s.col as f32 * m.cell_w, y + text_dy);
                let galley = ctx.fonts_mut(|f| f.layout_job(s.job));
                if s.bold {
                    painter.galley(pos + Vec2::new(bold_dx, 0.0), galley.clone(), theme.fg);
                }
                painter.galley(pos, galley, theme.fg);
            }
        };
        let mut col = 0;
        while col < cols {
            let cell = cell_ref(grid, &overlays, row, line, col);
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                flush(&mut seg);
                col += 1;
                continue;
            }
            let (mut fg, _) = cell_colors(cell, colors, theme);
            let is_cursor_cell =
                draw_block_cursor && cursor_row as usize == row && cursor.point.column.0 == col;
            if is_cursor_cell {
                // Drawn on top of the cursor later.
                flush(&mut seg);
                col += 1;
                continue;
            }
            let ch = if cell.c == '\0' || cell.c == '\t' {
                ' '
            } else {
                cell.c
            };
            let bold = cell.flags.contains(Flags::BOLD);
            // Segments track *fake* bold (double draw) only.
            let fake_bold = bold && !(opts.bold_font.is_some() && ch.is_ascii());
            let style = Style {
                fg,
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                strike: cell.flags.contains(Flags::STRIKEOUT),
            };
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            // Combining marks, but not our invisible RPROMPT line markers.
            let zerowidth = cell
                .zerowidth()
                .is_some_and(|z| z.iter().any(|&c| rprompt::marker_id(c).is_none()));
            let regular = !wide
                && !zerowidth
                && !is_box_char(ch)
                && view.is_regular(ctx, font, m.advance, ch);
            if !regular {
                flush(&mut seg);
                let width = if wide { 2 } else { 1 };
                let r = cell_rect(row, col, width);
                if opts.dimmed {
                    fg = mix(fg, theme.bg, 0.5);
                }
                if !draw_box_char(painter, r, ch, fg, m.ppp) {
                    let mut text = ch.to_string();
                    if let Some(z) = cell.zerowidth() {
                        text.extend(z.iter().filter(|&&c| rprompt::marker_id(c).is_none()));
                    }
                    let mut job = LayoutJob::single_section(text, fmt(font, style));
                    job.wrap.max_width = f32::INFINITY;
                    let galley = ctx.fonts_mut(|f| f.layout_job(job));
                    let gx = r.center().x - galley.size().x / 2.0;
                    let pos = Pos2::new((gx * m.ppp).round() / m.ppp, y + text_dy);
                    if bold {
                        painter.galley(pos + Vec2::new(bold_dx, 0.0), galley.clone(), fg);
                    }
                    painter.galley(pos, galley, fg);
                }
                col += width;
                continue;
            }
            if ch != ' ' || style.underline || style.strike {
                if let Some(s) = &seg {
                    if s.bold != fake_bold {
                        flush(&mut seg);
                    }
                }
            }
            let s = seg.get_or_insert_with(|| {
                let mut job = LayoutJob::default();
                job.wrap.max_width = f32::INFINITY;
                Segment {
                    col,
                    bold: fake_bold,
                    job,
                }
            });
            let style = if opts.dimmed {
                Style {
                    fg: mix(style.fg, theme.bg, 0.5),
                    ..style
                }
            } else {
                style
            };
            let format = match (&opts.bold_font, bold && ch.is_ascii()) {
                (Some(bf), true) => fmt(bf, style).spaced(m.letter_spacing()),
                _ => fmt(font, style).spaced(m.letter_spacing()),
            };
            let mut buf = [0u8; 4];
            let text = ch.encode_utf8(&mut buf);
            match s.job.sections.last_mut() {
                Some(last) if last.format == format => {
                    s.job.text.push_str(text);
                    last.byte_range.end = egui::text::ByteIndex(s.job.text.len());
                }
                _ => s.job.append(text, 0.0, format),
            }
            col += 1;
        }
        flush(&mut seg);
    }

    // Cursor.
    if cursor_visible {
        let row = cursor_row as usize;
        let col = cursor.point.column.0.min(cols.saturating_sub(1));
        let cell = &grid[Point::new(Line(cursor_row - offset), Column(col))];
        let width = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let r = snap_rect(cell_rect(row, col, width));
        let color = colors[NamedColor::Cursor as usize]
            .map(|c| Color32::from_rgb(c.r, c.g, c.b))
            .unwrap_or(theme.cursor);
        let color = if opts.dimmed {
            mix(color, theme.bg, 0.5)
        } else {
            color
        };
        let stroke_w = (1.0 / m.ppp).max(1.0);
        if !opts.focused {
            painter.rect_stroke(
                r.shrink(stroke_w / 2.0),
                CornerRadius::ZERO,
                Stroke::new(stroke_w, color),
                egui::StrokeKind::Inside,
            );
        } else if opts.blink_on {
            match cursor.shape {
                CursorShape::Block => {
                    painter.rect_filled(r, 0.0, color);
                    let ch = if cell.c == '\0' { ' ' } else { cell.c };
                    if ch != ' ' {
                        let style = Style {
                            fg: theme.bg,
                            italic: cell.flags.contains(Flags::ITALIC),
                            underline: false,
                            strike: false,
                        };
                        let mut job = LayoutJob::single_section(ch.to_string(), fmt(font, style));
                        job.wrap.max_width = f32::INFINITY;
                        let galley = ctx.fonts_mut(|f| f.layout_job(job));
                        let gx = r.center().x - galley.size().x / 2.0;
                        painter.galley(
                            Pos2::new((gx * m.ppp).round() / m.ppp, r.top() + text_dy),
                            galley,
                            theme.bg,
                        );
                    }
                }
                CursorShape::Beam => {
                    let w = (2.0 / m.ppp).max(1.5);
                    painter.rect_filled(
                        Rect::from_min_size(r.min, Vec2::new(w, r.height())),
                        0.0,
                        color,
                    );
                }
                CursorShape::Underline => {
                    let h = (2.0 / m.ppp).max(1.5);
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(r.left(), r.bottom() - h), r.max),
                        0.0,
                        color,
                    );
                }
                CursorShape::HollowBlock => {
                    painter.rect_stroke(
                        r.shrink(stroke_w / 2.0),
                        CornerRadius::ZERO,
                        Stroke::new(stroke_w, color),
                        egui::StrokeKind::Inside,
                    );
                }
                CursorShape::Hidden => {}
            }
        }
    }

    // Scrollback position indicator.
    if offset > 0 {
        let history = grid.history_size().max(1) as f32;
        let total = history + rows as f32;
        let h = (rect.height() * rows as f32 / total).max(16.0);
        let top = rect.top() + (rect.height() - h) * (1.0 - offset as f32 / history);
        let bar = Rect::from_min_size(Pos2::new(rect.right() - 5.0, top), Vec2::new(3.0, h));
        painter.rect_filled(bar, CornerRadius::same(2), theme.fg.gamma_multiply(0.3));
    }
}

trait Spaced {
    fn spaced(self, spacing: f32) -> Self;
}

impl Spaced for TextFormat {
    fn spaced(mut self, spacing: f32) -> Self {
        self.extra_letter_spacing = spacing;
        self
    }
}

fn fmt(font: &FontId, s: Style) -> TextFormat {
    TextFormat {
        font_id: font.clone(),
        color: s.fg,
        italics: s.italic,
        underline: if s.underline {
            Stroke::new(1.0, s.fg)
        } else {
            Stroke::NONE
        },
        strikethrough: if s.strike {
            Stroke::new(1.0, s.fg)
        } else {
            Stroke::NONE
        },
        ..Default::default()
    }
}

/// What the input handler asks the owner to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InputOutcome {
    pub bytes: Vec<u8>,
    /// The user pressed Enter (a command starts).
    pub submitted: bool,
    /// Enter on an exited session.
    pub restart: bool,
    /// ⌘K: clear the scrollback.
    pub clear: bool,
    /// Text to put on the clipboard.
    pub copy: Option<String>,
}

/// Grid cell under a screen position, clamped to the grid.
fn cell_at(pos: Pos2, origin: Pos2, m: &Metrics, size: (usize, usize)) -> (usize, usize, Side) {
    let fx = ((pos.x - origin.x) / m.cell_w).max(0.0);
    let fy = ((pos.y - origin.y) / m.cell_h).max(0.0);
    let col = (fx.floor() as usize).min(size.0.saturating_sub(1));
    let row = (fy.floor() as usize).min(size.1.saturating_sub(1));
    let side = if fx.fract() < 0.5 {
        Side::Left
    } else {
        Side::Right
    };
    (col, row, side)
}

fn mouse_report(button: u8, col: usize, row: usize, pressed: bool, sgr: bool) -> Vec<u8> {
    if sgr {
        format!(
            "\x1b[<{};{};{}{}",
            button,
            col + 1,
            row + 1,
            if pressed { 'M' } else { 'm' }
        )
        .into_bytes()
    } else {
        let b = if pressed { button } else { 3 };
        let enc = |v: usize| (32 + v + 1).min(255) as u8;
        vec![0x1b, b'[', b'M', 32 + b, enc(col), enc(row)]
    }
}

pub struct InputContext<'a> {
    pub focused: bool,
    pub response: &'a egui::Response,
    pub rect: Rect,
    pub metrics: Metrics,
    pub option_as_meta: bool,
    pub exited: bool,
}

/// Translate this frame's input into terminal actions. Mutates the
/// emulator for local actions (selection, scrollback).
pub fn handle_input(
    ui: &egui::Ui,
    term: &mut Term<Listener>,
    view: &mut ViewState,
    ic: InputContext<'_>,
) -> InputOutcome {
    let mut out = InputOutcome::default();
    let mode = *term.mode();
    let modes = KeyModes {
        app_cursor: mode.contains(TermMode::APP_CURSOR),
        option_as_meta: ic.option_as_meta,
    };
    let m = ic.metrics;
    let origin = m.origin(ic.rect);
    let size = (term.grid().columns(), term.grid().screen_lines());
    let offset = term.grid().display_offset() as i32;
    let now = ui.input(|i| i.time);
    let mut typed = false;

    // ---- keyboard
    if ic.focused {
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    view.skip_text = false;
                    if modifiers.mac_cmd || (modifiers.command && !cfg!(target_os = "macos")) {
                        match key {
                            egui::Key::A => {
                                select_all(term);
                                continue;
                            }
                            egui::Key::K => {
                                out.clear = true;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    if modifiers.ctrl && key == egui::Key::Backtick {
                        continue; // host's Toggle Terminal shortcut
                    }
                    if modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd {
                        let scroll = match key {
                            egui::Key::PageUp => Some(Scroll::PageUp),
                            egui::Key::PageDown => Some(Scroll::PageDown),
                            egui::Key::Home if !mode.contains(TermMode::ALT_SCREEN) => {
                                Some(Scroll::Top)
                            }
                            egui::Key::End if !mode.contains(TermMode::ALT_SCREEN) => {
                                Some(Scroll::Bottom)
                            }
                            _ => None,
                        };
                        if let Some(s) = scroll {
                            term.scroll_display(s);
                            continue;
                        }
                    }
                    if ic.exited {
                        if key == egui::Key::Enter {
                            out.restart = true;
                        }
                        continue;
                    }
                    if let Some(bytes) = keys::key_to_bytes(key, modifiers, modes) {
                        if key == egui::Key::Enter && !modifiers.any() {
                            out.submitted = true;
                        }
                        // A Text event for the same key press would double it.
                        view.skip_text = modifiers.ctrl || modifiers.alt || modifiers.mac_cmd;
                        out.bytes.extend_from_slice(&bytes);
                        typed = true;
                    }
                }
                egui::Event::Text(t) => {
                    if std::mem::take(&mut view.skip_text) || ic.exited {
                        continue;
                    }
                    out.bytes.extend_from_slice(&keys::text_to_bytes(&t));
                    typed = true;
                }
                egui::Event::Ime(egui::ImeEvent::Commit(t)) => {
                    if !ic.exited && !t.is_empty() {
                        out.bytes.extend_from_slice(&keys::text_to_bytes(&t));
                        typed = true;
                    }
                }
                egui::Event::Paste(t) => {
                    if !ic.exited {
                        out.bytes.extend_from_slice(&keys::paste_bytes(
                            &t,
                            mode.contains(TermMode::BRACKETED_PASTE),
                        ));
                        typed = true;
                    }
                }
                egui::Event::Copy | egui::Event::Cut => {
                    // Line markers of floating right prompts are invisible
                    // zero-width chars: never copy them.
                    if let Some(s) = term
                        .selection_to_string()
                        .map(|s| rprompt::strip_markers(&s))
                        .filter(|s| !s.is_empty())
                    {
                        out.copy = Some(s);
                    }
                }
                _ => {}
            }
        }
    }
    if typed {
        view.blink_epoch = now;
        term.selection = None;
        if offset != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    }

    // ---- mouse
    let resp = ic.response;
    let reporting = mode.intersects(TermMode::MOUSE_MODE);
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer = ui.input(|i| i.pointer.interact_pos());

    if reporting && !shift && !ic.exited {
        if let Some(pos) = pointer {
            let (col, row, _) = cell_at(pos, origin, &m, size);
            if resp.is_pointer_button_down_on() || view.mouse_button_down.is_some() {
                let pressed_now = ui.input(|i| i.pointer.primary_pressed());
                let released = ui.input(|i| i.pointer.primary_released());
                if pressed_now && resp.hovered() {
                    view.mouse_button_down = Some(0);
                    view.last_report_cell = Some((col, row));
                    out.bytes.extend(mouse_report(0, col, row, true, sgr));
                } else if released && view.mouse_button_down.is_some() {
                    view.mouse_button_down = None;
                    out.bytes.extend(mouse_report(0, col, row, false, sgr));
                } else if view.mouse_button_down.is_some()
                    && mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
                    && view.last_report_cell != Some((col, row))
                {
                    view.last_report_cell = Some((col, row));
                    out.bytes.extend(mouse_report(32, col, row, true, sgr));
                }
            }
        }
    } else if let Some(pos) = pointer {
        let (col, row, side) = cell_at(pos, origin, &m, size);
        let point = Point::new(Line(row as i32 - offset), Column(col));
        if resp.triple_clicked() {
            term.selection = Some(Selection::new(SelectionType::Lines, point, side));
        } else if resp.double_clicked() {
            term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
        } else if resp.drag_started() {
            term.selection = Some(Selection::new(SelectionType::Simple, point, side));
        } else if resp.dragged() {
            if let Some(sel) = term.selection.as_mut() {
                sel.update(point, side);
            }
            // Auto-scroll into the scrollback when dragging past the edge.
            if pos.y < ic.rect.top() {
                term.scroll_display(Scroll::Delta(1));
            } else if pos.y > ic.rect.bottom() {
                term.scroll_display(Scroll::Delta(-1));
            }
        } else if resp.clicked() {
            term.selection = None;
        }
    }

    // ---- wheel
    if resp.hovered() {
        let mut lines = 0.0f32;
        ui.input(|i| {
            for ev in &i.events {
                if let egui::Event::MouseWheel { unit, delta, .. } = ev {
                    lines += match unit {
                        egui::MouseWheelUnit::Point => delta.y / m.cell_h,
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * size.1 as f32,
                    };
                }
            }
        });
        view.scroll_accum += lines;
        let whole = view.scroll_accum.trunc() as i32;
        if whole != 0 {
            view.scroll_accum -= whole as f32;
            if reporting && !ic.exited {
                if let Some(pos) = pointer {
                    let (col, row, _) = cell_at(pos, origin, &m, size);
                    let button = if whole > 0 { 64 } else { 65 };
                    for _ in 0..whole.unsigned_abs().min(10) {
                        out.bytes.extend(mouse_report(button, col, row, true, sgr));
                    }
                }
            } else if mode.contains(TermMode::ALT_SCREEN)
                && mode.contains(TermMode::ALTERNATE_SCROLL)
            {
                // less, man, vim without mouse: wheel → arrow keys.
                let key = if whole > 0 {
                    egui::Key::ArrowUp
                } else {
                    egui::Key::ArrowDown
                };
                let bytes =
                    keys::key_to_bytes(key, egui::Modifiers::NONE, modes).unwrap_or_default();
                for _ in 0..whole.unsigned_abs().min(20) {
                    out.bytes.extend_from_slice(&bytes);
                }
            } else {
                term.scroll_display(Scroll::Delta(whole));
            }
        }
    }
    out
}

pub fn select_all(term: &mut Term<Listener>) {
    let grid = term.grid();
    let start = Point::new(grid.topmost_line(), Column(0));
    let end = Point::new(grid.bottommost_line(), grid.last_column());
    let mut sel = Selection::new(SelectionType::Simple, start, Side::Left);
    sel.update(end, Side::Right);
    term.selection = Some(sel);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last column ends exactly at the right content edge (rect − PAD)
    /// at every panel width, so zsh's right prompt sits flush right.
    #[test]
    fn grid_is_flush_right_at_every_width() {
        for ppp in [1.0f32, 2.0] {
            let m = Metrics {
                cell_w: 7.224,
                advance: 7.224,
                cell_h: 16.0,
                row_h: 14.0,
                ppp,
            };
            let mut w = 180.0f32;
            while w < 1900.0 {
                let rect = Rect::from_min_size(Pos2::new(13.0, 40.0), Vec2::new(w, 300.0));
                let cols = m.grid_size(rect).cols as usize;
                let f = m.fit(rect, cols);
                let right = m.origin(rect).x + cols as f32 * f.cell_w;
                let edge = rect.right() - PAD.x;
                // origin is pixel-snapped: allow that half pixel.
                assert!(
                    (right - edge).abs() <= 0.5 / ppp + 1e-3,
                    "w={w} ppp={ppp}: right {right} vs edge {edge}"
                );
                // The stretch is below one cell spread over the row.
                assert!(f.letter_spacing() < m.advance / cols as f32 + 1e-4, "w={w}");
                w += 7.37;
            }
        }
    }

    #[test]
    fn unsettled_grid_keeps_the_font_advance() {
        let m = Metrics {
            cell_w: 8.0,
            advance: 8.0,
            cell_h: 16.0,
            row_h: 14.0,
            ppp: 2.0,
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(816.0, 300.0));
        assert_eq!(m.grid_size(rect).cols, 100);
        assert_eq!(m.fit(rect, 100).cell_w, 8.0);
        // While a resize to 100 cols is still debounced the grid has 90.
        assert_eq!(m.fit(rect, 90).cell_w, 8.0);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(820.0, 300.0));
        assert!((m.fit(rect, 100).cell_w - 8.04).abs() < 1e-4);
    }
}
