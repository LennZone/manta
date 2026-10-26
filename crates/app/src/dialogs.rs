//! Modal dialogs: connect, copy/move confirmation, delete confirmation,
//! new folder and rename.

use eframe::egui::{self, Color32, CornerRadius, Key, RichText, Sense};
use std::sync::{Arc, Mutex};

use manta_core::{
    Auth, ConnectionConfig, ContainerOptions, FtpDataChannel, FtpOptions, Protocol, S3Options,
    SharedVfs, SmbOptions, VfsPath, WebDavOptions,
};
use manta_ops::ConflictPolicy;
use manta_vfs::container::discover::{DockerContainer, KubePod};
use manta_vfs::s3::{Endpoint, Provider, PROVIDERS};

use crate::app::Side;
use crate::icons;
use crate::theme::{self, pal, radius, space, text};
use crate::util;
use crate::widgets::{self, ButtonKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResult {
    /// Keep showing the dialog.
    Open,
    Cancel,
    Submit,
    /// Connect dialog: "Test Connection".
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    Password,
    KeyFile,
    Agent,
    Anonymous,
    /// S3: credentials from an AWS CLI profile.
    AwsProfile,
}

/// The S3 part of the connect dialog. The access key ID and secret use
/// [`ConnectForm::user`] and [`ConnectForm::password`].
pub struct S3Form {
    /// Provider preset id (see [`PROVIDERS`]).
    pub provider: &'static str,
    pub region: String,
    pub endpoint: String,
    /// The endpoint follows provider / region / account (not hand-edited).
    pub endpoint_auto: bool,
    /// Cloudflare R2 account ID.
    pub account: String,
    pub bucket: String,
    pub path_style: bool,
    /// AWS CLI profile ("" = `$AWS_PROFILE` / default).
    pub profile: String,
    /// Profiles from ~/.aws, read on a background thread.
    profiles: Arc<Mutex<Option<Vec<String>>>>,
    profiles_requested: bool,
}

impl Default for S3Form {
    fn default() -> Self {
        let mut f = S3Form {
            provider: "aws",
            region: String::new(),
            endpoint: String::new(),
            endpoint_auto: true,
            account: String::new(),
            bucket: String::new(),
            path_style: false,
            profile: String::new(),
            profiles: Arc::new(Mutex::new(None)),
            profiles_requested: false,
        };
        f.set_provider("aws");
        f
    }
}

impl S3Form {
    pub fn preset(&self) -> &'static Provider {
        Provider::by_id(self.provider).unwrap_or(&PROVIDERS[0])
    }

    /// Apply a provider preset: region, endpoint and addressing style.
    pub fn set_provider(&mut self, id: &str) {
        // Regions are provider-specific: start from the preset's default.
        let new = Provider::by_id(id).unwrap_or(&PROVIDERS[0]);
        self.region = new.default_region.to_string();
        self.provider = new.id;
        self.path_style = new.path_style;
        if !new.endpoint.is_empty() {
            self.endpoint_auto = true;
        }
        self.refresh_endpoint();
    }

    /// Recompute an automatic endpoint after region / account changes.
    pub fn refresh_endpoint(&mut self) {
        let preset = self.preset();
        if self.endpoint_auto && preset.id == "aws" && self.region.trim().is_empty() {
            // Region from the AWS profile: the backend derives the endpoint.
            self.endpoint.clear();
        } else if self.endpoint_auto && !preset.endpoint.is_empty() {
            let account = if self.account.trim().is_empty() {
                "<account-id>"
            } else {
                self.account.trim()
            };
            self.endpoint = preset.endpoint_for(&self.region, account);
        }
    }

    /// Read the AWS profiles once, off the UI thread.
    fn request_profiles(&mut self, ctx: &egui::Context) {
        if self.profiles_requested {
            return;
        }
        self.profiles_requested = true;
        let slot = self.profiles.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let list = manta_vfs::s3::aws_profiles();
            *slot.lock().unwrap() = Some(list);
            ctx.request_repaint();
        });
    }

    /// Tests: use these profiles instead of reading ~/.aws.
    #[cfg(test)]
    pub fn set_profiles(&mut self, list: Vec<String>) {
        *self.profiles.lock().unwrap() = Some(list);
        self.profiles_requested = true;
    }

    fn account_problem(&self) -> Option<&'static str> {
        (self.preset().needs_account && self.account.trim().is_empty())
            .then_some(t!("connect.problem.account"))
    }

    fn endpoint_problem(&self) -> Option<String> {
        if self.endpoint.trim().is_empty() {
            return (self.provider != "aws").then(|| t!("connect.problem.endpoint").to_string());
        }
        Endpoint::parse(&self.endpoint).err()
    }

    fn from_options(o: &S3Options) -> S3Form {
        let mut f = S3Form {
            provider: Provider::by_id(&o.provider).map_or("custom", |p| p.id),
            region: o.region.clone(),
            account: o.account_id.clone(),
            bucket: o.bucket.clone(),
            path_style: o.path_style,
            profile: o.profile.clone().unwrap_or_default(),
            ..Default::default()
        };
        let preset = f.preset();
        let auto = preset.endpoint_for(&o.region, &o.account_id);
        f.endpoint_auto =
            !preset.endpoint.is_empty() && (o.endpoint.is_empty() || o.endpoint == auto);
        f.endpoint = o.endpoint.clone();
        f.refresh_endpoint();
        f
    }
}

/// The WebDAV part of the connect dialog (server, user and password use
/// the common fields).
pub struct DavForm {
    /// HTTPS (`Protocol::WebDavs`) or plain HTTP (`Protocol::WebDav`).
    pub https: bool,
    /// Server type preset id (see [`manta_vfs::webdav::PRESETS`]).
    pub preset: &'static str,
    /// The WebDAV folder on the server (`/remote.php/dav/files/alice`).
    pub root: String,
    /// The path follows the preset and the user name (not hand-edited).
    pub root_auto: bool,
}

impl Default for DavForm {
    fn default() -> Self {
        DavForm {
            https: true,
            preset: "generic",
            root: String::new(),
            root_auto: true,
        }
    }
}

impl DavForm {
    /// Recompute an automatic path after a preset / user name change.
    pub fn refresh_root(&mut self, user: &str) {
        if self.root_auto {
            if let Some(r) = manta_vfs::webdav::preset_root(self.preset, user) {
                self.root = r;
            }
        }
    }

    pub fn set_preset(&mut self, id: &'static str, user: &str) {
        self.preset = id;
        self.root_auto = true;
        self.refresh_root(user);
    }

    fn from_options(o: &WebDavOptions, https: bool, user: &str) -> DavForm {
        let preset = manta_vfs::webdav::PRESETS
            .iter()
            .map(|(id, _)| *id)
            .find(|id| *id == o.preset)
            .unwrap_or("generic");
        let auto = manta_vfs::webdav::preset_root(preset, user).unwrap_or_default();
        DavForm {
            https,
            preset,
            root_auto: manta_vfs::webdav::normalize_root(&o.root)
                == manta_vfs::webdav::normalize_root(&auto),
            root: o.root.clone(),
        }
    }
}

fn dav_preset_name(id: &str) -> &'static str {
    match id {
        "nextcloud" => "Nextcloud",
        "owncloud" => "ownCloud",
        _ => t!("connect.webdav.preset.generic"),
    }
}

/// Shares found on an SMB server (off the UI thread).
#[derive(Debug, Clone, Default)]
pub struct ShareList {
    pub loading: bool,
    /// The server the list is for.
    pub server: String,
    pub result: Option<Result<Vec<manta_vfs::smb::Share>, String>>,
}

/// The SMB part of the connect dialog.
#[derive(Default)]
pub struct SmbForm {
    pub share: String,
    found: Arc<Mutex<ShareList>>,
    /// Show the share list below the field.
    pub picking: bool,
    /// Fixed data (tests): never run `smbutil`.
    fixed: bool,
}

impl SmbForm {
    /// Tests and renders: show these shares instead of asking the server.
    #[cfg(test)]
    pub fn set_found(&mut self, l: ShareList) {
        *self.found.lock().unwrap() = l;
        self.fixed = true;
        self.picking = true;
    }

    fn found(&self) -> ShareList {
        self.found.lock().map(|f| f.clone()).unwrap_or_default()
    }

    /// List the shares of `cfg`'s server on a background thread.
    fn request(&mut self, ctx: &egui::Context, cfg: ConnectionConfig) {
        self.picking = true;
        if self.fixed {
            return;
        }
        let slot = self.found.clone();
        if let Ok(mut f) = slot.lock() {
            f.loading = true;
            f.server = cfg.host.clone();
        }
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = manta_vfs::smb::list_shares(&cfg).map_err(|e| e.to_string());
            if let Ok(mut f) = slot.lock() {
                if f.server == cfg.host {
                    f.loading = false;
                    f.result = Some(r);
                }
            }
            ctx.request_repaint();
        });
    }
}

/// What the Container tab found (read-only discovery, off the UI thread).
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub docker: Option<Result<Vec<DockerContainer>, String>>,
    /// Kubernetes: all contexts and the current one.
    pub contexts: Vec<String>,
    pub current_context: Option<String>,
    pub namespaces: Vec<String>,
    /// Pods of the context / namespace in `pods_for`.
    pub pods: Option<Result<Vec<KubePod>, String>>,
    pub pods_for: (String, String),
    pub loading: bool,
}

/// The Container tab: a Docker container or a container of a pod.
#[derive(Default)]
pub struct ContainerForm {
    /// Docker: container name (or ID).
    pub container: String,
    pub context: String,
    pub namespace: String,
    pub pod: String,
    /// Kubernetes: container in the pod ("" = default).
    pub pod_container: String,
    found: Arc<Mutex<Found>>,
    /// What was last requested ("docker" / "k8s|ctx|ns").
    requested: Option<String>,
    /// Fixed data (tests): never run discovery.
    fixed: bool,
}

impl ContainerForm {
    /// Tests and renders: use this instead of asking docker / kubectl.
    #[cfg(test)]
    pub fn set_found(&mut self, f: Found) {
        *self.found.lock().unwrap() = f;
        self.fixed = true;
    }

    fn found(&self) -> Found {
        self.found.lock().map(|f| f.clone()).unwrap_or_default()
    }

    /// Discover containers / pods for the current selection, once per
    /// selection, on a background thread.
    fn request(&mut self, ctx: &egui::Context, kube: bool) {
        if self.fixed {
            return;
        }
        let key = if kube {
            format!("k8s|{}|{}", self.context, self.namespace)
        } else {
            "docker".to_string()
        };
        if self.requested.as_deref() == Some(key.as_str()) {
            return;
        }
        self.requested = Some(key);
        let slot = self.found.clone();
        let ctx = ctx.clone();
        let (context, namespace) = (self.context.clone(), self.namespace.clone());
        if let Ok(mut f) = slot.lock() {
            f.loading = true;
        }
        std::thread::spawn(move || {
            use manta_vfs::container::discover;
            let runner = crate::containers::discovery_env().runner;
            let mut f = slot.lock().map(|f| f.clone()).unwrap_or_default();
            if kube {
                if f.contexts.is_empty() {
                    f.contexts = discover::kube_contexts(&*runner).unwrap_or_default();
                    f.current_context = discover::kube_current_context(&*runner).ok().flatten();
                }
                let c = if context.is_empty() {
                    f.current_context.clone().unwrap_or_default()
                } else {
                    context
                };
                let ns = if namespace.is_empty() {
                    discover::kube_default_namespace(&*runner, &c)
                        .unwrap_or_else(|_| "default".into())
                } else {
                    namespace
                };
                f.namespaces = discover::kube_namespaces(&*runner, &c).unwrap_or_default();
                f.pods = Some(discover::kube_pods(&*runner, &c, &ns).map_err(|e| e.to_string()));
                f.pods_for = (c, ns);
            } else {
                f.docker = Some(discover::docker_containers(&*runner).map_err(|e| e.to_string()));
            }
            f.loading = false;
            if let Ok(mut slot) = slot.lock() {
                *slot = f;
            }
            ctx.request_repaint();
        });
    }

    fn from_options(o: &ContainerOptions, protocol: Protocol) -> ContainerForm {
        let mut f = ContainerForm::default();
        if protocol == Protocol::Docker {
            f.container = o.container.clone();
        } else {
            f.pod_container = o.container.clone();
            f.context = o.context.clone();
            f.namespace = o.namespace.clone();
            f.pod = o.pod.clone();
        }
        f
    }

    fn problem(&self, protocol: Protocol) -> Option<&'static str> {
        match protocol {
            Protocol::Docker if self.container.trim().is_empty() => {
                Some(t!("connect.problem.container"))
            }
            Protocol::Kubernetes if self.pod.trim().is_empty() => Some(t!("connect.problem.pod")),
            _ => None,
        }
    }
}

/// One row of a picker list.
struct PickRow {
    title: String,
    detail: String,
    dot: Color32,
    dimmed: bool,
    selected: bool,
}

/// A small scrolling list to pick from; returns the clicked row.
fn pick_list(ui: &mut egui::Ui, id: &str, rows: &[PickRow], empty: &str) -> Option<usize> {
    let p = pal(ui);
    let mut clicked = None;
    let frame = egui::Frame::new()
        .fill(p.content)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(radius::CONTROL))
        .inner_margin(egui::Margin::same(space::XS as i8));
    // Inside a (horizontal) form row: stack the rows, and fit the frame.
    let inner_w = ui.available_width() - 2.0 * space::XS - 2.0;
    ui.vertical(|ui| {
        frame.show(ui, |ui| {
            ui.set_width(inner_w);
            if rows.is_empty() {
                ui.add_space(space::SM);
                ui.horizontal(|ui| {
                    ui.add_space(space::SM);
                    ui.label(
                        RichText::new(empty)
                            .font(theme::regular(text::SMALL))
                            .color(p.text_tertiary),
                    );
                });
                ui.add_space(space::SM);
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt(id)
                .min_scrolled_height(104.0)
                .max_height(132.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for (i, r) in rows.iter().enumerate() {
                        let w = ui.available_width();
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::click());
                        // VoiceOver: "Public, Everyone, selected".
                        let spoken = if r.detail.is_empty() {
                            r.title.clone()
                        } else {
                            format!("{}, {}", r.title, r.detail)
                        };
                        crate::a11y::name(&resp, egui::accesskit::Role::ListBoxOption, &spoken);
                        if r.selected {
                            resp.ctx
                                .accesskit_node_builder(resp.id, |n| n.set_selected(true));
                        }
                        if r.selected {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(radius::ROW),
                                p.accent_soft,
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(radius::ROW),
                                p.hover,
                            );
                        }
                        ui.painter().circle_filled(
                            egui::pos2(rect.left() + 12.0, rect.center().y),
                            3.5,
                            r.dot,
                        );
                        let fg = if r.dimmed { p.text_secondary } else { p.text };
                        let mut job = egui::text::LayoutJob::default();
                        job.append(
                            &r.title,
                            0.0,
                            egui::TextFormat::simple(theme::regular(text::BODY), fg),
                        );
                        job.append(
                            &r.detail,
                            8.0,
                            egui::TextFormat::simple(theme::regular(text::SMALL), p.text_tertiary),
                        );
                        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 30.0);
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(
                            egui::pos2(rect.left() + 22.0, rect.center().y - g.size().y / 2.0),
                            g,
                            fg,
                        );
                        if resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                });
        })
    });
    clicked
}

pub struct ConnectForm {
    pub side: Side,
    /// Where the server opens ("…in pane 3."), set by the app; `None`:
    /// left / right by `side`.
    pub pane_note: Option<String>,
    pub protocol: Protocol,
    pub name: String,
    pub host: String,
    pub port: String,
    pub user: String,
    pub auth_kind: AuthKind,
    pub password: String,
    pub key_path: String,
    pub passphrase: String,
    pub initial_path: String,
    pub save_favorite: bool,
    /// Keep the password/passphrase in the Keychain (favorite).
    pub remember_password: bool,
    /// The favorite being edited has its secret in the Keychain: an empty
    /// password field means "keep the saved one".
    pub stored_secret: bool,
    /// Favorite being edited (replaced on save instead of appended).
    pub editing_favorite: Option<usize>,
    pub connecting: bool,
    pub error: Option<String>,
    /// Show per-field validation (after the first submit attempt).
    pub show_validation: bool,
    pub testing: bool,
    /// Result of "Test Connection": Ok(home dir) or the error.
    pub test_result: Option<Result<String, String>>,
    pub s3: Box<S3Form>,
    pub ctr: Box<ContainerForm>,
    /// FTPS: protection of the data connections.
    pub ftp_data: FtpDataChannel,
    pub dav: Box<DavForm>,
    pub smb: Box<SmbForm>,
    /// Offer Manta's own Google Drive connection: only when the user set
    /// up an OAuth client of their own (Settings → Cloud → Expert). Manta
    /// ships none; normally Google Drive comes through Google Drive for
    /// desktop (sidebar → Cloud).
    pub gdrive: bool,
    focus_requested: bool,
}

/// Parts recognised in a pasted server address like
/// `sftp://deploy@web-01:2222/var/www`.
#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedHost {
    protocol: Option<Protocol>,
    user: Option<String>,
    host: String,
    port: Option<String>,
    path: Option<String>,
}

fn parse_host(input: &str) -> ParsedHost {
    let mut rest = input.trim();
    let mut out = ParsedHost::default();
    for (scheme, proto) in [
        ("sftp://", Protocol::Sftp),
        ("ssh://", Protocol::Sftp),
        ("ftps://", Protocol::Ftps),
        ("ftp://", Protocol::Ftp),
        ("https://", Protocol::WebDavs),
        ("http://", Protocol::WebDav),
        ("webdavs://", Protocol::WebDavs),
        ("webdav://", Protocol::WebDav),
        ("davs://", Protocol::WebDavs),
        ("dav://", Protocol::WebDav),
        ("smb://", Protocol::Smb),
    ] {
        if rest.len() >= scheme.len() && rest[..scheme.len()].eq_ignore_ascii_case(scheme) {
            out.protocol = Some(proto);
            rest = &rest[scheme.len()..];
            break;
        }
    }
    if let Some(i) = rest.find('/') {
        let path = &rest[i..];
        if path.len() > 1 {
            out.path = Some(path.to_string());
        }
        rest = &rest[..i];
    }
    if let Some(i) = rest.rfind('@') {
        out.user = Some(rest[..i].to_string()).filter(|u| !u.is_empty());
        rest = &rest[i + 1..];
    }
    // host:port (but leave bare IPv6 addresses alone)
    if let Some(i) = rest.rfind(':') {
        let (h, p) = (&rest[..i], &rest[i + 1..]);
        if !h.contains(':') && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            out.port = Some(p.to_string());
            rest = h;
        }
    }
    out.host = rest.to_string();
    out
}

impl ConnectForm {
    pub fn new(side: Side) -> Self {
        ConnectForm {
            side,
            pane_note: None,
            protocol: Protocol::Sftp,
            name: String::new(),
            host: String::new(),
            port: ConnectionConfig::default_port(Protocol::Sftp).to_string(),
            user: std::env::var("USER").unwrap_or_default(),
            auth_kind: AuthKind::Password,
            password: String::new(),
            key_path: dirs::home_dir()
                .map(|h| h.join(".ssh/id_ed25519").display().to_string())
                .unwrap_or_default(),
            passphrase: String::new(),
            initial_path: String::new(),
            save_favorite: false,
            remember_password: false,
            stored_secret: false,
            editing_favorite: None,
            connecting: false,
            error: None,
            show_validation: false,
            testing: false,
            test_result: None,
            s3: Box::default(),
            ctr: Box::default(),
            ftp_data: FtpDataChannel::default(),
            dav: Box::default(),
            smb: Box::default(),
            gdrive: false,
            focus_requested: false,
        }
    }

    /// The favorite's secret is in the Keychain (`stored`).
    pub fn set_stored_secret(&mut self, stored: bool) {
        self.stored_secret = stored;
        if stored {
            self.remember_password = true;
        }
    }

    /// Placeholder of a secret field: says that the Keychain has it.
    fn secret_hint(&self) -> &'static str {
        if self.stored_secret {
            t!("connect.secret.stored")
        } else {
            ""
        }
    }

    pub fn from_config(cfg: &ConnectionConfig, side: Side, favorite: Option<usize>) -> Self {
        let mut f = ConnectForm::new(side);
        f.protocol = cfg.protocol;
        f.name = cfg.name.clone();
        f.host = cfg.host.clone();
        f.port = cfg.port.to_string();
        f.user = cfg.username.clone();
        f.initial_path = cfg.initial_path.clone().unwrap_or_default();
        f.ftp_data = cfg.ftp.as_ref().map(|o| o.data_channel).unwrap_or_default();
        f.editing_favorite = favorite;
        f.save_favorite = favorite.is_some();
        match &cfg.auth {
            Auth::None => f.auth_kind = AuthKind::Anonymous,
            Auth::Password(p) => {
                f.auth_kind = AuthKind::Password;
                f.password = p.clone();
                f.remember_password = !p.is_empty();
            }
            Auth::KeyFile { path, passphrase } => {
                f.auth_kind = AuthKind::KeyFile;
                f.key_path = path.clone();
                f.passphrase = passphrase.clone().unwrap_or_default();
                f.remember_password = passphrase.is_some();
            }
            Auth::Agent => f.auth_kind = AuthKind::Agent,
        }
        if let Some(o) = cfg
            .container
            .as_ref()
            .filter(|_| cfg.protocol.is_container())
        {
            f.ctr = Box::new(ContainerForm::from_options(o, cfg.protocol));
            f.auth_kind = AuthKind::Anonymous;
        } else if cfg.protocol == Protocol::Docker {
            f.ctr.container = cfg.host.clone();
        }
        if cfg.protocol.is_webdav() {
            let o = cfg.webdav.as_deref().cloned().unwrap_or_default();
            f.dav = Box::new(DavForm::from_options(
                &o,
                cfg.protocol == Protocol::WebDavs,
                &cfg.username,
            ));
        }
        if let Some(o) = cfg.smb.as_ref().filter(|_| cfg.protocol == Protocol::Smb) {
            f.smb.share = o.share.clone();
        }
        if let Some(o) = cfg.s3.as_ref().filter(|_| cfg.protocol == Protocol::S3) {
            f.s3 = Box::new(S3Form::from_options(o));
            if o.profile.is_some() {
                f.auth_kind = AuthKind::AwsProfile;
            }
        }
        f
    }

    /// Per-field problems of the S3 tab.
    fn s3_key_problem(&self) -> Option<&'static str> {
        if self.auth_kind != AuthKind::Password {
            return None;
        }
        if self.user.trim().is_empty() {
            Some(t!("connect.problem.access_key"))
        } else if self.password.is_empty() && !self.stored_secret {
            Some(t!("connect.problem.secret_key"))
        } else {
            None
        }
    }

    fn s3_config(&self) -> Result<ConnectionConfig, String> {
        if let Some(e) = self.s3.account_problem().or(self.s3_key_problem()) {
            return Err(e.to_string());
        }
        if let Some(e) = self.s3.endpoint_problem() {
            return Err(e);
        }
        let s3 = &self.s3;
        // AWS with an automatic endpoint: let the backend derive it from
        // the (possibly profile-provided) region.
        let aws_auto = s3.provider == "aws" && s3.endpoint_auto;
        let endpoint = if aws_auto || s3.endpoint.trim().is_empty() {
            Endpoint::aws(if s3.region.trim().is_empty() {
                "us-east-1"
            } else {
                s3.region.trim()
            })
        } else {
            Endpoint::parse(&s3.endpoint)?
        };
        let port = match endpoint.host.rsplit_once(':') {
            Some((_, p)) if p.chars().all(|c| c.is_ascii_digit()) => p.parse().unwrap_or(443),
            _ if endpoint.https => 443,
            _ => 80,
        };
        let (username, auth, profile) = match self.auth_kind {
            AuthKind::AwsProfile => (
                String::new(),
                Auth::None,
                Some(s3.profile.trim().to_string()),
            ),
            AuthKind::Anonymous => (String::new(), Auth::None, None),
            _ => (
                self.user.trim().to_string(),
                Auth::Password(self.password.clone()),
                None,
            ),
        };
        Ok(ConnectionConfig {
            name: self.name.trim().to_string(),
            protocol: Protocol::S3,
            host: endpoint.hostname().to_string(),
            port,
            username,
            auth,
            initial_path: None,
            s3: Some(Box::new(S3Options {
                provider: s3.provider.to_string(),
                endpoint: if aws_auto {
                    String::new()
                } else {
                    s3.endpoint.trim().to_string()
                },
                region: s3.region.trim().to_string(),
                bucket: s3.bucket.trim().trim_matches('/').to_string(),
                path_style: s3.path_style,
                profile,
                account_id: s3.account.trim().to_string(),
            })),
            container: None,
            ftp: None,
            webdav: None,
            smb: None,
        })
    }

    fn container_config(&self) -> Result<ConnectionConfig, String> {
        if let Some(e) = self.ctr.problem(self.protocol) {
            return Err(e.to_string());
        }
        let c = &self.ctr;
        let mut cfg = if self.protocol == Protocol::Docker {
            manta_vfs::container::docker_config(c.container.trim())
        } else {
            manta_vfs::container::kube_config(
                c.context.trim(),
                c.namespace.trim(),
                c.pod.trim(),
                c.pod_container.trim(),
            )
        };
        cfg.name = self.name.trim().to_string();
        let folder = self.initial_path.trim();
        cfg.initial_path = (!folder.is_empty()).then(|| folder.to_string());
        Ok(cfg)
    }

    /// What the dialog connects to, for "Connecting to …".
    fn target_label(&self) -> String {
        match self.protocol {
            Protocol::Docker => self.ctr.container.trim().to_string(),
            Protocol::Kubernetes => self.ctr.pod.trim().to_string(),
            Protocol::Smb if !self.smb.share.trim().is_empty() => {
                format!("{}/{}", self.host.trim(), self.smb.share.trim())
            }
            _ => self.host.trim().to_string(),
        }
    }

    /// Google Drive (Expert, own OAuth client): the account and a start
    /// folder; an empty account signs in with the browser.
    fn gdrive_fields(&mut self, ui: &mut egui::Ui) {
        field(ui, t!("connect.gdrive.account"), |ui| {
            widgets::show_input(
                ui,
                widgets::input(&mut self.user).desired_width(f32::INFINITY),
                t!("connect.gdrive.account.hint"),
            );
        });
        field(ui, t!("connect.folder"), |ui| {
            widgets::show_input(
                ui,
                widgets::input(&mut self.initial_path).desired_width(f32::INFINITY),
                t!("connect.gdrive.folder.hint"),
            );
        });
        field_note_wrapped(ui, t!("connect.gdrive.note"));
    }

    fn container_fields(&mut self, ui: &mut egui::Ui, show: bool) {
        let p = pal(ui);
        field(ui, t!("connect.engine"), |ui| {
            let mut proto = self.protocol;
            if widgets::segmented(
                ui,
                "connect-container-engine",
                &mut proto,
                &[
                    (Protocol::Docker, "Docker"),
                    (Protocol::Kubernetes, "Kubernetes"),
                ],
            ) {
                self.protocol = proto;
                self.test_result = None;
            }
        });
        let kube = self.protocol == Protocol::Kubernetes;
        self.ctr.request(ui.ctx(), kube);
        let found = self.ctr.found();
        if !kube {
            field(ui, t!("connect.container"), |ui| {
                widgets::show_input(
                    ui,
                    widgets::input(&mut self.ctr.container).desired_width(f32::INFINITY),
                    t!("connect.container.hint"),
                );
            });
            if show {
                field_problem(ui, self.ctr.problem(self.protocol));
            }
            let list = match &found.docker {
                Some(Ok(l)) => l.clone(),
                _ => Vec::new(),
            };
            let empty = match &found.docker {
                None => t!("connect.containers.loading").to_string(),
                Some(Err(e)) if e.contains("daemon not running") => {
                    t!("connect.containers.docker_down").into()
                }
                Some(Err(e)) => e.clone(),
                Some(Ok(_)) => t!("connect.containers.none").into(),
            };
            let rows: Vec<PickRow> = list
                .iter()
                .map(|c| PickRow {
                    title: c.name.clone(),
                    detail: if c.running() {
                        crate::containers::short_image(&c.image).to_string()
                    } else {
                        t!(
                            "connect.containers.stopped",
                            image = crate::containers::short_image(&c.image)
                        )
                    },
                    dot: if c.running() {
                        p.success
                    } else {
                        p.text_tertiary
                    },
                    dimmed: !c.running(),
                    selected: self.ctr.container == c.name || self.ctr.container == c.id,
                })
                .collect();
            field(ui, "", |ui| {
                if let Some(i) = pick_list(ui, "connect-docker-list", &rows, &empty) {
                    // By name, so a favorite survives re-creating the container.
                    self.ctr.container = list[i].name.clone();
                    self.test_result = None;
                }
            });
            field_note(ui, t!("connect.containers.saved_by_name"));
        } else {
            let current = found.current_context.clone().unwrap_or_default();
            field(ui, t!("connect.k8s.context"), |ui| {
                let shown = if self.ctr.context.is_empty() {
                    if current.is_empty() {
                        t!("connect.k8s.current_context_lower").to_string()
                    } else {
                        t!("connect.k8s.current_suffix", name = current)
                    }
                } else {
                    self.ctr.context.clone()
                };
                let mut chosen = self.ctr.context.clone();
                egui::ComboBox::from_id_salt("connect-k8s-context")
                    .width(ui.available_width())
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut chosen,
                            String::new(),
                            t!("connect.k8s.current_context"),
                        );
                        for c in &found.contexts {
                            ui.selectable_value(&mut chosen, c.clone(), c);
                        }
                    });
                if chosen != self.ctr.context {
                    self.ctr.context = chosen;
                    self.ctr.namespace.clear();
                    self.ctr.pod.clear();
                    self.test_result = None;
                }
            });
            field(ui, t!("connect.k8s.namespace"), |ui| {
                let shown = if self.ctr.namespace.is_empty() {
                    found.pods_for.1.clone()
                } else {
                    self.ctr.namespace.clone()
                };
                let mut chosen = shown.clone();
                egui::ComboBox::from_id_salt("connect-k8s-namespace")
                    .width(ui.available_width())
                    .selected_text(if shown.is_empty() {
                        "default".to_string()
                    } else {
                        shown.clone()
                    })
                    .show_ui(ui, |ui| {
                        for n in &found.namespaces {
                            ui.selectable_value(&mut chosen, n.clone(), n);
                        }
                    });
                if chosen != shown {
                    self.ctr.namespace = chosen;
                    self.ctr.pod.clear();
                    self.test_result = None;
                }
            });
            let pods = match &found.pods {
                Some(Ok(l)) => l.clone(),
                _ => Vec::new(),
            };
            let empty = match &found.pods {
                None => t!("connect.k8s.loading_pods").to_string(),
                Some(Err(e)) => e.clone(),
                Some(Ok(_)) => t!("connect.k8s.no_pods").into(),
            };
            // One row per container.
            let mut targets: Vec<(String, String)> = Vec::new();
            let mut rows: Vec<PickRow> = Vec::new();
            for pod in &pods {
                let running = pod.phase == "Running";
                let containers = if pod.containers.is_empty() {
                    vec![String::new()]
                } else {
                    pod.containers.clone()
                };
                for c in containers {
                    let detail = match (pod.containers.len() > 1, running) {
                        (true, true) => c.clone(),
                        (true, false) => format!("{c} · {}", pod_phase(&pod.phase)),
                        (false, true) => String::new(),
                        (false, false) => pod_phase(&pod.phase).to_string(),
                    };
                    rows.push(PickRow {
                        title: pod.name.clone(),
                        detail,
                        dot: match pod.phase.as_str() {
                            "Running" => p.success,
                            "Pending" => p.warning,
                            "Failed" | "Unknown" => p.danger,
                            _ => p.text_tertiary,
                        },
                        dimmed: !running,
                        selected: self.ctr.pod == pod.name
                            && (self.ctr.pod_container == c
                                || (pod.containers.len() <= 1
                                    && self.ctr.pod_container.is_empty())),
                    });
                    targets.push((pod.name.clone(), c));
                }
            }
            field(ui, t!("connect.k8s.pod"), |ui| {
                if let Some(i) = pick_list(ui, "connect-k8s-pods", &rows, &empty) {
                    let (pod, c) = targets[i].clone();
                    self.ctr.pod = pod;
                    self.ctr.pod_container = c;
                    if self.ctr.namespace.is_empty() {
                        self.ctr.namespace = found.pods_for.1.clone();
                    }
                    self.test_result = None;
                }
            });
            if show {
                field_problem(ui, self.ctr.problem(self.protocol));
            }
        }
        field(ui, t!("connect.folder"), |ui| {
            widgets::show_input(
                ui,
                widgets::input(&mut self.initial_path).desired_width(f32::INFINITY),
                t!("connect.folder.hint_workdir"),
            );
        });
    }

    /// Per-field problems, for inline validation.
    fn host_problem(&self) -> Option<&'static str> {
        parse_host(&self.host)
            .host
            .is_empty()
            .then_some(t!("connect.problem.host"))
    }

    fn port_problem(&self) -> Option<&'static str> {
        let parsed = parse_host(&self.host);
        let port = parsed.port.unwrap_or_else(|| self.port.trim().to_string());
        match port.parse::<u16>() {
            Ok(p) if p > 0 => None,
            _ => Some(t!("connect.problem.port")),
        }
    }

    fn share_problem(&self) -> Option<&'static str> {
        (self.protocol == Protocol::Smb
            && self.smb.share.trim().trim_matches('/').is_empty()
            && parse_host(&self.host)
                .path
                .is_none_or(|p| p.trim_matches('/').is_empty()))
        .then_some(t!("connect.problem.share"))
    }

    fn key_problem(&self) -> Option<&'static str> {
        (self.auth_kind == AuthKind::KeyFile && self.key_path.trim().is_empty())
            .then_some(t!("connect.problem.key_file"))
    }

    pub fn to_config(&self) -> Result<ConnectionConfig, String> {
        if self.protocol == Protocol::GoogleDrive {
            // No account yet: the app signs in with the browser first.
            let mut cfg =
                ConnectionConfig::google_drive(self.user.trim(), Some(self.initial_path.trim()));
            if !self.name.trim().is_empty() || self.user.trim().is_empty() {
                cfg.name = self.name.trim().to_string();
            }
            return Ok(cfg);
        }
        if self.protocol == Protocol::S3 {
            return self.s3_config();
        }
        if self.protocol.is_container() {
            return self.container_config();
        }
        if let Some(e) = self
            .host_problem()
            .or(self.port_problem())
            .or(self.key_problem())
            .or(self.share_problem())
        {
            return Err(e.to_string());
        }
        let parsed = parse_host(&self.host);
        let port: u16 = parsed
            .port
            .as_deref()
            .unwrap_or(self.port.trim())
            .parse()
            .map_err(|_| t!("connect.problem.port").to_string())?;
        let protocol = parsed.protocol.unwrap_or(self.protocol);
        let auth = match self.auth_kind {
            AuthKind::Password => Auth::Password(self.password.clone()),
            AuthKind::KeyFile => Auth::KeyFile {
                path: self.key_path.trim().to_string(),
                passphrase: (!self.passphrase.is_empty()).then(|| self.passphrase.clone()),
            },
            AuthKind::Agent => Auth::Agent,
            AuthKind::Anonymous | AuthKind::AwsProfile => Auth::None,
        };
        let initial = self.initial_path.trim();
        let typed_initial = (!initial.is_empty()).then(|| initial.to_string());
        let mut webdav = None;
        let mut smb = None;
        let initial = if protocol.is_webdav() {
            // A pasted URL's path is the WebDAV folder, not a start folder.
            let root = match parsed.path {
                Some(p) if self.dav.root.trim().is_empty() || self.dav.root_auto => p,
                _ => self.dav.root.clone(),
            };
            webdav = Some(Box::new(WebDavOptions {
                root: manta_vfs::webdav::normalize_root(&root),
                preset: self.dav.preset.to_string(),
            }));
            typed_initial
        } else if protocol == Protocol::Smb {
            // smb://server/share/folder: the share, then the folder.
            let mut share = self.smb.share.trim().trim_matches('/').to_string();
            let mut rest = None;
            if share.is_empty() {
                if let Some(p) = parsed.path {
                    let p = p.trim_matches('/');
                    let (s, r) = p.split_once('/').unwrap_or((p, ""));
                    share = manta_vfs::webdav::decode(s);
                    rest = (!r.is_empty()).then(|| format!("/{}", manta_vfs::webdav::decode(r)));
                }
            }
            smb = Some(Box::new(SmbOptions { share }));
            typed_initial.or(rest)
        } else {
            typed_initial.or(parsed.path)
        };
        Ok(ConnectionConfig {
            name: self.name.trim().to_string(),
            protocol,
            host: parsed.host,
            port,
            username: parsed.user.unwrap_or_else(|| self.user.trim().to_string()),
            auth,
            initial_path: initial,
            s3: None,
            container: None,
            ftp: (protocol == Protocol::Ftps).then(|| {
                Box::new(FtpOptions {
                    data_channel: self.ftp_data,
                })
            }),
            webdav,
            smb,
        })
    }

    /// Tests: switch tabs like a click would (Docker / Kubernetes too).
    #[cfg(test)]
    pub fn set_protocol_for_tests(&mut self, p: Protocol) {
        self.set_protocol(p);
        self.protocol = p;
    }

    fn set_protocol(&mut self, p: Protocol) {
        if p == Protocol::GoogleDrive {
            if self.protocol != p && !self.protocol.is_cloud() {
                self.user.clear();
            }
            self.protocol = p;
            self.auth_kind = AuthKind::Anonymous;
            return;
        }
        if self.protocol == Protocol::GoogleDrive && self.user.is_empty() {
            self.user = std::env::var("USER").unwrap_or_default();
        }
        if p.is_container() {
            if !self.protocol.is_container() {
                self.protocol = p;
                self.auth_kind = AuthKind::Anonymous;
            }
            return;
        }
        let old_default = ConnectionConfig::default_port(self.protocol).to_string();
        if self.port.trim().is_empty() || self.port.trim() == old_default {
            self.port = ConnectionConfig::default_port(p).to_string();
        }
        // The S3 key ID isn't the login name.
        let login = std::env::var("USER").unwrap_or_default();
        if p == Protocol::S3 && self.protocol != Protocol::S3 && self.user == login {
            self.user.clear();
        } else if p != Protocol::S3 && self.protocol == Protocol::S3 && self.user.is_empty() {
            self.user = login;
        }
        if p.is_webdav() {
            self.dav.https = p == Protocol::WebDavs;
        }
        self.protocol = p;
        let allowed = allowed_auth(p);
        if !allowed.contains(&self.auth_kind) {
            self.auth_kind = allowed[0];
        }
    }

    /// Split a pasted `user@host:port` into the separate fields.
    fn absorb_pasted_address(&mut self) {
        if self.protocol.is_container() {
            return;
        }
        let parsed = parse_host(&self.host);
        if parsed.protocol.is_none()
            && parsed.user.is_none()
            && parsed.port.is_none()
            && parsed.path.is_none()
        {
            return;
        }
        if let Some(p) = parsed.protocol {
            self.set_protocol(p);
        }
        if let Some(u) = parsed.user {
            self.user = u;
        }
        if let Some(port) = parsed.port {
            self.port = port;
        }
        if let Some(path) = parsed.path {
            if self.protocol.is_webdav() {
                // https://cloud.example.com/remote.php/dav/files/alice
                let root = manta_vfs::webdav::normalize_root(&path);
                let next = manta_vfs::webdav::preset_root("nextcloud", &self.user);
                if next.as_deref() == Some(root.as_str()) {
                    self.dav.preset = "nextcloud";
                    self.dav.root_auto = true;
                } else {
                    self.dav.preset = "generic";
                    self.dav.root_auto = false;
                }
                self.dav.root = root;
            } else if self.protocol == Protocol::Smb {
                let p = path.trim_matches('/').to_string();
                let (share, rest) = p.split_once('/').unwrap_or((&p, ""));
                self.smb.share = manta_vfs::webdav::decode(share);
                if !rest.is_empty() && self.initial_path.trim().is_empty() {
                    self.initial_path = format!("/{}", manta_vfs::webdav::decode(rest));
                }
            } else if self.initial_path.trim().is_empty() {
                self.initial_path = path;
            }
        }
        self.host = parsed.host;
    }

    fn server_fields(&mut self, ui: &mut egui::Ui, show: bool) {
        field(ui, t!("connect.server"), |ui| {
            let w = ui.available_width() - 64.0;
            let host = widgets::show_input(
                ui,
                widgets::input(&mut self.host).desired_width(w),
                t!("connect.server.hint"),
            );
            if !self.focus_requested {
                host.request_focus();
                self.focus_requested = true;
            }
            if host.lost_focus() {
                self.absorb_pasted_address();
            }
            if host.changed() {
                self.test_result = None;
            }
            widgets::show_input(
                ui,
                widgets::input(&mut self.port).desired_width(48.0),
                t!("connect.port.hint"),
            )
            .on_hover_text(t!("connect.port"));
        });
        if show {
            field_problem(ui, self.host_problem().or(self.port_problem()));
        }
        if self.protocol.is_webdav() {
            self.webdav_security(ui);
        }
        field(ui, t!("connect.user"), |ui| {
            let hint = if self.auth_kind == AuthKind::Anonymous {
                t!("connect.user.anonymous")
            } else {
                t!("connect.user.hint")
            };
            let r = widgets::show_input(
                ui,
                widgets::input(&mut self.user).desired_width(f32::INFINITY),
                hint,
            );
            if r.changed() {
                self.dav.refresh_root(&self.user);
            }
        });
        field(ui, t!("connect.sign_in_with"), |ui| {
            let smb = self.protocol == Protocol::Smb;
            let opts: Vec<(AuthKind, &str)> = allowed_auth(self.protocol)
                .iter()
                .map(|k| {
                    let label = if smb && *k == AuthKind::Anonymous {
                        t!("connect.auth.guest")
                    } else {
                        auth_label(*k)
                    };
                    (*k, label)
                })
                .collect();
            widgets::segmented(ui, "connect-auth", &mut self.auth_kind, &opts);
        });
        match self.auth_kind {
            AuthKind::Password => field(ui, t!("connect.password"), |ui| {
                let hint = self.secret_hint();
                widgets::show_input(
                    ui,
                    widgets::input(&mut self.password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                    hint,
                )
                .accessible_name(t!("connect.password"));
            }),
            AuthKind::KeyFile => {
                field(ui, t!("connect.key_file"), |ui| {
                    widgets::show_input(
                        ui,
                        widgets::input(&mut self.key_path).desired_width(f32::INFINITY),
                        "~/.ssh/id_ed25519",
                    );
                });
                if show {
                    field_problem(ui, self.key_problem());
                }
                field(ui, t!("connect.passphrase"), |ui| {
                    let hint = if self.stored_secret {
                        self.secret_hint()
                    } else {
                        t!("connect.passphrase.hint")
                    };
                    widgets::show_input(
                        ui,
                        widgets::input(&mut self.passphrase)
                            .password(true)
                            .desired_width(f32::INFINITY),
                        hint,
                    );
                });
            }
            AuthKind::Agent => field_note(ui, t!("connect.agent.note")),
            AuthKind::Anonymous if self.protocol == Protocol::Smb => {
                field_note_wrapped(ui, t!("connect.smb.guest.note"))
            }
            AuthKind::Anonymous | AuthKind::AwsProfile => {
                field_note(ui, t!("connect.anonymous.note"))
            }
        }
        if self.protocol.is_webdav() {
            self.webdav_fields(ui);
        }
        if self.protocol == Protocol::Smb {
            self.smb_fields(ui, show);
        }
        field(ui, t!("connect.folder"), |ui| {
            widgets::show_input(
                ui,
                widgets::input(&mut self.initial_path).desired_width(f32::INFINITY),
                if self.protocol.is_webdav() || self.protocol == Protocol::Smb {
                    t!("connect.folder.hint_top")
                } else {
                    t!("connect.folder.hint_home")
                },
            );
        });
        if self.protocol == Protocol::Ftps {
            self.ftps_fields(ui);
        }
    }

    /// WebDAV: HTTPS or plain HTTP.
    fn webdav_security(&mut self, ui: &mut egui::Ui) {
        field(ui, t!("connect.webdav.security"), |ui| {
            let mut https = self.dav.https;
            if widgets::segmented(
                ui,
                "connect-webdav-security",
                &mut https,
                &[(true, "HTTPS"), (false, t!("connect.webdav.http"))],
            ) {
                self.set_protocol(if https {
                    Protocol::WebDavs
                } else {
                    Protocol::WebDav
                });
                self.test_result = None;
            }
        });
        if !self.dav.https {
            field_note_wrapped(ui, t!("connect.webdav.http.note"));
        }
    }

    /// WebDAV: server type preset and the WebDAV folder.
    fn webdav_fields(&mut self, ui: &mut egui::Ui) {
        field(ui, t!("connect.webdav.preset"), |ui| {
            let mut chosen = self.dav.preset;
            egui::ComboBox::from_id_salt("webdav-preset")
                .width(ui.available_width())
                .selected_text(dav_preset_name(self.dav.preset))
                .show_ui(ui, |ui| {
                    for (id, _) in manta_vfs::webdav::PRESETS {
                        ui.selectable_value(&mut chosen, id, dav_preset_name(id));
                    }
                });
            if chosen != self.dav.preset {
                self.dav.set_preset(chosen, &self.user);
                self.test_result = None;
            }
        });
        field(ui, t!("connect.webdav.path"), |ui| {
            let r = widgets::show_input(
                ui,
                widgets::input(&mut self.dav.root).desired_width(f32::INFINITY),
                t!("connect.webdav.path.hint"),
            )
            .accessible_name(t!("connect.webdav.path"));
            if r.changed() {
                self.dav.root_auto = false;
                self.test_result = None;
            }
        });
        if self.dav.preset != "generic" {
            field_note_wrapped(ui, t!("connect.webdav.nextcloud.note"));
        }
    }

    /// The SMB server of the form, for the share list.
    fn smb_browse_config(&self) -> ConnectionConfig {
        let anonymous = self.auth_kind == AuthKind::Anonymous;
        ConnectionConfig {
            name: String::new(),
            protocol: Protocol::Smb,
            host: parse_host(&self.host).host,
            port: self.port.trim().parse().unwrap_or(0),
            username: if anonymous {
                String::new()
            } else {
                self.user.trim().to_string()
            },
            auth: if anonymous {
                Auth::None
            } else {
                Auth::Password(self.password.clone())
            },
            initial_path: None,
            s3: None,
            container: None,
            ftp: None,
            webdav: None,
            smb: None,
        }
    }

    /// SMB: the share, with a list of the server's shares.
    fn smb_fields(&mut self, ui: &mut egui::Ui, show: bool) {
        let mut browse = false;
        let can_browse = !parse_host(&self.host).host.is_empty();
        field(ui, t!("connect.smb.share"), |ui| {
            // The button's width (measured last frame) decides the field's.
            let bw_id = egui::Id::unique("connect-smb-browse-width");
            let bw: f32 = ui.data(|d| d.get_temp(bw_id)).unwrap_or(120.0);
            let w = (ui.available_width() - bw - ui.spacing().item_spacing.x - 2.0).max(80.0);
            let r = widgets::show_input(
                ui,
                widgets::input(&mut self.smb.share).desired_width(w),
                t!("connect.smb.share.hint"),
            )
            .accessible_name(t!("connect.smb.share"));
            if r.changed() {
                self.test_result = None;
            }
            let button = widgets::button(
                ui,
                t!("connect.smb.browse"),
                ButtonKind::Secondary,
                can_browse,
            );
            let measured = button.rect.width();
            if (measured - bw).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(bw_id, measured));
                ui.ctx().request_repaint();
            }
            browse = widgets::tooltip(button, t!("connect.smb.browse.tip"), "").clicked();
        });
        if browse {
            let cfg = self.smb_browse_config();
            self.smb.request(ui.ctx(), cfg);
        }
        if show {
            field_problem(ui, self.share_problem());
        }
        if self.smb.picking {
            let found = self.smb.found();
            match &found.result {
                _ if found.loading => {
                    field(ui, "", |ui| {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(
                            RichText::new(t!("connect.smb.loading"))
                                .font(theme::regular(text::SMALL))
                                .color(pal(ui).text_secondary),
                        );
                    });
                }
                Some(Ok(shares)) => {
                    let p = pal(ui);
                    let rows: Vec<PickRow> = shares
                        .iter()
                        .map(|s| PickRow {
                            title: s.name.clone(),
                            detail: s.comment.clone(),
                            dot: p.accent,
                            dimmed: false,
                            selected: s.name == self.smb.share.trim(),
                        })
                        .collect();
                    let mut picked = None;
                    field(ui, "", |ui| {
                        picked = pick_list(ui, "smb-shares", &rows, t!("connect.smb.none"));
                    });
                    if let Some(i) = picked {
                        self.smb.share = shares[i].name.clone();
                        self.smb.picking = false;
                        self.test_result = None;
                    }
                }
                Some(Err(e)) => {
                    let msg = t!("connect.smb.list_failed", error = e);
                    field(ui, "", |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(msg)
                                    .font(theme::regular(text::SMALL))
                                    .color(pal(ui).danger),
                            )
                            .wrap(),
                        );
                    });
                }
                None => {}
            }
        }
        field_note_wrapped(ui, t!("connect.smb.note"));
    }

    /// FTPS options: the data channel policy.
    fn ftps_fields(&mut self, ui: &mut egui::Ui) {
        field(ui, t!("connect.ftps.data"), |ui| {
            let changed = widgets::segmented(
                ui,
                "connect-ftps-data",
                &mut self.ftp_data,
                &[
                    (FtpDataChannel::Auto, t!("connect.ftps.data.auto")),
                    (FtpDataChannel::Require, t!("connect.ftps.data.require")),
                    (FtpDataChannel::Off, t!("connect.ftps.data.off")),
                ],
            );
            if changed {
                self.test_result = None;
            }
        });
        field_note_wrapped(
            ui,
            match self.ftp_data {
                FtpDataChannel::Auto => t!("connect.ftps.data.auto.note"),
                FtpDataChannel::Require => t!("connect.ftps.data.require.note"),
                FtpDataChannel::Off => t!("connect.ftps.data.off.note"),
            },
        );
    }

    fn s3_fields(&mut self, ui: &mut egui::Ui, show: bool) {
        field(ui, t!("connect.s3.provider"), |ui| {
            let mut chosen = self.s3.provider;
            egui::ComboBox::from_id_salt("s3-provider")
                .width(ui.available_width())
                .selected_text(provider_name(self.s3.preset()))
                .show_ui(ui, |ui| {
                    for pr in PROVIDERS {
                        ui.selectable_value(&mut chosen, pr.id, provider_name(pr));
                    }
                });
            if chosen != self.s3.provider {
                self.s3.set_provider(chosen);
                self.test_result = None;
            }
        });
        if self.s3.preset().needs_account {
            field(ui, t!("connect.s3.account"), |ui| {
                let r = widgets::show_input(
                    ui,
                    widgets::input(&mut self.s3.account).desired_width(f32::INFINITY),
                    t!("connect.s3.account.hint"),
                );
                if r.changed() {
                    self.s3.refresh_endpoint();
                }
            });
            if show {
                field_problem(ui, self.s3.account_problem());
            }
        }
        field(ui, t!("connect.s3.region"), |ui| {
            let hint = if self.auth_kind == AuthKind::AwsProfile {
                t!("connect.s3.region.from_profile")
            } else {
                self.s3.preset().default_region
            };
            let r = widgets::show_input(
                ui,
                widgets::input(&mut self.s3.region).desired_width(f32::INFINITY),
                hint,
            );
            if r.changed() {
                self.s3.refresh_endpoint();
                self.test_result = None;
            }
        });
        field(ui, t!("connect.s3.endpoint"), |ui| {
            let r = widgets::show_input(
                ui,
                widgets::input(&mut self.s3.endpoint).desired_width(f32::INFINITY),
                if self.s3.provider == "aws" {
                    t!("connect.s3.endpoint.follows_region")
                } else {
                    "https://s3.example.com"
                },
            );
            if r.changed() {
                self.s3.endpoint_auto = false;
                self.test_result = None;
            }
        });
        if show {
            field_problem(ui, self.s3.endpoint_problem().as_deref());
        }
        field(ui, t!("connect.sign_in_with"), |ui| {
            let opts: Vec<(AuthKind, &str)> = allowed_auth(Protocol::S3)
                .iter()
                .map(|k| (*k, s3_auth_label(*k)))
                .collect();
            widgets::segmented(ui, "connect-s3-auth", &mut self.auth_kind, &opts);
        });
        match self.auth_kind {
            AuthKind::AwsProfile => {
                self.s3.request_profiles(ui.ctx());
                let profiles = self.s3.profiles.lock().unwrap().clone();
                field(ui, t!("connect.s3.profile"), |ui| match &profiles {
                    None => {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                    Some(list) => {
                        let shown = if self.s3.profile.is_empty() {
                            "default"
                        } else {
                            self.s3.profile.as_str()
                        };
                        egui::ComboBox::from_id_salt("s3-profile")
                            .width(ui.available_width())
                            .selected_text(shown.to_string())
                            .show_ui(ui, |ui| {
                                for name in list {
                                    let value = if name == "default" {
                                        String::new()
                                    } else {
                                        name.clone()
                                    };
                                    ui.selectable_value(&mut self.s3.profile, value, name);
                                }
                            });
                    }
                });
                let note = match &profiles {
                    Some(l) if l.is_empty() => t!("connect.s3.no_profiles"),
                    _ => t!("connect.s3.profiles_note"),
                };
                field_note(ui, note);
            }
            AuthKind::Anonymous => field_note(ui, t!("connect.s3.public_note")),
            _ => {
                field(ui, t!("connect.s3.access_key_id"), |ui| {
                    widgets::show_input(
                        ui,
                        widgets::input(&mut self.user).desired_width(f32::INFINITY),
                        "AKIA…",
                    );
                });
                field(ui, t!("connect.s3.secret_key"), |ui| {
                    let hint = self.secret_hint();
                    widgets::show_input(
                        ui,
                        widgets::input(&mut self.password)
                            .password(true)
                            .desired_width(f32::INFINITY),
                        hint,
                    )
                    .accessible_name(t!("connect.s3.secret_access_key"));
                });
                if show {
                    field_problem(ui, self.s3_key_problem());
                }
            }
        }
        field(ui, t!("connect.s3.bucket"), |ui| {
            widgets::show_input(
                ui,
                widgets::input(&mut self.s3.bucket).desired_width(f32::INFINITY),
                t!("connect.s3.bucket.hint"),
            );
        });
        field(ui, "", |ui| {
            ui.checkbox(&mut self.s3.path_style, t!("connect.s3.path_style"))
                .on_hover_text(t!("connect.s3.path_style.tip"));
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let busy = self.connecting || self.testing;
        let modal = dialog_modal("connect-dialog", ctx).show(ctx, |ui| {
            ui.set_width(440.0);
            let subtitle = match &self.pane_note {
                Some(n) => n.as_str(),
                None if self.side == Side::RIGHT => t!("connect.subtitle.right"),
                None => t!("connect.subtitle.left"),
            };
            let title = if self.editing_favorite.is_some() {
                t!("connect.title.edit")
            } else {
                t!("connect.title")
            };
            dialog_header(
                ui,
                match self.protocol {
                    Protocol::S3 => icons::CLOUD,
                    Protocol::GoogleDrive => icons::GOOGLE_DRIVE_LOGO,
                    Protocol::Docker => icons::SHIPPING_CONTAINER,
                    Protocol::Kubernetes => icons::STEERING_WHEEL,
                    Protocol::Smb => icons::NETWORK,
                    _ => icons::GLOBE,
                },
                p.accent_text,
                p.accent_soft,
                title,
                subtitle,
            );
            ui.add_space(space::LG);

            ui.add_enabled_ui(!busy, |ui| {
                field(ui, t!("connect.protocol"), |ui| {
                    // Docker and Kubernetes share the Container tab.
                    // WebDAV over HTTP and HTTPS share the WebDAV tab.
                    let shown = if self.protocol.is_container() {
                        Protocol::Docker
                    } else if self.protocol.is_webdav() {
                        Protocol::WebDavs
                    } else {
                        self.protocol
                    };
                    let mut proto = shown;
                    let mut tabs: Vec<(Protocol, &str)> = vec![
                        (Protocol::Sftp, "SFTP"),
                        (Protocol::Ftp, "FTP"),
                        (Protocol::Ftps, "FTPS"),
                        (Protocol::WebDavs, "WebDAV"),
                        (Protocol::Smb, "SMB"),
                        (Protocol::S3, "S3"),
                        (Protocol::Docker, t!("connect.protocol.container")),
                    ];
                    if self.gdrive || self.protocol == Protocol::GoogleDrive {
                        tabs.push((Protocol::GoogleDrive, "Google Drive"));
                    }
                    if widgets::segmented(ui, "connect-protocol", &mut proto, &tabs)
                        && proto != shown
                    {
                        if proto == Protocol::WebDavs && !self.dav.https {
                            proto = Protocol::WebDav;
                        }
                        self.set_protocol(proto);
                        self.dav.refresh_root(&self.user);
                    }
                });
                let show = self.show_validation;
                if self.protocol == Protocol::S3 {
                    self.s3_fields(ui, show);
                } else if self.protocol == Protocol::GoogleDrive {
                    self.gdrive_fields(ui);
                } else if self.protocol.is_container() {
                    self.container_fields(ui, show);
                } else {
                    self.server_fields(ui, show);
                }
                ui.add_space(space::SM);
                field(ui, "", |ui| {
                    ui.vertical(|ui| {
                        ui.checkbox(&mut self.save_favorite, t!("connect.add_favorite"));
                        if self.save_favorite {
                            ui.add_space(space::XS);
                            widgets::show_input(
                                ui,
                                widgets::input(&mut self.name).desired_width(f32::INFINITY),
                                t!("connect.favorite_name.hint"),
                            );
                            if matches!(self.auth_kind, AuthKind::Password | AuthKind::KeyFile)
                                && self.protocol != Protocol::GoogleDrive
                            {
                                ui.add_space(space::XS);
                                let label = if self.protocol == Protocol::S3 {
                                    t!("connect.remember.secret")
                                } else if self.auth_kind == AuthKind::Password {
                                    t!("connect.remember.password")
                                } else {
                                    t!("connect.remember.passphrase")
                                };
                                ui.checkbox(&mut self.remember_password, label);
                                if self.remember_password {
                                    ui.label(
                                        RichText::new(t!("connect.remember.keychain"))
                                            .font(theme::regular(text::CAPTION))
                                            .color(p.text_secondary),
                                    );
                                }
                            }
                        }
                    });
                });
            });

            // Feedback: connection error, or test result.
            if let Some(err) = &self.error {
                let port = self.port.trim().parse().unwrap_or(0);
                let (title, hint) = friendly_connect_error(err, &self.host, port);
                ui.add_space(space::MD);
                banner(ui, Banner::Error, &title, hint, Some(err));
            } else if let Some(r) = &self.test_result {
                ui.add_space(space::MD);
                match r {
                    Ok(home) => {
                        let msg = match (self.protocol, home.as_str()) {
                            (Protocol::S3, "/") => t!("connect.test.s3_buckets").to_string(),
                            (Protocol::S3, b) => {
                                t!("connect.test.s3_bucket", bucket = b.trim_start_matches('/'))
                            }
                            (p, home) if p.is_container() => {
                                t!("connect.test.container", home = home)
                            }
                            (Protocol::Smb, _) => {
                                t!("connect.test.smb", share = self.smb.share.trim())
                            }
                            (p, _) if p.is_webdav() => t!("connect.test.webdav").to_string(),
                            _ => t!("connect.test.home", home = home),
                        };
                        banner(ui, Banner::Success, t!("connect.test.works"), &msg, None)
                    }
                    Err(e) => {
                        let port = self.port.trim().parse().unwrap_or(0);
                        let (title, hint) = friendly_connect_error(e, &self.host, port);
                        banner(ui, Banner::Error, &title, hint, Some(e));
                    }
                }
            }

            ui.add_space(space::LG);
            ui.horizontal(|ui| {
                // macOS: the help button sits bottom-left in a sheet.
                let help = match self.protocol {
                    Protocol::Sftp => "08-verbindungen.md#sftp",
                    Protocol::Ftp | Protocol::Ftps => "08-verbindungen.md#ftp",
                    Protocol::S3 => "08-verbindungen.md#s3",
                    Protocol::WebDav | Protocol::WebDavs => "08-verbindungen.md#webdav",
                    Protocol::Smb => "08-verbindungen.md#smb",
                    p if p.is_container() => "08-verbindungen.md#container",
                    _ => "08-verbindungen.md#verbinden",
                };
                crate::help::help_button(ui, help);
                ui.add_space(space::XS);
                if self.testing {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(RichText::new(t!("connect.testing")).color(p.text_secondary));
                } else if self.connecting {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(
                        RichText::new(t!("connect.connecting", target = self.target_label()))
                            .color(p.text_secondary),
                    );
                } else if self.protocol != Protocol::GoogleDrive
                    && widgets::button(ui, t!("connect.test"), ButtonKind::Secondary, true)
                        .clicked()
                {
                    self.show_validation = true;
                    result = DialogResult::Test;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, t!("connect.connect"), ButtonKind::Primary, !busy)
                        .clicked()
                    {
                        self.show_validation = true;
                        result = DialogResult::Submit;
                    }
                    if widgets::button(ui, t!("common.cancel"), ButtonKind::Secondary, true)
                        .clicked()
                    {
                        result = DialogResult::Cancel;
                    }
                });
            });
        });
        if !busy && result == DialogResult::Open && ctx.input(|i| i.key_pressed(Key::Enter)) {
            self.absorb_pasted_address();
            self.show_validation = true;
            result = DialogResult::Submit;
        }
        if modal.should_close() && result == DialogResult::Open {
            result = DialogResult::Cancel;
        }
        result
    }
}

/// Plain-language title and hint for a connection error.
pub fn friendly_connect_error(err: &str, host: &str, port: u16) -> (String, &'static str) {
    let e = err.to_lowercase();
    let host = if host.trim().is_empty() {
        t!("connect.err.the_server")
    } else {
        host.trim()
    };
    let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
    if let Some(r) = container_error(&e) {
        return r;
    }
    if has(&["no such share"]) {
        return (t!("connect.err.share").into(), t!("connect.err.share.hint"));
    }
    if has(&["not a webdav folder", "no webdav folder"]) {
        return (
            t!("connect.err.webdav_path").into(),
            t!("connect.err.webdav_path.hint"),
        );
    }
    if has(&["clock is too far off", "requesttimetooskewed"]) {
        return (t!("connect.err.clock").into(), t!("connect.err.clock.hint"));
    }
    if has(&["is in region"]) {
        return (
            t!("connect.err.region").into(),
            t!("connect.err.region.hint"),
        );
    }
    if has(&[
        "unknown host key",
        "unknown server key",
        "unbekannter server-schlüssel",
    ]) {
        return (
            t!("connect.err.host_key_unknown").into(),
            t!("connect.err.host_key_unknown.hint"),
        );
    }
    if has(&[
        "host key",
        "hostkey",
        "server key",
        "server-schlüssel",
        "mismatch",
        "known_hosts",
    ]) {
        (
            t!("connect.err.host_key").into(),
            t!("connect.err.host_key.hint"),
        )
    } else if has(&[
        "authentication",
        "auth fail",
        "password",
        "publickey",
        "permission denied",
        "login incorrect",
        "530",
    ]) {
        (t!("connect.err.auth").into(), t!("connect.err.auth.hint"))
    } else if has(&[
        "resolve",
        "lookup",
        "not known",
        "nodename",
        "no such host",
        "name or service",
    ]) {
        (
            t!("connect.err.not_found", host = host),
            t!("connect.err.not_found.hint"),
        )
    } else if has(&["refused"]) {
        (
            t!("connect.err.refused", host = host, port = port),
            t!("connect.err.refused.hint"),
        )
    } else if has(&["timed out", "timeout"]) {
        (
            t!("connect.err.timeout", host = host),
            t!("connect.err.timeout.hint"),
        )
    } else if has(&["tls", "ssl", "certificate", "handshake"]) {
        (t!("connect.err.tls").into(), t!("connect.err.tls.hint"))
    } else if has(&["key file", "private key", "no such file", "passphrase"]) {
        (t!("connect.err.key").into(), t!("connect.err.key.hint"))
    } else if has(&["agent"]) {
        (t!("connect.err.agent").into(), t!("connect.err.agent.hint"))
    } else {
        (t!("connect.err.generic", host = host), "")
    }
}

/// Titles and hints for Docker / Kubernetes errors (`e` is lower case).
fn container_error(e: &str) -> Option<(String, &'static str)> {
    let r = |t: &str, h: &'static str| Some((t.to_string(), h));
    if e.contains("docker not found") {
        r(
            t!("connect.err.docker_missing"),
            t!("connect.err.docker_missing.hint"),
        )
    } else if e.contains("daemon not running") {
        r(
            t!("connect.err.docker_down"),
            t!("connect.err.docker_down.hint"),
        )
    } else if e.contains("kubectl not found") {
        r(
            t!("connect.err.kubectl_missing"),
            t!("connect.err.kubectl_missing.hint"),
        )
    } else if e.contains("kubectl context unreachable") {
        r(t!("connect.err.cluster"), t!("connect.err.cluster.hint"))
    } else if e.contains("container is not running")
        || (e.contains("pod ") && e.contains("is not running"))
    {
        r(
            t!("connect.err.container_down"),
            t!("connect.err.container_down.hint"),
        )
    } else if e.contains("not found (removed or renamed?)") || e.contains("pod not found") {
        r(
            t!("connect.err.container_gone"),
            t!("connect.err.container_gone.hint"),
        )
    } else if e.contains("has no shell") {
        r(t!("connect.err.no_shell"), t!("connect.err.no_shell.hint"))
    } else {
        None
    }
}

fn allowed_auth(p: Protocol) -> &'static [AuthKind] {
    match p {
        Protocol::Docker | Protocol::Kubernetes => &[AuthKind::Anonymous],
        Protocol::Sftp => &[AuthKind::Password, AuthKind::KeyFile, AuthKind::Agent],
        Protocol::S3 => &[
            AuthKind::Password,
            AuthKind::AwsProfile,
            AuthKind::Anonymous,
        ],
        _ => &[AuthKind::Password, AuthKind::Anonymous],
    }
}

fn auth_label(k: AuthKind) -> &'static str {
    match k {
        AuthKind::Password => t!("connect.auth.password"),
        AuthKind::KeyFile => t!("connect.auth.key_file"),
        AuthKind::Agent => t!("connect.auth.agent"),
        AuthKind::Anonymous => t!("connect.auth.anonymous"),
        AuthKind::AwsProfile => t!("connect.auth.aws_profile"),
    }
}

fn s3_auth_label(k: AuthKind) -> &'static str {
    match k {
        AuthKind::Password => t!("connect.auth.access_key"),
        AuthKind::Anonymous => t!("connect.auth.public"),
        other => auth_label(other),
    }
}

/// A Kubernetes pod phase as shown to the user.
fn pod_phase(phase: &str) -> &str {
    match phase {
        "Running" => t!("connect.k8s.phase.running"),
        "Pending" => t!("connect.k8s.phase.pending"),
        "Succeeded" => t!("connect.k8s.phase.succeeded"),
        "Failed" => t!("connect.k8s.phase.failed"),
        "Unknown" => t!("connect.k8s.phase.unknown"),
        other => other,
    }
}

/// S3 provider name; "Other S3-compatible" is translated.
fn provider_name(p: &Provider) -> &'static str {
    if p.id == "custom" {
        t!("connect.s3.provider.custom")
    } else {
        p.name
    }
}

pub fn conflict_label(c: ConflictPolicy) -> &'static str {
    match c {
        ConflictPolicy::Ask => t!("dialog.conflict.ask"),
        ConflictPolicy::Overwrite => t!("dialog.conflict.replace"),
        ConflictPolicy::Skip => t!("dialog.conflict.skip"),
        ConflictPolicy::OverwriteIfNewer => t!("dialog.conflict.if_newer"),
        ConflictPolicy::Rename => t!("dialog.conflict.keep_both"),
    }
}

fn conflict_hint(c: ConflictPolicy) -> &'static str {
    match c {
        ConflictPolicy::Ask => t!("dialog.conflict.ask.hint"),
        ConflictPolicy::Overwrite => t!("dialog.conflict.replace.hint"),
        ConflictPolicy::Skip => t!("dialog.conflict.skip.hint"),
        ConflictPolicy::OverwriteIfNewer => t!("dialog.conflict.if_newer.hint"),
        ConflictPolicy::Rename => t!("dialog.conflict.keep_both.hint"),
    }
}

/// A pane the copy / move dialog can send the items to.
pub struct TransferTarget {
    pub side: crate::app::Side,
    /// Position, 1 = leftmost.
    pub number: usize,
    pub folder: String,
    /// "Local" or the connection's label.
    pub backend: String,
    pub remote: bool,
    pub fs: SharedVfs,
    pub dir: VfsPath,
    /// Why the items can't go there (same folder, read-only …).
    pub problem: Option<String>,
}

impl TransferTarget {
    fn title(&self) -> String {
        let icon = if self.remote {
            icons::GLOBE
        } else {
            icons::HARD_DRIVE
        };
        format!("{icon}  {} · {}", self.number, self.folder)
    }
}

pub struct TransferForm {
    /// The sources are in a read-only archive: "Extract" instead of "Copy".
    pub extract: bool,
    /// The archive the sources are in, if any.
    pub archive: Option<std::sync::Arc<crate::pane::ArchiveMount>>,
    /// The pane the items come from (drag & drop: the pane dragged from).
    pub src_side: crate::app::Side,
    /// The pane they go to (`dst_*` describe its folder).
    pub dst_side: crate::app::Side,
    /// With three or more panes: every other pane, for the "To" picker
    /// (empty otherwise).
    pub targets: Vec<TransferTarget>,
    pub mv: bool,
    pub src_fs: SharedVfs,
    pub sources: Vec<VfsPath>,
    pub dst_fs: SharedVfs,
    pub dst_dir: VfsPath,
    pub dst_label: String,
    pub conflict: ConflictPolicy,
    /// Which of `sources` are folders (for their icons).
    pub folders: Vec<VfsPath>,
    /// A drop: the dialog offers Copy / Move to choose from (`mv` is the
    /// preselection). F5 / F6 already said which one.
    pub choose: bool,
}

impl TransferForm {
    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let verb = match (self.mv, self.extract) {
            (true, _) => t!("dialog.transfer.move"),
            (false, true) => t!("dialog.transfer.extract"),
            (false, false) => t!("dialog.transfer.copy"),
        };
        let n = self.sources.len();
        let dst_name = folder_name(&self.dst_dir);
        let modal = dialog_modal("transfer-dialog", ctx).show(ctx, |ui| {
            ui.set_width(440.0);
            let name = self
                .sources
                .first()
                .map(|s| s.file_name().to_string())
                .unwrap_or_default();
            // Exactly one item: its name ("one" covers more numbers in
            // some languages, e.g. French 0 and Russian 21).
            let title = match (self.mv, self.extract, n == 1) {
                (true, _, true) => t!(
                    "dialog.transfer.title.move.single",
                    name = name,
                    dest = dst_name
                ),
                (false, true, true) => t!(
                    "dialog.transfer.title.extract.single",
                    name = name,
                    dest = dst_name
                ),
                (false, false, true) => t!(
                    "dialog.transfer.title.copy.single",
                    name = name,
                    dest = dst_name
                ),
                (true, _, false) => t!("dialog.transfer.title.move", count = n, dest = dst_name),
                (false, true, false) => {
                    t!("dialog.transfer.title.extract", count = n, dest = dst_name)
                }
                (false, false, false) => {
                    t!("dialog.transfer.title.copy", count = n, dest = dst_name)
                }
            };
            let glyph = if self.mv {
                icons::ARROW_LINE_RIGHT
            } else if self.extract {
                icons::ARCHIVE
            } else {
                icons::COPY
            };
            dialog_header(ui, glyph, p.accent_text, p.accent_soft, &title, "");
            ui.add_space(space::MD);
            item_list(ui, &self.sources, &self.folders);
            ui.add_space(space::MD);
            if self.choose && !self.extract {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("dialog.transfer.mode")).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut mv = self.mv;
                        widgets::segmented(
                            ui,
                            "transfer-mode",
                            &mut mv,
                            &[
                                (false, t!("dialog.transfer.copy")),
                                (true, t!("dialog.transfer.move")),
                            ],
                        );
                        self.mv = mv;
                    });
                });
                ui.label(
                    RichText::new(if self.mv {
                        t!("dialog.transfer.mode.move_hint")
                    } else {
                        t!("dialog.transfer.mode.copy_hint")
                    })
                    .font(theme::regular(text::SMALL))
                    .color(p.text_secondary),
                );
                ui.add_space(space::SM);
            }
            if self.targets.len() > 1 {
                self.target_picker(ui);
                ui.add_space(space::SM);
            }
            route(ui, &self.sources, &self.dst_label, &self.dst_dir);
            let problem = self
                .targets
                .iter()
                .find(|t| t.side == self.dst_side)
                .and_then(|t| t.problem.clone());
            if let Some(problem) = &problem {
                ui.add_space(space::XS);
                ui.label(
                    RichText::new(format!("{}  {problem}", icons::WARNING_CIRCLE))
                        .font(theme::regular(text::SMALL))
                        .color(p.danger),
                );
            }
            ui.add_space(space::LG);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("dialog.conflict.label")).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("conflict")
                        .width(170.0)
                        .selected_text(conflict_label(self.conflict))
                        .show_ui(ui, |ui| {
                            for c in [
                                ConflictPolicy::Ask,
                                ConflictPolicy::Skip,
                                ConflictPolicy::OverwriteIfNewer,
                                ConflictPolicy::Overwrite,
                                ConflictPolicy::Rename,
                            ] {
                                ui.selectable_value(&mut self.conflict, c, conflict_label(c))
                                    .on_hover_text(conflict_hint(c));
                            }
                        });
                });
            });
            ui.label(
                RichText::new(conflict_hint(self.conflict))
                    .font(theme::regular(text::SMALL))
                    .color(p.text_secondary),
            );
            ui.add_space(space::LG);
            let ok = self
                .targets
                .iter()
                .find(|t| t.side == self.dst_side)
                .is_none_or(|t| t.problem.is_none());
            buttons(ui, verb, ButtonKind::Primary, ok, &mut result);
        });
        if result == DialogResult::Submit
            && self
                .targets
                .iter()
                .any(|t| t.side == self.dst_side && t.problem.is_some())
        {
            result = DialogResult::Open;
        }
        finish(ctx, modal.should_close(), result)
    }

    /// Point the dialog at another target pane.
    pub fn set_target(&mut self, side: crate::app::Side) {
        if let Some(t) = self.targets.iter().find(|t| t.side == side) {
            self.dst_side = side;
            self.dst_fs = t.fs.clone();
            self.dst_dir = t.dir.clone();
            self.dst_label = t.backend.clone();
        }
    }

    /// "To: [2 · Downloads ▾]" — every other pane, by number and folder.
    fn target_picker(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("dialog.transfer.to")).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current = self
                    .targets
                    .iter()
                    .find(|t| t.side == self.dst_side)
                    .map(|t| t.title())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("transfer-target")
                    .width(240.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for t in &self.targets {
                            let mut text = t.title();
                            if t.problem.is_some() {
                                text = format!("{text}  —");
                            }
                            let tip = match &t.problem {
                                Some(why) => format!("{}\n{}", t.backend, why),
                                None => format!("{}\n{}", t.backend, t.dir),
                            };
                            let r = ui
                                .selectable_label(t.side == self.dst_side, text)
                                .on_hover_text(tip);
                            if r.clicked() {
                                pick = Some(t.side);
                            }
                        }
                    })
                    .response
                    .on_hover_text(t!("dialog.transfer.target_tip"));
            });
        });
        if let Some(side) = pick {
            self.set_target(side);
        }
    }
}

pub struct DeleteForm {
    pub side: crate::app::Side,
    pub fs: SharedVfs,
    pub paths: Vec<VfsPath>,
    /// Which of `paths` are folders (for their icons).
    pub folders: Vec<VfsPath>,
    /// The items are on a server (or container, cloud API): there is no
    /// Trash there, which the dialog says. `false`: local items deleted
    /// with ⌥⌘⌫ instead of going to the Trash.
    pub no_trash: bool,
    /// The items are in the Trash already ("Delete Immediately…" there).
    pub in_trash: bool,
}

impl DeleteForm {
    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let n = self.paths.len();
        let modal = dialog_modal("delete-dialog", ctx).show(ctx, |ui| {
            ui.set_width(420.0);
            let name = self
                .paths
                .first()
                .map(|s| s.file_name().to_string())
                .unwrap_or_default();
            let title = if n == 1 {
                t!("dialog.delete.title.single", name = name)
            } else {
                t!("dialog.delete.title", count = n)
            };
            dialog_header(ui, icons::TRASH, p.danger, p.danger_soft, &title, "");
            ui.add_space(space::MD);
            item_list(ui, &self.paths, &self.folders);
            ui.add_space(space::MD);
            let location = crate::i18n::fs_label(&self.fs.label());
            if self.no_trash {
                ui.add(
                    egui::Label::new(
                        RichText::new(t!("dialog.delete.no_trash", location = location))
                            .font(theme::semibold(text::BODY))
                            .color(p.danger),
                    )
                    .wrap(),
                );
                ui.add_space(space::XS);
            }
            let body = if self.no_trash {
                t!("dialog.delete.body_remote", count = n)
            } else if self.in_trash {
                t!("dialog.delete.body_trash", count = n)
            } else {
                t!("dialog.delete.body", count = n)
            };
            ui.add(egui::Label::new(RichText::new(body).color(p.text_secondary)).wrap());
            ui.add_space(space::LG);
            buttons(
                ui,
                t!("dialog.delete.button"),
                ButtonKind::Danger,
                true,
                &mut result,
            );
        });
        finish(ctx, modal.should_close(), result)
    }
}

/// "Empty Trash…": asks before deleting everything in the Trash for good.
pub struct EmptyTrashForm {
    /// Items in the Trash, when known.
    pub items: Option<usize>,
    /// Their total size, once measured (`None`: unknown or still measuring).
    pub bytes: Option<u64>,
    /// The size is being measured in the background.
    pub measuring: bool,
    /// Manta can list the Trash itself; otherwise Finder empties it.
    pub readable: bool,
}

impl EmptyTrashForm {
    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let modal = dialog_modal("empty-trash-dialog", ctx).show(ctx, |ui| {
            ui.set_width(420.0);
            dialog_header(
                ui,
                icons::TRASH,
                p.danger,
                p.danger_soft,
                t!("dialog.empty_trash.title"),
                "",
            );
            ui.add_space(space::MD);
            let what = match (self.items, self.bytes) {
                (Some(n), Some(b)) => format!(
                    "{} · {}",
                    t!("pane.items", count = n),
                    crate::util::format_size(b)
                ),
                (Some(n), None) if self.measuring => format!(
                    "{} · {}",
                    t!("pane.items", count = n),
                    t!("dialog.empty_trash.measuring")
                ),
                (Some(n), None) => t!("pane.items", count = n),
                (None, _) => String::new(),
            };
            if !what.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(what)
                            .font(theme::semibold(text::BODY))
                            .color(p.text),
                    )
                    .wrap(),
                );
                ui.add_space(space::XS);
            }
            ui.add(
                egui::Label::new(
                    RichText::new(t!("dialog.empty_trash.body")).color(p.text_secondary),
                )
                .wrap(),
            );
            if !self.readable {
                ui.add_space(space::XS);
                ui.add(
                    egui::Label::new(
                        RichText::new(t!("dialog.empty_trash.by_finder"))
                            .font(theme::regular(text::CAPTION))
                            .color(p.text_tertiary),
                    )
                    .wrap(),
                );
            }
            ui.add_space(space::LG);
            buttons(
                ui,
                t!("dialog.empty_trash.button"),
                ButtonKind::Danger,
                true,
                &mut result,
            );
        });
        finish(ctx, modal.should_close(), result)
    }
}

/// One-time notice after an update: what changed since 0.2 (copying,
/// conflicts, languages, the location chip, the menu bar).
pub struct WhatsNewForm;

impl WhatsNewForm {
    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let modal = dialog_modal("whats-new", ctx).show(ctx, |ui| {
            ui.set_width(460.0);
            dialog_header(
                ui,
                icons::STAR,
                p.accent,
                p.accent_soft,
                t!("whats_new.title"),
                t!("whats_new.subtitle"),
            );
            ui.add_space(space::MD);
            let row = |ui: &mut egui::Ui, glyph: &str, title: &str, body: &str| {
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 22.0), Sense::hover());
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(radius::CONTROL), p.stripe);
                    widgets::paint_icon(ui, r, glyph, 14.0, p.text, false);
                    ui.add_space(space::SM);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = space::XXS;
                        ui.add(
                            egui::Label::new(
                                RichText::new(title)
                                    .font(theme::semibold(text::BODY))
                                    .color(p.text),
                            )
                            .wrap(),
                        );
                        ui.add(
                            egui::Label::new(RichText::new(body).color(p.text_secondary)).wrap(),
                        );
                    });
                });
                ui.add_space(space::SM);
            };
            let general = |last: &str| {
                format!(
                    "{} › {} › {last}",
                    t!("settings.title"),
                    t!("settings.general")
                )
            };
            row(
                ui,
                icons::COPY,
                t!("whats_new.drop"),
                &t!("whats_new.drop.body", path = general(t!("settings.drop"))),
            );
            let (replace, keep_both) = (
                t!("dialog.conflict.replace"),
                t!("dialog.conflict.keep_both"),
            );
            let (skip, merge) = (t!("dialog.conflict.skip"), t!("conflict.merge"));
            let conflicts = t!(
                "whats_new.conflicts.body",
                replace = replace,
                keep_both = keep_both,
                skip = skip,
                merge = merge
            );
            row(ui, icons::QUESTION, t!("whats_new.conflicts"), &conflicts);
            row(
                ui,
                icons::GLOBE,
                t!("whats_new.languages"),
                &t!(
                    "whats_new.languages.body",
                    path = general(t!("settings.language"))
                ),
            );
            row(
                ui,
                icons::SIGNPOST,
                t!("whats_new.location"),
                t!("whats_new.location.body"),
            );
            row(
                ui,
                icons::LIST,
                t!("whats_new.menu_bar"),
                t!("whats_new.menu_bar.body"),
            );
            ui.add_space(space::SM);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, t!("whats_new.ok"), ButtonKind::Primary, true).clicked() {
                    result = DialogResult::Submit;
                }
            });
        });
        finish(ctx, modal.should_close(), result)
    }
}

/// Used for "New Folder", "New File" and "Rename".
pub struct NameForm {
    pub tab_id: u64,
    pub fs: SharedVfs,
    pub dir: VfsPath,
    /// `Some(old name)` for rename, `None` for new folder / new file.
    pub original: Option<String>,
    /// With `original: None`: create an empty file instead of a folder.
    pub new_file: bool,
    pub name: String,
    /// Names already present in `dir` (for collision checks).
    pub existing: Vec<String>,
    pub error: Option<String>,
    focus_requested: bool,
}

impl NameForm {
    pub fn new(
        tab_id: u64,
        fs: SharedVfs,
        dir: VfsPath,
        original: Option<String>,
        existing: Vec<String>,
    ) -> Self {
        let name = match &original {
            Some(o) => o.clone(),
            None => util::unique_name(&existing, t!("dialog.new_folder.default_name")),
        };
        NameForm {
            tab_id,
            fs,
            dir,
            original,
            name,
            existing,
            new_file: false,
            error: None,
            focus_requested: false,
        }
    }

    /// The "New File" dialog: an empty file named in `dir`.
    pub fn new_file(tab_id: u64, fs: SharedVfs, dir: VfsPath, existing: Vec<String>) -> Self {
        let name = util::unique_name(&existing, t!("dialog.new_file.default_name"));
        NameForm {
            name,
            new_file: true,
            ..NameForm::new(tab_id, fs, dir, None, existing)
        }
    }

    /// Validate; on success returns the trimmed name.
    pub fn validated(&self) -> Result<String, String> {
        util::validate_file_name(&self.name)?;
        let name = self.name.trim().to_string();
        if self.original.as_deref() != Some(name.as_str()) && self.existing.contains(&name) {
            return Err(t!("dialog.name.exists", name = name));
        }
        Ok(name)
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> DialogResult {
        let mut result = DialogResult::Open;
        let p = theme::pal_ctx(ctx);
        let (title, ok, glyph) = match &self.original {
            Some(o) => (
                t!("dialog.rename.title", name = o),
                t!("dialog.rename.button"),
                icons::PENCIL_SIMPLE,
            ),
            None if self.new_file => (
                t!("dialog.new_file.title").to_string(),
                t!("dialog.new_file.button"),
                icons::FILE_PLUS,
            ),
            None => (
                t!("dialog.new_folder.title").to_string(),
                t!("dialog.new_folder.button"),
                icons::FOLDER_PLUS,
            ),
        };
        let live = self.validated();
        let modal = dialog_modal("name-dialog", ctx).show(ctx, |ui| {
            ui.set_width(380.0);
            dialog_header(
                ui,
                glyph,
                p.accent_text,
                p.accent_soft,
                &title,
                &t!("dialog.name.in_folder", folder = folder_name(&self.dir)),
            );
            ui.add_space(space::LG);
            let resp = widgets::show_input(
                ui,
                widgets::input(&mut self.name).desired_width(f32::INFINITY),
                "",
            )
            .accessible_name(if self.original.is_some() {
                t!("dialog.rename.field")
            } else if self.new_file {
                t!("dialog.new_file.field")
            } else {
                t!("dialog.new_folder.field")
            });
            if !self.focus_requested {
                resp.request_focus();
                select_name(
                    ui.ctx(),
                    resp.id,
                    &self.name,
                    self.original.is_some() || self.new_file,
                );
                self.focus_requested = true;
            }
            if resp.changed() {
                self.error = None;
            }
            let problem = self.error.clone().or_else(|| live.as_ref().err().cloned());
            ui.add_space(space::XS);
            let msg = problem.as_deref().unwrap_or(" ");
            ui.label(
                RichText::new(msg)
                    .font(theme::regular(text::SMALL))
                    .color(p.danger),
            );
            ui.add_space(space::MD);
            buttons(ui, ok, ButtonKind::Primary, live.is_ok(), &mut result);
        });
        if result == DialogResult::Open && live.is_err() && ctx.input(|i| i.key_pressed(Key::Enter))
        {
            return DialogResult::Open;
        }
        finish(ctx, modal.should_close(), result)
    }
}

// ------------------------------------------------------------------ helpers

/// Select the name in the text edit `id`: only the stem (without the
/// extension) when `stem`, like Finder does.
pub(crate) fn select_name(ctx: &egui::Context, id: egui::Id, name: &str, stem: bool) {
    let len = match name.rfind('.') {
        Some(i) if i > 0 && stem => name[..i].chars().count(),
        _ => name.chars().count(),
    };
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(len),
        );
        state.cursor.set_char_range(Some(range));
        state.store(ctx, id);
    }
}

pub(crate) fn folder_name(dir: &VfsPath) -> String {
    let n = dir.file_name();
    if n.is_empty() {
        "/".into()
    } else {
        n.to_string()
    }
}

pub(crate) fn dialog_modal(id: &str, ctx: &egui::Context) -> egui::Modal {
    let p = theme::pal_ctx(ctx);
    egui::Modal::new(egui::Id::unique(id))
        .backdrop_color(crate::a11y::scrim(p.dark, 60, 110))
        .frame(
            egui::Frame::new()
                .fill(p.elevated)
                .stroke(egui::Stroke::new(1.0, p.separator))
                .corner_radius(CornerRadius::same(radius::WINDOW))
                .shadow(ctx.global_style().visuals.window_shadow)
                .inner_margin(egui::Margin::same(20)),
        )
}

pub(crate) fn dialog_header(
    ui: &mut egui::Ui,
    glyph: &str,
    fg: Color32,
    bg: Color32,
    title: &str,
    subtitle: &str,
) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::hover());
        ui.painter()
            .rect_filled(r, CornerRadius::same(radius::PANEL + 1), bg);
        widgets::paint_icon(ui, r, glyph, 20.0, fg, false);
        ui.add_space(space::XS);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::XXS;
            if subtitle.is_empty() {
                ui.add_space(8.0);
            }
            ui.add(
                egui::Label::new(
                    RichText::new(title)
                        .font(theme::semibold(text::TITLE))
                        .color(p.text),
                )
                .wrap(),
            );
            if !subtitle.is_empty() {
                ui.label(RichText::new(subtitle).color(p.text_secondary));
            }
        });
    });
}

/// Width of the form label column (German labels are longer).
fn label_w() -> f32 {
    match crate::i18n::locale() {
        crate::i18n::Locale::En | crate::i18n::Locale::Zh => 92.0,
        _ => 124.0,
    }
}

/// A form row with a right-aligned label, macOS style.
fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(label_w(), 24.0), Sense::hover());
        if !label.is_empty() {
            ui.painter().text(
                r.right_center(),
                egui::Align2::RIGHT_CENTER,
                label,
                theme::regular(text::BODY),
                p.text_secondary,
            );
        }
        ui.add_space(space::XS);
        add(ui);
    });
    ui.add_space(space::XXS);
}

fn field_note(ui: &mut egui::Ui, note: &str) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.add_space(label_w() + space::XS + space::MD);
        ui.label(
            RichText::new(note)
                .font(theme::regular(text::SMALL))
                .color(p.text_secondary),
        );
    });
    ui.add_space(space::XXS);
}

/// A longer note under a field, wrapped to the form's input column.
fn field_note_wrapped(ui: &mut egui::Ui, note: &str) {
    let p = pal(ui);
    let indent = label_w() + space::XS + space::MD;
    let w = (ui.available_width() - indent).max(120.0);
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.allocate_ui(egui::vec2(w, 0.0), |ui| {
            ui.set_max_width(w);
            ui.add(
                egui::Label::new(
                    RichText::new(note)
                        .font(theme::regular(text::SMALL))
                        .color(p.text_secondary),
                )
                .wrap(),
            );
        });
    });
    ui.add_space(space::XXS);
}

fn field_problem(ui: &mut egui::Ui, problem: Option<&str>) {
    let Some(problem) = problem else { return };
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.add_space(label_w() + space::XS + space::MD);
        ui.label(
            RichText::new(format!("{}  {problem}", icons::WARNING_CIRCLE))
                .font(theme::regular(text::SMALL))
                .color(p.danger),
        );
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Banner {
    Error,
    Success,
}

fn banner(ui: &mut egui::Ui, kind: Banner, title: &str, hint: &str, detail: Option<&str>) {
    let p = pal(ui);
    let (fg, bg, glyph) = match kind {
        Banner::Error => (p.danger, p.danger_soft, icons::WARNING_CIRCLE),
        Banner::Success => (p.success, p.success_soft, icons::CHECK_CIRCLE),
    };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::PANEL))
        .inner_margin(egui::Margin::symmetric(
            space::LG as i8,
            space::MD as i8 + 2,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                widgets::icon(ui, glyph, 17.0, fg);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    ui.add(
                        egui::Label::new(
                            RichText::new(title)
                                .font(theme::semibold(text::BODY))
                                .color(fg),
                        )
                        .wrap(),
                    );
                    if !hint.is_empty() {
                        ui.add(egui::Label::new(RichText::new(hint).color(p.text)).wrap());
                    }
                    if let Some(d) = detail {
                        ui.add(
                            egui::Label::new(
                                RichText::new(d)
                                    .font(theme::regular(text::CAPTION))
                                    .color(p.text_secondary),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    }
                });
            });
        });
}

/// Up to five item names with icons, then "and N more".
pub(crate) fn item_list(ui: &mut egui::Ui, paths: &[VfsPath], folders: &[VfsPath]) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.stripe)
        .corner_radius(CornerRadius::same(radius::PANEL))
        .inner_margin(egui::Margin::symmetric(
            space::MD as i8 + 2,
            space::SM as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XXS;
            for path in paths.iter().take(5) {
                let name = path.file_name();
                let kind = icons::file_kind(name, folders.contains(path));
                // Unknown kinds get a neutral icon.
                let (glyph, tint) = if kind == icons::FileKind::Other {
                    (icons::FILE, p.generic)
                } else {
                    (kind.icon(), kind.tint(p))
                };
                ui.horizontal(|ui| {
                    widgets::icon(ui, glyph, 15.0, tint);
                    ui.add(egui::Label::new(RichText::new(name).color(p.text)).truncate());
                });
            }
            if paths.len() > 5 {
                ui.label(
                    RichText::new(t!("dialog.items.more", count = paths.len() - 5))
                        .color(p.text_secondary),
                );
            }
        });
}

/// "From → To" line for transfers.
fn route(ui: &mut egui::Ui, sources: &[VfsPath], dst_label: &str, dst_dir: &VfsPath) {
    let p = pal(ui);
    let from = sources
        .first()
        .and_then(|s| s.parent())
        .map(|d| folder_name(&d))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new(t!("dialog.route.from")).color(p.text_secondary));
        widgets::chip(ui, Some(icons::FOLDER_SIMPLE), &from, p.text, p.stripe);
        widgets::icon(ui, icons::ARROW_RIGHT, 13.0, p.text_secondary);
        widgets::chip(
            ui,
            Some(icons::FOLDER_SIMPLE),
            &folder_name(dst_dir),
            p.accent_text,
            p.accent_soft,
        )
        .on_hover_text(format!("{dst_label}  {dst_dir}"));
        ui.label(
            RichText::new(t!(
                "dialog.route.on",
                location = crate::i18n::fs_label(dst_label)
            ))
            .color(p.text_secondary),
        );
    });
}

pub(crate) fn buttons(
    ui: &mut egui::Ui,
    ok: &str,
    kind: ButtonKind,
    enabled: bool,
    result: &mut DialogResult,
) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if widgets::button(ui, ok, kind, enabled).clicked() {
            *result = DialogResult::Submit;
        }
        if widgets::button(ui, t!("common.cancel"), ButtonKind::Secondary, true).clicked() {
            *result = DialogResult::Cancel;
        }
    });
}

pub(crate) fn finish(
    ctx: &egui::Context,
    should_close: bool,
    mut result: DialogResult,
) -> DialogResult {
    if result == DialogResult::Open && ctx.input(|i| i.key_pressed(Key::Enter)) {
        result = DialogResult::Submit;
    }
    if result == DialogResult::Open && should_close {
        result = DialogResult::Cancel;
    }
    result
}

pub enum Dialog {
    Connect(ConnectForm),
    Transfer(TransferForm),
    Delete(DeleteForm),
    /// "Empty Trash…" (sidebar, ⇧⌘⌫).
    EmptyTrash(EmptyTrashForm),
    WhatsNew(WhatsNewForm),
    Name(NameForm),
    Compress(crate::compress_sheet::CompressForm),
    /// SFTP host key question (unknown or changed key).
    HostKey(Box<crate::host_key::HostKeyForm>),
    /// A transfer stopped at an existing item ([`ConflictPolicy::Ask`]).
    Conflict(Box<crate::conflict_dialog::ConflictForm>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_form_to_config() {
        let mut f = ConnectForm::new(Side::Left);
        assert!(f.to_config().is_err());
        f.host = " example.com ".into();
        f.user = "bob".into();
        f.password = "pw".into();
        let cfg = f.to_config().unwrap();
        assert_eq!(cfg.host, "example.com");
        assert_eq!(cfg.port, 22);
        assert_eq!(cfg.auth, Auth::Password("pw".into()));
        assert_eq!(cfg.initial_path, None);

        f.set_protocol(Protocol::Ftp);
        assert_eq!(f.port, "21");
        f.port = "2121".into();
        f.set_protocol(Protocol::Ftps);
        assert_eq!(f.port, "2121", "custom port is kept");

        let back = ConnectForm::from_config(&cfg, Side::Right, Some(3));
        assert_eq!(back.to_config().unwrap(), cfg);
    }

    #[test]
    fn s3_form_presets_and_config() {
        let mut f = ConnectForm::new(Side::Left);
        f.set_protocol(Protocol::S3);
        assert_eq!(f.user, "", "the login name isn't an access key");
        assert_eq!(f.s3.endpoint, "https://s3.us-east-1.amazonaws.com");
        assert!(f.to_config().unwrap_err().contains("access key ID"));
        f.user = "AKIAEXAMPLE".into();
        f.password = "secret".into();
        f.s3.region = "eu-central-1".into();
        f.s3.refresh_endpoint();
        assert_eq!(f.s3.endpoint, "https://s3.eu-central-1.amazonaws.com");
        let cfg = f.to_config().unwrap();
        assert_eq!(
            (cfg.protocol, cfg.host.as_str(), cfg.port),
            (Protocol::S3, "s3.eu-central-1.amazonaws.com", 443)
        );
        assert_eq!(cfg.username, "AKIAEXAMPLE");
        assert_eq!(cfg.auth, Auth::Password("secret".into()));
        let o = cfg.s3.clone().unwrap();
        assert_eq!(
            (o.endpoint.as_str(), o.region.as_str(), o.path_style),
            ("", "eu-central-1", false)
        );
        let back = ConnectForm::from_config(&cfg, Side::Left, None);
        assert_eq!(back.to_config().unwrap(), cfg);

        // R2 needs the account ID and fills the endpoint with it.
        f.s3.set_provider("r2");
        assert_eq!(f.s3.region, "auto");
        assert!(f.s3.path_style);
        assert!(f.to_config().unwrap_err().contains("account"));
        f.s3.account = "abc123".into();
        f.s3.refresh_endpoint();
        assert_eq!(f.s3.endpoint, "https://abc123.r2.cloudflarestorage.com");
        assert_eq!(
            f.to_config().unwrap().s3.unwrap().endpoint,
            "https://abc123.r2.cloudflarestorage.com"
        );

        // MinIO on a custom port, AWS profile, one bucket.
        f.s3.set_provider("minio");
        f.s3.endpoint = "http://127.0.0.1:9000".into();
        f.s3.endpoint_auto = false;
        f.s3.bucket = "/media/".into();
        f.auth_kind = AuthKind::AwsProfile;
        f.s3.profile = "work".into();
        let cfg = f.to_config().unwrap();
        assert_eq!(
            (cfg.host.as_str(), cfg.port, cfg.auth.clone()),
            ("127.0.0.1", 9000, Auth::None)
        );
        let o = cfg.s3.clone().unwrap();
        assert_eq!(
            (o.bucket.as_str(), o.profile.as_deref()),
            ("media", Some("work"))
        );
        let back = ConnectForm::from_config(&cfg, Side::Left, None);
        assert_eq!(back.auth_kind, AuthKind::AwsProfile);
        assert!(!back.s3.endpoint_auto);
        assert_eq!(back.to_config().unwrap(), cfg);
        f.s3.endpoint = "ftp://nope".into();
        assert!(f.to_config().is_err());

        // Secrets follow the "remember" rule.
        let mut f = ConnectForm::from_config(&cfg, Side::Left, None);
        f.auth_kind = AuthKind::Password;
        f.user = "k".into();
        f.password = "s".into();
        let stripped = crate::favorites::strip_secrets(&f.to_config().unwrap());
        assert_eq!(stripped.auth, Auth::Password(String::new()));
        assert!(crate::favorites::needs_secret(&stripped));
    }

    #[test]
    fn pasted_addresses() {
        let p = parse_host("sftp://deploy@web-01.example.com:2222/var/www");
        assert_eq!(p.protocol, Some(Protocol::Sftp));
        assert_eq!(p.user.as_deref(), Some("deploy"));
        assert_eq!(p.host, "web-01.example.com");
        assert_eq!(p.port.as_deref(), Some("2222"));
        assert_eq!(p.path.as_deref(), Some("/var/www"));
        assert_eq!(parse_host("example.com").host, "example.com");
        assert_eq!(parse_host("::1").host, "::1", "bare IPv6 untouched");

        let mut f = ConnectForm::new(Side::Left);
        f.host = "anna@files.example.org:2121".into();
        f.set_protocol(Protocol::Ftp);
        let cfg = f.to_config().unwrap();
        assert_eq!(
            (cfg.username.as_str(), cfg.host.as_str(), cfg.port),
            ("anna", "files.example.org", 2121)
        );
        f.absorb_pasted_address();
        assert_eq!(
            (f.user.as_str(), f.host.as_str(), f.port.as_str()),
            ("anna", "files.example.org", "2121")
        );
    }

    #[test]
    fn webdav_form() {
        let mut f = ConnectForm::new(Side::Left);
        f.set_protocol(Protocol::WebDavs);
        assert_eq!(f.port, "443");
        assert!(f.dav.https);
        f.host = "cloud.example.com".into();
        f.user = "anna".into();
        f.password = "pw".into();
        f.dav.set_preset("nextcloud", &f.user);
        assert_eq!(f.dav.root, "/remote.php/dav/files/anna");
        // The path follows the user name until it is edited.
        f.user = "ben".into();
        f.dav.refresh_root(&f.user);
        assert_eq!(f.dav.root, "/remote.php/dav/files/ben");
        let cfg = f.to_config().unwrap();
        assert_eq!(cfg.protocol, Protocol::WebDavs);
        assert_eq!(cfg.port, 443);
        let o = cfg.webdav.as_ref().unwrap();
        assert_eq!(
            (o.root.as_str(), o.preset.as_str()),
            ("/remote.php/dav/files/ben", "nextcloud")
        );
        let back = ConnectForm::from_config(&cfg, Side::Left, None);
        assert!(back.dav.root_auto && back.dav.https);
        assert_eq!(back.to_config().unwrap(), cfg);

        // Plain HTTP switches the default port.
        f.set_protocol(Protocol::WebDav);
        assert_eq!((f.port.as_str(), f.dav.https), ("80", false));
        f.dav.root = "/dav/".into();
        f.dav.root_auto = false;
        let cfg = f.to_config().unwrap();
        assert_eq!(cfg.protocol, Protocol::WebDav);
        assert_eq!(cfg.webdav.unwrap().root, "/dav");

        // A pasted URL: scheme, host, port and WebDAV path.
        let mut f = ConnectForm::new(Side::Left);
        f.user = "anna".into();
        f.host = "https://cloud.example.com:8443/remote.php/dav/files/anna/".into();
        f.absorb_pasted_address();
        assert_eq!(f.protocol, Protocol::WebDavs);
        assert_eq!(
            (f.host.as_str(), f.port.as_str()),
            ("cloud.example.com", "8443")
        );
        assert_eq!(f.dav.preset, "nextcloud");
        assert_eq!(f.dav.root, "/remote.php/dav/files/anna");
        let mut f = ConnectForm::new(Side::Left);
        f.host = "http://nas.local/dav".into();
        f.absorb_pasted_address();
        assert_eq!((f.protocol, f.dav.preset), (Protocol::WebDav, "generic"));
        assert_eq!(f.dav.root, "/dav");
    }

    #[test]
    fn smb_form() {
        let mut f = ConnectForm::new(Side::Left);
        f.set_protocol(Protocol::Smb);
        assert_eq!(f.port, "445");
        f.host = "nas.local".into();
        f.user = "anna".into();
        f.password = "pw".into();
        assert_eq!(f.to_config().unwrap_err(), t!("connect.problem.share"));
        f.smb.share = "Team".into();
        let cfg = f.to_config().unwrap();
        assert_eq!(cfg.protocol, Protocol::Smb);
        assert_eq!(cfg.smb.as_ref().unwrap().share, "Team");
        assert_eq!(f.target_label(), "nas.local/Team");
        let back = ConnectForm::from_config(&cfg, Side::Left, None);
        assert_eq!(back.smb.share, "Team");
        assert_eq!(back.to_config().unwrap(), cfg);
        // Guest: no user, no password.
        f.auth_kind = AuthKind::Anonymous;
        assert_eq!(f.to_config().unwrap().auth, Auth::None);

        // smb://server/share/folder
        let mut f = ConnectForm::new(Side::Left);
        f.host = "smb://anna@nas.local/Team%20Files/Projects/2026".into();
        f.absorb_pasted_address();
        assert_eq!(f.protocol, Protocol::Smb);
        assert_eq!(
            (f.host.as_str(), f.user.as_str(), f.smb.share.as_str()),
            ("nas.local", "anna", "Team Files")
        );
        assert_eq!(f.initial_path, "/Projects/2026");
        // Not absorbed yet (Enter right away): to_config splits it too.
        let mut f = ConnectForm::new(Side::Left);
        f.set_protocol(Protocol::Smb);
        f.host = "nas.local/Media/Movies".into();
        let cfg = f.to_config().unwrap();
        assert_eq!(cfg.smb.unwrap().share, "Media");
        assert_eq!(cfg.initial_path.as_deref(), Some("/Movies"));
        let browse = f.smb_browse_config();
        assert_eq!(
            (browse.host.as_str(), browse.protocol),
            ("nas.local", Protocol::Smb)
        );
    }

    #[test]
    fn friendly_errors() {
        let (t, _) = friendly_connect_error(
            "authentication failed: Username/PublicKey combination invalid",
            "h",
            22,
        );
        assert!(t.contains("credentials"));
        let (t, _) = friendly_connect_error(
            "connection error: Connection refused (os error 61)",
            "h",
            22,
        );
        assert!(t.contains("refused"));
        let (t, _) = friendly_connect_error("connection error: failed to lookup address information: nodename nor servname provided, or not known", "nope.invalid", 22);
        assert!(t.contains("find"));
        let (t, h) = friendly_connect_error(
            "connection error: your Mac's clock is too far off from the server's time",
            "h",
            443,
        );
        assert!(t.contains("clock") && !h.is_empty());
        let (t, _) = friendly_connect_error(
            "connection error: /b: the bucket is in region eu-west-1; set the region to eu-west-1",
            "h",
            443,
        );
        assert!(t.contains("region"));
        let (t, _) = friendly_connect_error(
            "authentication failed: the secret access key doesn't match the access key ID",
            "h",
            443,
        );
        assert!(t.contains("credentials"));
    }
}
