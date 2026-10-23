//! The SFTP host key question ("trust on first use").
//!
//! The VFS never blocks on UI: connecting to a server whose key isn't in
//! `known_hosts` fails with [`VfsError::UnknownHostKey`], a changed key with
//! [`VfsError::HostKeyMismatch`]. The app then shows [`HostKeyForm`]:
//! - unknown key: host, key type and SHA-256 fingerprint with "Connect and
//!   Remember" / "Cancel". Accepting appends the key to `known_hosts`
//!   ([`manta_vfs::known_hosts::trust_host_key`]) and connects again.
//! - changed key: a red warning (possible man-in-the-middle attack), how to
//!   remove the old entry, and only "Close" — never "connect anyway".
//!
//! Background reconnects (transfer resume) never ask; they fail.

use eframe::egui::{self, CornerRadius, RichText};
use manta_core::{HostKeyInfo, VfsError};

use crate::dialogs::{dialog_header, dialog_modal, finish, ConnectForm, DialogResult};
use crate::icons;
use crate::theme::{self, pal, radius, space, text};
use crate::widgets::{self, ButtonKind};
use crate::worker::ConnectRequest;

/// What to do once the user accepted an unknown key.
pub enum HostKeyRetry {
    /// Connect again (connect dialog, favorite, sidebar).
    Connect(Box<ConnectRequest>),
    /// Run the connect dialog's "Test Connection" again.
    Test,
    /// Reconnect for waiting transfers (the Reconnect button).
    Reconnect(manta_ops::Endpoint),
}

/// A host key question raised by a connection attempt.
pub struct HostKeyForm {
    pub info: HostKeyInfo,
    /// The key differs from `known_hosts` (refused) instead of unknown.
    pub changed: bool,
    pub retry: HostKeyRetry,
    /// The Connect dialog that was open; shown again afterwards.
    pub back: Option<Box<ConnectForm>>,
}

/// Unknown / changed host key carried by a connect error, if any.
pub fn question(e: &VfsError) -> Option<(Box<HostKeyInfo>, bool)> {
    match e {
        VfsError::UnknownHostKey(i) => Some((i.clone(), false)),
        VfsError::HostKeyMismatch(i) => Some((i.clone(), true)),
        _ => None,
    }
}

/// Outcome of a connect on a worker thread: a host key question is not an
/// error, the app asks it.
pub enum Checked<T> {
    Done(T),
    Ask {
        info: Box<HostKeyInfo>,
        changed: bool,
    },
}

impl<T> Checked<T> {
    /// Turn a host key error into [`Checked::Ask`], other errors into text.
    pub fn from_result(r: Result<T, VfsError>) -> Result<Checked<T>, String> {
        match r {
            Ok(v) => Ok(Checked::Done(v)),
            Err(e) => match question(&e) {
                Some((info, changed)) => Ok(Checked::Ask { info, changed }),
                None => Err(e.to_string()),
            },
        }
    }
}

/// The command that removes the old entry (`ssh-keygen -R host` or
/// `ssh-keygen -R '[host]:port'`; `-f file` for a non-default file).
pub fn remove_command(info: &HostKeyInfo) -> String {
    let pattern = info.host_label();
    let pattern = if pattern.starts_with('[') {
        format!("'{pattern}'")
    } else {
        pattern
    };
    let default =
        dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts").display().to_string());
    if info.known_hosts.is_empty() || Some(&info.known_hosts) == default.as_ref() {
        format!("ssh-keygen -R {pattern}")
    } else {
        format!("ssh-keygen -R {pattern} -f '{}'", info.known_hosts)
    }
}

/// `~/…` for paths in the home directory (shorter in the dialog).
fn short_path(p: &str) -> String {
    match dirs::home_dir().map(|h| h.display().to_string()) {
        Some(h) if p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

impl HostKeyForm {
    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let host = self.info.host_label();
        let file = short_path(&self.info.known_hosts);
        let modal = dialog_modal("host-key-dialog", ctx).show(ctx, |ui| {
            ui.set_width(500.0);
            if self.changed {
                dialog_header(
                    ui,
                    icons::SHIELD_WARNING,
                    p.danger,
                    p.danger_soft,
                    t!("hostkey.changed.title"),
                    &host,
                );
                ui.add_space(space::MD);
                egui::Frame::new()
                    .fill(p.danger_soft)
                    .corner_radius(CornerRadius::same(radius::CONTROL))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(t!("hostkey.changed.warning"))
                                    .font(theme::semibold(text::BODY))
                                    .color(p.danger),
                            )
                            .wrap(),
                        );
                    });
                ui.add_space(space::SM);
                let body = t!("hostkey.changed.body", host = host, file = file);
                ui.add(egui::Label::new(RichText::new(body).color(p.text_secondary)).wrap());
            } else {
                dialog_header(
                    ui,
                    icons::KEY,
                    p.warning,
                    p.warning_soft,
                    t!("hostkey.unknown.title"),
                    &host,
                );
                ui.add_space(space::MD);
                let body = t!("hostkey.unknown.body", host = host);
                ui.add(egui::Label::new(RichText::new(body).color(p.text_secondary)).wrap());
            }
            ui.add_space(space::MD);
            key_table(ui, &self.info, &host);
            ui.add_space(space::MD);
            if self.changed {
                ui.add(
                    egui::Label::new(RichText::new(t!("hostkey.changed.fix")).color(p.text)).wrap(),
                );
                ui.add_space(space::XS);
                let cmd = remove_command(&self.info);
                egui::Frame::new()
                    .fill(p.stripe)
                    .corner_radius(CornerRadius::same(radius::CONTROL))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(&cmd)
                                    .font(theme::mono(text::BODY))
                                    .color(p.text),
                            )
                            .selectable(true)
                            .wrap(),
                        );
                    });
                ui.add_space(space::LG);
                button_row(ui, |ui| {
                    if widgets::button(ui, t!("hostkey.close"), ButtonKind::Primary, true).clicked()
                    {
                        result = DialogResult::Cancel;
                    }
                    if widgets::button(ui, t!("hostkey.copy_command"), ButtonKind::Secondary, true)
                        .clicked()
                    {
                        ui.ctx().copy_text(cmd.clone());
                    }
                });
            } else {
                let saved = t!("hostkey.unknown.saved_in", file = file);
                ui.add(
                    egui::Label::new(
                        RichText::new(saved)
                            .font(theme::regular(text::CAPTION))
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(space::LG);
                button_row(ui, |ui| {
                    if widgets::button(ui, t!("hostkey.accept"), ButtonKind::Primary, true)
                        .clicked()
                    {
                        result = DialogResult::Submit;
                    }
                    if widgets::button(ui, t!("common.cancel"), ButtonKind::Secondary, true)
                        .clicked()
                    {
                        result = DialogResult::Cancel;
                    }
                });
            }
        });
        let r = finish(ctx, modal.should_close(), result);
        // Enter never accepts a changed key.
        if self.changed && r == DialogResult::Submit {
            DialogResult::Cancel
        } else {
            r
        }
    }
}

/// A right-aligned row of buttons exactly as tall as a button (a plain
/// `with_layout` would center them in all the height left in the modal and
/// leave empty space below them).
/// The buttons on the right, "?" (manual: checking the server key) on
/// the left, as in macOS sheets.
fn button_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        crate::help::help_button(ui, "08-verbindungen.md#server-schluessel");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

/// Server, key type and fingerprint (selectable, monospace).
fn key_table(ui: &mut egui::Ui, info: &HostKeyInfo, host: &str) {
    let p = pal(ui);
    egui::Grid::new("host-key-table")
        .num_columns(2)
        .spacing([space::MD, space::XS])
        .show(ui, |ui| {
            let label = |ui: &mut egui::Ui, s: &str| {
                ui.label(RichText::new(s).color(p.text_secondary));
            };
            label(ui, t!("hostkey.host"));
            ui.label(RichText::new(host).color(p.text));
            ui.end_row();
            label(ui, t!("hostkey.key_type"));
            ui.label(
                RichText::new(&info.key_type)
                    .font(theme::mono(text::BODY))
                    .color(p.text),
            );
            ui.end_row();
            label(ui, t!("hostkey.fingerprint"));
            ui.add(
                egui::Label::new(
                    RichText::new(&info.fingerprint)
                        .font(theme::mono(text::BODY))
                        .color(p.text),
                )
                .selectable(true)
                .wrap(),
            );
            ui.end_row();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(port: u16, file: &str) -> HostKeyInfo {
        HostKeyInfo {
            host: "example.com".into(),
            port,
            key_type: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
            key: vec![1, 2, 3],
            known_hosts: file.into(),
        }
    }

    #[test]
    fn remove_commands() {
        let home = dirs::home_dir()
            .unwrap()
            .join(".ssh/known_hosts")
            .display()
            .to_string();
        assert_eq!(
            remove_command(&info(22, &home)),
            "ssh-keygen -R example.com"
        );
        assert_eq!(
            remove_command(&info(2222, &home)),
            "ssh-keygen -R '[example.com]:2222'"
        );
        assert_eq!(
            remove_command(&info(22, "/tmp/kh")),
            "ssh-keygen -R example.com -f '/tmp/kh'"
        );
    }

    #[test]
    fn questions_from_errors() {
        let i = Box::new(info(22, ""));
        assert!(matches!(
            question(&VfsError::UnknownHostKey(i.clone())),
            Some((_, false))
        ));
        assert!(matches!(
            question(&VfsError::HostKeyMismatch(i)),
            Some((_, true))
        ));
        assert!(question(&VfsError::Auth("x".into())).is_none());
        let r: Result<Checked<u8>, String> =
            Checked::from_result(Err(VfsError::Auth("bad".into())));
        assert_eq!(r.err().as_deref(), Some("authentication failed: bad"));
    }
}
