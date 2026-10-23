//! The "Compress…" sheet: format, archive name, level and the
//! `.DS_Store` / `__MACOSX` exclusion. Submitting enqueues a
//! `manta_ops::CompressJob`.

use eframe::egui::{self, RichText};
use manta_core::{SharedVfs, VfsPath};
use manta_ops::CompressFormat;

use crate::app::Side;
use crate::dialogs::{
    buttons, dialog_header, dialog_modal, finish, folder_name, item_list, DialogResult,
};
use crate::icons;
use crate::theme::{self, pal, space, text};
use crate::util;
use crate::widgets::{self, ButtonKind};

pub struct CompressForm {
    pub side: Side,
    pub fs: SharedVfs,
    pub sources: Vec<VfsPath>,
    /// Which of `sources` are folders (for icons).
    pub folders: Vec<VfsPath>,
    /// The folder the archive is created in (the sources' folder).
    pub dir: VfsPath,
    /// "Local" or the connection label.
    pub dir_label: String,
    /// Names already in `dir`.
    pub existing: Vec<String>,
    /// Size of the selection, if known.
    pub total_size: Option<u64>,
    pub format: CompressFormat,
    /// The archive name without the extension.
    pub stem: String,
    pub level: u32,
    pub exclude_junk: bool,
    focus_requested: bool,
}

impl CompressForm {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        side: Side,
        fs: SharedVfs,
        dir: VfsPath,
        dir_label: String,
        sources: Vec<VfsPath>,
        folders: Vec<VfsPath>,
        existing: Vec<String>,
        total_size: Option<u64>,
    ) -> Self {
        let format = CompressFormat::Zip;
        let base = default_stem(&sources, &folders);
        let stem = free_stem(&existing, &base, format.ext());
        CompressForm {
            side,
            fs,
            sources,
            folders,
            dir,
            dir_label,
            existing,
            total_size,
            format,
            stem,
            level: format.default_level(),
            exclude_junk: true,
            focus_requested: false,
        }
    }

    /// The archive's file name, e.g. "site.tar.gz".
    pub fn file_name(&self) -> String {
        format!("{}{}", self.stem.trim(), self.format.ext())
    }

    pub fn validated(&self) -> Result<String, String> {
        let name = self.file_name();
        if self.stem.trim().is_empty() {
            return Err(t!("compress.err.empty").into());
        }
        match util::file_name_problem(&name) {
            None => {}
            Some(util::NameProblem::Empty) => return Err(t!("compress.err.empty").into()),
            Some(util::NameProblem::Reserved) => return Err(t!("compress.err.reserved").into()),
            Some(util::NameProblem::Slash) => return Err(t!("compress.err.slash").into()),
        }
        if self.existing.contains(&name) {
            return Err(t!("dialog.name.exists", name = name));
        }
        Ok(name)
    }

    fn set_format(&mut self, f: CompressFormat) {
        if f == self.format {
            return;
        }
        // Keep the suggestion free of collisions for the new extension.
        let base = strip_number(self.stem.trim());
        if self
            .existing
            .iter()
            .any(|e| *e == format!("{}{}", self.stem.trim(), f.ext()))
        {
            self.stem = free_stem(&self.existing, &base, f.ext());
        }
        if self.level == self.format.default_level() {
            self.level = f.default_level();
        }
        self.format = f;
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let n = self.sources.len();
        let live = self.validated();
        let modal = dialog_modal("compress-dialog", ctx).show(ctx, |ui| {
            ui.set_width(440.0);
            let name = self
                .sources
                .first()
                .map(|s| s.file_name().to_string())
                .unwrap_or_default();
            let title = if n == 1 {
                t!("compress.title.single", name = name)
            } else {
                t!("compress.title", count = n)
            };
            let mut sub = t!(
                "compress.subtitle",
                folder = folder_name(&self.dir),
                location = crate::i18n::fs_label(&self.dir_label)
            );
            // Folder sizes aren't known here: show a size only for files.
            if let Some(size) = self
                .total_size
                .filter(|s| *s > 0 && self.folders.is_empty())
            {
                sub = format!("{} · {sub}", util::format_size(size));
            }
            dialog_header(
                ui,
                icons::FILE_ARCHIVE,
                p.archive,
                p.archive.gamma_multiply(0.16),
                &title,
                &sub,
            );
            ui.add_space(space::MD);
            item_list(ui, &self.sources, &self.folders);
            ui.add_space(space::LG);

            row(ui, t!("compress.format"), |ui| {
                let mut f = self.format;
                let opts: Vec<(CompressFormat, &str)> = CompressFormat::ALL
                    .iter()
                    .map(|f| (*f, f.label()))
                    .collect();
                if widgets::segmented(ui, "compress-format", &mut f, &opts) {
                    self.set_format(f);
                }
            });
            ui.add_space(space::SM);
            row(ui, t!("compress.name"), |ui| {
                let ext_w = ui
                    .painter()
                    .layout_no_wrap(
                        self.format.ext().to_string(),
                        theme::regular(text::BODY),
                        p.text,
                    )
                    .size()
                    .x;
                let w = (ui.available_width() - ext_w - space::SM).max(80.0);
                let resp = widgets::show_input(
                    ui,
                    widgets::input(&mut self.stem).desired_width(w),
                    t!("compress.default_stem"),
                )
                .accessible_name(t!("compress.name.a11y"));
                if !self.focus_requested {
                    resp.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(self.stem.chars().count()),
                        );
                        state.cursor.set_char_range(Some(all));
                        state.store(ui.ctx(), resp.id);
                    }
                    self.focus_requested = true;
                }
                ui.label(RichText::new(self.format.ext()).color(p.text_secondary));
            });
            ui.add_space(space::SM);
            row(ui, t!("compress.level"), |ui| {
                ui.label(
                    RichText::new(t!("compress.faster"))
                        .font(theme::regular(text::SMALL))
                        .color(p.text_secondary),
                );
                ui.spacing_mut().slider_width = 170.0;
                let rail = if p.dark {
                    egui::Color32::from_rgb(70, 70, 76)
                } else {
                    egui::Color32::from_rgb(222, 222, 228)
                };
                ui.visuals_mut().widgets.inactive.bg_fill = rail;
                ui.add(
                    egui::Slider::new(&mut self.level, 1..=9)
                        .show_value(false)
                        .step_by(1.0),
                )
                .on_hover_text(t!("compress.level.tip", level = self.level));
                ui.label(
                    RichText::new(t!("compress.smaller"))
                        .font(theme::regular(text::SMALL))
                        .color(p.text_secondary),
                );
                ui.label(
                    RichText::new(format!("{}", self.level))
                        .font(theme::medium(text::SMALL))
                        .color(p.text_tertiary),
                );
            });
            ui.add_space(space::SM);
            row(ui, "", |ui| {
                if widgets::toggle_switch(ui, self.exclude_junk, true).clicked() {
                    self.exclude_junk = !self.exclude_junk;
                }
                ui.label(RichText::new(t!("compress.exclude_junk")).color(p.text));
            });
            ui.add_space(space::XS);
            let msg = live
                .as_ref()
                .err()
                .map(|e| format!("{}  {e}", icons::WARNING_CIRCLE));
            ui.label(
                RichText::new(msg.unwrap_or_else(|| " ".into()))
                    .font(theme::regular(text::SMALL))
                    .color(p.danger),
            );
            ui.add_space(space::XS);
            buttons(
                ui,
                t!("compress.button"),
                ButtonKind::Primary,
                live.is_ok(),
                &mut result,
            );
        });
        if result == DialogResult::Open
            && live.is_err()
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
        {
            return DialogResult::Open;
        }
        finish(ctx, modal.should_close(), result)
    }
}

const ROW_LABEL_W: f32 = 64.0;

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(ROW_LABEL_W, 26.0), egui::Sense::hover());
        if !label.is_empty() {
            ui.painter().text(
                r.right_center(),
                egui::Align2::RIGHT_CENTER,
                label,
                theme::regular(text::BODY),
                p.text_secondary,
            );
        }
        ui.add_space(space::SM);
        add(ui);
    });
}

/// "site" for one item (a file loses its extension), "Archive" for several.
pub fn default_stem(sources: &[VfsPath], folders: &[VfsPath]) -> String {
    match sources {
        [one] => {
            let name = one.file_name();
            match name.rfind('.') {
                Some(i) if i > 0 && !folders.contains(one) => name[..i].to_string(),
                _ if name.is_empty() => t!("compress.default_stem").into(),
                _ => name.to_string(),
            }
        }
        _ => t!("compress.default_stem").into(),
    }
}

/// `base`, or "base 2", "base 3" … so that `stem + ext` is not taken.
pub fn free_stem(existing: &[String], base: &str, ext: &str) -> String {
    let taken = |s: &str| existing.iter().any(|e| *e == format!("{s}{ext}"));
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|s| !taken(s))
        .expect("infinite")
}

/// "Archive 2" → "Archive".
fn strip_number(stem: &str) -> String {
    match stem.rsplit_once(' ') {
        Some((base, n)) if !base.is_empty() && n.parse::<u32>().is_ok() => base.to_string(),
        _ => stem.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> VfsPath {
        VfsPath::new(s)
    }

    #[test]
    fn smart_default_names() {
        assert_eq!(default_stem(&[p("/a/site")], &[p("/a/site")]), "site");
        assert_eq!(default_stem(&[p("/a/report.pdf")], &[]), "report");
        assert_eq!(default_stem(&[p("/a/.env")], &[]), ".env");
        assert_eq!(default_stem(&[p("/a/x"), p("/a/y")], &[]), "Archive");
        let existing = vec![
            "Archive.zip".to_string(),
            "Archive 2.zip".to_string(),
            "Archive.tar.gz".to_string(),
        ];
        assert_eq!(free_stem(&existing, "Archive", ".zip"), "Archive 3");
        assert_eq!(free_stem(&existing, "Archive", ".tar.xz"), "Archive");
        assert_eq!(strip_number("Archive 3"), "Archive");
        assert_eq!(strip_number("v 1.2"), "v 1.2");
    }
}
