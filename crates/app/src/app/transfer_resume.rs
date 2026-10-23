//! Transfers that survive network drops and restarts, on the app side: the
//! transfer manager with journal and network monitor, the endpoints of
//! enqueued jobs (so a restart can reconnect), the "Resume interrupted
//! transfers?" prompt at launch, and reconnecting to the servers restored
//! jobs need (saved favorites in the background, other servers through
//! the Connect dialog — secrets are never stored in the journal).

use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui;

use manta_core::{ConnectionConfig, SharedVfs};
use manta_ops::{Endpoint, JobState, SystemNetwork, TransferManager, TransferOptions};

use super::MantaApp;
use crate::dialogs::{ConnectForm, Dialog};
use crate::favorites::{self, Favorite};
use crate::host_key::{Checked, HostKeyRetry};
use crate::transfers::{PromptRow, ResumeChoice, ResumePrompt};

/// The app's transfer manager: journal in
/// `~/Library/Application Support/Manta/transfers.json` and the system's
/// network monitor (tests: neither).
pub(crate) fn new_manager() -> TransferManager {
    if cfg!(test) {
        return TransferManager::new(3);
    }
    TransferManager::with_options(TransferOptions {
        max_parallel: 3,
        journal: dirs::data_dir().map(|d| d.join("Manta").join("transfers.json")),
        network: Some(SystemNetwork::start()),
        ..Default::default()
    })
}

/// Endpoint, whether the user asked (Reconnect button), outcome.
type ConnectResult = (Endpoint, bool, Result<Checked<SharedVfs>, String>);

pub(crate) struct ResumeState {
    /// The launch prompt was prepared (once per run).
    checked: bool,
    prompt: Option<ResumePrompt>,
    /// Background reconnects to favorites in flight.
    connecting: Vec<Endpoint>,
    tx: Sender<ConnectResult>,
    rx: Receiver<ConnectResult>,
}

impl Default for ResumeState {
    fn default() -> Self {
        let (tx, rx) = channel();
        ResumeState {
            checked: false,
            prompt: None,
            connecting: Vec::new(),
            tx,
            rx,
        }
    }
}

impl MantaApp {
    /// The connection `fs` belongs to, for the journal.
    pub(super) fn transfer_endpoint(&self, fs: &SharedVfs) -> Endpoint {
        if manta_ops::same_fs(fs, &self.local) {
            return Endpoint::Local;
        }
        match self
            .connections
            .iter()
            .find(|c| manta_ops::same_fs(&c.fs, fs))
        {
            Some(c) => Endpoint::remote(&c.config),
            None => Endpoint::guess(fs),
        }
    }

    /// An open backend for `ep` (local file system, open connections).
    fn open_backend(&self, ep: &Endpoint) -> Option<SharedVfs> {
        match ep {
            Endpoint::Local => Some(self.local.clone()),
            Endpoint::Remote { .. } => self
                .connections
                .iter()
                .find(|c| ep.matches(&c.config))
                .map(|c| c.fs.clone()),
            Endpoint::Unknown { .. } => None,
        }
    }

    /// The saved favorite for `ep`, if any, and whether its secret is in
    /// the Keychain.
    fn favorite_for(&self, ep: &Endpoint) -> Option<(ConnectionConfig, bool)> {
        self.favorites.items.iter().find_map(|f| match f {
            Favorite::Connection { config, keychain } if ep.matches(config) => {
                Some((config.clone(), *keychain))
            }
            _ => None,
        })
    }

    /// Once per frame: the launch prompt, reconnect results and requests.
    pub(super) fn transfer_resume_frame(&mut self, ctx: &egui::Context) {
        if !self.resume.checked {
            self.resume.checked = true;
            let jobs = self.transfers.interrupted();
            if !jobs.is_empty() {
                let rows = jobs
                    .iter()
                    .map(|j| PromptRow {
                        kind: j.kind,
                        title: j.title.clone(),
                        bytes_done: j.bytes_done,
                        bytes_total: j.bytes_total,
                        files_total: j.files_total,
                        needs: j
                            .endpoints()
                            .into_iter()
                            .find(|ep| {
                                self.open_backend(ep).is_none() && self.favorite_for(ep).is_none()
                            })
                            .map(Endpoint::label),
                    })
                    .collect();
                self.resume.prompt = Some(ResumePrompt { rows });
            }
        }
        if self.dialog.is_none() {
            if let Some(prompt) = &mut self.resume.prompt {
                if let Some(choice) = prompt.ui(ctx) {
                    self.resume.prompt = None;
                    self.resume_choice(choice, ctx);
                }
            }
        }

        while let Ok((ep, user, result)) = self.resume.rx.try_recv() {
            self.resume.connecting.retain(|e| !e.same_connection(&ep));
            match result {
                Ok(Checked::Done(fs)) => self.transfers.provide_connection(&ep, fs),
                // The Reconnect button: ask like the Connect dialog does.
                Ok(Checked::Ask { info, changed }) if user => {
                    self.on_host_key(*info, changed, HostKeyRetry::Reconnect(ep));
                }
                Ok(Checked::Ask { info, changed }) => {
                    log::warn!(
                        "reconnecting to {} for a transfer: host key {} ({} {}); waiting for the user",
                        ep.label(),
                        if changed { "CHANGED" } else { "unknown" },
                        info.key_type,
                        info.fingerprint
                    );
                    if changed {
                        self.set_error_detail(t!("hostkey.refused"), info.host_label());
                    }
                }
                Err(e) => log::warn!("reconnecting to {} for a transfer failed: {e}", ep.label()),
            }
        }
        for ep in self.transfers_view.take_reconnect_requests() {
            self.reconnect_endpoint(&ep, true, ctx);
        }
    }

    fn resume_choice(&mut self, choice: ResumeChoice, ctx: &egui::Context) {
        match choice {
            ResumeChoice::Resume => {
                let resolve = |ep: &Endpoint| self.open_backend(ep);
                self.transfers.restore_interrupted(&resolve);
                self.show_transfers = true;
                // Saved favorites reconnect by themselves.
                let needed: Vec<Endpoint> = self
                    .transfers
                    .snapshot()
                    .into_iter()
                    .filter(|j| matches!(j.state, JobState::NeedsConnection(_)))
                    .flat_map(|j| j.resume.needs)
                    .collect();
                for ep in needed {
                    self.reconnect_endpoint(&ep, false, ctx);
                }
            }
            ResumeChoice::Discard => {
                let resolve = |ep: &Endpoint| self.open_backend(ep);
                self.transfers.discard_interrupted(&resolve);
            }
            ResumeChoice::Later => {}
        }
    }

    /// Get a backend for `ep` to a waiting transfer: an open connection, a
    /// background login with a saved favorite, or (`user`: the Reconnect
    /// button) the Connect dialog.
    pub(super) fn reconnect_endpoint(&mut self, ep: &Endpoint, user: bool, ctx: &egui::Context) {
        if let Some(fs) = self.open_backend(ep) {
            self.transfers.provide_connection(ep, fs);
            return;
        }
        let Some(cfg) = ep.config().cloned() else {
            return;
        };
        match self.favorite_for(ep) {
            Some((fav, keychain)) if keychain || !favorites::needs_secret(&fav) => {
                if self.resume.connecting.iter().any(|e| e.same_connection(ep)) {
                    return;
                }
                self.resume.connecting.push(ep.clone());
                let (tx, ep, ctx) = (self.resume.tx.clone(), ep.clone(), ctx.clone());
                let secrets = keychain.then(|| self.secrets.clone());
                std::thread::Builder::new()
                    .name("manta-transfer-reconnect".into())
                    .spawn(move || {
                        // Never asks about host keys: an unknown or changed
                        // key fails, the job keeps waiting for a connection.
                        let r = match secrets {
                            Some(store) => crate::secrets::fill_from(&*store, fav),
                            None => Ok(fav),
                        }
                        .and_then(|fav| Checked::from_result(manta_vfs::connect(&fav)));
                        let _ = tx.send((ep, user, r));
                        ctx.request_repaint();
                    })
                    .ok();
            }
            Some((fav, _)) if user => {
                let mut form = self.connect_form(ConnectForm::from_config(&fav, self.active, None));
                form.error = Some(t!("error.enter_password").into());
                self.dialog = Some(Dialog::Connect(form));
            }
            None if user => {
                // Connecting the usual way hands the connection over to the
                // waiting transfer (see `provide_connection` on connect).
                let form = self.connect_form(ConnectForm::from_config(&cfg, self.active, None));
                self.dialog = Some(Dialog::Connect(form));
            }
            _ => {}
        }
    }
}
