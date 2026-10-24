//! "Go to Path" popup (⇧⌘G, like Finder's "Go to Folder"): type or paste a
//! path and open it in a new tab of the active pane.
//!
//! Input is forgiving: surrounding whitespace and quotes, `file://` URLs,
//! shell-escaped spaces, `~`, relative paths (against the active tab's
//! folder), `..` and trailing slashes are all accepted. Whether the path
//! exists is checked on a worker thread, never on the UI thread; problems are
//! shown inline and the popup stays open. Tab completes folder names from a
//! background listing of the parent folder.

use eframe::egui::{self, Color32, CornerRadius, Key, Modifiers, RichText, Sense};
use manta_core::{EntryKind, SharedVfs, VfsError, VfsPath};

use crate::app::Side;
use crate::icons;
use crate::theme::{self, pal, radius, space, text};
use crate::widgets;

// ------------------------------------------------------------------ input

/// Strip what pasting tends to bring along: whitespace, one pair of matching
/// quotes, a `file://` scheme (with percent-encoding) and backslash-escaped
/// spaces from a terminal.
pub fn clean_input(raw: &str) -> String {
    let mut s = raw.trim();
    for q in ['"', '\'', '“', '‘'] {
        let close = match q {
            '“' => '”',
            '‘' => '’',
            c => c,
        };
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(close) {
            s = s[q.len_utf8()..s.len() - close.len_utf8()].trim();
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("file://") {
        // file:///path or file://localhost/path
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return percent_decode(rest);
    }
    s.replace("\\ ", " ")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether the (cleaned) input refers to the home folder (`~` or `~/…`).
pub fn needs_home(cleaned: &str) -> bool {
    cleaned == "~" || cleaned.starts_with("~/")
}

/// Turn user input into an absolute, normalised path.
///
/// `base` is the active tab's folder (for relative input), `home` the
/// backend's home folder (for `~`; `None` if unknown).
pub fn resolve_path(raw: &str, base: &VfsPath, home: Option<&VfsPath>) -> Result<VfsPath, String> {
    let s = clean_input(raw);
    if s.is_empty() {
        return Err(t!("goto.err.empty").into());
    }
    if needs_home(&s) {
        let home = home.ok_or(t!("goto.err.no_home"))?;
        return Ok(VfsPath::new(&format!("{}/{}", home, &s[1..])));
    }
    if s.starts_with('/') {
        Ok(VfsPath::new(&s))
    } else {
        Ok(VfsPath::new(&format!("{}/{}", base, s)))
    }
}

/// Split input for completion: the folder part (up to and including the
/// last `/`) and the name prefix being typed after it.
pub fn split_for_completion(input: &str) -> (&str, &str) {
    match input.rfind('/') {
        Some(i) => (&input[..=i], &input[i + 1..]),
        None if input == "~" => ("~/", ""),
        None => ("", input),
    }
}

/// Result of completing `prefix` against folder `names`.
#[derive(Debug, PartialEq, Eq)]
pub struct Completion {
    /// What to put after the folder part: a whole name plus "/" when the
    /// match is unique, else the longest common prefix of all matches.
    pub text: String,
    /// All matching names (sorted).
    pub matches: Vec<String>,
}

/// Complete `prefix` (case-insensitive, like Finder). `None` if nothing
/// matches. Hidden folders are only offered once the prefix starts with ".".
pub fn complete(prefix: &str, names: &[String]) -> Option<Completion> {
    let lp = prefix.to_lowercase();
    let mut matches: Vec<String> = names
        .iter()
        .filter(|n| n.to_lowercase().starts_with(&lp))
        .filter(|n| prefix.starts_with('.') || !n.starts_with('.'))
        .cloned()
        .collect();
    matches.sort_by_key(|n| n.to_lowercase());
    match matches.as_slice() {
        [] => None,
        [one] => Some(Completion {
            text: format!("{one}/"),
            matches,
        }),
        [first, rest @ ..] => {
            let mut common: Vec<char> = first.chars().collect();
            for n in rest {
                let len = common
                    .iter()
                    .zip(n.chars())
                    .take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
                    .count();
                common.truncate(len);
            }
            let common: String = common.into_iter().collect();
            // Never shorten what was typed.
            let text = if common.chars().count() >= prefix.chars().count() {
                common
            } else {
                prefix.to_string()
            };
            Some(Completion { text, matches })
        }
    }
}

/// Show local paths under the home folder as `~/…`.
pub fn display_path(path: &VfsPath, home: Option<&VfsPath>) -> String {
    match home {
        Some(h) if !h.is_root() && path == h => "~".into(),
        Some(h) if !h.is_root() && path.starts_with(h) => {
            format!("~/{}", path.strip_prefix(h).unwrap_or_default())
        }
        _ => path.to_string(),
    }
}

// ------------------------------------------------------------------ checks

/// Where to go: a folder, and optionally a file in it to select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotoTarget {
    pub dir: VfsPath,
    pub select: Option<String>,
}

fn friendly(err: &VfsError) -> String {
    match err {
        VfsError::NotFound(_) => t!("goto.err.not_found").into(),
        VfsError::PermissionDenied(_) => t!("goto.err.permission").into(),
        VfsError::NotADirectory(_) => t!("goto.err.not_folder").into(),
        VfsError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            t!("goto.err.not_found").into()
        }
        VfsError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            t!("goto.err.permission").into()
        }
        VfsError::Connection(_) => t!("goto.err.unreachable").into(),
        other => {
            let s = other.to_string();
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => s,
            }
        }
    }
}

fn home_if_needed(fs: &SharedVfs, cleaned: &str) -> Result<Option<VfsPath>, String> {
    if needs_home(cleaned) {
        fs.home().map(Some).map_err(|e| friendly(&e))
    } else {
        Ok(None)
    }
}

/// Blocking (worker thread): resolve `raw` and check that it exists. A file
/// resolves to its folder with the file selected. A folder must also be
/// listable, so "Permission denied" shows here and not as an empty tab.
pub fn check_target(fs: &SharedVfs, raw: &str, base: &VfsPath) -> Result<GotoTarget, String> {
    let home = home_if_needed(fs, &clean_input(raw))?;
    let path = resolve_path(raw, base, home.as_ref())?;
    let entry = fs.stat(&path).map_err(|e| friendly(&e))?;
    let is_dir = match entry.kind {
        EntryKind::Directory => true,
        // A symlink may point to a folder: try to list it.
        EntryKind::Symlink => fs.list(&path).is_ok(),
        _ => false,
    };
    if is_dir {
        fs.list(&path).map_err(|e| friendly(&e))?;
        Ok(GotoTarget {
            dir: path,
            select: None,
        })
    } else {
        Ok(GotoTarget {
            dir: path.parent().unwrap_or_else(VfsPath::root),
            select: Some(path.file_name().to_string()),
        })
    }
}

/// Blocking (worker thread): names of the folders in the folder that the
/// completion `dir_part` (e.g. "~/Doc" → "~/") refers to.
pub fn list_folders(fs: &SharedVfs, dir_part: &str, base: &VfsPath) -> Result<Vec<String>, String> {
    let dir = if dir_part.is_empty() {
        base.clone()
    } else {
        let home = home_if_needed(fs, &clean_input(dir_part))?;
        resolve_path(dir_part, base, home.as_ref())?
    };
    let entries = fs.list(&dir).map_err(|e| friendly(&e))?;
    Ok(entries
        .into_iter()
        .filter(|e| matches!(e.kind, EntryKind::Directory | EntryKind::Symlink) || e.is_dir())
        .map(|e| e.name)
        .collect())
}

// ------------------------------------------------------------------ popup

/// What the popup asks the app to do this frame.
pub enum GotoRequest {
    /// Check `input` in the background, then open it (new tab or current).
    Check {
        gen: u64,
        input: String,
        new_tab: bool,
    },
    /// List the folders of `dir_part` for completion.
    ListFolders { gen: u64, dir_part: String },
}

pub struct GotoPopup {
    pub side: Side,
    pub fs: SharedVfs,
    pub conn: Option<u64>,
    /// Active tab's folder (relative paths resolve against it).
    pub base: VfsPath,
    /// "Local" or "user@host (SFTP)".
    pub backend: String,
    /// Local home, for `~/…` display of recent paths.
    pub home: Option<VfsPath>,
    /// Recent folders on this backend, most recent first.
    pub recent: Vec<VfsPath>,
    pub input: String,
    pub error: Option<String>,
    /// Generation of the in-flight check, if any.
    pub checking: Option<u64>,
    gen: u64,
    /// Folder names for completion, keyed by the folder part they were listed for.
    folders: Option<(String, Vec<String>)>,
    /// A listing was requested for this folder part; complete when it arrives.
    pending_completion: Option<(u64, String)>,
    /// Ambiguous completion: matching names (shown as a list).
    candidates: Vec<String>,
    /// Muted note under the field (e.g. "No folders start with …").
    note: Option<String>,
    /// Highlighted row in the list (candidates or recent).
    highlight: Option<usize>,
    /// Set by the app's raw-input hook: Tab was pressed (egui would
    /// otherwise use it to move focus).
    pub tab_pressed: bool,
    just_opened: bool,
    /// Move the text cursor to the end after a programmatic change.
    cursor_to_end: bool,
}

const ROW_H: f32 = 30.0;
const MAX_ROWS: usize = 6;
const FIELD_FONT: f32 = text::TITLE + 2.0;

fn field_id() -> egui::Id {
    egui::Id::unique("goto-path-field")
}

impl GotoPopup {
    pub fn new(
        side: Side,
        fs: SharedVfs,
        conn: Option<u64>,
        base: VfsPath,
        backend: String,
        home: Option<VfsPath>,
        recent: Vec<VfsPath>,
    ) -> Self {
        GotoPopup {
            side,
            fs,
            conn,
            base,
            backend,
            home,
            recent,
            input: String::new(),
            error: None,
            checking: None,
            gen: 0,
            folders: None,
            pending_completion: None,
            candidates: Vec::new(),
            note: None,
            highlight: None,
            tab_pressed: false,
            just_opened: true,
            cursor_to_end: false,
        }
    }

    fn next_gen(&mut self) -> u64 {
        self.gen += 1;
        self.gen
    }

    /// A check finished: `Some(target)` closes the popup (the app opens it),
    /// an error is shown inline.
    pub fn on_checked(
        &mut self,
        gen: u64,
        result: Result<GotoTarget, String>,
    ) -> Option<GotoTarget> {
        if self.checking != Some(gen) {
            return None; // stale or cancelled
        }
        self.checking = None;
        match result {
            Ok(t) => Some(t),
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    /// A folder listing for completion arrived.
    pub fn on_folders(&mut self, gen: u64, dir_part: String, result: Result<Vec<String>, String>) {
        let Some((want_gen, want_dir)) = &self.pending_completion else {
            return;
        };
        if *want_gen != gen || *want_dir != dir_part {
            return;
        }
        self.pending_completion = None;
        match result {
            Ok(names) => {
                self.folders = Some((dir_part, names));
                self.apply_completion();
            }
            Err(e) => self.note = Some(e),
        }
    }

    /// Complete the input from the cached listing (if it's for the current
    /// folder part). Returns false if a listing is needed first.
    fn apply_completion(&mut self) -> bool {
        let (dir_part, prefix) = split_for_completion(&self.input);
        let (dir_part, prefix) = (dir_part.to_string(), prefix.to_string());
        let Some((cached_dir, names)) = &self.folders else {
            return false;
        };
        if *cached_dir != dir_part {
            return false;
        }
        match complete(&prefix, names) {
            None => {
                self.candidates.clear();
                self.note = Some(if prefix.is_empty() {
                    t!("goto.no_folders").into()
                } else {
                    t!("goto.no_match", prefix = prefix)
                });
            }
            Some(c) => {
                self.input = format!("{dir_part}{}", c.text);
                self.candidates = if c.matches.len() > 1 {
                    c.matches
                } else {
                    Vec::new()
                };
                self.note = None;
                self.cursor_to_end = true;
            }
        }
        self.highlight = None;
        self.error = None;
        true
    }

    /// Number of rows in the list under the field (tests).
    #[cfg(test)]
    pub fn rows_len(&self) -> usize {
        self.rows().1.len()
    }

    /// Forget completion candidates and the highlight (tests).
    #[cfg(test)]
    pub fn clear_list(&mut self) {
        self.candidates.clear();
        self.highlight = None;
    }

    /// Rows of the list under the field: completion candidates if any,
    /// else recent folders matching the input.
    fn rows(&self) -> (&'static str, Vec<(String, String)>) {
        if !self.candidates.is_empty() {
            let (dir_part, _) = split_for_completion(&self.input);
            let rows = self
                .candidates
                .iter()
                .map(|n| (n.clone(), format!("{dir_part}{n}/")))
                .collect();
            return (t!("goto.matching"), rows);
        }
        let q = clean_input(&self.input).to_lowercase();
        let rows = self
            .recent
            .iter()
            .map(|p| (display_path(p, self.home.as_ref()), p.to_string()))
            .filter(|(shown, full)| {
                q.is_empty()
                    || shown.to_lowercase().contains(&q)
                    || full.to_lowercase().contains(&q)
            })
            .take(MAX_ROWS)
            .collect();
        (t!("goto.recent"), rows)
    }

    /// Draw the popup. Returns `false` when it should close.
    pub fn ui(&mut self, ctx: &egui::Context, requests: &mut Vec<GotoRequest>) -> bool {
        let p = theme::pal_ctx(ctx);
        let (title, rows) = self.rows();
        let n_rows = rows.len();

        // Keys handled before the text field sees them.
        let (up, down, enter, cmd_enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            return false;
        }
        if n_rows > 0 {
            if down {
                self.highlight = Some(self.highlight.map_or(0, |h| (h + 1) % n_rows));
            }
            if up {
                self.highlight = Some(
                    self.highlight
                        .map_or(n_rows - 1, |h| (h + n_rows - 1) % n_rows),
                );
            }
        }
        if self.highlight.is_some_and(|h| h >= n_rows) {
            self.highlight = None;
        }
        let mut submit: Option<(String, bool)> = None;
        if enter || cmd_enter {
            let input = match self.highlight.and_then(|h| rows.get(h)) {
                Some((_, full)) => full.clone(),
                None => self.input.clone(),
            };
            submit = Some((input, !cmd_enter));
        }
        if std::mem::take(&mut self.tab_pressed) && !self.apply_completion() {
            let (dir_part, _) = split_for_completion(&self.input);
            let dir_part = dir_part.to_string();
            let gen = self.next_gen();
            self.pending_completion = Some((gen, dir_part.clone()));
            self.note = None;
            requests.push(GotoRequest::ListFolders { gen, dir_part });
        }

        let screen = ctx.content_rect();
        let width = (screen.width() - 2.0 * space::XL).min(600.0);

        // Dim the app behind; a click outside closes.
        let backdrop = egui::Area::new(egui::Id::unique("goto-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // A click beside the sheet closes it (VoiceOver: "Close").
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::CLICK);
                crate::a11y::name(&resp, egui::accesskit::Role::Button, t!("common.close"));
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    crate::a11y::scrim(p.dark, 40, 90),
                );
                resp
            });
        if backdrop.inner.clicked() {
            return false;
        }

        let mut clicked_row: Option<String> = None;
        egui::Area::new(egui::Id::unique("goto-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(
                screen.center().x - width / 2.0,
                screen.top() + (screen.height() * 0.14).max(48.0),
            ))
            .show(ctx, |ui| {
                let p = pal(ui);
                egui::Frame::new()
                    .fill(p.elevated)
                    .stroke(egui::Stroke::new(1.0, p.separator))
                    .corner_radius(CornerRadius::same(radius::WINDOW))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(egui::Margin::same(space::SM as i8))
                    .show(ui, |ui| {
                        ui.set_width(width - 2.0 * space::SM);
                        self.field_row(ui);
                        if let Some(err) = &self.error {
                            message_row(ui, icons::WARNING_CIRCLE, err, p.danger);
                        } else if let Some(note) = &self.note {
                            message_row(ui, icons::INFO, note, p.text_secondary);
                        }
                        ui.add_space(space::XS);
                        ui.painter().hline(
                            ui.min_rect().x_range(),
                            ui.cursor().top(),
                            egui::Stroke::new(1.0, p.separator),
                        );
                        ui.add_space(space::XS);
                        if !rows.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(space::SM);
                                widgets::section_title(ui, title);
                            });
                            ui.add_space(space::XXS);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (idx, (shown, full)) in rows.iter().enumerate() {
                                if self.list_row(ui, idx, shown, full, up || down) {
                                    clicked_row = Some(full.clone());
                                }
                            }
                            ui.spacing_mut().item_spacing.y = space::SM;
                            ui.add_space(space::XS);
                        }
                        self.footer(ui);
                    });
            });

        if let Some(full) = clicked_row {
            let new_tab = !ctx.input(|i| i.modifiers.command);
            submit = Some((full, new_tab));
        }
        if let Some((input, new_tab)) = submit {
            if self.checking.is_none() {
                if clean_input(&input).is_empty() {
                    self.error = Some(t!("goto.err.empty").into());
                } else {
                    if input != self.input {
                        self.input = input.clone();
                        self.cursor_to_end = true;
                    }
                    let gen = self.next_gen();
                    self.checking = Some(gen);
                    self.error = None;
                    requests.push(GotoRequest::Check {
                        gen,
                        input,
                        new_tab,
                    });
                }
            }
        }
        true
    }

    fn field_row(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        // A fixed-height row, so `Align::Center` centres within it (in a
        // top-down parent, `with_layout` would claim all the height).
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 40.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_space(space::SM);
                widgets::icon(ui, icons::SIGNPOST, 18.0, p.text_secondary);
                ui.add_space(space::XXS);
                let spinner_w = if self.checking.is_some() { 24.0 } else { 0.0 };
                let id = field_id();
                if std::mem::take(&mut self.cursor_to_end) {
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                        let end = egui::text::CCursor::new(self.input.chars().count());
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(end)));
                        state.store(ui.ctx(), id);
                    }
                }
                let resp = widgets::show_with_hint(
                    ui,
                    egui::TextEdit::singleline(&mut self.input)
                        .id(id)
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(2, 0)))
                        .font(theme::regular(FIELD_FONT))
                        .min_size(egui::vec2(0.0, 40.0))
                        .align(egui::Align2::LEFT_CENTER)
                        .desired_width(ui.available_width() - spinner_w - space::SM),
                    theme::regular(FIELD_FONT),
                    t!("goto.hint"),
                )
                .accessible_name(t!("goto.a11y"));
                if self.just_opened || !resp.has_focus() {
                    resp.request_focus();
                    self.just_opened = false;
                }
                if resp.changed() {
                    self.error = None;
                    self.note = None;
                    self.candidates.clear();
                    self.highlight = None;
                    // Typing cancels a running check (its result is ignored).
                    self.checking = None;
                }
                if self.checking.is_some() {
                    ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
                }
            },
        );
    }

    fn list_row(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        shown: &str,
        full: &str,
        keyed: bool,
    ) -> bool {
        let p = pal(ui);
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            self.highlight = Some(idx);
        }
        let sel = self.highlight == Some(idx);
        if sel {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::ROW + 2), p.selection);
            if keyed {
                ui.scroll_to_rect(rect, None);
            }
        }
        let fg = if sel { p.text_on_accent } else { p.text };
        let ir = egui::Rect::from_min_size(
            rect.min + egui::vec2(space::SM, 0.0),
            egui::vec2(26.0, ROW_H),
        );
        widgets::paint_icon(
            ui,
            ir,
            icons::FOLDER_SIMPLE,
            16.0,
            if sel { fg } else { p.folder },
            false,
        );
        let avail = rect.right() - ir.right() - space::MD - space::XS;
        let g = widgets::truncate_start(ui, shown, theme::regular(text::BODY), fg, avail);
        ui.painter().galley(
            egui::pos2(ir.right() + space::XS, rect.center().y - g.size().y / 2.0),
            g,
            fg,
        );
        resp.on_hover_text(full).clicked()
    }

    fn footer(&self, ui: &mut egui::Ui) {
        let p = pal(ui);
        ui.horizontal(|ui| {
            ui.add_space(space::SM);
            let glyph = if self.conn.is_some() {
                icons::GLOBE
            } else {
                icons::HARD_DRIVE
            };
            widgets::icon(ui, glyph, 12.0, p.text_secondary);
            ui.spacing_mut().item_spacing.x = 0.0;
            let caption = theme::regular(text::CAPTION);
            ui.label(
                RichText::new(t!("goto.footer.new_tab_on"))
                    .font(caption.clone())
                    .color(p.text_secondary),
            );
            ui.label(
                RichText::new(&self.backend)
                    .font(theme::semibold(text::CAPTION))
                    .color(p.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(space::SM);
                ui.label(
                    RichText::new(t!("goto.footer.keys", ret = icons::RETURN))
                        .font(caption)
                        .color(p.text_tertiary),
                );
            });
        });
    }
}

fn message_row(ui: &mut egui::Ui, glyph: &str, msg: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.add_space(space::SM + 2.0);
        widgets::icon(ui, glyph, 14.0, color);
        ui.label(
            RichText::new(msg)
                .font(theme::medium(text::SMALL))
                .color(color),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> VfsPath {
        VfsPath::new(s)
    }

    #[test]
    fn resolves_absolute_relative_and_home() {
        let base = p("/Users/me/Projects");
        let home = p("/Users/me");
        let r = |s: &str| resolve_path(s, &base, Some(&home));
        assert_eq!(r("/usr/local/"), Ok(p("/usr/local")));
        assert_eq!(r("/usr//local///"), Ok(p("/usr/local")));
        assert_eq!(r("~"), Ok(p("/Users/me")));
        assert_eq!(r("~/"), Ok(p("/Users/me")));
        assert_eq!(r("~/Downloads"), Ok(p("/Users/me/Downloads")));
        assert_eq!(r("src"), Ok(p("/Users/me/Projects/src")));
        assert_eq!(r("./src/../lib/"), Ok(p("/Users/me/Projects/lib")));
        assert_eq!(r(".."), Ok(p("/Users/me")));
        assert_eq!(r("/a/b/../../.."), Ok(p("/")));
        // "~name" is not the home folder, just a relative name.
        assert_eq!(r("~backup"), Ok(p("/Users/me/Projects/~backup")));
    }

    #[test]
    fn cleans_pasted_input() {
        let base = p("/");
        let home = p("/Users/me");
        let r = |s: &str| resolve_path(s, &base, Some(&home));
        assert_eq!(r("  /tmp/x \n"), Ok(p("/tmp/x")));
        assert_eq!(r("'/Users/me/My Docs'"), Ok(p("/Users/me/My Docs")));
        assert_eq!(r("\"~/Downloads/\""), Ok(p("/Users/me/Downloads")));
        assert_eq!(r("“/tmp”"), Ok(p("/tmp")));
        assert_eq!(r("/Users/me/My\\ Docs"), Ok(p("/Users/me/My Docs")));
        assert_eq!(r("file:///Users/me/My%20Docs/"), Ok(p("/Users/me/My Docs")));
        assert_eq!(r("file://localhost/private/tmp"), Ok(p("/private/tmp")));
        assert_eq!(r("file:///caf%C3%A9"), Ok(p("/café")));
        // Broken escapes are kept literally.
        assert_eq!(r("file:///a%2"), Ok(p("/a%2")));
    }

    #[test]
    fn rejects_empty_and_unknown_home() {
        let base = p("/");
        assert!(resolve_path("   ", &base, None).is_err());
        assert!(resolve_path("''", &base, None).is_err());
        assert!(resolve_path("~/x", &base, None).is_err());
    }

    #[test]
    fn completion() {
        let names: Vec<String> = ["Desktop", "Documents", "Downloads", ".config", "src"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(split_for_completion("~/Doc"), ("~/", "Doc"));
        assert_eq!(split_for_completion("~"), ("~/", ""));
        assert_eq!(split_for_completion("src"), ("", "src"));
        let c = complete("doc", &names).unwrap();
        assert_eq!(c.text, "Documents/");
        assert_eq!(c.matches, vec!["Documents".to_string()]);
        let c = complete("D", &names).unwrap();
        assert_eq!(c.text, "D");
        assert_eq!(c.matches.len(), 3);
        let c = complete("Do", &names).unwrap();
        assert_eq!(c.text, "Do");
        let c = complete("Dow", &names).unwrap();
        assert_eq!(c.text, "Downloads/");
        assert!(complete("x", &names).is_none());
        // Hidden folders only when asked for.
        assert!(complete("", &names)
            .unwrap()
            .matches
            .iter()
            .all(|n| !n.starts_with('.')));
        assert_eq!(complete(".c", &names).unwrap().text, ".config/");
    }

    #[test]
    fn display_under_home() {
        let home = p("/Users/me");
        assert_eq!(
            display_path(&p("/Users/me/Downloads"), Some(&home)),
            "~/Downloads"
        );
        assert_eq!(display_path(&p("/Users/me"), Some(&home)), "~");
        assert_eq!(display_path(&p("/Users/meow"), Some(&home)), "/Users/meow");
        assert_eq!(display_path(&p("/tmp"), None), "/tmp");
    }

    #[test]
    fn checks_targets_on_local_fs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/file.txt"), "x").unwrap();
        let fs: SharedVfs = std::sync::Arc::new(manta_vfs::LocalFs::new());
        let base = VfsPath::new(&root.to_string_lossy());
        let t = check_target(&fs, "sub/", &base).unwrap();
        assert_eq!(t.dir, base.join("sub"));
        assert_eq!(t.select, None);
        let t = check_target(&fs, "sub/file.txt", &base).unwrap();
        assert_eq!(t.dir, base.join("sub"));
        assert_eq!(t.select.as_deref(), Some("file.txt"));
        assert_eq!(
            check_target(&fs, "nope", &base).unwrap_err(),
            "Folder not found"
        );
        let names = list_folders(&fs, "", &base).unwrap();
        assert_eq!(names, vec!["sub".to_string()]);
    }
}
